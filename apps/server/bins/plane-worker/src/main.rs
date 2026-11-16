// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-worker`: the Rust-owned task worker.
//!
//! The only handler registered in Phase 4 is the test task `plane.jobs.echo`;
//! real tasks register here as their domains move (Phase 5). The queue,
//! claiming, retries and shutdown live in `plane-jobs`.

use std::time::Duration;

use anyhow::Context;
use plane_core::config::{Role, Settings};
use plane_db::DbPools;
use plane_jobs::listener::Listener;
use plane_jobs::worker::{JobContext, TaskRegistry, Worker, WorkerConfig};

fn main() -> anyhow::Result<()> {
    init_logging();
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
    let worker = Worker::new(db, registry(), config, worker_id.clone());
    tracing::info!(target: "plane.worker", worker = %worker_id, "starting");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        let listener = Listener::connect(&settings).context("starting the job listener")?;
        worker
            .run(shutdown_signal(), Some(listener))
            .await
            .context("running the worker")
    })?;
    Ok(())
}

fn registry() -> TaskRegistry {
    let mut registry = TaskRegistry::new();
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
