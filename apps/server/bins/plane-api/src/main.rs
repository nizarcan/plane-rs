// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-api`: the HTTP server.
//!
//! Thin on purpose (`RUST_REWRITE.md`, "Target architecture"): load the
//! configuration, install logging, build the runtime, the route table and the
//! (lazy) database pools, then serve until `SIGTERM`/`SIGINT`.
//!
//! Startup is Django's: a configuration value Django would reject at import time
//! is a non-zero exit with the variable named (ENV.md §0). Postgres, Redis and
//! RabbitMQ are *not* contacted: the pools connect lazily, like Django's, so the
//! process serves `/` while they are down.

mod app;
mod handlers;
mod host;
mod logging;
mod middleware;
mod response;
mod routes;
#[cfg(feature = "test-endpoints")]
mod test_endpoints;

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context;
use app::AppState;
use axum::Router;
use mimalloc::MiMalloc;
use plane_core::config::{Role, Settings};
use plane_db::DbPools;
use routes::RouteTable;

/// The only `unsafe`-adjacent item the conventions allow
/// (`apps/server/AGENTS.md`, "Rust conventions"; DECISIONS.md D3). The
/// declaration itself is safe code; `MiMalloc` implements `GlobalAlloc`.
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> ExitCode {
    // Logging first: `Settings::from_env` logs the SECRET_KEY warnings.
    logging::init_from_env();
    match start() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(target: "plane.api", "{error:#}");
            eprintln!("plane-api: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn start() -> anyhow::Result<()> {
    let settings = Settings::from_env(Role::Api)
        .map_err(|error| anyhow::anyhow!("configuration error: {error}"))?;
    for warning in &settings.warnings {
        tracing::warn!(
            target: "plane",
            variable = %warning.variable,
            "{}",
            warning.message
        );
    }
    if settings.secret_key.is_random() {
        // Django silently uses a random key; it is worth a warning because
        // sessions and encrypted instance configuration stop working across
        // restarts and processes (ENV.md §1.1).
        tracing::warn!(
            target: "plane",
            "SECRET_KEY is unset or empty; using a random key for this process. \
             Sessions and encrypted instance configuration will not survive a restart."
        );
    }

    let routes = RouteTable::load().context("loading the embedded Django route table")?;
    tracing::info!(
        target: "plane.api",
        routes = routes.len(),
        "loaded the Django route table"
    );

    // Building the pools opens no connection (Django connects lazily too).
    let db = DbPools::from_settings(&settings)?;
    plane_db::tls::install_crypto_provider();

    let mut runtime = tokio::runtime::Builder::new_multi_thread();
    runtime.enable_all();
    if let Some(worker_threads) = settings.rust.worker_threads {
        runtime.worker_threads(worker_threads);
    }
    let runtime = runtime.build().context("building the tokio runtime")?;
    runtime.block_on(serve(settings, routes, db))
}

async fn serve(settings: Settings, routes: RouteTable, db: DbPools) -> anyhow::Result<()> {
    let bind = settings.rust.bind;
    let state = Arc::new(AppState {
        settings,
        routes,
        db,
    });
    let app = Router::new()
        .fallback(middleware::handle)
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    tracing::info!(target: "plane.api", %bind, "listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("serving HTTP")?;
    tracing::info!(target: "plane.api", "shut down cleanly");
    Ok(())
}

/// `SIGTERM` (what a container orchestrator sends) or `SIGINT` (Ctrl-C).
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(target: "plane.api", %error, "installing the Ctrl-C handler failed");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(target: "plane.api", %error, "installing the SIGTERM handler failed");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!(target: "plane.api", "SIGINT received, draining"),
        () = terminate => tracing::info!(target: "plane.api", "SIGTERM received, draining"),
    }
}
