// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-beat`: the scheduler that can run as more than one replica.
//!
//! The static 13 entries are seeded on startup. Only entries with
//! `plane_jobs.schedules.owner = 'rust'` fire here; every real entry stays
//! Celery-owned in Phase 4. `PLANE_RS_JOBS_TEST_SCHEDULE_SECONDS` adds the
//! test-only entry used by `tools/jobs/beat_exactly_once.sh`.

use std::time::Duration;

use anyhow::Context;
use plane_core::config::{Role, Settings};
use plane_db::DbPools;
use plane_jobs::beat::{Beat, BeatConfig};

fn main() -> anyhow::Result<()> {
    init_logging();
    let settings = Settings::from_env(Role::Beat).map_err(|error| anyhow::anyhow!("{error}"))?;
    let db = DbPools::from_settings(&settings)?;
    let config = BeatConfig {
        poll: Duration::from_millis(env_u64("PLANE_RS_BEAT_POLL_MS", 1000).max(10)),
        handover_gap: Duration::from_secs(env_u64("PLANE_RS_JOBS_HANDOVER_GAP_SECONDS", 30)),
        handover_margin: Duration::from_secs(
            env_u64("PLANE_RS_JOBS_HANDOVER_MARGIN_SECONDS", 15).max(1),
        ),
    };
    let test_interval = std::env::var("PLANE_RS_JOBS_TEST_SCHEDULE_SECONDS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|seconds| *seconds > 0);
    if test_interval.is_some() {
        tracing::warn!(
            target: "plane.beat",
            "test schedule enabled (PLANE_RS_JOBS_TEST_SCHEDULE_SECONDS)"
        );
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        // The migrator owns the schema; beat is a consumer like the worker.
        plane_bootstrap::wait::wait_for_db(&db)
            .await
            .context("waiting for the database")?;
        plane_bootstrap::wait::wait_for_migrations(&db)
            .await
            .context("waiting for migrations")?;
        let beat = Beat::new(db, config, test_interval)
            .await
            .context("seeding the schedule")?;
        tracing::info!(target: "plane.beat", "starting");
        beat.run(shutdown_signal())
            .await
            .context("running the scheduler")
    })
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
                tracing::warn!(target: "plane.beat", %error, "cannot install SIGTERM handler");
            }
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
