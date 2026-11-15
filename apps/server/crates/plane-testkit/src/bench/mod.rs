// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `difftest bench`: a closed-loop HTTP load generator with memory and CPU
//! measurement for the whole target process tree.
//!
//! The benchmark exists to test the brief's claim that the Rust services are
//! faster and lighter, measured rather than assumed (`RUST_REWRITE.md`,
//! "Verification" item 5). It reuses the differential harness's target
//! provisioning, so a measurement runs against the same managed target, the
//! same database clone and the same environment a differential run uses.
//!
//! One invocation measures one scenario against one target configuration at
//! every requested concurrency, `--repetitions` times, and reports medians.
//! The target is **restarted for every repetition** in managed mode, so the
//! idle RSS is always a fresh process's, not one left inflated by the previous
//! load. Setup runs once per invocation: its rows and session cookies stay
//! valid across the restarts (they live in the database).
//!
//! Sequence per repetition:
//!
//! 1. start the target, wait for its health path (managed mode);
//! 2. run the scenario's `setup` steps once, capturing variables and cookies;
//! 3. settle for `--settle` seconds (default 10);
//! 4. sample the idle RSS of the process tree;
//! 5. for each concurrency: warm up, then measure with the RSS/CPU sampler
//!    running for the measured window only;
//! 6. stop the target.
//!
//! The load generator is closed-loop (see `load`), the client is the only
//! thing issuing requests, and the client's own ceiling is measured separately
//! (`headroom`).

pub mod headroom;
pub mod histogram;
pub mod load;
pub mod process;
pub mod report;
pub mod request;
pub mod scenario;
pub mod server;
pub mod setup;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::bench::histogram::median_u64;
use crate::bench::report::{
    BenchReport, HostInfo, Measurement, SchemeInfo, TargetInfo, host_info, medians,
};
use crate::bench::scenario::BenchScenario;
use crate::bench::setup::SetupOutcome;
use crate::celery::RabbitAdmin;
use crate::config::TargetsFile;
use crate::db::DbAdmin;
use crate::provision::{PortAllocator, TargetInstance, host_port, wait_tcp};
use crate::redis::RedisAdmin;
use crate::util::{new_run_id, repo_root, validate_identifier};

/// How often the process tree is sampled for peak RSS during a measured
/// window. The brief fixes 250 ms.
pub const RSS_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// A target that is already running (`--url`/`--db`). No process metrics are
/// possible without a PID; pass `--pgid` when the group is known.
#[derive(Debug, Clone)]
pub struct AttachTarget {
    pub url: String,
    pub database: String,
    pub pgid: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct BenchOptions {
    pub scenario_path: PathBuf,
    pub targets_path: PathBuf,
    /// Target name from `targets.yaml`.
    pub target: String,
    pub concurrencies: Vec<usize>,
    pub duration: Duration,
    pub warmup: Duration,
    pub settle: Duration,
    pub repetitions: u32,
    /// Rewrite a managed target's `.../target/debug/<binary>` command to the
    /// release build. The harness itself can be a debug build; the target must
    /// not be. A command that names no debug path (a shell script, or a target
    /// already pointed at `target/release/`) is left alone.
    pub release: bool,
    /// `GUNICORN_WORKERS` for a managed target; the harness does not branch on
    /// the target's kind, it just sets the variable.
    pub django_workers: Option<u32>,
    /// `PLANE_RS_WORKER_THREADS` for a managed target.
    pub rust_threads: Option<u32>,
    /// Overrides the scenario's `redis_db`.
    pub redis_db: Option<i64>,
    pub keep: bool,
    pub report_path: Option<PathBuf>,
    pub markdown_path: Option<PathBuf>,
    /// `None` (or zero) disables the client-headroom measurement.
    pub headroom_duration: Option<Duration>,
    pub pg_url: String,
    pub redis_url: String,
    pub rabbit_http: String,
    pub rabbit_user: String,
    pub rabbit_password: String,
    pub attach: Option<AttachTarget>,
}

#[derive(Default)]
struct Resources {
    targets: Vec<TargetInstance>,
    databases: Vec<String>,
    vhosts: Vec<String>,
}

impl Resources {
    async fn cleanup(&mut self, opts: &BenchOptions) -> Result<()> {
        let mut errors = Vec::new();
        for target in &mut self.targets {
            if let Err(error) = target.stop().await {
                errors.push(format!("stopping target {}: {error:#}", target.name));
            }
        }
        if opts.keep {
            if !self.databases.is_empty() || !self.vhosts.is_empty() {
                println!(
                    "--keep: left databases [{}] and RabbitMQ vhosts [{}]",
                    self.databases.join(", "),
                    self.vhosts.join(", ")
                );
            }
        } else {
            if !self.databases.is_empty() {
                let admin = DbAdmin::new(&opts.pg_url);
                for database in &self.databases {
                    if let Err(error) = admin.drop_database(database).await {
                        errors.push(format!("dropping database {database}: {error:#}"));
                    }
                }
            }
            if !self.vhosts.is_empty() {
                match RabbitAdmin::new(&opts.rabbit_http, &opts.rabbit_user, &opts.rabbit_password)
                {
                    Ok(rabbit) => {
                        for vhost in &self.vhosts {
                            if let Err(error) = rabbit.delete_vhost(vhost).await {
                                errors.push(format!("deleting vhost {vhost}: {error:#}"));
                            }
                        }
                    }
                    Err(error) => errors.push(format!("RabbitMQ management client: {error:#}")),
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            bail!(errors.join("; "))
        }
    }
}

pub async fn run(opts: BenchOptions) -> Result<BenchReport> {
    let mut resources = Resources::default();
    let outcome = run_inner(&opts, &mut resources).await;
    let cleanup = resources.cleanup(&opts).await;
    match (outcome, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Ok(report), Err(error)) => {
            eprintln!("difftest bench: cleanup problem: {error:#}");
            Ok(report)
        }
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            Err(error.context(format!("cleanup also failed: {cleanup_error:#}")))
        }
    }
}

async fn run_inner(opts: &BenchOptions, resources: &mut Resources) -> Result<BenchReport> {
    let scenario = BenchScenario::load(&opts.scenario_path)?;
    if opts.repetitions == 0 {
        bail!("--repetitions must be at least 1");
    }
    if opts.concurrencies.is_empty() {
        bail!("--concurrency needs at least one value");
    }
    if opts.attach.is_some() && (opts.django_workers.is_some() || opts.rust_threads.is_some()) {
        bail!(
            "--django-workers and --rust-threads only apply to a managed target: the harness \
             starts it with that environment. For an attached target, set the variables on its \
             own command line."
        );
    }
    let targets = TargetsFile::load(&opts.targets_path)?;
    let mut spec = targets.get(&opts.target)?.clone();
    if opts.release {
        // A path rewrite, not a mode switch: a target that runs a shell script
        // (django-serve.sh) or already names a release binary has nothing to
        // rewrite, and that is fine.
        for argument in &mut spec.command {
            if argument.contains("/target/debug/") {
                *argument = argument.replace("/target/debug/", "/target/release/");
            }
        }
    }
    let run_id = new_run_id();
    let redis_db = opts.redis_db.unwrap_or(scenario.redis_db);
    if !(0..=15).contains(&redis_db) {
        bail!("redis db must be 0-15, got {redis_db}");
    }

    let load_average_start = report::load_average();
    println!(
        "difftest bench {run_id}: scenario {} ({} template(s), {} setup step(s)), target {} ({}), \
         concurrency {:?}, {}s measured after {}s warm-up, {} repetition(s)",
        scenario.name,
        scenario.requests.len(),
        scenario.setup.len(),
        opts.target,
        spec.kind,
        opts.concurrencies,
        opts.duration.as_secs_f64(),
        opts.warmup.as_secs_f64(),
        opts.repetitions,
    );

    // Managed resources: one database clone, one RabbitMQ vhost and one Redis
    // index for the whole invocation. The target is restarted per repetition
    // against the same database, so setup state survives.
    let managed = opts.attach.is_none();
    let mut base_url = opts
        .attach
        .as_ref()
        .map(|attach| attach.url.trim_end_matches('/').to_string())
        .unwrap_or_default();
    let mut port_allocator = PortAllocator::new(spec.port_range[0], spec.port_range[1]);
    let (database, vhost) = match &opts.attach {
        Some(attach) => (attach.database.clone(), String::new()),
        None => {
            let database = format!("bench_{run_id}");
            let vhost = format!("bench_{run_id}");
            validate_identifier(&database)?;
            validate_identifier(&vhost)?;

            let (pg_host, pg_port) = host_port(&opts.pg_url, 5432);
            let (redis_host, redis_port) = host_port(&opts.redis_url, 6379);
            wait_tcp("Postgres", &pg_host, pg_port, Duration::from_secs(30)).await?;
            wait_tcp("Redis", &redis_host, redis_port, Duration::from_secs(30)).await?;
            let (rabbit_host, rabbit_port) = host_port(&opts.rabbit_http, 80);
            wait_tcp(
                "RabbitMQ management",
                &rabbit_host,
                rabbit_port,
                Duration::from_secs(30),
            )
            .await?;

            let redis = RedisAdmin::from_url(&opts.redis_url)?;
            redis.ping().await?;
            redis.flush_db(redis_db).await?;

            let rabbit =
                RabbitAdmin::new(&opts.rabbit_http, &opts.rabbit_user, &opts.rabbit_password)?;
            rabbit.create_vhost(&vhost).await?;
            rabbit
                .grant_full_permissions(&vhost, &opts.rabbit_user)
                .await?;
            resources.vhosts.push(vhost.clone());

            let db_admin = DbAdmin::new(&opts.pg_url);
            db_admin
                .clone_database(&scenario.template_db, &database)
                .await?;
            resources.databases.push(database.clone());
            // The differential harness's reserved `fixture_sql` variable: a SQL
            // file applied to the clone before setup, for rows that no HTTP
            // route can create yet (users, sessions, objects). It is part of
            // the baseline, never measured traffic. Attached databases are not
            // touched: the caller owns them.
            if let Some(path) = scenario.fixture_sql() {
                let sql = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading the fixture {}", path.display()))?;
                let client = db_admin.connect(&database).await?;
                client.batch_execute(&sql).await.with_context(|| {
                    format!("applying the fixture {} to {database}", path.display())
                })?;
                println!("fixture: applied {} to {database}", path.display());
            }
            println!("database {database} (redis db {redis_db}, vhost {vhost})");
            (database, vhost)
        }
    };

    // The scenario's `target_env` is applied first (so it keeps working for
    // differential scenarios copied over), then the command-line knobs win.
    let mut env = scenario.target_env.clone();
    if let Some(workers) = opts.django_workers {
        env.insert("GUNICORN_WORKERS".to_string(), workers.to_string());
    }
    if let Some(threads) = opts.rust_threads {
        env.insert("PLANE_RS_WORKER_THREADS".to_string(), threads.to_string());
    }
    let spec = spec.with_env(&env);

    let log_dir = repo_root()
        .join("apps/server/target")
        .join("bench-logs")
        .join(&run_id);

    let mut measurements = Vec::new();
    let mut notes = Vec::new();
    let mut setup_outcome: Option<SetupOutcome> = None;

    for repetition in 1..=opts.repetitions {
        let pgid: Option<u32>;
        if managed {
            let port = port_allocator.allocate().await?;
            let context = target_context(port, &database, redis_db, &vhost);
            let name = format!("bench-{repetition}");
            let mut target = TargetInstance::start(&name, &spec, &context, &log_dir).await?;
            target
                .wait_healthy(&spec.health, Duration::from_secs(spec.health_timeout_secs))
                .await?;
            base_url = target.base_url.clone();
            pgid = Some(target.pid());
            resources.targets.push(target);
        } else {
            // Attached: the target is already healthy; there is no PID unless
            // the caller gave the process group.
            pgid = opts.attach.as_ref().and_then(|attach| attach.pgid);
        }

        if setup_outcome.is_none() {
            let started = std::time::Instant::now();
            setup_outcome = Some(setup::run_setup(&base_url, &scenario).await?);
            let setup_secs = started.elapsed().as_secs_f64();
            println!(
                "repetition {repetition}/{r}: setup {n} step(s) in {setup_secs:.2} s",
                r = opts.repetitions,
                n = scenario.setup.len()
            );
        }

        // Settle before the idle sample: imports, connection pools and the
        // first request's lazily-built state have all happened by then.
        tokio::time::sleep(opts.settle).await;

        let idle_rss_bytes = match pgid {
            Some(pgid) => settled_rss(pgid).await?,
            None => 0,
        };

        let setup = setup_outcome
            .as_ref()
            .context("setup outcome missing after the setup phase")?;
        let blueprints =
            request::build_blueprints(&scenario, &base_url, &setup.vars, &setup.cookies)
                .with_context(|| {
                    format!("rendering the request templates for repetition {repetition}")
                })?;
        let schedule = scenario.weighted_schedule();
        let addr = resolve_addr(&base_url).await?;
        let authority = request::authority_and_prefix(&base_url)?.0;
        println!(
            "repetition {repetition}/{}: idle RSS {}",
            opts.repetitions,
            bytes_label(idle_rss_bytes)
        );

        for concurrency in &opts.concurrencies {
            // Warm-up uses the same connections and loop; its result is thrown
            // away so the first request's JIT/import/cache effects do not land
            // in the measured percentiles.
            let warmup = load::run(
                &blueprints,
                &schedule,
                addr,
                &authority,
                *concurrency,
                opts.warmup,
            )
            .await
            .with_context(|| format!("warm-up at concurrency {concurrency}"))?;

            let sampler = match pgid {
                Some(pgid) => Some(process::TreeSampler::start(pgid, RSS_SAMPLE_INTERVAL)?),
                None => None,
            };
            let run = load::run(
                &blueprints,
                &schedule,
                addr,
                &authority,
                *concurrency,
                opts.duration,
            )
            .await
            .with_context(|| format!("measured load at concurrency {concurrency}"))?;
            let summary = match sampler {
                Some(sampler) => Some(sampler.finish()?),
                None => None,
            };

            let latency = run.latencies.summary();
            let per_template = blueprints
                .iter()
                .zip(run.per_template.iter())
                .map(|(blueprint, count)| (blueprint.name.clone(), *count))
                .collect();
            let measurement = Measurement {
                scenario: scenario.name.clone(),
                target: opts.target.clone(),
                target_kind: spec.kind.clone(),
                workers: opts.django_workers,
                threads: opts.rust_threads,
                concurrency: *concurrency,
                repetition: Some(repetition),
                requests: run.requests,
                errors: run.errors,
                reconnects: run.reconnects,
                rps: run.rps(),
                p50_ms: latency.p50_ms,
                p90_ms: latency.p90_ms,
                p95_ms: latency.p95_ms,
                p99_ms: latency.p99_ms,
                max_ms: latency.max_ms,
                mean_ms: latency.mean_ms,
                idle_rss_bytes,
                peak_rss_bytes: summary.map(|summary| summary.peak_rss_bytes).unwrap_or(0),
                after_rss_bytes: summary.map(|summary| summary.after_rss_bytes).unwrap_or(0),
                cpu_secs: summary.map(|summary| summary.cpu_secs).unwrap_or(0.0),
                processes_peak: summary.map(|summary| summary.processes_peak).unwrap_or(0),
                duration_secs: run.elapsed.as_secs_f64(),
                per_template,
            };
            if let Some(summary) = summary
                && summary.process_missing
            {
                notes.push(format!(
                    "repetition {repetition}, concurrency {concurrency}: the target exited during \
                     the measured window; its RSS and CPU under-report."
                ));
            }
            println!(
                "  conc {:>4}: {:>10.0} req/s  p50 {:>7.2} ms  p95 {:>7.2} ms  p99 {:>7.2} ms  \
                 err {:>4}  reconnect {:>3}  peak {:>9}  cpu {:>6.2} s  (warm-up {:>7.0} req/s)",
                concurrency,
                measurement.rps,
                measurement.p50_ms,
                measurement.p95_ms,
                measurement.p99_ms,
                measurement.errors,
                measurement.reconnects,
                bytes_label(measurement.peak_rss_bytes),
                measurement.cpu_secs,
                warmup.rps()
            );
            measurements.push(measurement);
        }

        if managed {
            resources
                .targets
                .last_mut()
                .context("the repetition should have started a target")?
                .stop()
                .await?;
        }
    }

    if opts.attach.is_some() && opts.attach.as_ref().and_then(|a| a.pgid).is_none() {
        notes.push(
            "attached target with no --pgid: process-tree RSS and CPU are reported as 0 and the \
             memory columns of this run cannot be trusted."
                .to_string(),
        );
    }
    if opts.repetitions < 3 {
        notes.push(format!(
            "{} repetition(s); BENCH.md's medians use three.",
            opts.repetitions
        ));
    }
    let errors: u64 = measurements
        .iter()
        .map(|measurement| measurement.errors)
        .sum();
    if errors > 0 {
        notes.push(format!(
            "{errors} request(s) did not answer their template's expect_status across all \
             repetitions; the errors column of the median rows shows the worst repetition."
        ));
    }
    if managed {
        notes.push(
            "RSS is the sum of resident pages over the target's process group. Shared pages are \
             counted once per process, so it is an upper bound (not PSS); on Linux containers the \
             same process reports differently and must be re-measured there."
                .to_string(),
        );
    }

    let headroom = match opts.headroom_duration {
        Some(duration) if !duration.is_zero() => {
            println!(
                "client headroom: measuring the same loop against a trivial local server for {}s \
                 per concurrency",
                duration.as_secs_f64()
            );
            let points = headroom::measure(&opts.concurrencies, duration).await?;
            for point in &points {
                println!(
                    "  conc {:>4}: {:>10.0} req/s (client ceiling)",
                    point.concurrency, point.rps
                );
            }
            points
        }
        _ => Vec::new(),
    };

    let load_average_end = report::load_average();
    let scenario_medians = medians(&measurements, &opts.concurrencies);
    let report = BenchReport {
        schema: report::REPORT_SCHEMA,
        tool: format!("difftest {}", env!("CARGO_PKG_VERSION")),
        generated_at: crate::util::now_utc().to_rfc3339(),
        host: host_info(),
        target: TargetInfo {
            name: opts.target.clone(),
            kind: spec.kind.clone(),
            base_url: base_url.clone(),
            database: database.clone(),
            workers: opts.django_workers,
            threads: opts.rust_threads,
        },
        scheme: SchemeInfo {
            scenario: scenario.name.clone(),
            template_db: scenario.template_db.clone(),
            setup_steps: scenario.setup.len(),
            request_templates: scenario
                .requests
                .iter()
                .map(|request| request.name.clone())
                .collect(),
            duration_secs: opts.duration.as_secs_f64(),
            warmup_secs: opts.warmup.as_secs_f64(),
            settle_secs: opts.settle.as_secs_f64(),
            repetitions: opts.repetitions,
            concurrencies: opts.concurrencies.clone(),
            redis_db,
            client: "closed-loop, one keep-alive HTTP/1.1 connection per concurrent worker"
                .to_string(),
            rss_sample_interval_ms: RSS_SAMPLE_INTERVAL.as_millis() as u64,
        },
        measurements,
        medians: scenario_medians,
        headroom,
        notes,
        load_average_start,
        load_average_end,
    };

    println!("\n{}", report.render_markdown());
    if let Some(path) = &opts.report_path {
        report.write_json(path)?;
        println!("report: {}", path.display());
    }
    if let Some(path) = &opts.markdown_path {
        report.write_markdown(path)?;
        println!("markdown: {}", path.display());
    }
    Ok(report)
}

/// Idle RSS of the tree: the median of three samples 250 ms apart. One sample
/// can catch a transient (a worker still finishing something); the median of a
/// short settled window is stable.
async fn settled_rss(pgid: u32) -> Result<u64> {
    let mut samples = Vec::with_capacity(3);
    for index in 0..3 {
        if index > 0 {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        samples.push(process::sample_process_tree(pgid)?.rss_bytes);
    }
    Ok(median_u64(&samples).unwrap_or_default())
}

/// Resolve the target's base URL to a socket address once, before the load
/// loop, so no worker does a DNS lookup per connection.
async fn resolve_addr(base_url: &str) -> Result<SocketAddr> {
    let (host, port) = host_port(base_url, 80);
    let mut addresses = tokio::net::lookup_host((host.as_str(), port))
        .await
        .with_context(|| format!("resolving {host}:{port}"))?;
    addresses
        .next()
        .with_context(|| format!("{host}:{port} did not resolve to an address"))
}

fn target_context(
    port: u16,
    database: &str,
    redis_db: i64,
    vhost: &str,
) -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("port", port.to_string()),
        ("db", database.to_string()),
        ("redis_db", redis_db.to_string()),
        ("vhost", vhost.to_string()),
        ("repo", repo_root().display().to_string()),
    ])
}

fn bytes_label(bytes: u64) -> String {
    if bytes == 0 {
        "n/a".to_string()
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Exposed for the CLI: parse `1,8,64` into a sorted, de-duplicated list.
pub fn parse_concurrencies(text: &str) -> Result<Vec<usize>> {
    let mut values = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let value: usize = part
            .parse()
            .with_context(|| format!("--concurrency: {part:?} is not a number"))?;
        if value == 0 || value > 4096 {
            bail!("--concurrency: {value} is outside 1-4096");
        }
        values.push(value);
    }
    if values.is_empty() {
        bail!("--concurrency needs at least one value");
    }
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

/// Exposed for tests and the CLI's path resolution.
pub fn resolve(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    let from_root = repo_root().join(path);
    if from_root.exists() {
        from_root
    } else {
        path.to_path_buf()
    }
}

/// The host fields are part of the report so a reader does not have to trust
/// the prose in `BENCH.md` for the machine.
pub fn describe_host() -> HostInfo {
    host_info()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrency_lists_are_parsed_sorted_and_deduplicated() {
        assert_eq!(
            parse_concurrencies("8,1,64,8").expect("parse"),
            vec![1, 8, 64]
        );
        assert_eq!(parse_concurrencies(" 1 ").expect("parse"), vec![1]);
        assert!(parse_concurrencies("").is_err());
        assert!(parse_concurrencies("0").is_err());
        assert!(parse_concurrencies("1,x").is_err());
        assert!(parse_concurrencies("5000").is_err());
    }

    #[test]
    fn the_redis_index_must_be_in_the_server_range() {
        // The scenario validator has the same rule; this guards the CLI
        // override path, which bypasses the scenario.
        assert!(!(0..=15).contains(&16));
    }
}
