// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `difftest task`: one Celery task, executed by both backends, its effects
//! compared: database rows (the whole schema), the tasks each side publishes,
//! S3 objects (bytes, content type, ACL) and the requests a task makes to a
//! local fake HTTP server.
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
//! A generated value the task stores is compared structurally: a UUID that was
//! not in that clone's before snapshot becomes `<NEW-UUID>`, and the case's own
//! `normalize` rules (the same ones its SQL rows use) apply to the diff values
//! too — a presigned URL normalises the same way everywhere. A fixture value
//! stays literal, so writing the wrong one is still caught.
//!
//! ## Expected failures
//!
//! `--expect-django-failure`/`--expect-worker-failure` declare that the task
//! itself is expected to fail (the upstream `hard_delete` `DO_NOTHING` bug,
//! `plans/CUTOVER.md` §6). The DB effects committed before the failure are
//! still compared, so B has to fail at the same statement.
//!
//! ## Case files
//!
//! The worker binary must be built with the `difftest` feature
//! (`cargo build -p plane-worker --features difftest`) when a case drives the
//! link crawler: its fake HTTP server lives on loopback, and the crawl
//! handler's SSRF escape is compiled in only with that feature. A default
//! worker ignores `PLANE_RS_DIFFTEST_ALLOW_LOOPBACK` by design
//! (`crates/plane-tasks/Cargo.toml`).
//!
//! With `--case` the run adds what the task's case file names
//! (`crate::task_case`): narrow SQL projections printed next to the
//! whole-schema diff, two agent-owned S3 buckets the tool creates, seeds, reads
//! and deletes, a `127.0.0.1` recorder whose responses the case scripts, and
//! (with `"smtp": {}`) one `fake_smtp.py` listener per side. Without `--case`
//! the whole-schema diff and the published-work comparison still apply, so the
//! P0 fixtures keep working.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use crate::celery::{CeleryDrain, CeleryMessage, RabbitAdmin, amqp_url_with_vhost};
use crate::db::{DbAdmin, DbDiff, diff_snapshots};
use crate::redis::{RedisAdmin, RedisDiff, RedisSnapshotter, diff_snapshots as redis_diff};
use crate::s3::S3Admin;
use crate::task_case::{
    AttemptsCase, Recorder, SideBuckets, SmtpCase, TaskCase, compare_objects, compare_requests,
    compare_rows, compare_smtp_logs,
};
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
            case_path: None,
            s3_endpoint: "http://127.0.0.1:59000".to_string(),
            s3_access_key: "access-key".to_string(),
            s3_secret_key: "secret-key".to_string(),
            s3_bucket_a: None,
            s3_bucket_b: None,
            live_url: None,
            expect_django_failure: None,
            expect_worker_failure: None,
        }
    }
}

/// Split a fixture into positional and keyword arguments.
///
/// `{"args": [...], "kwargs": {...}}` (only those two keys, `args` an array)
/// is a positional call; anything else is a keyword-argument object.
fn split_call(
    value: &serde_json::Value,
) -> (
    Vec<serde_json::Value>,
    serde_json::Map<String, serde_json::Value>,
) {
    let object = value.as_object().cloned().unwrap_or_default();
    let is_positional = object.get("args").is_some_and(serde_json::Value::is_array)
        && object.keys().all(|key| key == "args" || key == "kwargs");
    if !is_positional {
        return (Vec::new(), object);
    }
    let args = object
        .get("args")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let kwargs = object
        .get("kwargs")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    (args, kwargs)
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
        let python = plane_oracle::python();
        let dir = std::env::temp_dir().join(format!("plane-difftest-smtp-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        // Port 0: the kernel picks a free port, so two `difftest task` runs
        // (another agent's and this one's) never deliver into each other's
        // listener. The script writes the bound port to `--port-file`; the
        // HTTP recorder already binds `127.0.0.1:0` the same way.
        let start_one = |name: &str| -> Result<SmtpListener> {
            let path = dir.join(format!("{name}.jsonl"));
            let _ = std::fs::remove_file(&path);
            let port_path = dir.join(format!("{name}.port"));
            let _ = std::fs::remove_file(&port_path);
            let child = std::process::Command::new(&python)
                .arg(&script)
                .arg("--port")
                .arg("0")
                .arg("--port-file")
                .arg(&port_path)
                .arg("--output")
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| format!("starting fake_smtp.py for {name}"))?;
            let port = wait_for_port_file(&port_path)?;
            Ok(SmtpListener { child, path, port })
        };
        let _ = case;
        let a = start_one("a")?;
        let b = start_one("b")?;
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

/// The port `fake_smtp.py` bound, read from the file it writes after `bind`.
///
/// The child writes the file before it starts serving, so a short poll is
/// enough; a missing file after the budget is a start-up failure, not a hang.
fn wait_for_port_file(path: &std::path::Path) -> Result<u16> {
    for _ in 0..200 {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(port) = text.trim().parse::<u16>()
            && port != 0
        {
            return Ok(port);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    bail!(
        "fake_smtp.py did not write its bound port to {}",
        path.display()
    )
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
    // A task whose call sites are positional (`forgot_password`, `magic_link`,
    // the e-mail tasks) writes `{"args": [...], "kwargs": {...}}`; the plain
    // object form stays the keyword-argument shorthand. Split after the
    // `{{http}}` substitution above, so both forms carry the recorder URL.
    let (call_args, call_kwargs) = split_call(&kwargs);
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
    // A case with an `http` block (or `--live-url`) drives a loopback fake
    // server, which a worker built without the `difftest` feature blocks. That
    // failure looks like a plausible database diff (`titl: None`), so refuse
    // the case up front instead of reporting it as a mismatch.
    if case.as_ref().is_some_and(|case| case.http.is_some()) || options.live_url.is_some() {
        let worker = worker_path(options);
        if !worker_supports_difftest(&worker)? {
            bail!(
                "{} was built without the `difftest` feature; this case drives a loopback \
                 server and would fail silently. Rebuild it: \
                 CARGO_PROFILE_DEV_DEBUG=0 cargo build -p plane-worker --features difftest",
                worker.display()
            );
        }
    }
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
    let setup = setup.map(|sql| match &recorder {
        // A webhook fixture stores its target URL in a row, so the fake HTTP
        // server's base URL has to reach the setup SQL as well as the kwargs.
        Some(recorder) => sql.replace("{{http}}", recorder.url()),
        None => sql,
    });
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
        &call_args,
        &call_kwargs,
        &admin,
        &rabbit,
        Captures {
            case: case.as_ref(),
            recorder: recorder.as_ref(),
            s3: s3.as_ref(),
            live_url: live_url.as_deref(),
        },
    )
    .await;

    if let Some(smtp) = &mut smtp {
        smtp.finish()?;
    }
    // Only compare transcripts when both sides ran: a failed side leaves one
    // log empty and the real error is `outcome`.
    let smtp_outcome = if outcome.is_ok() {
        match (&case, &smtp) {
            (Some(case), Some(smtp)) if case.smtp.is_some() => {
                compare_smtp_logs(&smtp.a.path, &smtp.b.path)
            }
            _ => Ok(()),
        }
    } else {
        Ok(())
    };

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
    outcome?;
    smtp_outcome
}

/// One side's published work, in the order it left the process.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct Outgoing {
    task: String,
    args: serde_json::Value,
    kwargs: serde_json::Value,
}

/// The per-case capture handles a run may have.
struct Captures<'a> {
    case: Option<&'a TaskCase>,
    recorder: Option<&'a Recorder>,
    s3: Option<&'a S3Capture>,
    live_url: Option<&'a str>,
}

async fn execute(
    options: &TaskOptions,
    call_args: &[serde_json::Value],
    call_kwargs: &serde_json::Map<String, serde_json::Value>,
    admin: &DbAdmin,
    rabbit: &RabbitAdmin,
    captures: Captures<'_>,
) -> Result<()> {
    let case = captures.case;
    // A case cannot name the caller's buckets; both sides' own bucket names
    // become `<BUCKET>` before the case's normalisers run (D26).
    let buckets = SideBuckets {
        a: options.s3_bucket_a.as_deref(),
        b: options.s3_bucket_b.as_deref(),
    };
    let recorder = captures.recorder;
    let bucket_a = captures.s3.map(|s3| s3.bucket_a.as_str());
    let bucket_b = captures.s3.map(|s3| s3.bucket_b.as_str());
    let live_url = captures.live_url;
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

    let mut redis_a = redis_side(&options.a_db, options.a_redis_db, &options.vhost).await?;
    let mut redis_b = redis_side(&options.b_db, options.b_redis_db, &options.vhost).await?;
    redis_a.0.flush_db(options.a_redis_db).await?;
    redis_b.0.flush_db(options.b_redis_db).await?;
    if let Some(case) = case {
        for seed in &case.redis {
            redis_a
                .0
                .set_raw(
                    options.a_redis_db,
                    &seed.key,
                    seed.value.as_bytes(),
                    seed.ttl,
                )
                .await?;
            redis_b
                .0
                .set_raw(
                    options.b_redis_db,
                    &seed.key,
                    seed.value.as_bytes(),
                    seed.ttl,
                )
                .await?;
        }
    }
    let before_redis_a = redis_a.1.snapshot().await?.snapshot;
    let before_redis_b = redis_b.1.snapshot().await?.snapshot;

    let started: DateTime<Utc> = Utc::now();
    let before_a = admin.snapshot(&options.a_db, &catalog).await?.snapshot;
    let attempts = case.and_then(|case| case.attempts.as_ref());
    let http_start = recorder.map_or(0, Recorder::request_count);
    let django = run_django(
        options,
        call_args,
        call_kwargs,
        bucket_a,
        live_url,
        attempts,
    )
    .await?;
    // Capture immediately: later requests append to the same log.
    let django_http = recorder.map(|recorder| recorder.slice(http_start));
    let messages_a = drain_messages(rabbit, &drain, &options.vhost).await?;
    let after_redis_a = redis_a.1.snapshot().await?.snapshot;
    let before_b = admin.snapshot(&options.b_db, &catalog).await?.snapshot;
    let rust_start = recorder.map_or(0, Recorder::request_count);
    let worker = run_worker(
        options,
        call_args,
        call_kwargs,
        admin,
        bucket_b,
        live_url,
        attempts,
    )
    .await?;
    let rust_http = recorder.map(|recorder| recorder.slice(rust_start));
    let messages_b = drain_messages(rabbit, &drain, &options.vhost).await?;
    let jobs_b = rust_jobs(admin, &options.b_db, &worker.job_id).await?;
    let after_redis_b = redis_b.1.snapshot().await?.snapshot;
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
    normalise_outgoing(&mut outgoing_a, case);
    normalise_outgoing(&mut outgoing_b, case);
    // `python_value` decodes a `uuid.UUID` envelope, which is the transport's
    // representation of the same Python value; a task still delivered through
    // Celery on both sides is additionally compared as raw JSON, so an
    // envelope that one side omitted is still a difference.
    let raw_a = raw_celery_payloads(&messages_a);
    let raw_b = raw_celery_payloads(&messages_b);
    if raw_a != raw_b {
        for index in 0..raw_a.len().max(raw_b.len()) {
            let left = raw_a.get(index);
            let right = raw_b.get(index);
            if left != right {
                println!("celery {index}:\n  django: {left:?}\n  rust:   {right:?}");
            }
        }
        bail!("Celery wire payloads differ");
    }

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
    normalise_diff(
        &mut diff_a,
        started,
        case,
        &known_uuids(&before_a),
        options.s3_bucket_a.as_deref(),
    );
    normalise_diff(
        &mut diff_b,
        started,
        case,
        &known_uuids(&before_b),
        options.s3_bucket_b.as_deref(),
    );
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

    // The case's own projections, when it names any: a narrow, readable view
    // of the same effects (the whole-schema diff above is authoritative).
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
            compare_rows(case, &query.label, buckets, &a, &b)?;
        }
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

    // Redis is a side effect too: `issue_activity` writes the request origin
    // under the issue id with `EX 600` (`issue_activities_task.py:1574-1577`).
    // Each side's DB was flushed first, so only the task's writes appear. The
    // TTL is measured a few milliseconds apart; everything else is exact.
    let redis_a_diff = redis_diff(&before_redis_a, &after_redis_a);
    let redis_b_diff = redis_diff(&before_redis_b, &after_redis_b);
    if !same_redis_diff(&redis_a_diff, &redis_b_diff) {
        println!("redis django: {}", redis_a_diff.to_json());
        println!("redis rust:   {}", redis_b_diff.to_json());
        bail!("redis effects differ");
    }
    println!(
        "PASS: redis effects are identical ({} keys)",
        redis_a_diff.total_changes()
    );

    if let Some(s3) = captures.s3 {
        let (a, b) = s3.observe().await?;
        compare_objects(case.expect("S3 capture requires a case"), buckets, &a, &b)?;
    }
    if let (Some(a), Some(b)) = (django_http, rust_http) {
        compare_requests(case.expect("http capture requires a case"), buckets, &a, &b)?;
    }
    Ok(())
}

/// A fresh `RedisAdmin` and snapshotter for one side's logical database.
async fn redis_side(
    database: &str,
    index: i64,
    vhost: &str,
) -> Result<(RedisAdmin, RedisSnapshotter)> {
    let env = dev_env(database, index, vhost, None, None, false)?;
    let url = env
        .get("REDIS_URL")
        .ok_or_else(|| anyhow::anyhow!("django-env.sh did not set REDIS_URL"))?;
    let admin = RedisAdmin::from_url(url)?;
    let snapshotter = admin.snapshotter(index).await?;
    Ok((admin, snapshotter))
}

/// Compare two Redis diffs, allowing the `PTTL` a few seconds of skew (the
/// two sides run at different instants; the value and key are exact).
fn same_redis_diff(a: &RedisDiff, b: &RedisDiff) -> bool {
    if a.removed != b.removed
        || a.added.len() != b.added.len()
        || !a.changed.is_empty()
        || !b.changed.is_empty()
    {
        return false;
    }
    for (key, entry) in &a.added {
        let Some(other) = b.added.get(key) else {
            return false;
        };
        if entry.kind != other.kind
            || entry.size != other.size
            || entry.value != other.value
            || (entry.ttl_ms - other.ttl_ms).abs() > 10_000
        {
            return false;
        }
    }
    true
}

/// Normalise the values `IssueActivitySerializer` renders that are not part
/// of the effect: the row's random uuid and its two insert timestamps. The
/// array is sorted because a Python `set` decides the label/assignee order
/// (`PYTHONHASHSEED`), and D26 compares inserted rows as a multiset.
fn normalise_activity_payload(value: &mut serde_json::Value) {
    let serde_json::Value::Array(items) = value else {
        return;
    };
    for item in items.iter_mut() {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        object.insert("id".to_string(), serde_json::json!("<ID>"));
        for key in ["created_at", "updated_at"] {
            if object.contains_key(key) {
                object.insert(key.to_string(), serde_json::json!("<TS>"));
            }
        }
    }
    items.sort_by_cached_key(|item| serde_json::to_string(item).unwrap_or_default());
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

/// A Celery message in the same Python-value shape as a `plane_jobs` row.
///
/// `task.apply()` captures what the task published, and kombu's JSON envelope
/// renders a `uuid.UUID` argument as `{"__type__": "uuid", "__value__": …}`.
/// The queue representation (`Job::from_message`) stores the value the Python
/// task receives at run time, so both sides are decoded through
/// [`crate::ownership::python_value`] before the comparison (`DECISIONS.md`
/// D28). The raw wire payload of a task that both sides still deliver through
/// Celery is compared separately by [`raw_celery_payloads`], so this decoding
/// cannot hide a wire difference there.
fn message_to_outgoing(message: &CeleryMessage) -> Outgoing {
    Outgoing {
        task: message.task.clone().unwrap_or_default(),
        args: crate::ownership::python_value(&message.args),
        kwargs: crate::ownership::python_value(&message.kwargs),
    }
}

/// The wire payload of the messages whose task is still Celery-owned, in
/// order. A Rust-owned task's Celery message is A-only, so it is excluded; a
/// task both sides publish (B's handler publishing a Celery-owned task) must
/// match byte for byte here.
///
/// `epoch=int(timezone.now().timestamp())` is the one argument that carries
/// the clock (`issue_automation_task.py:82,145`); the two sides run a second
/// apart, so it is replaced by `<EPOCH>` before the comparison, exactly as
/// [`normalise_outgoing`] does for the decoded form.
fn raw_celery_payloads(messages: &[CeleryMessage]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| {
            message
                .task
                .as_deref()
                .is_some_and(|task| !crate::ownership::rust_owned(task))
        })
        .map(|message| {
            let mut kwargs = message.kwargs.clone();
            if let Some(object) = kwargs.as_object_mut() {
                if object.contains_key("epoch") {
                    object.insert("epoch".to_string(), serde_json::json!("<EPOCH>"));
                }
                // `notifications`' `issue_activities_created` is a JSON string
                // holding the created rows. Their ids, `auto_now` timestamps
                // and (for label/assignee sets) order are random, so replace
                // only those; everything else, including each row's key order,
                // is still compared.
                if message.task.as_deref() == Some("plane.bgtasks.notification_task.notifications")
                    && let Some(serde_json::Value::String(text)) =
                        object.get("issue_activities_created")
                    && let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text)
                {
                    normalise_activity_payload(&mut value);
                    object.insert(
                        "issue_activities_created".to_string(),
                        serde_json::Value::String(
                            serde_json::to_string(&value).unwrap_or_default(),
                        ),
                    );
                }
            }
            format!(
                "{}\u{0}{}\u{0}{}",
                message.task.clone().unwrap_or_default(),
                serde_json::to_string(&message.args).unwrap_or_default(),
                serde_json::to_string(&kwargs).unwrap_or_default()
            )
        })
        .collect()
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
///
/// The case's `normalize` rules apply to every outgoing argument string too.
/// A webhook delivery failure publishes its transport error as the
/// deactivation e-mail's `reason`, and the two HTTP stacks' messages differ by
/// design (D9); the case masks that one string rather than the whole argument.
fn normalise_outgoing(outgoing: &mut [Outgoing], case: Option<&TaskCase>) {
    for entry in outgoing {
        if let Some(case) = case {
            normalise_published_strings(&mut entry.args, case);
            normalise_published_strings(&mut entry.kwargs, case);
        }
        let Some(kwargs) = entry.kwargs.as_object_mut() else {
            continue;
        };
        if kwargs.contains_key("epoch") {
            kwargs.insert("epoch".to_string(), serde_json::json!("<EPOCH>"));
        }
        // `notifications(issue_activities_created=json.dumps(...))`: the
        // serialised rows carry a fresh uuid and two `auto_now` timestamps.
        // Parse the string so a key-order difference cannot matter.
        if let Some(serde_json::Value::String(text)) = kwargs.get("issue_activities_created")
            && let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text)
        {
            normalise_activity_payload(&mut value);
            kwargs.insert("issue_activities_created".to_string(), value);
        }
    }
}

/// Apply the case's regex rules to every string inside a published argument
/// (objects, arrays and string values).
fn normalise_published_strings(value: &mut serde_json::Value, case: &TaskCase) {
    match value {
        serde_json::Value::String(text) => *text = case.normalize_text(text),
        serde_json::Value::Array(items) => {
            for item in items {
                normalise_published_strings(item, case);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                normalise_published_strings(item, case);
            }
        }
        _ => {}
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
    /// Celery's `Retry.when` (a jittered countdown in seconds), when the
    /// single-attempt run ended in `RETRY`.
    countdown: Option<f64>,
}

/// A = `task.apply()` in the real Django, with the dev stack's environment.
async fn run_django(
    options: &TaskOptions,
    call_args: &[serde_json::Value],
    call_kwargs: &serde_json::Map<String, serde_json::Value>,
    bucket: Option<&str>,
    live_url: Option<&str>,
    attempts: Option<&AttemptsCase>,
) -> Result<DjangoOutcome> {
    let root = repo_root();
    let env = dev_env(
        &options.a_db,
        options.a_redis_db,
        &options.vhost,
        bucket,
        live_url,
        true,
    )?;
    let mut command = Command::new(plane_oracle::python());
    command
        .arg(root.join("apps/server/tools/jobs/run_django_task.py"))
        .arg(&options.task)
        .arg(serde_json::to_string(call_kwargs)?)
        .arg(serde_json::to_string(call_args)?);
    if let Some(attempts) = attempts {
        // `request.retries` is 0-based; the case names attempts from 1.
        command.arg((attempts.number - 1).to_string());
    }
    let output = command
        .current_dir(plane_oracle::api_dir())
        .env_clear()
        .env("PYTHONPATH", plane_oracle::api_dir())
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
        countdown: result.get("countdown").and_then(serde_json::Value::as_f64),
    };
    if let Some(attempts) = attempts {
        if attempts.expect_retry {
            if outcome.status != "RETRY" {
                bail!(
                    "Django task was expected to retry on attempt {} but ended with {}: {}",
                    attempts.number,
                    outcome.status,
                    outcome.result
                );
            }
            let countdown = outcome.countdown.ok_or_else(|| {
                anyhow::anyhow!("Django ended in RETRY without a numeric countdown")
            })?;
            if countdown < 0.0 || countdown > f64::from(attempts.retry_backoff_seconds) {
                bail!(
                    "Django retry countdown {countdown}s is outside [0, {}]",
                    attempts.retry_backoff_seconds
                );
            }
            println!(
                "django: RETRY countdown={countdown}s (policy 0-{}s)",
                attempts.retry_backoff_seconds
            );
        } else if outcome.status != "SUCCESS" {
            bail!(
                "Django task ended with {} on attempt {}: {}",
                outcome.status,
                attempts.number,
                outcome.result
            );
        }
        return Ok(outcome);
    }
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

/// The worker binary a run will use.
fn worker_path(options: &TaskOptions) -> PathBuf {
    options
        .worker
        .clone()
        .unwrap_or_else(|| repo_root().join("apps/server/target/debug/plane-worker"))
}

/// Whether `worker` was compiled with the `difftest` feature.
///
/// The worker answers `--difftest-feature` with `difftest-feature=enabled` or
/// `difftest-feature=disabled` and exits without starting. A binary that
/// predates the flag prints nothing, which is treated as "without".
fn worker_supports_difftest(worker: &std::path::Path) -> Result<bool> {
    let output = std::process::Command::new(worker)
        .arg("--difftest-feature")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("probing {} for the difftest feature", worker.display()))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "difftest-feature=enabled")
}

/// B = insert a job row, then `plane-worker --once` in a fresh process.
async fn run_worker(
    options: &TaskOptions,
    call_args: &[serde_json::Value],
    call_kwargs: &serde_json::Map<String, serde_json::Value>,
    admin: &DbAdmin,
    bucket: Option<&str>,
    live_url: Option<&str>,
    attempts: Option<&AttemptsCase>,
) -> Result<WorkerOutcome> {
    let root = repo_root();
    let worker = worker_path(options);
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

    let payload = serde_json::json!({"args": call_args, "kwargs": call_kwargs});
    let client = admin.connect(&options.b_db).await?;
    let id = uuid::Uuid::new_v4().to_string();
    // `tokio-postgres` is built without its `uuid`/`serde_json` features in
    // this workspace (feature isolation), so the two generated values are
    // inlined with SQL quoting rather than bound.
    // A retry case starts the row at `number - 1`: `claim` increments it, so
    // the handler sees the case's attempt number. The retry policy is the one
    // the production enqueue path applies (`plane-jobs::policy`), stated by
    // the case because the harness cannot link the queue crate.
    let (attempts_before, max_retries, backoff, jitter) = match attempts {
        Some(attempts) => (
            attempts.number - 1,
            attempts.max_retries,
            attempts.retry_backoff_seconds,
            attempts.retry_jitter,
        ),
        None => (0, 0, 0, false),
    };
    let insert = format!(
        "INSERT INTO plane_jobs.jobs \
         (id, task, payload, max_retries, retry_backoff_seconds, retry_jitter, attempts) \
         VALUES ('{id}'::uuid, '{}', '{}'::json, {max_retries}, {backoff}, {jitter}, {attempts_before})",
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
        // Execute only the harness-inserted job. A Rust-owned task the handler
        // publishes stays queued so `rust_jobs` compares its payload.
        .env("PLANE_RS_JOB_ONCE_JOBS", "1")
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
            &format!(
                "SELECT state, last_error, attempts, \
                        EXTRACT(EPOCH FROM (run_at - updated_at))::float8 \
                 FROM plane_jobs.jobs WHERE id = '{id}'::uuid"
            ),
            &[],
        )
        .await
        .context("reading the job row")?;
    let state: String = row.try_get(0)?;
    let last_error: Option<String> = row.try_get(1)?;
    let job_attempts: i32 = row.try_get(2)?;
    let run_ahead: f64 = row.try_get(3)?;
    if let Some(attempts) = attempts {
        if job_attempts != attempts.number {
            bail!(
                "Rust job ran as attempt {job_attempts}, not the case's {}",
                attempts.number
            );
        }
        if attempts.expect_retry {
            if state != "failed" {
                bail!(
                    "Rust job was expected to schedule a retry on attempt {} but ended in state \
                     {state}: {}",
                    attempts.number,
                    last_error.unwrap_or_else(|| "no error recorded".to_string())
                );
            }
            let bound = f64::from(attempts.retry_backoff_seconds);
            if run_ahead < -0.001 || run_ahead > bound {
                bail!("Rust retry is scheduled {run_ahead}s ahead, outside [0, {bound}]");
            }
            println!(
                "rust: RETRY run_at=+{run_ahead}s (policy 0-{bound}s), last_error={:?}",
                last_error.unwrap_or_default()
            );
            return Ok(WorkerOutcome { state, job_id: id });
        }
    }
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

/// UUID-shaped text, for the generated-value rule below.
static UUID_TEXT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
        .expect("static UUID regex")
});

/// Every UUID-shaped string a database held before the task ran.
fn known_uuids(snapshot: &crate::db::DbSnapshot) -> std::collections::HashSet<String> {
    let mut known = std::collections::HashSet::new();
    for rows in snapshot.tables.values() {
        for row in rows.values() {
            collect_uuids(row, &mut known);
        }
    }
    known
}

fn collect_uuids(value: &serde_json::Value, known: &mut std::collections::HashSet<String>) {
    match value {
        serde_json::Value::String(text) => {
            for found in UUID_TEXT.find_iter(text) {
                known.insert(found.as_str().to_string());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_uuids(item, known);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_uuids(item, known);
            }
        }
        _ => {}
    }
}

/// Normalise a value the diff prints: the case's normalisers, then a UUID the
/// run generated (it was not in the before snapshot) as `<NEW-UUID>`.
fn normalise_value(
    value: &mut serde_json::Value,
    case: Option<&TaskCase>,
    known: &std::collections::HashSet<String>,
    bucket: Option<&str>,
) {
    normalise_timestamp(value);
    let Some(text) = value.as_str() else {
        return;
    };
    // The caller's bucket name, not the case's: see `SideBuckets`.
    let text = match bucket.filter(|bucket| !bucket.is_empty()) {
        Some(bucket) if text.contains(bucket) => std::borrow::Cow::Owned(
            text.replace(bucket, crate::task_case::SideBuckets::PLACEHOLDER),
        ),
        _ => std::borrow::Cow::Borrowed(text),
    };
    let mut out = case.map_or_else(|| text.to_string(), |case| case.normalize_text(&text));
    out = UUID_TEXT
        .replace_all(&out, |found: &regex::Captures<'_>| {
            if known.contains(found.get(0).map_or("", |m| m.as_str())) {
                found.get(0).map_or("", |m| m.as_str()).to_string()
            } else {
                "<NEW-UUID>".to_string()
            }
        })
        .into_owned();
    *value = serde_json::Value::String(out);
}

/// Normalise the clock, generated primary keys and generated values. See the
/// module docs.
///
/// Every value that parses as an RFC 3339 instant becomes `<TS>`: the two
/// fixture applications call `now()` a few milliseconds apart, so the *same*
/// unchanged fixture row would otherwise differ between the clones. A column
/// the task moved still shows up as a `changed` entry, because the harness
/// diffs before and after within each database — normalising the values only
/// hides the clock, not the update.
fn normalise_diff(
    diff: &mut DbDiff,
    _started: DateTime<Utc>,
    case: Option<&TaskCase>,
    known: &std::collections::HashSet<String>,
    bucket: Option<&str>,
) {
    for table in diff.tables.values_mut() {
        for row in table.inserted.iter_mut().chain(table.deleted.iter_mut()) {
            if let Some(object) = row.as_object_mut() {
                for (column, value) in object.iter_mut() {
                    if column == "id" {
                        *value = serde_json::json!("<ID>");
                    } else if column == "epoch" {
                        *value = serde_json::json!("<EPOCH>");
                    } else {
                        normalise_value(value, case, known, bucket);
                    }
                }
            }
        }
        // The snapshot iterates a map keyed by primary key, so a table with
        // several inserts would otherwise compare in each side's random-uuid
        // order. The diff is a multiset comparison.
        table.inserted.sort_by_key(canonical_json);
        table.deleted.sort_by_key(canonical_json);
        for updated in &mut table.updated {
            if let Some(changed) = updated.get_mut("changed").and_then(|v| v.as_object_mut()) {
                for (column, entry) in changed.iter_mut() {
                    if let Some(pair) = entry.as_object_mut() {
                        for side in ["a", "b"] {
                            if let Some(value) = pair.get_mut(side) {
                                if column == "epoch" {
                                    *value = serde_json::json!("<EPOCH>");
                                } else {
                                    normalise_value(value, case, known, bucket);
                                }
                            }
                        }
                    }
                }
            }
        }
        // The rows are keyed by primary key, and two sides insert independent
        // random UUIDs, so their primary-key order is not an effect. Compare
        // each list as a multiset: sort the rows now that the ids are `<ID>`.
        table.inserted.sort_by_cached_key(canonical_row);
        table.deleted.sort_by_cached_key(canonical_row);
        table.updated.sort_by_cached_key(canonical_row);
    }
}

fn canonical_row(row: &serde_json::Value) -> String {
    serde_json::to_string(row).unwrap_or_default()
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

fn canonical_json(row: &serde_json::Value) -> String {
    serde_json::to_string(row).unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    /// The probe is what keeps a default-build worker from turning an HTTP
    /// case into a plausible database diff (D26/D29).
    #[test]
    fn the_worker_probe_reads_the_feature_answer() {
        let dir = std::env::temp_dir().join(format!("plane-worker-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let script = dir.join("enabled");
        std::fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = \"--difftest-feature\" ]; then echo difftest-feature=enabled; fi\n",
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(worker_supports_difftest(&script).expect("probe"));

        let plain = dir.join("disabled");
        std::fs::write(&plain, "#!/bin/sh\nexit 0\n").expect("script");
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(!worker_supports_difftest(&plain).expect("probe"));
    }

    use std::collections::BTreeMap;

    use super::*;
    use crate::db::{DbDiff, TableDiff};

    fn diff_with_two_inserts(first: &str, second: &str) -> DbDiff {
        let mut table = TableDiff::default();
        table.inserted.push(serde_json::json!({
            "id": first,
            "created_at": "2026-11-19T10:00:00+00:00",
            "name": "a",
        }));
        table.inserted.push(serde_json::json!({
            "id": second,
            "created_at": "2026-11-19T10:00:00+00:00",
            "name": "b",
        }));
        let mut diff = DbDiff::default();
        diff.tables.insert("page_logs".to_string(), table);
        diff
    }

    #[test]
    fn independently_inserted_rows_compare_as_a_multiset() {
        // The two sides generate independent uuid4 primary keys; the diff is
        // keyed by pk, so the lists arrive in different orders.
        let mut django = diff_with_two_inserts(
            "00000000-0000-0000-0000-000000000001",
            "00000000-0000-0000-0000-000000000002",
        );
        let mut rust = diff_with_two_inserts(
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee",
        );
        normalise_diff(
            &mut django,
            Utc::now(),
            None,
            &std::collections::HashSet::new(),
            None,
        );
        normalise_diff(
            &mut rust,
            Utc::now(),
            None,
            &std::collections::HashSet::new(),
            None,
        );
        assert_eq!(django, rust);
    }

    fn message(task: &str, args: serde_json::Value, kwargs: serde_json::Value) -> CeleryMessage {
        CeleryMessage {
            queue: "celery".to_string(),
            task: Some(task.to_string()),
            headers: BTreeMap::new(),
            properties: BTreeMap::new(),
            args,
            kwargs,
            body_extra: None,
        }
    }

    #[test]
    fn a_celery_uuid_envelope_decodes_to_its_python_value() {
        let id = "00000000-0000-0000-0000-000000000020";
        let envelope = serde_json::json!({
            "__type__": "uuid",
            "__value__": {"hex": id.replace('-', "")},
        });
        let a = message(
            "plane.bgtasks.deletion_task.soft_delete_related_objects",
            serde_json::json!([envelope.clone()]),
            serde_json::json!({"model_name": "project", "entity_identifier": envelope}),
        );
        let outgoing = message_to_outgoing(&a);
        assert_eq!(outgoing.args, serde_json::json!([id]));
        assert_eq!(outgoing.kwargs["entity_identifier"], id);
        // The same delivery queued by B: `Job::from_message` stores the value.
        let job = Outgoing {
            task: "plane.bgtasks.deletion_task.soft_delete_related_objects".to_string(),
            args: serde_json::json!([id]),
            kwargs: serde_json::json!({"model_name": "project", "entity_identifier": id}),
        };
        assert_eq!(outgoing, job);
    }

    #[test]
    fn a_bound_port_is_read_back_from_the_port_file() {
        let path = std::env::temp_dir().join(format!(
            "plane-difftest-port-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |value| value.as_nanos())
        ));
        std::fs::write(&path, "54321").expect("write port file");
        assert_eq!(wait_for_port_file(&path).expect("port"), 54321);
        // An unreadable file is a start-up failure, not a hang.
        std::fs::write(&path, "not a port").expect("write port file");
        assert!(wait_for_port_file(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_celery_owned_wire_difference_is_still_a_difference() {
        // Never ported (owner decision 5), so its wire form is compared.
        let task = "plane.license.bgtasks.telemetry_metrics.push_instance_metrics";
        let id = "00000000-0000-0000-0000-000000000020";
        let envelope = serde_json::json!({
            "__type__": "uuid",
            "__value__": {"hex": id.replace('-', "")},
        });
        let a = message(task, serde_json::json!([envelope]), serde_json::json!({}));
        let b = message(task, serde_json::json!([id]), serde_json::json!({}));
        assert_ne!(raw_celery_payloads(&[a]), raw_celery_payloads(&[b]));
        // A Rust-owned delivery is excluded from the wire comparison.
        let owned = message(
            "plane.bgtasks.webhook_task.webhook_send_task",
            serde_json::json!([]),
            serde_json::json!({}),
        );
        assert!(raw_celery_payloads(&[owned]).is_empty());
    }
}
