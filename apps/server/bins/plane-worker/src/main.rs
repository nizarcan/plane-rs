// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-worker`: the Rust-owned task worker.
//!
//! Boot: parse the environment like `plane-api`, create the pools, make sure
//! the `plane_jobs` schema exists (idempotent; the migrator owns it in
//! production), then register every handler from `plane-tasks`. The queue,
//! claiming, retries and shutdown live in `plane-jobs`.
//!
//! `--once` drains whatever is runnable and exits; `difftest task` uses it to
//! execute one enqueued job in a fresh process. Normal operation runs until
//! SIGTERM/SIGINT, drains in-flight handlers, and releases anything still
//! leased to this worker (`plane-jobs/src/worker.rs:151-232`).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use plane_core::config::{Role, Settings};
use plane_db::DbPools;
use plane_jobs::listener::Listener;
use plane_jobs::worker::{JobContext, TaskRegistry, Worker, WorkerConfig};
use plane_tasks::TaskContext;

/// `--difftest-feature` answers, for `difftest task`'s HTTP cases.
const DIFFTEST_FEATURE_ENABLED: &str = "difftest-feature=enabled";
const DIFFTEST_FEATURE_DISABLED: &str = "difftest-feature=disabled";

fn main() -> anyhow::Result<()> {
    // `difftest task` asks this before an HTTP case: the harness's fake server
    // is on loopback and the SSRF escape is compiled in only with the
    // `difftest` feature (D26). A plain probe must not start the worker, touch
    // the queue or print a log line.
    if std::env::args().any(|arg| arg == "--difftest-feature") {
        println!(
            "{}",
            if cfg!(feature = "difftest") {
                DIFFTEST_FEATURE_ENABLED
            } else {
                DIFFTEST_FEATURE_DISABLED
            }
        );
        return Ok(());
    }
    init_logging();
    let once = std::env::args().skip(1).any(|arg| arg == "--once");
    let settings = Settings::from_env(Role::Worker).map_err(|error| anyhow::anyhow!("{error}"))?;
    let db = DbPools::from_settings(&settings)?;
    let config = WorkerConfig {
        concurrency: env_usize("PLANE_RS_JOB_CONCURRENCY", 4).max(1),
        lease: Duration::from_secs(env_u64("PLANE_RS_JOB_LEASE_SECONDS", 60).max(1)),
        poll: Duration::from_millis(env_u64("PLANE_RS_JOB_POLL_MS", 1000).max(10)),
        drain: Duration::from_secs(env_u64("PLANE_RS_JOB_DRAIN_SECONDS", 30)),
    };
    let worker_id = format!(
        "{}:{}:{}",
        std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string()),
        std::process::id(),
        uuid::Uuid::new_v4()
    );
    let context = Arc::new(TaskContext::new(db.clone(), settings.clone()));
    let worker = Worker::new(db.clone(), registry(context), config, worker_id.clone());
    tracing::info!(target: "plane.worker", worker = %worker_id, once, "starting");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        // The migrator is the production owner; running the idempotent DDL
        // here keeps a fresh worker (and `difftest task`) usable without it.
        {
            let client = db
                .write_pool()
                .get()
                .await
                .context("connecting to the job database")?;
            plane_jobs::migrate(&**client)
                .await
                .context("creating the plane_jobs schema")?;
        }
        if once {
            // `difftest task` sets this to 1: the job under test only. Anything
            // it publishes stays in `plane_jobs` for the harness to compare.
            let limit = env_usize("PLANE_RS_JOB_ONCE_JOBS", 0);
            let processed = worker
                .drain_n(if limit == 0 { usize::MAX } else { limit })
                .await
                .context("draining the queue")?;
            tracing::info!(target: "plane.worker", processed, "drained");
            return Ok(());
        }
        let listener = Listener::connect(&settings).context("starting the job listener")?;
        worker
            .run(shutdown_signal(), Some(listener))
            .await
            .context("running the worker")
    })?;
    Ok(())
}

fn registry(context: Arc<TaskContext>) -> TaskRegistry {
    let mut registry = plane_tasks::registry(context);
    // The test-only echo task stays available for the Phase 4 exit check and
    // the worker tests.
    registry.register("plane.jobs.echo", |ctx: JobContext| async move {
        if let Some(ms) = ctx
            .payload
            .get("sleep_ms")
            .and_then(serde_json::Value::as_u64)
        {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
        tracing::info!(
            target: "plane.worker",
            job = %ctx.id,
            task = %ctx.task,
            attempts = ctx.attempts,
            "echo"
        );
        Ok(())
    });
    registry
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .json()
        .try_init();
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = terminate.recv() => {}
                }
                return;
            }
            Err(error) => {
                tracing::warn!(target: "plane.worker", %error, "cannot install SIGTERM handler");
            }
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
