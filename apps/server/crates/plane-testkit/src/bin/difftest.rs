// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `difftest`: replay a recorded request session against two targets and
//! compare status, headers, bodies, database changes and enqueued Celery
//! messages.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use plane_testkit::bench::{AttachTarget, BenchOptions, parse_concurrencies};
use plane_testkit::config::Scenario;
use plane_testkit::db::{DbAdmin, measure_snapshot};
use plane_testkit::record::{RecordOptions, run as run_record};
use plane_testkit::runner::{AttachOptions, RunOptions, run};
use plane_testkit::selftest;
use plane_testkit::util::repo_root;

const DEFAULT_PG_URL: &str = "postgresql://plane:plane@127.0.0.1:55432/plane";
const DEFAULT_REDIS_URL: &str = "redis://127.0.0.1:56379";
const DEFAULT_RABBIT_AMQP: &str = "amqp://plane:plane@127.0.0.1:55672";
const DEFAULT_RABBIT_HTTP: &str = "http://127.0.0.1:55673";

#[derive(Debug, Parser)]
#[command(
    name = "difftest",
    about = "Differential HTTP/database/Celery test harness for the Plane backend rewrite"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Replay a scenario against two targets and compare every step.
    Run(Box<RunArgs>),
    /// Measure throughput, latency, memory and CPU of one target under load.
    Bench(Box<BenchArgs>),
    /// Record a browser session through a forwarding proxy into a scenario.
    Record(RecordArgs),
    /// Exercise the normaliser and comparator on built-in synthetic cases.
    Selftest,
    /// Print table/row counts and snapshot timing for a database.
    Snapshot(SnapshotArgs),
    /// Execute one Celery task on both backends and compare its effects.
    Task(Box<TaskArgs>),
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Scenario file (a recorded request session).
    #[arg(long)]
    scenario: PathBuf,
    /// Target name from targets.yaml for A (the reference).
    #[arg(long, default_value = "django")]
    a: String,
    /// Target name from targets.yaml for B (the candidate).
    #[arg(long, default_value = "django")]
    b: String,
    /// Keep the databases, RabbitMQ vhosts and target logs after the run.
    #[arg(long)]
    keep: bool,
    /// Write the full JSON report to this file.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Targets definition file.
    #[arg(long, default_value = "scenarios/targets.yaml")]
    targets: PathBuf,
    /// Normalisation table.
    #[arg(long, default_value = "scenarios/normalize.yaml")]
    normalize: PathBuf,
    #[arg(long, default_value = DEFAULT_PG_URL)]
    pg_url: String,
    #[arg(long, default_value = DEFAULT_REDIS_URL)]
    redis_url: String,
    #[arg(long, default_value = DEFAULT_RABBIT_AMQP)]
    rabbit_amqp: String,
    #[arg(long, default_value = DEFAULT_RABBIT_HTTP)]
    rabbit_http: String,
    #[arg(long, default_value = "plane")]
    rabbit_user: String,
    #[arg(long, default_value = "plane")]
    rabbit_password: String,
    /// Attach to an already-running target A at this base URL instead of
    /// starting one.
    #[arg(long)]
    a_url: Option<String>,
    #[arg(long)]
    b_url: Option<String>,
    /// Database used by an attached target A (no clone, no drop).
    #[arg(long)]
    a_db: Option<String>,
    #[arg(long)]
    b_db: Option<String>,
    /// Redis logical database for target A. Overrides the scenario's first
    /// index in managed and attach mode.
    #[arg(long)]
    a_redis_db: Option<i64>,
    /// Redis logical database for target B. Overrides the scenario's second
    /// index in managed and attach mode.
    #[arg(long)]
    b_redis_db: Option<i64>,
    /// RabbitMQ vhost used by an attached target A.
    #[arg(long)]
    a_vhost: Option<String>,
    #[arg(long)]
    b_vhost: Option<String>,
    /// Do not drain or compare Celery messages.
    #[arg(long)]
    no_celery: bool,
    /// Do not snapshot or compare Redis.
    #[arg(long)]
    no_redis: bool,
    /// Override a scenario variable for target A only (`name=value`).
    #[arg(long = "a-var", value_parser = parse_assignment)]
    a_vars: Vec<(String, String)>,
    /// Override a scenario variable for target B only (`name=value`).
    #[arg(long = "b-var", value_parser = parse_assignment)]
    b_vars: Vec<(String, String)>,
}

#[derive(Debug, Args)]
struct BenchArgs {
    /// Bench scenario file (`scenarios/bench/*.yaml`).
    #[arg(long)]
    scenario: PathBuf,
    /// Target name from targets.yaml (`django`, `rust`, ...).
    #[arg(long)]
    target: String,
    /// Concurrency levels to measure, for example `1,8,64`.
    #[arg(long, default_value = "1,8,64")]
    concurrency: String,
    /// Seconds each concurrency level is measured for.
    #[arg(long, default_value_t = 20.0)]
    duration: f64,
    /// Seconds of discarded warm-up before each measured window.
    #[arg(long, default_value_t = 5.0)]
    warmup: f64,
    /// Seconds to settle between starting the target and sampling its idle RSS.
    #[arg(long, default_value_t = 10.0)]
    settle: f64,
    /// How many independent repetitions to run; the report shows medians.
    #[arg(long, default_value_t = 3)]
    repetitions: u32,
    /// Use the release build of a managed target: rewrites `target/debug/` to
    /// `target/release/` in its command. A command with no debug path (a shell
    /// script, or a target already pointed at the release binary) is unchanged.
    #[arg(long)]
    release: bool,
    /// `GUNICORN_WORKERS` for the managed target (`targets.yaml` defaults it to 1).
    #[arg(long)]
    django_workers: Option<u32>,
    /// `PLANE_RS_WORKER_THREADS` for the managed target; omit for tokio's default.
    #[arg(long)]
    rust_threads: Option<u32>,
    /// Redis logical database to flush (default: the scenario's, else 3).
    #[arg(long)]
    redis_db: Option<i64>,
    /// Measure the client's own throughput too, this many seconds per level.
    #[arg(long, default_value_t = 5.0)]
    headroom_duration: f64,
    /// Keep the database, RabbitMQ vhost and target logs after the run.
    #[arg(long)]
    keep: bool,
    /// Write the JSON report to this file.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Write the Markdown table to this file.
    #[arg(long)]
    markdown: Option<PathBuf>,
    /// Targets definition file.
    #[arg(long, default_value = "scenarios/targets.yaml")]
    targets: PathBuf,
    /// Attach to an already-running target at this base URL instead of
    /// starting one (requires --db).
    #[arg(long)]
    url: Option<String>,
    /// Database the attached target serves (no clone, no drop).
    #[arg(long)]
    db: Option<String>,
    /// Process group of the attached target, for RSS/CPU measurement.
    #[arg(long)]
    pgid: Option<u32>,
    #[arg(long, default_value = DEFAULT_PG_URL)]
    pg_url: String,
    #[arg(long, default_value = DEFAULT_REDIS_URL)]
    redis_url: String,
    #[arg(long, default_value = DEFAULT_RABBIT_HTTP)]
    rabbit_http: String,
    #[arg(long, default_value = "plane")]
    rabbit_user: String,
    #[arg(long, default_value = "plane")]
    rabbit_password: String,
    /// Per-task comparison case: extra SQL queries, an S3 capture and a local
    /// fake HTTP server (`tools/jobs/fixtures/*.case.json`).
    #[arg(long)]
    case: Option<PathBuf>,
    /// Dev MinIO endpoint for the S3 capture.
    #[arg(long, default_value = "http://127.0.0.1:59000")]
    s3_endpoint: String,
    #[arg(long, default_value = "access-key")]
    s3_access_key: String,
    #[arg(long, default_value = "secret-key")]
    s3_secret_key: String,
    /// Bucket for the Django side; created, seeded and deleted by the tool.
    #[arg(long)]
    s3_bucket_a: Option<String>,
    /// Bucket for the Rust side; created, seeded and deleted by the tool.
    #[arg(long)]
    s3_bucket_b: Option<String>,
    /// `LIVE_BASE_URL` for both sides (`{{http}}` = the case's fake server).
    #[arg(long)]
    live_url: Option<String>,
}

#[derive(Debug, Args)]
struct RecordArgs {
    /// Address the recording proxy listens on.
    #[arg(long, default_value = "127.0.0.1:18090")]
    listen: String,
    /// Target the proxy forwards to, unchanged.
    #[arg(long)]
    upstream: String,
    /// Scenario file to write. Rewritten after every request.
    #[arg(long)]
    out: PathBuf,
    /// Scenario name written into the file.
    #[arg(long, default_value = "recorded")]
    name: String,
    /// Redis DB indexes written into the scenario, [A, B].
    #[arg(long, default_value = "1,2")]
    redis_db: String,
}

#[derive(Debug, Args)]
struct TaskArgs {
    /// Registered Celery task name, for example
    /// `plane.bgtasks.recent_visited_task.recent_visited_task`.
    #[arg(long)]
    task: String,
    /// JSON file holding the task's keyword arguments.
    #[arg(long)]
    kwargs: PathBuf,
    /// SQL applied to both database clones before the task runs.
    #[arg(long)]
    setup: Option<PathBuf>,
    /// Database for the Django side (recreated by the tool).
    #[arg(long, default_value = "t_p8p0_task_a")]
    a_db: String,
    /// Database for the Rust side (recreated by the tool).
    #[arg(long, default_value = "t_p8p0_task_b")]
    b_db: String,
    /// Redis logical database for Django.
    #[arg(long, default_value_t = 5)]
    a_redis_db: i64,
    /// Redis logical database for the worker.
    #[arg(long, default_value_t = 6)]
    b_redis_db: i64,
    /// RabbitMQ vhost the Django run publishes `.delay()` to (created and
    /// dropped by the tool).
    #[arg(long, default_value = "p8p0_task")]
    vhost: String,
    /// Keep the databases and the vhost after the run.
    #[arg(long)]
    keep: bool,
    /// Template database both sides are cloned from.
    #[arg(long, default_value = "plane_tpl")]
    template_db: String,
    /// `plane-worker` binary (default: `target/debug/plane-worker`).
    #[arg(long)]
    worker: Option<PathBuf>,
    /// The task is expected to fail on Django with this substring in its
    /// exception (pass the flag with no value to accept any failure).
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    expect_django_failure: Option<String>,
    /// The worker's job is expected to end `dead` with this substring in
    /// `last_error` (pass the flag with no value to accept any failure).
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    expect_worker_failure: Option<String>,
    #[arg(long, default_value = DEFAULT_PG_URL)]
    pg_url: String,
    #[arg(long, default_value = DEFAULT_RABBIT_AMQP)]
    rabbit_amqp: String,
    #[arg(long, default_value = DEFAULT_RABBIT_HTTP)]
    rabbit_http: String,
    #[arg(long, default_value = "plane")]
    rabbit_user: String,
    #[arg(long, default_value = "plane")]
    rabbit_password: String,
    /// Per-task comparison case: extra SQL queries, an S3 capture and a local
    /// fake HTTP server (`tools/jobs/fixtures/*.case.json`).
    #[arg(long)]
    case: Option<PathBuf>,
    /// Dev MinIO endpoint for the S3 capture.
    #[arg(long, default_value = "http://127.0.0.1:59000")]
    s3_endpoint: String,
    #[arg(long, default_value = "access-key")]
    s3_access_key: String,
    #[arg(long, default_value = "secret-key")]
    s3_secret_key: String,
    /// Bucket for the Django side; created, seeded and deleted by the tool.
    #[arg(long)]
    s3_bucket_a: Option<String>,
    /// Bucket for the Rust side; created, seeded and deleted by the tool.
    #[arg(long)]
    s3_bucket_b: Option<String>,
    /// `LIVE_BASE_URL` for both sides (`{{http}}` = the case's fake server).
    #[arg(long)]
    live_url: Option<String>,
}

#[derive(Debug, Args)]
struct SnapshotArgs {
    /// Database name to snapshot.
    #[arg(long)]
    db: String,
    /// Number of timed snapshots.
    #[arg(long, default_value_t = 3)]
    iterations: usize,
    #[arg(long, default_value = DEFAULT_PG_URL)]
    pg_url: String,
    /// Comma-separated schemas to include.
    #[arg(long, default_value = "public")]
    schemas: String,
}

fn parse_assignment(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((key, value)) if !key.is_empty() => Ok((key.to_string(), value.to_string())),
        _ => Err(format!("expected name=value, got {text:?}")),
    }
}

/// Resolve an input file relative to the current directory first, then to the
/// repository root, so the documented `cd apps/server && difftest ...` and a
/// run from the repository root both work.
fn resolve_input(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    let from_root = repo_root().join(path);
    if from_root.exists() {
        return from_root;
    }
    path.to_path_buf()
}

fn assignments(values: &[(String, String)]) -> BTreeMap<String, String> {
    values.iter().cloned().collect()
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match real_main(cli).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("difftest: {error:#}");
            ExitCode::from(2)
        }
    }
}

async fn real_main(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Selftest => {
            selftest::run()?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Bench(args) => {
            let concurrencies = parse_concurrencies(&args.concurrency)?;
            if !(args.duration.is_finite() && args.duration > 0.0) {
                bail!("--duration must be a positive number of seconds");
            }
            if !(args.warmup.is_finite() && args.warmup >= 0.0) {
                bail!("--warmup must be zero or more seconds");
            }
            if !(args.settle.is_finite() && args.settle >= 0.0) {
                bail!("--settle must be zero or more seconds");
            }
            if !(args.headroom_duration.is_finite() && args.headroom_duration >= 0.0) {
                bail!("--headroom-duration must be zero or more seconds");
            }
            let attach = match (&args.url, &args.db) {
                (Some(url), Some(db)) => Some(AttachTarget {
                    url: url.clone(),
                    database: db.clone(),
                    pgid: args.pgid,
                }),
                (None, None) => {
                    if args.pgid.is_some() {
                        bail!("--pgid only applies together with --url and --db");
                    }
                    if args.keep {
                        bail!("--keep only applies to a managed target");
                    }
                    None
                }
                _ => bail!("attach mode needs --url and --db together"),
            };
            let options = BenchOptions {
                scenario_path: resolve_input(&args.scenario),
                targets_path: resolve_input(&args.targets),
                target: args.target.clone(),
                concurrencies,
                duration: std::time::Duration::from_secs_f64(args.duration),
                warmup: std::time::Duration::from_secs_f64(args.warmup),
                settle: std::time::Duration::from_secs_f64(args.settle),
                repetitions: args.repetitions,
                release: args.release,
                django_workers: args.django_workers,
                rust_threads: args.rust_threads,
                redis_db: args.redis_db,
                keep: args.keep,
                report_path: args.report.clone(),
                markdown_path: args.markdown.clone(),
                headroom_duration: if args.headroom_duration > 0.0 {
                    Some(std::time::Duration::from_secs_f64(args.headroom_duration))
                } else {
                    None
                },
                pg_url: args.pg_url.clone(),
                redis_url: args.redis_url.clone(),
                rabbit_http: args.rabbit_http.clone(),
                rabbit_user: args.rabbit_user.clone(),
                rabbit_password: args.rabbit_password.clone(),
                attach,
            };
            plane_testkit::bench::run(options).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Record(args) => {
            let redis_db: Vec<i64> = args
                .redis_db
                .split(',')
                .map(|value| {
                    value
                        .trim()
                        .parse::<i64>()
                        .with_context(|| format!("parsing --redis-db value {value:?}"))
                })
                .collect::<Result<_>>()?;
            if redis_db.len() != 2 {
                bail!("--redis-db needs exactly two indexes, for example 1,2");
            }
            run_record(RecordOptions {
                listen: args.listen,
                upstream: args.upstream,
                out: args.out,
                name: args.name,
                redis_db,
            })
            .await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Task(args) => {
            let args = *args;
            let mut options =
                plane_testkit::task::TaskOptions::defaults(args.task, resolve_input(&args.kwargs));
            options.setup_path = args.setup.as_ref().map(|path| resolve_input(path));
            options.a_db = args.a_db;
            options.b_db = args.b_db;
            options.a_redis_db = args.a_redis_db;
            options.b_redis_db = args.b_redis_db;
            options.vhost = args.vhost;
            options.keep = args.keep;
            options.template_db = args.template_db;
            options.worker = args.worker.map(|path| resolve_input(&path));
            options.pg_url = args.pg_url;
            options.rabbit_amqp = args.rabbit_amqp;
            options.rabbit_http = args.rabbit_http;
            options.rabbit_user = args.rabbit_user;
            options.rabbit_password = args.rabbit_password;
            options.case_path = args.case.as_ref().map(|path| resolve_input(path));
            options.s3_endpoint = args.s3_endpoint;
            options.s3_access_key = args.s3_access_key;
            options.s3_secret_key = args.s3_secret_key;
            options.s3_bucket_a = args.s3_bucket_a;
            options.s3_bucket_b = args.s3_bucket_b;
            options.live_url = args.live_url;
            options.expect_django_failure = args.expect_django_failure;
            options.expect_worker_failure = args.expect_worker_failure;
            plane_testkit::task::run(&options).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Snapshot(args) => {
            let admin = DbAdmin::new(&args.pg_url);
            let schemas: Vec<String> = args
                .schemas
                .split(',')
                .map(|schema| schema.trim().to_string())
                .filter(|schema| !schema.is_empty())
                .collect();
            // `measure_snapshot` reads `public`; support a comma-separated
            // override by loading the catalog here and dumping rows ourselves
            // when it differs.
            if schemas == ["public"] {
                measure_snapshot(&admin, &args.db, args.iterations).await?;
            } else {
                let catalog = admin.load_catalog(&args.db, &schemas, &[]).await?;
                println!(
                    "database {}: {} tables in schemas {}",
                    args.db,
                    catalog.table_count(),
                    schemas.join(",")
                );
                let mut timings = Vec::new();
                let mut snapshot = None;
                for _ in 0..args.iterations.max(1) {
                    let result = admin.snapshot(&args.db, &catalog).await?;
                    timings.push(result.duration);
                    snapshot = Some(result.snapshot);
                }
                let total_rows: usize = snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.row_count())
                    .unwrap_or(0);
                println!("total rows: {total_rows}");
                let min = timings.iter().min().copied().unwrap_or_default();
                let max = timings.iter().max().copied().unwrap_or_default();
                let avg = timings
                    .iter()
                    .map(|duration| duration.as_secs_f64())
                    .sum::<f64>()
                    / timings.len() as f64;
                println!(
                    "snapshot timing over {} run(s): min {:.1} ms, avg {:.1} ms, max {:.1} ms",
                    timings.len(),
                    min.as_secs_f64() * 1000.0,
                    avg * 1000.0,
                    max.as_secs_f64() * 1000.0
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Run(args) => {
            let scenario_path = resolve_input(&args.scenario);
            let scenario = Scenario::load(&scenario_path)?;
            let attach = build_attach(&args, &scenario)?;
            let options = RunOptions {
                scenario_path,
                targets_path: resolve_input(&args.targets),
                normalize_path: resolve_input(&args.normalize),
                target_a: args.a.clone(),
                target_b: args.b.clone(),
                keep: args.keep,
                report_path: args.report.clone(),
                pg_url: args.pg_url.clone(),
                redis_url: args.redis_url.clone(),
                rabbit_amqp: args.rabbit_amqp.clone(),
                rabbit_http: args.rabbit_http.clone(),
                rabbit_user: args.rabbit_user.clone(),
                rabbit_password: args.rabbit_password.clone(),
                a_vars: assignments(&args.a_vars),
                b_vars: assignments(&args.b_vars),
                a_redis_db: args.a_redis_db,
                b_redis_db: args.b_redis_db,
                no_redis: args.no_redis,
                attach,
            };
            let report = run(options).await?;
            if let Some(path) = &args.report {
                report
                    .write(path)
                    .with_context(|| format!("writing {}", path.display()))?;
                println!("report: {}", path.display());
            }
            let failed = report.failed_steps().count();
            println!(
                "difftest: {}/{} steps ok in {:.1} s",
                report.steps.len() - failed,
                report.steps.len(),
                report.total_secs
            );
            for (label, stats) in &report.snapshot {
                println!(
                    "snapshot {label}: {} snapshots, avg {:.1} ms, max {:.1} ms",
                    stats.count, stats.average_ms, stats.max_ms
                );
            }
            for (label, stats) in &report.redis_snapshot {
                println!(
                    "redis snapshot {label}: {} snapshots, avg {:.2} ms, max {:.1} ms",
                    stats.count, stats.average_ms, stats.max_ms
                );
            }
            for target in &report.normalisation {
                let names: Vec<&str> = target.rules.iter().map(|rule| rule.rule.as_str()).collect();
                println!(
                    "normalisation {}: {} rule(s) applied{}",
                    target.target,
                    names.len(),
                    if names.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", names.join(", "))
                    }
                );
            }
            if failed == 0 {
                println!("difftest: all steps match");
                Ok(ExitCode::SUCCESS)
            } else {
                println!("difftest: {failed} step(s) differ");
                Ok(ExitCode::FAILURE)
            }
        }
    }
}

fn build_attach(args: &RunArgs, _scenario: &Scenario) -> Result<Option<AttachOptions>> {
    let any_attach =
        args.a_url.is_some() || args.b_url.is_some() || args.a_db.is_some() || args.b_db.is_some();
    if !any_attach {
        if args.no_celery {
            bail!("--no-celery is only meaningful together with --a-url/--b-url");
        }
        return Ok(None);
    }
    let (Some(a_url), Some(b_url), Some(a_db), Some(b_db)) =
        (&args.a_url, &args.b_url, &args.a_db, &args.b_db)
    else {
        bail!("attach mode needs --a-url, --b-url, --a-db and --b-db together");
    };
    let celery = !args.no_celery;
    let (a_vhost, b_vhost) = if celery {
        let (Some(a_vhost), Some(b_vhost)) = (&args.a_vhost, &args.b_vhost) else {
            bail!(
                "attach mode needs --a-vhost and --b-vhost so the harness can drain the targets' queues (or pass --no-celery)"
            );
        };
        (a_vhost.clone(), b_vhost.clone())
    } else {
        (
            args.a_vhost.clone().unwrap_or_default(),
            args.b_vhost.clone().unwrap_or_default(),
        )
    };
    Ok(Some(AttachOptions {
        a_url: a_url.trim_end_matches('/').to_string(),
        b_url: b_url.trim_end_matches('/').to_string(),
        a_db: a_db.clone(),
        b_db: b_db.clone(),
        a_vhost,
        b_vhost,
        celery,
    }))
}
