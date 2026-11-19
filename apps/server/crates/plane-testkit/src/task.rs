// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `difftest task`: one Celery task, executed by both backends, its effects
//! compared: database rows, S3 objects (bytes, content type, ACL) and the
//! requests a task makes to a local fake HTTP server.
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
//!
//! # Case files
//!
//! The worker binary must be built with the `difftest` feature
//! (`cargo build -p plane-worker --features difftest`) when a case drives the
//! link crawler: its fake HTTP server lives on loopback, and the crawl
//! handler's SSRF escape is compiled in only with that feature. A default
//! worker ignores `PLANE_RS_DIFFTEST_ALLOW_LOOPBACK` by design
//! (`crates/plane-tasks/Cargo.toml`).
//!
//! With `--case` the run compares what the task's case file names
//! (`crate::task_case`): arbitrary SQL queries, two agent-owned S3 buckets the
//! tool creates/seeds/reads/deletes, and a `127.0.0.1` recorder whose responses
//! the case scripts. Without `--case` the original `recent_visited_task`
//! comparison applies, so the P0 fixtures keep working.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use crate::celery::RabbitAdmin;
use crate::db::DbAdmin;
use crate::s3::S3Admin;
use crate::task_case::{
    Recorder, SmtpCase, TaskCase, compare_objects, compare_requests, compare_rows,
    compare_smtp_logs,
};
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
    /// Per-task comparison case (SQL queries, S3 capture, fake HTTP server).
    pub case_path: Option<PathBuf>,
    /// Dev MinIO endpoint for the S3 capture.
    pub s3_endpoint: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    /// Bucket for the Django side. Both are created, seeded and deleted by
    /// this tool; they must not be the shared `uploads` bucket.
    pub s3_bucket_a: Option<String>,
    pub s3_bucket_b: Option<String>,
    /// `LIVE_BASE_URL` for both sides; `{{http}}` expands to the recorder's
    /// base URL, so a case can answer `LIVE_URL/convert-document/`.
    pub live_url: Option<String>,
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
            case_path: None,
            s3_endpoint: "http://127.0.0.1:59000".to_string(),
            s3_access_key: "access-key".to_string(),
            s3_secret_key: "secret-key".to_string(),
            s3_bucket_a: None,
            s3_bucket_b: None,
            live_url: None,
        }
    }
}

/// Replace `{{http}}` in every string of the kwargs with the recorder URL.
fn substitute(value: &mut serde_json::Value, http: &str) {
    match value {
        serde_json::Value::String(text) => {
            if text.contains("{{http}}") {
                *text = text.replace("{{http}}", http);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                substitute(item, http);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                substitute(item, http);
            }
        }
        _ => {}
    }
}

/// The S3 half of a run: the admin client plus the two bucket names.
struct S3Capture {
    admin: S3Admin,
    bucket_a: String,
    bucket_b: String,
}

impl S3Capture {
    async fn observe(
        &self,
    ) -> Result<(
        Vec<crate::s3::ObservedObject>,
        Vec<crate::s3::ObservedObject>,
    )> {
        let a = self.admin.observe(&self.bucket_a).await?;
        let b = self.admin.observe(&self.bucket_b).await?;
        Ok((a, b))
    }

    async fn cleanup(&self) {
        let _ = self.admin.empty_bucket(&self.bucket_a).await;
        let _ = self.admin.delete_bucket(&self.bucket_a).await;
        let _ = self.admin.empty_bucket(&self.bucket_b).await;
        let _ = self.admin.delete_bucket(&self.bucket_b).await;
    }
}

/// The two `fake_smtp.py` listeners and their per-side instance-configuration
/// rewriting.
struct SmtpCapture {
    a: SmtpListener,
    b: SmtpListener,
}

struct SmtpListener {
    child: std::process::Child,
    path: PathBuf,
    port: u16,
}

impl SmtpCapture {
    fn start(case: &SmtpCase) -> Result<Self> {
        let root = repo_root();
        let script = root.join("apps/server/tools/mail/fake_smtp.py");
        let python = root.join("apps/api/.venv/bin/python");
        let dir = std::env::temp_dir().join(format!("plane-difftest-smtp-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let start_one = |port: u16, name: &str| -> Result<SmtpListener> {
            let path = dir.join(format!("{name}.jsonl"));
            let _ = std::fs::remove_file(&path);
            let child = std::process::Command::new(&python)
                .arg(&script)
                .arg("--port")
                .arg(port.to_string())
                .arg("--output")
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| format!("starting fake_smtp.py on {port}"))?;
            Ok(SmtpListener { child, path, port })
        };
        let _ = case;
        let a = start_one(18418, "a")?;
        let b = start_one(18419, "b")?;
        Ok(Self { a, b })
    }

    /// Point one clone's instance configuration at its listener.
    async fn configure(&self, admin: &DbAdmin, database: &str, port: u16) -> Result<()> {
        let client = admin.connect(database).await?;
        let statements = [
            ("EMAIL_HOST", "127.0.0.1".to_string()),
            ("EMAIL_PORT", port.to_string()),
            ("EMAIL_USE_TLS", "0".to_string()),
            ("EMAIL_USE_SSL", "0".to_string()),
            ("EMAIL_HOST_USER", String::new()),
            ("EMAIL_HOST_PASSWORD", String::new()),
            (
                "EMAIL_FROM",
                "Team Plane <team@mailer.plane.so>".to_string(),
            ),
        ];
        for (key, value) in statements {
            let sql = format!(
                "UPDATE instance_configurations SET value = '{}' WHERE key = '{}'",
                value.replace('\'', "''"),
                key
            );
            client.execute(&sql, &[]).await?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        for listener in [&mut self.a, &mut self.b] {
            let _ = listener.child.kill();
            let _ = listener.child.wait();
        }
        Ok(())
    }
}

impl Drop for SmtpCapture {
    fn drop(&mut self) {
        let _ = self.finish();
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
    let mut kwargs: serde_json::Value = serde_json::from_str(
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
    let case = match &options.case_path {
        Some(path) => Some(TaskCase::load(path)?),
        None => None,
    };
    if let Some(case) = &case
        && let Some(task) = &case.task
        && task != &options.task
    {
        bail!(
            "case {} is for {task}, not {}",
            options
                .case_path
                .as_ref()
                .map_or_else(String::new, |path| path.display().to_string()),
            options.task
        );
    }
    let recorder = match case.as_ref().and_then(|case| case.http.clone()) {
        Some(http) => {
            let recorder = Recorder::start(http).await?;
            substitute(&mut kwargs, recorder.url());
            Some(recorder)
        }
        None => None,
    };
    let live_url = options.live_url.as_ref().map(|value| match &recorder {
        Some(recorder) => value.replace("{{http}}", recorder.url()),
        None => value.clone(),
    });
    let s3 = match case.as_ref().and_then(|case| case.s3.as_ref()) {
        Some(s3_case) => {
            let (Some(bucket_a), Some(bucket_b)) =
                (options.s3_bucket_a.clone(), options.s3_bucket_b.clone())
            else {
                bail!("the case captures S3; pass --s3-bucket-a and --s3-bucket-b");
            };
            if !bucket_a.starts_with("p8") || !bucket_b.starts_with("p8") {
                bail!("S3 test buckets must start with p8 (agent-owned names)");
            }
            if bucket_a == bucket_b {
                bail!("--s3-bucket-a and --s3-bucket-b must differ");
            }
            let admin = S3Admin::new(
                &options.s3_endpoint,
                &options.s3_access_key,
                &options.s3_secret_key,
            );
            admin.recreate_bucket(&bucket_a).await?;
            admin.recreate_bucket(&bucket_b).await?;
            for seed in &s3_case.seeds {
                let object = seed.object()?;
                admin.put_object(&bucket_a, &object).await?;
                admin.put_object(&bucket_b, &object).await?;
            }
            Some(S3Capture {
                admin,
                bucket_a,
                bucket_b,
            })
        }
        None => None,
    };
    let mut smtp = match case.as_ref().and_then(|case| case.smtp.as_ref()) {
        Some(smtp_case) => Some(SmtpCapture::start(smtp_case)?),
        None => None,
    };
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
    if let Some(smtp) = &smtp {
        smtp.configure(&admin, &options.a_db, smtp.a.port).await?;
        smtp.configure(&admin, &options.b_db, smtp.b.port).await?;
    }
    rabbit.create_vhost(&options.vhost).await?;
    rabbit
        .grant_full_permissions(&options.vhost, &options.rabbit_user)
        .await?;

    let outcome = execute(
        options,
        &kwargs,
        &admin,
        case.as_ref(),
        recorder.as_ref(),
        s3.as_ref(),
        live_url.as_deref(),
    )
    .await;

    if let Some(smtp) = &mut smtp {
        smtp.finish()?;
    }
    if let Some(case) = &case
        && case.smtp.is_some()
        && let Some(smtp) = &smtp
    {
        compare_smtp_logs(&smtp.a.path, &smtp.b.path)?;
    }

    if !options.keep {
        let _ = admin.drop_database(&options.a_db).await;
        let _ = admin.drop_database(&options.b_db).await;
        let _ = rabbit.delete_vhost(&options.vhost).await;
    }
    // The buckets are always the tool's, so they always go, even with
    // `--keep` (which keeps databases for inspection, not S3 corpora).
    if let Some(s3) = &s3 {
        s3.cleanup().await;
    }
    outcome
}

async fn execute(
    options: &TaskOptions,
    kwargs: &serde_json::Value,
    admin: &DbAdmin,
    case: Option<&TaskCase>,
    recorder: Option<&Recorder>,
    s3: Option<&S3Capture>,
    live_url: Option<&str>,
) -> Result<()> {
    let bucket_a = s3.map(|s3| s3.bucket_a.as_str());
    let bucket_b = s3.map(|s3| s3.bucket_b.as_str());
    let http_start = recorder.map_or(0, Recorder::request_count);
    run_django(options, kwargs, bucket_a, live_url).await?;
    // Capture immediately: later requests append to the same log.
    let django_http = recorder.map(|recorder| recorder.slice(http_start));
    let rust_start = recorder.map_or(0, Recorder::request_count);
    run_worker(options, kwargs, admin, bucket_b, live_url).await?;
    let rust_http = recorder.map(|recorder| recorder.slice(rust_start));

    if let Some(case) = case {
        for query in &case.sql {
            let a = query_rows(admin, &options.a_db, &query.query).await?;
            let b = query_rows(admin, &options.b_db, &query.query).await?;
            println!(
                "difftest task: {} — sql {}: django rows={} rust rows={}",
                options.task,
                query.label,
                a.len(),
                b.len()
            );
            compare_rows(case, &query.label, &a, &b)?;
        }
        if !case.sql.is_empty() {
            println!("PASS: database effects are identical");
        }
    }

    // No case file: the P0 behaviour, one table for `recent_visited_task`.
    if case.is_none_or(|case| case.sql.is_empty()) {
        let a = visit_rows(admin, &options.a_db).await?;
        let b = visit_rows(admin, &options.b_db).await?;
        println!(
            "difftest task: {} — django rows={} rust rows={}",
            options.task,
            a.len(),
            b.len()
        );
        if a != b {
            for index in 0..a.len().max(b.len()) {
                let left = a.get(index).map(Vec::as_slice).unwrap_or_default();
                let right = b.get(index).map(Vec::as_slice).unwrap_or_default();
                if left != right {
                    println!("row {index}:\n  django: {left:?}\n  rust:   {right:?}");
                }
            }
            bail!("database effects differ");
        }
        println!("PASS: database effects are identical");
    }

    if let Some(s3) = s3 {
        let (a, b) = s3.observe().await?;
        compare_objects(case.expect("S3 capture requires a case"), &a, &b)?;
    }
    if let (Some(a), Some(b)) = (django_http, rust_http) {
        compare_requests(case.expect("http capture requires a case"), &a, &b)?;
    }
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
async fn run_django(
    options: &TaskOptions,
    kwargs: &serde_json::Value,
    bucket: Option<&str>,
    live_url: Option<&str>,
) -> Result<()> {
    let root = repo_root();
    let env = dev_env(
        &options.a_db,
        options.a_redis_db,
        &options.vhost,
        bucket,
        live_url,
        true,
    )?;
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
    bucket: Option<&str>,
    live_url: Option<&str>,
) -> Result<()> {
    let root = repo_root();
    let worker = options
        .worker
        .clone()
        .unwrap_or_else(|| root.join("apps/server/target/debug/plane-worker"));
    let env = dev_env(
        &options.b_db,
        options.b_redis_db,
        &options.vhost,
        bucket,
        live_url,
        true,
    )?;

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

/// One case query, every column rendered as text (the query casts them).
async fn query_rows(admin: &DbAdmin, database: &str, sql: &str) -> Result<Vec<Vec<String>>> {
    let client = admin.connect(database).await?;
    let messages = client
        .simple_query(sql)
        .await
        .with_context(|| format!("running a case query on {database}"))?;
    let mut rows = Vec::new();
    for message in messages {
        if let tokio_postgres::SimpleQueryMessage::Row(row) = message {
            rows.push(
                (0..row.len())
                    .map(|index| row.get(index).unwrap_or("NULL").to_string())
                    .collect(),
            );
        }
    }
    Ok(rows)
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
fn dev_env(
    db: &str,
    redis_db: i64,
    vhost: &str,
    bucket: Option<&str>,
    live_url: Option<&str>,
    allow_loopback: bool,
) -> Result<BTreeMap<String, String>> {
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
    if let Some(bucket) = bucket {
        // `settings.AWS_STORAGE_BUCKET_NAME` and `S3Storage`'s
        // `AWS_S3_BUCKET_NAME` are read separately.
        env.insert("AWS_STORAGE_BUCKET_NAME".to_string(), bucket.to_string());
        env.insert("AWS_S3_BUCKET_NAME".to_string(), bucket.to_string());
    }
    if let Some(live_url) = live_url {
        // `settings.LIVE_URL = urljoin(LIVE_BASE_URL, LIVE_BASE_PATH)` with the
        // default path `/live/`.
        env.insert("LIVE_BASE_URL".to_string(), live_url.to_string());
    }
    if allow_loopback {
        // `difftest task`'s local fake HTTP server is on 127.0.0.1. Django's
        // runner patches its own `is_blocked_ip`; the worker's crawl handler
        // reads this switch. Neither is set in a deployment.
        env.insert("PLANE_DIFFTEST_ALLOW_LOOPBACK".to_string(), "1".to_string());
        env.insert(
            "PLANE_RS_DIFFTEST_ALLOW_LOOPBACK".to_string(),
            "1".to_string(),
        );
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
