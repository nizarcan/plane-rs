// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Orchestration of one differential run.
//!
//! For each step the harness brackets the request with database snapshots,
//! drains the target's Celery queues, normalises every part, and compares.
//! Steps run in the recorded order with both targets advancing together, so
//! target state never depends on the order the harness happens to poll.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::celery::{
    CeleryDrain, CeleryMessage, RabbitAdmin, amqp_url_with_vhost, wait_management,
};
use crate::compare::{
    Difference, FormattingStatus, check_drf_formatting, compare_bodies, compare_celery, compare_db,
    compare_formatting, compare_headers, compare_redis, compare_status,
};
use crate::config::{CaptureSource, NormalizeRules, Scenario, Step, TargetsFile};
use crate::db::{DbAdmin, DbDiff, DbSnapshot, DbSnapshotter, diff_snapshots};
use crate::http::{HttpClient, HttpRequest, HttpResponse};
use crate::normalize::{
    BodyValue, GZIP_RANDOM_WHY, NormalizedBody, NormalizerConfig, TargetNormalizer,
    collect_known_uuids, content_type_is_json,
};
use crate::provision::{PortAllocator, TargetInstance, host_port, wait_tcp};
use crate::redis::{
    RedisAdmin, RedisDiff, RedisSnapshot, RedisSnapshotter, diff_snapshots as diff_redis_snapshots,
};
use crate::report::{ProbeReport, RunReport, SideReport, SnapshotStats, StepReport, TargetRules};
use crate::util::{ellipsize, new_run_id, now_utc, repo_root, validate_identifier};

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub scenario_path: PathBuf,
    pub targets_path: PathBuf,
    pub normalize_path: PathBuf,
    pub target_a: String,
    pub target_b: String,
    pub keep: bool,
    pub report_path: Option<PathBuf>,
    pub pg_url: String,
    pub redis_url: String,
    pub rabbit_amqp: String,
    pub rabbit_http: String,
    pub rabbit_user: String,
    pub rabbit_password: String,
    /// Per-target scenario variable overrides (`--a-var name=value`). Used to
    /// build a deliberately divergent B for the harness self-proof.
    pub a_vars: BTreeMap<String, String>,
    pub b_vars: BTreeMap<String, String>,
    /// Redis logical database overrides (`--a-redis-db`/`--b-redis-db`). They
    /// win over the scenario's `redis_db` in managed and attach mode alike.
    pub a_redis_db: Option<i64>,
    pub b_redis_db: Option<i64>,
    /// Do not snapshot or compare Redis.
    pub no_redis: bool,
    pub attach: Option<AttachOptions>,
}

#[derive(Debug, Clone)]
pub struct AttachOptions {
    pub a_url: String,
    pub b_url: String,
    pub a_db: String,
    pub b_db: String,
    pub a_vhost: String,
    pub b_vhost: String,
    pub celery: bool,
}

// ---------------------------------------------------------------------------
// Resources created by a run
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Resources {
    targets: Vec<TargetInstance>,
    databases: Vec<String>,
    vhosts: Vec<String>,
}

impl Resources {
    async fn cleanup(&mut self, opts: &RunOptions, run_id: &str) -> Result<()> {
        let mut errors = Vec::new();
        for target in &mut self.targets {
            if let Err(error) = target.stop().await {
                errors.push(format!("stopping target {}: {error:#}", target.name));
            }
        }
        if opts.keep {
            if !self.databases.is_empty() || !self.vhosts.is_empty() {
                println!(
                    "--keep: left databases [{}] and RabbitMQ vhosts [{}] for run {run_id}",
                    self.databases.join(", "),
                    self.vhosts.join(", ")
                );
            }
            return if errors.is_empty() {
                Ok(())
            } else {
                bail!(errors.join("; "))
            };
        }
        if !self.databases.is_empty() {
            let admin = DbAdmin::new(&opts.pg_url);
            for database in &self.databases {
                if let Err(error) = admin.drop_database(database).await {
                    errors.push(format!("dropping database {database}: {error:#}"));
                }
            }
        }
        if !self.vhosts.is_empty() {
            match RabbitAdmin::new(&opts.rabbit_http, &opts.rabbit_user, &opts.rabbit_password) {
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
        if errors.is_empty() {
            Ok(())
        } else {
            bail!(errors.join("; "))
        }
    }
}

// ---------------------------------------------------------------------------
// Per-target state
// ---------------------------------------------------------------------------

struct Side {
    target: String,
    base_url: String,
    database: String,
    vhost: String,
    http: HttpClient,
    vars: BTreeMap<String, String>,
    normalizer: TargetNormalizer,
    snapshotter: DbSnapshotter,
    last_snapshot: DbSnapshot,
    redis: Option<RedisSnapshotter>,
    last_redis: RedisSnapshot,
    celery: Option<CeleryDrain>,
    stats: SnapshotStats,
    redis_stats: SnapshotStats,
}

struct SideOutcome {
    status: u16,
    raw_body: Vec<u8>,
    duration: Duration,
    snapshot_ms: f64,
    redis_ms: f64,
    normalise_ms: f64,
    headers: Vec<(String, String)>,
    body: NormalizedBody,
    formatting: FormattingStatus,
    db_diff: DbDiff,
    redis_diff: RedisDiff,
    messages: Vec<CeleryMessage>,
    violations: Vec<crate::normalize::Violation>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run(opts: RunOptions) -> Result<RunReport> {
    let mut resources = Resources::default();
    let run_id = new_run_id();
    let outcome = run_inner(&opts, &run_id, &mut resources).await;
    let cleanup = resources.cleanup(&opts, &run_id).await;
    match (outcome, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Ok(report), Err(error)) => {
            eprintln!("difftest: cleanup problem: {error:#}");
            Ok(report)
        }
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            Err(error.context(format!("cleanup also failed: {cleanup_error:#}")))
        }
    }
}

async fn run_inner(
    opts: &RunOptions,
    run_id: &str,
    resources: &mut Resources,
) -> Result<RunReport> {
    let started_at = now_utc();
    let scenario = Scenario::load(&opts.scenario_path)?;
    let rules = Arc::new(NormalizeRules::load(&opts.normalize_path)?);
    let targets = TargetsFile::load(&opts.targets_path)?;
    let spec_a = targets.get(&opts.target_a)?.clone();
    let spec_b = targets.get(&opts.target_b)?.clone();

    // `--a-redis-db`/`--b-redis-db` override the scenario in both modes: the
    // flags select where the harness flushes and snapshots, so a scenario
    // shared with another agent can be run on private indexes without editing
    // it. In managed mode the value also renders `{redis_db}` into the target's
    // environment, because it is the same variable the harness uses.
    let plan = target_plan(
        &spec_a,
        &spec_b,
        &scenario,
        opts.a_redis_db,
        opts.b_redis_db,
    );
    let a_redis_db = plan.a_redis_db;
    let b_redis_db = plan.b_redis_db;

    println!(
        "difftest run {run_id}: scenario {} ({} steps), A={} ({}) B={} ({})",
        scenario.name,
        scenario.steps.len(),
        opts.target_a,
        spec_a.kind,
        opts.target_b,
        spec_b.kind
    );

    // Session payloads can only be verified with the target's own SECRET_KEY.
    // Fail before starting anything rather than reporting every session step
    // as a shape violation.
    let secret_key_a = spec_a.secret_key(&repo_root())?;
    let secret_key_b = spec_b.secret_key(&repo_root())?;
    if rules.requires_secret_key() {
        for (label, target) in [("A", &opts.target_a), ("B", &opts.target_b)] {
            let has_key = match label {
                "A" => secret_key_a.is_some(),
                _ => secret_key_b.is_some(),
            };
            if !has_key {
                bail!(
                    "normalize.yaml verifies session payloads, but target {label} ({target}) has no `secret_key_file`; \
                     add one to targets.yaml (for the dev stack: apps/server/dev/.env.local, variable SECRET_KEY)"
                );
            }
        }
    }

    let vars_a = side_vars(&scenario, &opts.a_vars);
    let vars_b = side_vars(&scenario, &opts.b_vars);
    let passwords_a = Arc::new(resolve_passwords(&scenario, &vars_a)?);
    let passwords_b = Arc::new(resolve_passwords(&scenario, &vars_b)?);

    let (pg_host, pg_port) = host_port(&opts.pg_url, 5432);
    let (redis_host, redis_port) = host_port(&opts.redis_url, 6379);
    let (amqp_host, amqp_port) = host_port(&opts.rabbit_amqp, 5672);
    let rabbit = Arc::new(RabbitAdmin::new(
        &opts.rabbit_http,
        &opts.rabbit_user,
        &opts.rabbit_password,
    )?);

    wait_tcp("Postgres", &pg_host, pg_port, Duration::from_secs(30)).await?;
    wait_tcp("Redis", &redis_host, redis_port, Duration::from_secs(30)).await?;
    wait_tcp(
        "RabbitMQ AMQP",
        &amqp_host,
        amqp_port,
        Duration::from_secs(30),
    )
    .await?;
    wait_management(&rabbit, Duration::from_secs(30)).await?;
    let redis = RedisAdmin::from_url(&opts.redis_url)?;
    redis.ping().await?;

    let db_admin = DbAdmin::new(&opts.pg_url);
    let mut known_uuids: HashSet<String> = HashSet::new();

    let (a_url, b_url, a_db, b_db, a_vhost, b_vhost) = if let Some(attach) = &opts.attach {
        (
            attach.a_url.clone(),
            attach.b_url.clone(),
            attach.a_db.clone(),
            attach.b_db.clone(),
            attach.a_vhost.clone(),
            attach.b_vhost.clone(),
        )
    } else {
        let a_db = format!("dt_{run_id}_a");
        let b_db = format!("dt_{run_id}_b");
        let a_vhost = format!("dt_{run_id}_a");
        let b_vhost = format!("dt_{run_id}_b");
        validate_identifier(&a_db)?;
        validate_identifier(&b_db)?;

        for (vhost, database, index) in
            [(&a_vhost, &a_db, a_redis_db), (&b_vhost, &b_db, b_redis_db)]
        {
            resources.vhosts.push(vhost.clone());
            rabbit.create_vhost(vhost).await?;
            rabbit
                .grant_full_permissions(vhost, &opts.rabbit_user)
                .await?;
            resources.databases.push(database.clone());
            redis.flush_db(index).await?;
        }

        // The template is marked ALLOW_CONNECTIONS false (dev/README.md), so
        // its UUIDs are read from the two clones' baselines below. They are
        // byte-identical to the template's, and the baselines also cover
        // attach mode, where the template is unknown.
        db_admin
            .clone_database(&scenario.template_db, &a_db)
            .await?;
        db_admin
            .clone_database(&scenario.template_db, &b_db)
            .await?;

        // `apps/server/target/` is gitignored by apps/server/.gitignore.
        let log_dir = repo_root()
            .join("apps/server/target")
            .join("difftest-logs")
            .join(run_id);
        // Scenario-level `target_env` (FILE_SIZE_LIMIT and friends) is merged
        // into each managed target's `env`, which is applied after `env_from`.
        // Attached targets are not started here and get it from their own
        // command line instead.
        let spec_a = spec_a.with_env(&scenario.target_env);
        let spec_b = spec_b.with_env(&scenario.target_env);
        // One allocator per target: B's `port_range` is its own declaration in
        // `targets.yaml`, not a second draw from A's range.
        let mut ports_a = PortAllocator::new(plan.a_ports.0, plan.a_ports.1);
        let mut ports_b = PortAllocator::new(plan.b_ports.0, plan.b_ports.1);
        let port_a = ports_a.allocate().await?;
        let port_a_context = target_context(port_a, &a_db, a_redis_db, &a_vhost);
        let mut target_a = TargetInstance::start("a", &spec_a, &port_a_context, &log_dir).await?;
        let port_b = ports_b.allocate().await?;
        let port_b_context = target_context(port_b, &b_db, b_redis_db, &b_vhost);
        let mut target_b = TargetInstance::start("b", &spec_b, &port_b_context, &log_dir).await?;
        let (health_a, health_b) = tokio::join!(
            target_a.wait_healthy(
                &spec_a.health,
                Duration::from_secs(spec_a.health_timeout_secs)
            ),
            target_b.wait_healthy(
                &spec_b.health,
                Duration::from_secs(spec_b.health_timeout_secs)
            ),
        );
        health_a?;
        health_b?;
        let a_url = target_a.base_url.clone();
        let b_url = target_b.base_url.clone();
        resources.targets.push(target_a);
        resources.targets.push(target_b);
        (a_url, b_url, a_db, b_db, a_vhost, b_vhost)
    };

    for ((label, database, index), url) in [("A", &a_db, a_redis_db), ("B", &b_db, b_redis_db)]
        .into_iter()
        .zip([&a_url, &b_url])
    {
        if opts.attach.is_some() {
            redis.flush_db(index).await?;
        }
        // The run header names the endpoint and the Redis index every step will
        // use: a managed run's ports come from the target's `port_range`, and
        // `--a-redis-db`/`--b-redis-db` can move the index away from the
        // scenario's.
        println!("target {label}: {database} at {url} (redis db {index})");
    }

    // The scenario's fixture, if any, is applied to both clones before the
    // baseline snapshot, so the fixture rows are part of the baseline and never
    // appear in a step diff. `/auth/` and `/api/` are not ported yet, so a
    // scenario that needs users, sessions or objects cannot create them over
    // HTTP; `tools/policy/gen_fixture.py` writes the deterministic SQL.
    if let Some(path) = scenario.fixture_sql() {
        let sql = std::fs::read_to_string(&path)
            .with_context(|| format!("reading the fixture {}", path.display()))?;
        for (label, database) in [("A", &a_db), ("B", &b_db)] {
            let client = db_admin.connect(database).await?;
            client.batch_execute(&sql).await.with_context(|| {
                format!(
                    "applying the fixture {} to target {label} ({database})",
                    path.display()
                )
            })?;
            println!("fixture: applied {} to target {label}", path.display());
        }
    }

    let (catalog_a, catalog_b) = tokio::try_join!(
        db_admin.load_catalog(&a_db, &rules.db_schemas, &rules.db_exclude_tables),
        db_admin.load_catalog(&b_db, &rules.db_schemas, &rules.db_exclude_tables),
    )?;
    let snapshotter_a = db_admin.snapshotter(&a_db, catalog_a).await?;
    let snapshotter_b = db_admin.snapshotter(&b_db, catalog_b).await?;
    let (baseline_a, baseline_b) =
        tokio::try_join!(snapshotter_a.snapshot(), snapshotter_b.snapshot())?;
    println!(
        "baseline: A {} rows ({:.1} ms), B {} rows ({:.1} ms)",
        baseline_a.snapshot.row_count(),
        baseline_a.duration.as_secs_f64() * 1000.0,
        baseline_b.snapshot.row_count(),
        baseline_b.duration.as_secs_f64() * 1000.0
    );
    for snapshot in [&baseline_a.snapshot, &baseline_b.snapshot] {
        known_uuids.extend(collect_known_uuids(
            snapshot
                .tables
                .values()
                .flat_map(|rows| rows.values().cloned()),
        ));
    }
    let known_uuids = Arc::new(known_uuids);

    let mut redis_a = if opts.no_redis {
        None
    } else {
        Some(redis.snapshotter(a_redis_db).await?)
    };
    let mut redis_b = if opts.no_redis {
        None
    } else {
        Some(redis.snapshotter(b_redis_db).await?)
    };
    let (baseline_redis_a, baseline_redis_b) = match (&mut redis_a, &mut redis_b) {
        (Some(a), Some(b)) => {
            let (a, b) = tokio::join!(a.snapshot(), b.snapshot());
            let (a, b) = (a?, b?);
            println!(
                "redis baseline: A {} keys ({:.1} ms), B {} keys ({:.1} ms)",
                a.snapshot.key_count(),
                a.duration.as_secs_f64() * 1000.0,
                b.snapshot.key_count(),
                b.duration.as_secs_f64() * 1000.0
            );
            (a, b)
        }
        _ => (
            crate::redis::SnapshotResult {
                snapshot: RedisSnapshot::default(),
                duration: Duration::ZERO,
            },
            crate::redis::SnapshotResult {
                snapshot: RedisSnapshot::default(),
                duration: Duration::ZERO,
            },
        ),
    };

    let mut side_a = Side {
        target: spec_a.kind.clone(),
        base_url: a_url.clone(),
        database: a_db.clone(),
        vhost: a_vhost.clone(),
        http: HttpClient::new(&a_url)?,
        vars: vars_a,
        normalizer: TargetNormalizer::new(NormalizerConfig {
            rules: Arc::clone(&rules),
            base_url: a_url.clone(),
            known_uuids: Arc::clone(&known_uuids),
            secret_key: secret_key_a.map(Arc::new),
            passwords: passwords_a,
        }),
        snapshotter: snapshotter_a,
        last_snapshot: baseline_a.snapshot,
        redis: redis_a,
        last_redis: baseline_redis_a.snapshot,
        celery: None,
        stats: SnapshotStats::default(),
        redis_stats: SnapshotStats::default(),
    };
    let mut side_b = Side {
        target: spec_b.kind.clone(),
        base_url: b_url.clone(),
        database: b_db.clone(),
        vhost: b_vhost.clone(),
        http: HttpClient::new(&b_url)?,
        vars: vars_b,
        normalizer: TargetNormalizer::new(NormalizerConfig {
            rules: Arc::clone(&rules),
            base_url: b_url.clone(),
            known_uuids: Arc::clone(&known_uuids),
            secret_key: secret_key_b.map(Arc::new),
            passwords: passwords_b,
        }),
        snapshotter: snapshotter_b,
        last_snapshot: baseline_b.snapshot,
        redis: redis_b,
        last_redis: baseline_redis_b.snapshot,
        celery: None,
        stats: SnapshotStats::default(),
        redis_stats: SnapshotStats::default(),
    };
    side_a
        .stats
        .record(baseline_a.duration.as_secs_f64() * 1000.0);
    side_b
        .stats
        .record(baseline_b.duration.as_secs_f64() * 1000.0);
    if !opts.no_redis {
        side_a
            .redis_stats
            .record(baseline_redis_a.duration.as_secs_f64() * 1000.0);
        side_b
            .redis_stats
            .record(baseline_redis_b.duration.as_secs_f64() * 1000.0);
    }

    let celery_enabled = match &opts.attach {
        Some(attach) => attach.celery,
        None => true,
    };
    if celery_enabled {
        let url_a = amqp_url_with_vhost(
            &opts.rabbit_amqp,
            &a_vhost,
            &opts.rabbit_user,
            &opts.rabbit_password,
        )?;
        let url_b = amqp_url_with_vhost(
            &opts.rabbit_amqp,
            &b_vhost,
            &opts.rabbit_user,
            &opts.rabbit_password,
        )?;
        side_a.celery = Some(CeleryDrain::connect(&url_a).await?);
        side_b.celery = Some(CeleryDrain::connect(&url_b).await?);
    }

    let mut step_reports = Vec::with_capacity(scenario.steps.len());
    let mut all_ok = true;
    let mut probe_compared_denials = 0usize;
    let mut probe_reached_handler = 0usize;
    let mut probe_mismatches = 0usize;
    // Redis keys introduced by a reached-handler step, per target. See the
    // pass-through branch below.
    let mut ignored_redis_a: std::collections::BTreeSet<Vec<u8>> =
        std::collections::BTreeSet::new();
    let mut ignored_redis_b: std::collections::BTreeSet<Vec<u8>> =
        std::collections::BTreeSet::new();
    for (index, step) in scenario.steps.iter().enumerate() {
        let number = index + 1;
        let (a_result, b_result) = tokio::join!(
            execute_step(&mut side_a, step, &scenario, Some(rabbit.as_ref())),
            execute_step(&mut side_b, step, &scenario, Some(rabbit.as_ref()))
        );
        let mut a_outcome =
            a_result.with_context(|| format!("step {number} ({}) on target A", step.name))?;
        let mut b_outcome =
            b_result.with_context(|| format!("step {number} ({}) on target B", step.name))?;

        // gzip output length depends on the random filename Django adds and on
        // the ASGI worker's chunk boundaries, so content-length is only stable
        // for bodies that were never compressed. It is still compared whenever
        // the bodies were not encoded and needed no normalisation.
        let mut a_headers = a_outcome.headers.clone();
        let mut b_headers = b_outcome.headers.clone();
        let random_encoding = content_encoding_is_random(&a_outcome.raw_body)
            || content_encoding_is_random(&b_outcome.raw_body);
        if random_encoding {
            normalise_content_encoding(&mut a_headers);
            normalise_content_encoding(&mut b_headers);
            side_a
                .normalizer
                .note_rule("content-encoding-random", GZIP_RANDOM_WHY);
            side_b
                .normalizer
                .note_rule("content-encoding-random", GZIP_RANDOM_WHY);
        }
        let compressed = a_headers
            .iter()
            .chain(b_headers.iter())
            .any(|(name, _)| name.eq_ignore_ascii_case("content-encoding"));
        let keep_content_length =
            !a_outcome.body.modified && !b_outcome.body.modified && !compressed;
        let a_headers = filter_headers(&a_headers, &rules, keep_content_length);
        let b_headers = filter_headers(&b_headers, &rules, keep_content_length);

        // The policy probe's pass-through rule. A B response carrying
        // `X-Plane-RS-Probe: handler` means the Rust runtime ran the full
        // policy and then answered 501 without running the ported handler.
        // * If A denied the request (401/403/405/406/415/429) the step is a
        //   DIFF: Rust let through what Django refused.
        // * Otherwise the step is `reached-handler`: the response and the
        //   DB/Redis/Celery side effects are not compared, because B has no
        //   handler while A's real view may write. Semantic violations are
        //   still reported (they are never about the probe).
        // * Every other step is compared exactly as usual.
        let probe_reached_b = has_probe_header(&b_outcome.headers);
        let django_denied = is_policy_denial(a_outcome.status, &a_outcome.raw_body);
        let probe = if probe_reached_b {
            Some(ProbeReport {
                reached_handler: !django_denied,
                django_denied,
            })
        } else {
            None
        };

        let mut differences = Vec::new();
        if probe_reached_b && django_denied {
            probe_mismatches += 1;
            differences.push(Difference::new(
                "probe",
                "B",
                format!(
                    "A denied with {} ({}) but B reached the probe handler; Rust let \
                     through a request Django refused",
                    a_outcome.status,
                    body_excerpt(&a_outcome.raw_body)
                ),
            ));
        } else if probe_reached_b {
            probe_reached_handler += 1;
            // A ran its real handler (B did not), so any Redis key it wrote or
            // re-wrote is a side effect of an ignored step. Remember those keys
            // and drop them from later steps: their TTL keeps decreasing, so
            // every later snapshot would report them as "changed" on A alone.
            for key in a_outcome
                .redis_diff
                .added
                .keys()
                .chain(a_outcome.redis_diff.changed.keys())
            {
                ignored_redis_a.insert(key.clone());
            }
            for key in b_outcome
                .redis_diff
                .added
                .keys()
                .chain(b_outcome.redis_diff.changed.keys())
            {
                ignored_redis_b.insert(key.clone());
            }
            // Normalised for the report only; nothing is compared here.
            side_a
                .normalizer
                .normalize_redis_diff(&mut a_outcome.redis_diff);
            side_b
                .normalizer
                .normalize_redis_diff(&mut b_outcome.redis_diff);
            a_outcome
                .violations
                .extend(side_a.normalizer.take_violations());
            b_outcome
                .violations
                .extend(side_b.normalizer.take_violations());
            if scenario.probe_compare_celery {
                // `APITokenLogMiddleware` runs below the handler on both
                // targets, so its `process_logs` message is compared even
                // though the handler is not. The two `log_data` fields that
                // copy the response (`response_code`, `response_body`) are
                // masked: the pass-through rule already ignores that response.
                let mut a_messages = a_outcome.messages.clone();
                let mut b_messages = b_outcome.messages.clone();
                for message in a_messages.iter_mut().chain(b_messages.iter_mut()) {
                    mask_pass_through_response(message);
                }
                differences.extend(compare_celery(&a_messages, &b_messages));
            }
        } else {
            // Raw keys first: the ignore set holds the raw keys a pass-through
            // step wrote, and only then are the remaining entries normalised.
            filter_ignored_redis(&mut a_outcome.redis_diff, &ignored_redis_a);
            filter_ignored_redis(&mut b_outcome.redis_diff, &ignored_redis_b);
            side_a
                .normalizer
                .normalize_redis_diff(&mut a_outcome.redis_diff);
            side_b
                .normalizer
                .normalize_redis_diff(&mut b_outcome.redis_diff);
            a_outcome
                .violations
                .extend(side_a.normalizer.take_violations());
            b_outcome
                .violations
                .extend(side_b.normalizer.take_violations());
            if django_denied {
                probe_compared_denials += 1;
            }
            differences.extend(compare_status(a_outcome.status, b_outcome.status));
            differences.extend(compare_headers(&a_headers, &b_headers));
            differences.extend(compare_bodies(&a_outcome.body, &b_outcome.body));
            differences.extend(compare_formatting(
                &a_outcome.formatting,
                &b_outcome.formatting,
            ));
            differences.extend(compare_db(&a_outcome.db_diff, &b_outcome.db_diff));
            differences.extend(compare_redis(&a_outcome.redis_diff, &b_outcome.redis_diff));
            differences.extend(compare_celery(&a_outcome.messages, &b_outcome.messages));
        }
        // A violation is a difference on its own: a target that writes an
        // unverifiable session or password hash cannot be rescued by another
        // target that does the same thing. A reached-handler step is exempt:
        // its violation can only be about the output of A's real handler
        // (a new API token's shape, a response body), which B's probe never
        // produces. Every fixture user's session is exercised by compared
        // steps too, so nothing is left unchecked.
        if !probe_reached_b || django_denied {
            for (label, outcome) in [("A", &a_outcome), ("B", &b_outcome)] {
                for violation in &outcome.violations {
                    differences.push(Difference::new(
                        violation.part.clone(),
                        violation.path.clone(),
                        format!("{label}: {}", violation.message),
                    ));
                }
            }
        }
        if let (Some(expected), None) = (step.expect_status, &probe) {
            for (label, outcome) in [("A", &a_outcome), ("B", &b_outcome)] {
                if outcome.status != expected {
                    differences.push(Difference::new(
                        "expect_status",
                        label,
                        format!("scenario expected {expected}, got {}", outcome.status),
                    ));
                }
            }
        }

        let ok = differences.is_empty();
        all_ok &= ok;
        if ok && probe.as_ref().is_some_and(|probe| probe.reached_handler) {
            println!(
                "  pass {number:02} {} {} (reached-handler, not compared; db {:.0} ms)",
                step.method, step.path, a_outcome.snapshot_ms
            );
        } else if ok {
            println!(
                "  ok   {number:02} {} {} ({:.0} ms, db {:.0} ms, redis {:.1} ms, norm {:.1} ms)",
                step.method,
                step.path,
                a_outcome.duration.as_secs_f64() * 1000.0,
                a_outcome.snapshot_ms,
                a_outcome.redis_ms,
                a_outcome.normalise_ms
            );
        } else {
            println!(
                "  DIFF {number:02} {} {} — {} difference(s)",
                step.method,
                step.path,
                differences.len()
            );
            for difference in &differences {
                let rendered = difference.render();
                for line in rendered.lines() {
                    println!("       {line}");
                }
            }
        }

        let side_report = |side: &Side, outcome: &SideOutcome| SideReport {
            target: side.target.clone(),
            url: side.base_url.clone(),
            database: side.database.clone(),
            status: outcome.status,
            duration_ms: outcome.duration.as_secs_f64() * 1000.0,
            snapshot_ms: outcome.snapshot_ms,
            redis_ms: outcome.redis_ms,
            normalise_ms: outcome.normalise_ms,
            headers: headers_to_map(&filter_headers(&outcome.headers, &rules, true)),
            body: match &outcome.body.value {
                BodyValue::Json(_) | BodyValue::Text(_) => {
                    Some(ellipsize(&outcome.body.pretty(), 20_000))
                }
                BodyValue::Binary(_) => Some(ellipsize(&outcome.body.pretty(), 20_000)),
            },
            body_modified: outcome.body.modified,
            formatting: match &outcome.formatting {
                FormattingStatus::NotJson => "not-json".to_string(),
                FormattingStatus::Pass => "pass".to_string(),
                FormattingStatus::Fail(error) => format!("fail: {error}"),
            },
            db: serde_json::to_value(&outcome.db_diff).unwrap_or(Value::Null),
            redis: outcome.redis_diff.to_json(),
            celery: serde_json::to_value(&outcome.messages).unwrap_or(Value::Null),
        };

        step_reports.push(StepReport {
            index: number,
            name: step.name.clone(),
            method: step.method.clone(),
            path: step.path.clone(),
            a: side_report(&side_a, &a_outcome),
            b: side_report(&side_b, &b_outcome),
            differences,
            ok,
            probe,
        });
    }

    let finished_at = now_utc();
    let mut snapshot = BTreeMap::new();
    snapshot.insert("A".to_string(), side_a.stats.clone());
    snapshot.insert("B".to_string(), side_b.stats.clone());
    let mut redis_snapshot = BTreeMap::new();
    redis_snapshot.insert("A".to_string(), side_a.redis_stats.clone());
    redis_snapshot.insert("B".to_string(), side_b.redis_stats.clone());
    println!(
        "probe: {probe_compared_denials} denial(s) compared, \
         {probe_reached_handler} reached-handler, {probe_mismatches} mismatch(es), \
         {} step(s) total",
        step_reports.len()
    );
    println!(
        "placeholders: A {} value(s), B {} value(s)",
        side_a.normalizer.placeholder_count(),
        side_b.normalizer.placeholder_count()
    );
    let report = RunReport {
        run_id: run_id.to_string(),
        scenario: scenario.name.clone(),
        targets: BTreeMap::from([
            (
                "A".to_string(),
                format!("{} ({})", opts.target_a, spec_a.kind),
            ),
            (
                "B".to_string(),
                format!("{} ({})", opts.target_b, spec_b.kind),
            ),
        ]),
        a_url: a_url.clone(),
        b_url: b_url.clone(),
        a_db: a_db.clone(),
        b_db: b_db.clone(),
        started_at: started_at.to_rfc3339(),
        finished_at: finished_at.to_rfc3339(),
        total_secs: (finished_at - started_at).num_milliseconds() as f64 / 1000.0,
        snapshot,
        redis_snapshot,
        normalisation: vec![
            TargetRules {
                target: "A".to_string(),
                rules: side_a.normalizer.applied(),
            },
            TargetRules {
                target: "B".to_string(),
                rules: side_b.normalizer.applied(),
            },
        ],
        steps: step_reports,
        probe_compared_denials,
        probe_reached_handler,
        probe_mismatches,
        ok: all_ok,
    };
    Ok(report)
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

/// The choices managed mode makes before it starts either target: which port
/// range each side allocates from and which Redis index each side uses.
///
/// Kept separate from [`run_inner`] so the regression test can check both
/// without a database, Redis, broker or process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TargetPlan {
    a_ports: (u16, u16),
    b_ports: (u16, u16),
    a_redis_db: i64,
    b_redis_db: i64,
}

/// `spec_a`'s range and indexes for A, `spec_b`'s for B; the CLI overrides the
/// scenario's `redis_db` per side.
fn target_plan(
    spec_a: &crate::config::TargetSpec,
    spec_b: &crate::config::TargetSpec,
    scenario: &Scenario,
    a_redis_db: Option<i64>,
    b_redis_db: Option<i64>,
) -> TargetPlan {
    TargetPlan {
        a_ports: (spec_a.port_range[0], spec_a.port_range[1]),
        b_ports: (spec_b.port_range[0], spec_b.port_range[1]),
        a_redis_db: redis_index(a_redis_db, &scenario.redis_db, 0),
        b_redis_db: redis_index(b_redis_db, &scenario.redis_db, 1),
    }
}

/// The Redis index one side uses: the CLI override when given, else the
/// scenario's `[A, B]` entry. Side 0 is A, side 1 is B.
fn redis_index(override_value: Option<i64>, scenario: &[i64], side: usize) -> i64 {
    // `Scenario::validate` guarantees two entries; `get` keeps this a total
    // function for a scenario built in a test.
    override_value.unwrap_or_else(|| scenario.get(side).copied().unwrap_or_default())
}

fn side_vars(
    scenario: &Scenario,
    overrides: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut vars = scenario.vars.clone();
    for (key, value) in overrides {
        vars.insert(key.clone(), value.clone());
    }
    vars
}

/// Django's `compress_string` adds a random 1-100 byte filename to the gzip
/// header, and `GZipMiddleware` only keeps the compressed body when it is
/// shorter than the original. For a body near the 200-byte threshold the
/// decision is therefore random. Include the worst-case 102-byte filename and
/// terminator (`django/utils/text.py:351`).
fn content_encoding_is_random(raw: &[u8]) -> bool {
    if raw.len() < 200 {
        return false;
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
    if std::io::Write::write_all(&mut encoder, raw).is_err() {
        return true;
    }
    let Ok(compressed) = encoder.finish() else {
        return true;
    };
    compressed.len() + 102 >= raw.len()
}

fn normalise_content_encoding(headers: &mut Vec<(String, String)>) {
    const MARKER: &str = "<gzip-or-identity>";
    match headers
        .iter_mut()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
    {
        Some((_, value)) => *value = MARKER.to_string(),
        None => headers.push(("Content-Encoding".to_string(), MARKER.to_string())),
    }
}

fn headers_to_map(headers: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, value) in headers {
        map.entry(name.to_ascii_lowercase())
            .or_default()
            .push(value.clone());
    }
    map
}

fn filter_headers(
    headers: &[(String, String)],
    rules: &NormalizeRules,
    keep_content_length: bool,
) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| {
            if name.eq_ignore_ascii_case("content-length") {
                return keep_content_length;
            }
            !rules.ignore_header(name)
        })
        .cloned()
        .collect()
}

/// Drop Redis diff entries whose key was written by an ignored (reached-handler)
/// step earlier in the run.
fn filter_ignored_redis(
    diff: &mut crate::redis::RedisDiff,
    ignored: &std::collections::BTreeSet<Vec<u8>>,
) {
    if ignored.is_empty() {
        return;
    }
    diff.added.retain(|key, _| !ignored.contains(key));
    diff.changed.retain(|key, _| !ignored.contains(key));
    diff.removed.retain(|key| !ignored.contains(key));
}

/// A short, printable excerpt of a response body for a difference message.
fn body_excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim();
    if trimmed.chars().count() > 120 {
        let mut excerpt: String = trimmed.chars().take(120).collect();
        excerpt.push('…');
        excerpt
    } else {
        trimmed.to_string()
    }
}

/// Mask the two `log_data` fields a probe pass-through step cannot compare:
/// they are copies of the response, and the response itself is not compared on
/// such a step.
///
/// `kwargsrepr` is Celery's Python `repr` of the same `kwargs` dict; it is
/// dropped because it embeds those two values as text and cannot be re-rendered
/// here. The structured `kwargs` it mirrors is compared, so a difference in the
/// rest of the message still fails the step. A compared (non-pass-through)
/// step compares `kwargsrepr` in full.
fn mask_pass_through_response(message: &mut crate::celery::CeleryMessage) {
    const MASK: &str = "<pass-through response>";
    message.headers.remove("kwargsrepr");
    let Value::Object(kwargs) = &mut message.kwargs else {
        return;
    };
    let Some(Value::Object(log_data)) = kwargs.get_mut("log_data") else {
        return;
    };
    for key in ["response_code", "response_body"] {
        if let Some(value) = log_data.get_mut(key) {
            *value = Value::String(MASK.to_string());
        }
    }
}

/// `X-Plane-RS-Probe: handler`, set by the `policy-probe` feature's handler.
fn has_probe_header(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("x-plane-rs-probe") && value == "handler")
}

/// Whether A's response is a **policy** denial, i.e. produced before the view
/// body ran.
///
/// 401 is only produced by authentication. 405 is the `BaseViewSet` guard and
/// method dispatch, 406 content negotiation, 415 a parser, 429 a throttle.
///
/// 403 is ambiguous: permission classes, the CSRF/origin checks and
/// `allow_permission` produce a small set of exact bodies, but a handler may
/// also answer 403 itself (`plane/app/views/asset/v2.py` does, for an asset the
/// caller cannot see). Treating every 403 as policy would turn Django's own
/// handler response into a false DIFF, so the known policy bodies are matched
/// exactly and anything else is a handler answer. This narrows the check to
/// what it claims to test — it never lets a policy difference through, because
/// a Rust policy that allows what Django's policy denied produces one of the
/// policy bodies.
fn is_policy_denial(status: u16, body: &[u8]) -> bool {
    match status {
        401 | 405 | 406 | 415 | 429 => true,
        403 => {
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
                return false;
            };
            // `allow_permission` returns exactly this body. The policy probe
            // does not send requests to routes whose *handler* can return it
            // too (see `gen_policy_scenario.py`), so seeing it here means the
            // policy denied.
            if value.get("error").and_then(|value| value.as_str())
                == Some("You don't have the required permissions.")
            {
                return true;
            }
            let Some(detail) = value.get("detail").and_then(|value| value.as_str()) else {
                return false;
            };
            matches!(
                detail,
                "You do not have permission to perform this action."
                    | "Authentication credentials were not provided."
                    | "CSRF Failed: origin not allowed"
                    | "You are not allowed to view this issue"
            ) || detail.starts_with("CSRF ")
                || detail.starts_with("Origin checking failed")
                || detail.starts_with("Referer checking failed")
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// One step on one target
// ---------------------------------------------------------------------------

async fn execute_step(
    side: &mut Side,
    step: &Step,
    scenario: &Scenario,
    rabbit: Option<&RabbitAdmin>,
) -> Result<SideOutcome> {
    let before = std::mem::take(&mut side.last_snapshot);
    let before_redis = std::mem::take(&mut side.last_redis);
    let request = build_request(side, step, scenario)?;
    let started: DateTime<Utc> = now_utc();
    let response = side
        .http
        .send(&request)
        .await
        .with_context(|| format!("{} {}", step.method, request.path_and_query))?;
    let ended: DateTime<Utc> = now_utc();

    let snapshot = side.snapshotter.snapshot().await?;
    let snapshot_ms = snapshot.duration.as_secs_f64() * 1000.0;
    side.stats.record(snapshot_ms);
    let after = snapshot.snapshot;

    let (after_redis, redis_ms) = match &mut side.redis {
        Some(sampler) => {
            let result = sampler
                .snapshot()
                .await
                .with_context(|| format!("snapshotting Redis db {}", sampler.index))?;
            let millis = result.duration.as_secs_f64() * 1000.0;
            side.redis_stats.record(millis);
            (result.snapshot, millis)
        }
        None => (RedisSnapshot::default(), 0.0),
    };

    let mut messages = Vec::new();
    if let (Some(rabbit), Some(drain)) = (rabbit, &side.celery) {
        let queues = rabbit
            .queues(&side.vhost)
            .await
            .with_context(|| format!("listing queues for vhost {}", side.vhost))?;
        messages = drain
            .drain(&queues)
            .await
            .with_context(|| format!("draining {} queue(s)", queues.len()))?;
    }

    let captures = collect_captures(side, step, &response)?;

    // Normalisation order is fixed: headers (Location first), body, database,
    // Celery, Redis. Placeholder numbering depends on it.
    let normalise_started = Instant::now();
    side.normalizer.start_step(started, ended);
    let headers = side.normalizer.normalize_headers(&response.headers);
    let json = content_type_is_json(&response.headers);
    let body = side.normalizer.normalize_body(&response.body, json);
    let mut db_diff = diff_snapshots(&before, &after);
    side.normalizer.normalize_db_diff(&mut db_diff);
    for message in &mut messages {
        side.normalizer.normalize_celery(message);
    }
    // Redis is left raw: the caller filters the keys a pass-through step
    // introduced **before** normalising. A page-cache placeholder contains its
    // TTL bucket (`:1:<page-cache:716:1>`), which changes as the entry ages, so
    // a placeholder recorded from one step would never match the next step's
    // name and the ignore list would stop working after ten seconds.
    let redis_diff = diff_redis_snapshots(&before_redis, &after_redis);
    let violations = side.normalizer.take_violations();
    let normalise_ms = normalise_started.elapsed().as_secs_f64() * 1000.0;

    let formatting = if json {
        match check_drf_formatting(&response.body) {
            Ok(()) => FormattingStatus::Pass,
            Err(error) => FormattingStatus::Fail(error),
        }
    } else {
        FormattingStatus::NotJson
    };

    side.last_snapshot = after;
    side.last_redis = after_redis;
    for (key, value) in captures {
        side.vars.insert(key, value);
    }

    Ok(SideOutcome {
        status: response.status,
        raw_body: response.body.clone(),
        duration: (ended - started).to_std().unwrap_or_default(),
        snapshot_ms,
        redis_ms,
        normalise_ms,
        headers,
        body,
        formatting,
        db_diff,
        redis_diff,
        messages,
        violations,
    })
}

fn resolve_passwords(scenario: &Scenario, vars: &BTreeMap<String, String>) -> Result<Vec<String>> {
    scenario
        .password_vars
        .iter()
        .map(|name| {
            vars.get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("password_vars entry {name:?} has no value"))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Request construction
// ---------------------------------------------------------------------------

fn template_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"\{\{\s*([A-Za-z0-9_.-]+)\s*\}\}").expect("static template regex")
    })
}

fn render_vars(template: &str, vars: &BTreeMap<String, String>, base_url: &str) -> Result<String> {
    let mut missing: Option<String> = None;
    let rendered = template_regex()
        .replace_all(template, |captures: &regex::Captures<'_>| {
            let name = captures
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or_default()
                .to_string();
            if name == "base_url" {
                return base_url.to_string();
            }
            match vars.get(&name) {
                Some(value) => value.clone(),
                None => {
                    missing = Some(name.clone());
                    String::new()
                }
            }
        })
        .into_owned();
    if let Some(name) = missing {
        bail!("undefined scenario variable {name:?} in {template:?}");
    }
    Ok(rendered)
}

fn build_request(side: &Side, step: &Step, scenario: &Scenario) -> Result<HttpRequest> {
    let vars = {
        let mut merged = scenario.vars.clone();
        merged.extend(side.vars.clone());
        merged
    };
    let path = render_vars(&step.path, &vars, &side.base_url)?;
    let mut path_and_query = path;
    if let Some(query) = &step.query {
        let encoded = match query {
            crate::config::Query::Map(map) => {
                let mut pairs = Vec::with_capacity(map.len());
                for (key, value) in map {
                    pairs.push((
                        render_vars(key, &vars, &side.base_url)?,
                        render_vars(value, &vars, &side.base_url)?,
                    ));
                }
                form_encode(&pairs)
            }
            crate::config::Query::Raw(raw) => render_vars(raw, &vars, &side.base_url)?,
        };
        if !encoded.is_empty() {
            if path_and_query.contains('?') {
                path_and_query.push('&');
            } else {
                path_and_query.push('?');
            }
            path_and_query.push_str(&encoded);
        }
    }

    let mut headers = Vec::with_capacity(step.headers.len() + 1);
    let mut body = None;
    let mut content_type: Option<&'static str> = None;
    if let Some(value) = &step.json {
        let mut rendered = value.clone();
        render_json_vars(&mut rendered, &vars, &side.base_url)?;
        body = Some(serde_json::to_vec(&rendered).context("serialising the JSON body")?);
        content_type = Some("application/json");
    } else if let Some(form) = &step.form {
        let mut pairs = Vec::with_capacity(form.len());
        for (key, value) in form {
            pairs.push((
                render_vars(key, &vars, &side.base_url)?,
                render_vars(value, &vars, &side.base_url)?,
            ));
        }
        body = Some(form_encode(&pairs).into_bytes());
        content_type = Some("application/x-www-form-urlencoded");
    } else if let Some(raw) = &step.body {
        body = Some(render_vars(raw, &vars, &side.base_url)?.into_bytes());
        content_type = Some("text/plain;charset=UTF-8");
    }
    let has_content_type = step
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("content-type"));
    if let Some(content_type) = content_type
        && !has_content_type
    {
        headers.push(("Content-Type".to_string(), content_type.to_string()));
    }
    for (name, value) in &step.headers {
        headers.push((name.clone(), render_vars(value, &vars, &side.base_url)?));
    }

    Ok(HttpRequest {
        method: step.method.clone(),
        path_and_query,
        headers,
        body,
        jar: step.jar.clone(),
    })
}

fn render_json_vars(
    value: &mut Value,
    vars: &BTreeMap<String, String>,
    base_url: &str,
) -> Result<()> {
    match value {
        Value::String(text) => {
            *text = render_vars(text, vars, base_url)?;
        }
        Value::Array(items) => {
            for item in items {
                render_json_vars(item, vars, base_url)?;
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                render_json_vars(item, vars, base_url)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn form_encode(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", url_encode(key), url_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Captures
// ---------------------------------------------------------------------------

fn collect_captures(
    side: &Side,
    step: &Step,
    response: &HttpResponse,
) -> Result<BTreeMap<String, String>> {
    let mut captures = BTreeMap::new();
    for capture in &step.capture {
        let value = match capture.source {
            CaptureSource::Json => {
                let pointer = capture.pointer.as_deref().unwrap_or_default();
                let body: Value = serde_json::from_slice(&response.body).with_context(|| {
                    format!(
                        "step {}: capture {} needs a JSON response body",
                        step.name, capture.name
                    )
                })?;
                let found = crate::util::json_pointer(&body, pointer)
                    .with_context(|| format!("step {}: capture {}", step.name, capture.name))?;
                match found {
                    Value::String(text) => text.clone(),
                    other => serde_json::to_string(other)?,
                }
            }
            CaptureSource::Header => {
                let name = capture.header.as_deref().unwrap_or_default();
                response.header(name).map(str::to_string).ok_or_else(|| {
                    anyhow::anyhow!(
                        "step {}: capture {}: header {name} missing",
                        step.name,
                        capture.name
                    )
                })?
            }
            CaptureSource::Cookie => {
                let name = capture.cookie.as_deref().unwrap_or_default();
                side.http
                    .jar(&step.jar)
                    .and_then(|jar| jar.get(name))
                    .map(str::to_string)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "step {}: capture {}: cookie {name} missing from jar {}",
                            step.name,
                            capture.name,
                            step.jar
                        )
                    })?
            }
            CaptureSource::Regex => {
                let header = capture.header.as_deref().unwrap_or_default();
                let pattern = capture.pattern.as_deref().unwrap_or_default();
                let haystack = response.header(header).ok_or_else(|| {
                    anyhow::anyhow!(
                        "step {}: capture {}: header {header} missing",
                        step.name,
                        capture.name
                    )
                })?;
                let regex = regex::Regex::new(pattern).with_context(|| {
                    format!(
                        "step {}: capture {}: invalid regex",
                        step.name, capture.name
                    )
                })?;
                let captures = regex.captures(haystack).ok_or_else(|| {
                    anyhow::anyhow!(
                        "step {}: capture {}: pattern {pattern:?} did not match {header:?}={haystack:?}",
                        step.name,
                        capture.name
                    )
                })?;
                captures
                    .get(1)
                    .or_else(|| captures.get(0))
                    .map(|m| m.as_str().to_string())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "step {}: capture {}: regex has no capture",
                            step.name,
                            capture.name
                        )
                    })?
            }
        };
        captures.insert(capture.name.clone(), value);
    }
    Ok(captures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn url_encoding_matches_form_rules() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("a+b&c=d"), "a%2Bb%26c%3Dd");
        assert_eq!(
            form_encode(&[("a b".to_string(), "c&d".to_string())]),
            "a%20b=c%26d"
        );
    }

    #[test]
    fn variables_are_rendered_and_missing_ones_fail() {
        let vars = BTreeMap::from([("slug".to_string(), "ws".to_string())]);
        assert_eq!(
            render_vars("/api/workspaces/{{ slug }}/", &vars, "http://127.0.0.1:1")
                .expect("render"),
            "/api/workspaces/ws/"
        );
        assert_eq!(
            render_vars("{{base_url}}/x", &vars, "http://127.0.0.1:1").expect("render"),
            "http://127.0.0.1:1/x"
        );
        assert!(render_vars("{{nope}}", &vars, "http://x").is_err());
    }

    #[test]
    fn json_body_variables_are_rendered_recursively() {
        let mut value = json!({"name": "{{name}}", "nested": ["{{name}}", 2]});
        render_json_vars(
            &mut value,
            &BTreeMap::from([("name".to_string(), "x".to_string())]),
            "http://y",
        )
        .expect("render");
        assert_eq!(value, json!({"name": "x", "nested": ["x", 2]}));
    }

    #[test]
    fn near_threshold_bodies_are_detected_as_a_random_gzip_zone() {
        assert!(!content_encoding_is_random(b"{}"));
        let big = serde_json::to_vec(&vec!["x".repeat(64); 60]).expect("json");
        assert!(
            big.len() > 600,
            "test body should be well above the threshold"
        );
        assert!(!content_encoding_is_random(&big));
        // 200-300 high-entropy bytes is where the two possible outcomes meet.
        let mut state = 0x1234_5678_9abc_def0_u64;
        let text: String = (0..210)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                char::from(b'a' + ((state >> 33) % 26) as u8)
            })
            .collect();
        let borderline = serde_json::to_vec(&text).expect("json");
        assert!(borderline.len() >= 200 && borderline.len() < 400);
        assert!(content_encoding_is_random(&borderline));
    }

    #[test]
    fn random_content_encoding_is_marked_on_both_shapes() {
        let mut with = vec![("Content-Encoding".to_string(), "gzip".to_string())];
        normalise_content_encoding(&mut with);
        let mut without = vec![("Content-Type".to_string(), "application/json".to_string())];
        normalise_content_encoding(&mut without);
        assert_eq!(with[0].1, without[1].1);
    }

    #[test]
    fn content_length_is_user_controlled() {
        let rules = NormalizeRules {
            ignore_headers: vec!["content-length".to_string(), "date".to_string()],
            shapes: BTreeMap::new(),
            volatile_cookies: vec![],
            volatile_json_keys: vec![],
            volatile_headers: vec![],
            volatile_celery_headers: vec![],
            volatile_db_columns: vec![],
            timestamp_slack_secs: 2,
            db_schemas: vec!["public".to_string()],
            db_exclude_tables: vec![],
        };
        let headers = vec![
            ("Content-Length".to_string(), "2".to_string()),
            ("Date".to_string(), "x".to_string()),
            ("Vary".to_string(), "Cookie".to_string()),
        ];
        assert_eq!(filter_headers(&headers, &rules, true).len(), 2);
        assert_eq!(filter_headers(&headers, &rules, false).len(), 1);
    }

    fn spec(first: u16, last: u16) -> crate::config::TargetSpec {
        serde_yaml_ng::from_str(&format!(
            "kind: test\nport_range: [{first}, {last}]\ncommand: [\"/bin/true\"]\nhealth: /\n"
        ))
        .expect("target spec")
    }

    fn scenario(redis_db: [i64; 2]) -> crate::config::Scenario {
        serde_yaml_ng::from_str(&format!(
            "name: test\nredis_db: [{}, {}]\nsteps: []\n",
            redis_db[0], redis_db[1]
        ))
        .expect("scenario")
    }

    #[test]
    fn managed_mode_uses_each_targets_own_port_range_and_redis_index() {
        // Regression: B used to draw its port from A's range, and both sides
        // used the scenario's `redis_db` even when the CLI flags disagreed.
        let spec_a = spec(45100, 45109);
        let spec_b = spec(45110, 45119);
        let scenario = scenario([6, 7]);

        let plan = target_plan(&spec_a, &spec_b, &scenario, None, None);
        assert_eq!(plan.a_ports, (45100, 45109));
        assert_eq!(plan.b_ports, (45110, 45119));
        assert_eq!((plan.a_redis_db, plan.b_redis_db), (6, 7));

        // The flags override the scenario per side, in managed mode too.
        let plan = target_plan(&spec_a, &spec_b, &scenario, Some(9), Some(0));
        assert_eq!(plan.a_ports, (45100, 45109), "ports are unaffected");
        assert_eq!(plan.b_ports, (45110, 45119), "ports are unaffected");
        assert_eq!((plan.a_redis_db, plan.b_redis_db), (9, 0));
    }

    #[test]
    fn cli_redis_index_overrides_the_scenario() {
        let scenario = [6, 7];
        assert_eq!(redis_index(None, &scenario, 0), 6);
        assert_eq!(redis_index(None, &scenario, 1), 7);
        assert_eq!(redis_index(Some(9), &scenario, 0), 9);
        assert_eq!(redis_index(Some(0), &scenario, 1), 0);
    }
}
