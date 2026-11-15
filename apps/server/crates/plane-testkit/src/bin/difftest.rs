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
    /// Record a browser session through a forwarding proxy into a scenario.
    Record(RecordArgs),
    /// Exercise the normaliser and comparator on built-in synthetic cases.
    Selftest,
    /// Print table/row counts and snapshot timing for a database.
    Snapshot(SnapshotArgs),
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
    #[arg(long)]
    a_redis_db: Option<i64>,
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

fn build_attach(args: &RunArgs, scenario: &Scenario) -> Result<Option<AttachOptions>> {
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
        a_redis_db: args.a_redis_db.unwrap_or(scenario.redis_db[0]),
        b_redis_db: args.b_redis_db.unwrap_or(scenario.redis_db[1]),
        a_vhost,
        b_vhost,
        celery,
    }))
}
