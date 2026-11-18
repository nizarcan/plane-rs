// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `difftest task`: one Celery task, executed by both backends, DB effects
//! compared.
//!
//! A = the real Django task through `task.apply()` in-process
//! (`tools/jobs/run_django_task.py`); B = a job enqueued into `plane_jobs` and
//! executed by `plane-worker --once` in a fresh process. Both start from the
//! same database clone plus the same fixture SQL, and the tool compares
//! `user_recent_visits` row by row (`plans/CUTOVER.md` §2 "Handler
//! correctness").
//!
//! Why a table comparison and not the harness's generic `diff_snapshots`:
//! the task inserts a row with `uuid.uuid4()`, so each side's new primary key
//! is legitimately different. The harness's diff keys rows by primary key and
//! would report two inserts. The comparison here keeps every column that
//! describes the effect (entity, user, project, workspace, audit user ids,
//! soft-delete flag) and normalises only the generated timestamps and the new
//! row's id.
//!
//! `plane_jobs` on B is the transport, not an effect: it is excluded, and the
//! tool fails if B's job is not `done` (a `dead` job names its `last_error`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use crate::celery::RabbitAdmin;
use crate::db::DbAdmin;
use crate::util::{repo_root, validate_identifier};
use anyhow::{Context, Result, bail};
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
    /// RabbitMQ vhost for the Django run's `.delay()` call, created and
    /// dropped by this tool.
    pub vhost: String,
    /// Keep the databases and vhost after the run.
    pub keep: bool,
    /// Postgres URL of the server (the template database lives here).
    pub pg_url: String,
    /// RabbitMQ management URL and credentials, for creating the vhost.
    pub rabbit_http: String,
    pub rabbit_user: String,
    pub rabbit_password: String,
    /// `plane-worker` binary. Defaults to `target/debug/plane-worker`.
    pub worker: Option<PathBuf>,
    /// Template database cloned for both sides.
    pub template_db: String,
}

impl TaskOptions {
    /// The default database names and Redis indexes for this agent's runs.
    pub fn defaults(task: impl Into<String>, kwargs_path: PathBuf) -> Self {
        Self {
            task: task.into(),
            kwargs_path,
            setup_path: None,
            a_db: "t_p8p0_task_a".to_string(),
            b_db: "t_p8p0_task_b".to_string(),
            a_redis_db: 5,
            b_redis_db: 6,
            vhost: "p8p0_task".to_string(),
            keep: false,
            pg_url: "postgresql://plane:plane@127.0.0.1:55432/plane".to_string(),
            rabbit_http: "http://127.0.0.1:55673".to_string(),
            rabbit_user: "plane".to_string(),
            rabbit_password: "plane".to_string(),
            worker: None,
            template_db: "plane_tpl".to_string(),
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

    let outcome = execute(options, &kwargs, &admin).await;

    if !options.keep {
        let _ = admin.drop_database(&options.a_db).await;
        let _ = admin.drop_database(&options.b_db).await;
        let _ = rabbit.delete_vhost(&options.vhost).await;
    }
    outcome
}

async fn execute(options: &TaskOptions, kwargs: &serde_json::Value, admin: &DbAdmin) -> Result<()> {
    run_django(options, kwargs).await?;
    run_worker(options, kwargs, admin).await?;

    let a = visit_rows(admin, &options.a_db).await?;
    let b = visit_rows(admin, &options.b_db).await?;
    println!(
        "difftest task: {} — django rows={} rust rows={}",
        options.task,
        a.len(),
        b.len()
    );
    if a != b {
        let limit = a.len().max(b.len());
        for index in 0..limit {
            let left = a.get(index).map(Vec::as_slice).unwrap_or_default();
            let right = b.get(index).map(Vec::as_slice).unwrap_or_default();
            if left != right {
                println!("row {index}:\n  django: {left:?}\n  rust:   {right:?}");
            }
        }
        bail!("database effects differ");
    }
    println!("PASS: database effects are identical");
    Ok(())
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

/// A = `task.apply()` in the real Django, with the dev stack's environment.
async fn run_django(options: &TaskOptions, kwargs: &serde_json::Value) -> Result<()> {
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
    if !output.status.success() {
        bail!(
            "Django task failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("django: {}", stdout.trim());
    let result: serde_json::Value =
        serde_json::from_str(stdout.trim()).context("parsing the Django task result")?;
    if result.get("status").and_then(serde_json::Value::as_str) != Some("SUCCESS") {
        bail!("Django task ended with {}", stdout.trim());
    }
    Ok(())
}

/// B = insert a job row, then `plane-worker --once` in a fresh process.
async fn run_worker(
    options: &TaskOptions,
    kwargs: &serde_json::Value,
    admin: &DbAdmin,
) -> Result<()> {
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
    if state != "done" {
        bail!(
            "Rust job ended in state {state}: {}",
            last_error.unwrap_or_else(|| "no error recorded".to_string())
        );
    }
    Ok(())
}

/// A single-quoted SQL literal (the values are generated, not user input).
fn sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

/// The effect, one entry per physical row. See the module docs for why the id
/// and generated timestamps are normalised.
async fn visit_rows(admin: &DbAdmin, database: &str) -> Result<Vec<Vec<String>>> {
    let client = admin.connect(database).await?;
    let rows = client
        .query(
            "SELECT entity_name, \
                    COALESCE(entity_identifier::text, 'NULL'), \
                    user_id::text, \
                    COALESCE(project_id::text, 'NULL'), \
                    workspace_id::text, \
                    COALESCE(created_by_id::text, 'NULL'), \
                    COALESCE(updated_by_id::text, 'NULL'), \
                    CASE WHEN deleted_at IS NULL THEN 'live' ELSE 'deleted' END, \
                    CASE WHEN visited_at IS NULL THEN 'NULL' ELSE '<TS>' END, \
                    CASE WHEN created_at IS NULL THEN 'NULL' ELSE '<TS>' END, \
                    CASE WHEN updated_at IS NULL THEN 'NULL' ELSE '<TS>' END \
             FROM user_recent_visits \
             ORDER BY entity_identifier NULLS FIRST, entity_name, deleted_at NULLS FIRST, created_at",
            &[],
        )
        .await
        .context("reading user_recent_visits")?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (0..row.len())
                .map(|index| {
                    row.try_get::<_, Option<String>>(index)
                        .unwrap_or_default()
                        .unwrap_or_else(|| "NULL".to_string())
                })
                .collect()
        })
        .collect())
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
