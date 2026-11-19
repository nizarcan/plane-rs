// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `difftest task`: one Celery task, executed by both backends, compared.
//!
//! A = the real Django task through `task.apply()` in-process
//! (`tools/jobs/run_django_task.py`); B = a job enqueued into `plane_jobs` and
//! executed by `plane-worker --once` in a fresh process. Both start from the
//! same database clone plus the same fixture SQL. The tool compares
//!
//! * the database diff (every table in schema `public`, keyed by primary key),
//! * the Celery messages each side publishes (`task`, `args`, `kwargs`), and
//! * the `plane_jobs` rows B's handler enqueues (the Rust-owned half of the
//!   same comparison; A cannot enqueue those).
//!
//! (`plans/CUTOVER.md` §2 "Handler correctness"; `DECISIONS.md` D25: a handler
//! reproduces every side effect, including the tasks it publishes.)
//!
//! ## Normalisation
//!
//! A task writes the clock and, for an insert, generates a new primary key.
//! The diff normaliser therefore replaces a timestamp value that falls inside
//! the run window with `<TS>` and an inserted row's `id` with `<ID>`. Fixture
//! timestamps (days old) are compared exactly, so a task that forgets to bump
//! `visited_at` still fails. The `epoch=` keyword of a published
//! `issue_activity` message is `<EPOCH>` for the same reason; the rest of the
//! published arguments must match exactly.
//!
//! ## Expected failures
//!
//! `--expect-django-failure`/`--expect-worker-failure` declare that the task
//! itself is expected to fail (the upstream `hard_delete` `DO_NOTHING` bug,
//! `plans/CUTOVER.md` §6). The DB effects committed before the failure are
//! still compared, so B has to fail at the same statement.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use crate::celery::{CeleryDrain, CeleryMessage, RabbitAdmin, amqp_url_with_vhost};
use crate::db::{DbAdmin, DbDiff, diff_snapshots};
use crate::util::{repo_root, validate_identifier};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::process::Command;

/// Everything one `difftest task` run needs.
#[derive(Debug, Clone)]
pub struct TaskOptions {
    /// Registered Celery task name.
    pub task: String,
    /// JSON object with the task's keyword arguments.
    pub kwargs_path: PathBuf,
    /// Optional SQL applied to both databases before the task runs.
    pub setup_path: Option<PathBuf>,
    /// Database A (Django), created and dropped by this tool.
    pub a_db: String,
    /// Database B (Rust).
    pub b_db: String,
    /// Redis index for Django.
    pub a_redis_db: i64,
    /// Redis index for the worker.
    pub b_redis_db: i64,
    /// RabbitMQ vhost for the run, created and dropped by this tool.
    pub vhost: String,
    /// Keep the databases and vhost after the run.
    pub keep: bool,
    /// Postgres URL of the server (the template database lives here).
    pub pg_url: String,
    /// RabbitMQ management URL and credentials, for creating the vhost.
    pub rabbit_http: String,
    /// RabbitMQ AMQP URL (without a vhost), for draining published messages.
    pub rabbit_amqp: String,
    pub rabbit_user: String,
    pub rabbit_password: String,
    /// `plane-worker` binary. Defaults to `target/debug/plane-worker`.
    pub worker: Option<PathBuf>,
    /// Template database cloned for both sides.
    pub template_db: String,
    /// A substring of Django's failure, when the task is expected to fail.
    pub expect_django_failure: Option<String>,
    /// A substring of the worker's `last_error`, when it is expected to fail.
    pub expect_worker_failure: Option<String>,
}

impl TaskOptions {
    /// The default database names and Redis indexes for this agent's runs.
    pub fn defaults(task: impl Into<String>, kwargs_path: PathBuf) -> Self {
        Self {
            task: task.into(),
            kwargs_path,
            setup_path: None,
            a_db: "t_p8d_task_a".to_string(),
            b_db: "t_p8d_task_b".to_string(),
            a_redis_db: 3,
            b_redis_db: 4,
            vhost: "p8d_task".to_string(),
            keep: false,
            pg_url: "postgresql://plane:plane@127.0.0.1:55432/plane".to_string(),
            rabbit_http: "http://127.0.0.1:55673".to_string(),
            rabbit_amqp: "amqp://plane:plane@127.0.0.1:55672".to_string(),
            rabbit_user: "plane".to_string(),
            rabbit_password: "plane".to_string(),
            worker: None,
            template_db: "plane_tpl".to_string(),
            expect_django_failure: None,
            expect_worker_failure: None,
        }
    }
}

/// Run one differential task case.
///
/// # Errors
///
/// Any setup, process or comparison failure; the message names the side.
pub async fn run(options: &TaskOptions) -> Result<()> {
    validate_identifier(&options.a_db)?;
    validate_identifier(&options.b_db)?;
    validate_identifier(&options.template_db)?;
    validate_identifier(&options.vhost)?;
    let kwargs: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&options.kwargs_path)
            .with_context(|| format!("reading {}", options.kwargs_path.display()))?,
    )
    .with_context(|| format!("parsing {}", options.kwargs_path.display()))?;
    if !kwargs.is_object() {
        bail!(
            "{} must hold a JSON object of keyword arguments",
            options.kwargs_path.display()
        );
    }
    let setup = match &options.setup_path {
        Some(path) => Some(
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        ),
        None => None,
    };

    let admin = DbAdmin::new(&options.pg_url);
    let rabbit = RabbitAdmin::new(
        &options.rabbit_http,
        &options.rabbit_user,
        &options.rabbit_password,
    )?;

    // Both databases start from the same migrated template plus the same
    // fixture. Recreate them so a previous failed run cannot leak rows.
    prepare_database(
        &admin,
        &options.template_db,
        &options.a_db,
        setup.as_deref(),
    )
    .await?;
    prepare_database(
        &admin,
        &options.template_db,
        &options.b_db,
        setup.as_deref(),
    )
    .await?;
    rabbit.create_vhost(&options.vhost).await?;
    rabbit
        .grant_full_permissions(&options.vhost, &options.rabbit_user)
        .await?;

    let outcome = execute(options, &kwargs, &admin, &rabbit).await;

    if !options.keep {
        let _ = admin.drop_database(&options.a_db).await;
        let _ = admin.drop_database(&options.b_db).await;
        let _ = rabbit.delete_vhost(&options.vhost).await;
    }
    outcome
}

/// One side's published work, in the order it left the process.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct Outgoing {
    task: String,
    args: serde_json::Value,
    kwargs: serde_json::Value,
}

async fn execute(
    options: &TaskOptions,
    kwargs: &serde_json::Value,
    admin: &DbAdmin,
    rabbit: &RabbitAdmin,
) -> Result<()> {
    let catalog = admin
        .load_catalog(&options.a_db, &["public".to_string()], &[])
        .await?;
    let drain = CeleryDrain::connect(&amqp_url_with_vhost(
        &options.rabbit_amqp,
        &options.vhost,
        &options.rabbit_user,
        &options.rabbit_password,
    )?)
    .await?;

    let started: DateTime<Utc> = Utc::now();
    let before_a = admin.snapshot(&options.a_db, &catalog).await?.snapshot;
    let django = run_django(options, kwargs).await?;
    let messages_a = drain_messages(rabbit, &drain, &options.vhost).await?;
    let before_b = admin.snapshot(&options.b_db, &catalog).await?.snapshot;
    let worker = run_worker(options, kwargs, admin).await?;
    let messages_b = drain_messages(rabbit, &drain, &options.vhost).await?;
    let jobs_b = rust_jobs(admin, &options.b_db, &worker.job_id).await?;
    let after_a = admin.snapshot(&options.a_db, &catalog).await?.snapshot;
    let after_b = admin.snapshot(&options.b_db, &catalog).await?.snapshot;

    let mut outgoing_a = messages_a
        .iter()
        .map(message_to_outgoing)
        .collect::<Vec<_>>();
    let mut outgoing_b = messages_b
        .iter()
        .map(message_to_outgoing)
        .collect::<Vec<_>>();
    outgoing_b.extend(jobs_b.iter().map(job_to_outgoing));
    normalise_outgoing(&mut outgoing_a);
    normalise_outgoing(&mut outgoing_b);

    println!(
        "difftest task: {} — django {:?}, rust {:?}, django messages={} rust messages={} rust jobs={}",
        options.task,
        django.status,
        worker.state,
        outgoing_a.len(),
        messages_b.len(),
        jobs_b.len()
    );

    let mut diff_a = diff_snapshots(&before_a, &after_a);
    let mut diff_b = diff_snapshots(&before_b, &after_b);
    normalise_diff(&mut diff_a, started);
    normalise_diff(&mut diff_b, started);
    if diff_a != diff_b {
        print_diff("django", &diff_a);
        print_diff("rust", &diff_b);
        bail!("database effects differ");
    }
    println!(
        "PASS: database effects are identical ({} row changes)",
        diff_a.total_changes()
    );
    for (table, changes) in &diff_a.tables {
        println!(
            "  {table}: +{} -{} ~{}",
            changes.inserted.len(),
            changes.deleted.len(),
            changes.updated.len()
        );
    }

    if outgoing_a != outgoing_b {
        for index in 0..outgoing_a.len().max(outgoing_b.len()) {
            let left = outgoing_a.get(index);
            let right = outgoing_b.get(index);
            if left != right {
                println!(
                    "outgoing {index}:\n  django: {}\n  rust:   {}",
                    serde_json::to_string_pretty(&left)
                        .unwrap_or_else(|_| "<unserialisable>".to_string()),
                    serde_json::to_string_pretty(&right)
                        .unwrap_or_else(|_| "<unserialisable>".to_string())
                );
            }
        }
        bail!("published tasks differ");
    }
    println!(
        "PASS: published tasks are identical ({} messages/jobs)",
        outgoing_a.len()
    );
    Ok(())
}

async fn drain_messages(
    rabbit: &RabbitAdmin,
    drain: &CeleryDrain,
    vhost: &str,
) -> Result<Vec<CeleryMessage>> {
    // The publisher returns after the broker accepted the frame; a short pause
    // keeps a slow local delivery from being read as "no message".
    tokio::time::sleep(Duration::from_millis(250)).await;
    let queues = rabbit.queues(vhost).await?;
    drain.drain(&queues).await
}

fn message_to_outgoing(message: &CeleryMessage) -> Outgoing {
    Outgoing {
        task: message.task.clone().unwrap_or_default(),
        args: message.args.clone(),
        kwargs: message.kwargs.clone(),
    }
}

fn job_to_outgoing(row: &tokio_postgres::Row) -> Outgoing {
    let task: String = row.get(0);
    let text: String = row.get(1);
    let payload: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    Outgoing {
        task,
        args: payload
            .get("args")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        kwargs: payload
            .get("kwargs")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    }
}

/// `epoch=int(timezone.now().timestamp())` is the only argument a task
/// publishes that carries the clock (`issue_automation_task.py:82,145`).
fn normalise_outgoing(outgoing: &mut [Outgoing]) {
    for entry in outgoing {
        if let Some(kwargs) = entry.kwargs.as_object_mut()
            && kwargs.contains_key("epoch")
        {
            kwargs.insert("epoch".to_string(), serde_json::json!("<EPOCH>"));
        }
    }
}

async fn prepare_database(
    admin: &DbAdmin,
    template: &str,
    target: &str,
    setup: Option<&str>,
) -> Result<()> {
    if admin.database_exists(target).await? {
        admin.drop_database(target).await?;
    }
    admin.clone_database(template, target).await?;
    if let Some(setup) = setup {
        let client = admin.connect(target).await?;
        client
            .batch_execute(setup)
            .await
            .with_context(|| format!("applying fixture SQL to {target}"))?;
    }
    Ok(())
}

struct DjangoOutcome {
    status: String,
    exc_type: String,
    result: String,
}

/// A = `task.apply()` in the real Django, with the dev stack's environment.
async fn run_django(options: &TaskOptions, kwargs: &serde_json::Value) -> Result<DjangoOutcome> {
    let root = repo_root();
    let env = dev_env(&options.a_db, options.a_redis_db, &options.vhost)?;
    let output = Command::new(root.join("apps/api/.venv/bin/python"))
        .arg(root.join("apps/server/tools/jobs/run_django_task.py"))
        .arg(&options.task)
        .arg(serde_json::to_string(kwargs)?)
        .current_dir(root.join("apps/api"))
        .env_clear()
        .env("PYTHONPATH", root.join("apps/api"))
        .envs(&env)
        .stdin(Stdio::null())
        .output()
        .await
        .context("running the Django task")?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() {
        bail!(
            "Django task failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!("django: {stdout}");
    let result: serde_json::Value =
        serde_json::from_str(&stdout).context("parsing the Django task result")?;
    let outcome = DjangoOutcome {
        status: result
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        exc_type: result
            .get("exc_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        result: result
            .get("result")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
    };
    match &options.expect_django_failure {
        None => {
            if outcome.status != "SUCCESS" {
                bail!(
                    "Django task ended with {}: {}",
                    outcome.status,
                    outcome.result
                );
            }
        }
        Some(needle) => {
            if outcome.status != "FAILURE" {
                bail!(
                    "Django task was expected to fail but ended with {}",
                    outcome.status
                );
            }
            let text = format!("{}: {}", outcome.exc_type, outcome.result);
            if !needle.is_empty() && !text.contains(needle.as_str()) {
                bail!("Django failure {text:?} does not contain {needle:?}");
            }
        }
    }
    Ok(outcome)
}

struct WorkerOutcome {
    state: String,
    job_id: String,
}

/// B = insert a job row, then `plane-worker --once` in a fresh process.
async fn run_worker(
    options: &TaskOptions,
    kwargs: &serde_json::Value,
    admin: &DbAdmin,
) -> Result<WorkerOutcome> {
    let root = repo_root();
    let worker = options
        .worker
        .clone()
        .unwrap_or_else(|| root.join("apps/server/target/debug/plane-worker"));
    let env = dev_env(&options.b_db, options.b_redis_db, &options.vhost)?;

    // Create/modify the `plane_jobs` schema before the insert. The worker
    // migrates on boot too, but that would happen after the insert.
    let migrator = root.join("apps/server/target/debug/plane-migrator");
    let migrate = Command::new(&migrator)
        .env_clear()
        .envs(&env)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("running {}", migrator.display()))?;
    if !migrate.status.success() {
        bail!(
            "plane-migrator failed: {}",
            String::from_utf8_lossy(&migrate.stderr).trim()
        );
    }

    let payload = serde_json::json!({"args": [], "kwargs": kwargs});
    let client = admin.connect(&options.b_db).await?;
    let id = uuid::Uuid::new_v4().to_string();
    // `tokio-postgres` is built without its `uuid`/`serde_json` features in
    // this workspace (feature isolation), so the two generated values are
    // inlined with SQL quoting rather than bound.
    let insert = format!(
        "INSERT INTO plane_jobs.jobs (id, task, payload, max_retries) \
         VALUES ('{id}'::uuid, '{}', '{}'::jsonb, 0)",
        sql_literal(&options.task),
        sql_literal(&payload.to_string()),
    );
    client
        .execute(&insert, &[])
        .await
        .context("enqueueing the job")?;

    let output = Command::new(&worker)
        .arg("--once")
        .env_clear()
        .envs(&env)
        .env("PLANE_RS_JOB_CONCURRENCY", "1")
        .env("PLANE_RS_JOB_POLL_MS", "10")
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("running {}", worker.display()))?;
    if !output.status.success() {
        bail!(
            "plane-worker failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let row = client
        .query_one(
            &format!("SELECT state, last_error FROM plane_jobs.jobs WHERE id = '{id}'::uuid"),
            &[],
        )
        .await
        .context("reading the job row")?;
    let state: String = row.try_get(0)?;
    let last_error: Option<String> = row.try_get(1)?;
    match &options.expect_worker_failure {
        None => {
            if state != "done" {
                bail!(
                    "Rust job ended in state {state}: {}",
                    last_error.unwrap_or_else(|| "no error recorded".to_string())
                );
            }
        }
        Some(needle) => {
            if state != "dead" {
                bail!("Rust job was expected to fail but ended in state {state}");
            }
            let error = last_error.unwrap_or_default();
            if !needle.is_empty() && !error.contains(needle.as_str()) {
                bail!("Rust failure {error:?} does not contain {needle:?}");
            }
        }
    }
    Ok(WorkerOutcome { state, job_id: id })
}

/// The jobs B's handler enqueued, excluding the one the harness inserted.
async fn rust_jobs(
    admin: &DbAdmin,
    database: &str,
    harness_job: &str,
) -> Result<Vec<tokio_postgres::Row>> {
    let client = admin.connect(database).await?;
    client
        .query(
            "SELECT task, payload::text FROM plane_jobs.jobs \
             WHERE id <> ($1::text)::uuid AND state IN ('queued', 'running', 'done') \
             ORDER BY created_at, id",
            &[&harness_job],
        )
        .await
        .context("reading the jobs the handler enqueued")
}

/// A single-quoted SQL literal (the values are generated, not user input).
fn sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

/// Normalise the clock and generated primary keys. See the module docs.
///
/// Every value that parses as an RFC 3339 instant becomes `<TS>`: the two
/// fixture applications call `now()` a few milliseconds apart, so the *same*
/// unchanged fixture row would otherwise differ between the clones. A column
/// the task moved still shows up as a `changed` entry, because the harness
/// diffs before and after within each database — normalising the values only
/// hides the clock, not the update.
fn normalise_diff(diff: &mut DbDiff, _started: DateTime<Utc>) {
    for table in diff.tables.values_mut() {
        for row in table.inserted.iter_mut().chain(table.deleted.iter_mut()) {
            if let Some(object) = row.as_object_mut() {
                for (column, value) in object.iter_mut() {
                    if column == "id" {
                        *value = serde_json::json!("<ID>");
                    } else {
                        normalise_timestamp(value);
                    }
                }
            }
        }
        for updated in &mut table.updated {
            if let Some(changed) = updated.get_mut("changed").and_then(|v| v.as_object_mut()) {
                for entry in changed.values_mut() {
                    if let Some(pair) = entry.as_object_mut() {
                        for side in ["a", "b"] {
                            if let Some(value) = pair.get_mut(side) {
                                normalise_timestamp(value);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A timestamp becomes `<TS>`; a date (`archived_at`, `target_date`) is left
/// alone, so an automation task that archives on the wrong day still fails.
fn normalise_timestamp(value: &mut serde_json::Value) {
    let Some(text) = value.as_str() else {
        return;
    };
    if text.contains('T') && DateTime::parse_from_rfc3339(text).is_ok() {
        *value = serde_json::json!("<TS>");
    }
}

fn print_diff(side: &str, diff: &DbDiff) {
    for (table, changes) in &diff.tables {
        println!(
            "  {side} {table}: +{} -{} ~{}",
            changes.inserted.len(),
            changes.deleted.len(),
            changes.updated.len()
        );
        for row in &changes.inserted {
            println!("    + {row}");
        }
        for row in &changes.deleted {
            println!("    - {row}");
        }
        for row in &changes.updated {
            println!("    ~ {row}");
        }
    }
}

/// `dev/django-env.sh` evaluated with this run's database, Redis index and
/// vhost. The script is the single source of truth for the dev environment, so
/// the worker and Django see the same variables the differential harness uses.
fn dev_env(db: &str, redis_db: i64, vhost: &str) -> Result<BTreeMap<String, String>> {
    let script = repo_root().join("apps/server/dev/django-env.sh");
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg("set -a; source \"$1\" >/dev/null; env -0")
        .arg("difftest-task")
        .arg(&script)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("PLANE_DB", db)
        .env("PLANE_REDIS_DB", redis_db.to_string())
        .env("PLANE_MQ_VHOST", vhost)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .with_context(|| format!("sourcing {}", script.display()))?;
    if !output.status.success() {
        bail!(
            "sourcing {} failed: {}",
            script.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let mut env = BTreeMap::new();
    for entry in output.stdout.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(entry);
        if let Some((key, value)) = text.split_once('=') {
            env.insert(key.to_string(), value.to_string());
        }
    }
    // `REDIS_URL` is derived by the harness's convention, not by
    // django-env.sh itself (it reads PLANE_REDIS_DB, which is already set).
    env.insert(
        "PLANE_RS_JOB_DRAIN_SECONDS".to_string(),
        Duration::from_secs(30).as_secs().to_string(),
    );
    Ok(env)
}

/// The paths `difftest task` resolves relative to the repository root.
pub fn default_paths() -> (PathBuf, PathBuf) {
    let root = repo_root();
    (
        root.join("apps/server/target/debug/plane-worker"),
        root.join("apps/server/target/debug/plane-migrator"),
    )
}
