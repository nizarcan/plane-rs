// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Build the DRF runtime and the handlers registered in this binary.
//!
//! Phase 2 has one handler: the policy probe. Phase 3 adds the real handlers in
//! `handlers/` and registers them here, action by action. A route that is not
//! registered keeps answering the pipeline's 501, which is what the coexistence
//! proxy expects while Django still serves it.

use anyhow::Context;
use plane_core::config::Settings;
use plane_core::error::TEXT_PLAIN;
use plane_drf::dispatch::Runtime;
use plane_drf::error::ViewError;
use plane_drf::policy::PolicyTable;
use plane_drf::registry::Registry;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

/// Load `routes.json`, build the registry and the runtime.
///
/// The policy load is where an unknown permission/auth/throttle class, an
/// unreadable `allow_permission` or an unimplemented lifecycle override aborts
/// startup. Nothing about a route is defaulted.
pub fn build_runtime(settings: &Settings) -> anyhow::Result<Runtime> {
    let policy = PolicyTable::load().context("loading the embedded DRF policy")?;
    tracing::info!(
        target: "plane.api.drf",
        routes = policy.len(),
        views = policy.views().len(),
        "loaded the DRF policy table"
    );
    #[allow(unused_mut)]
    let mut registry = Registry::new(policy).context("building the DRF handler registry")?;

    // The two `plane.web.views` functions are already ported (Phase 1). They
    // stay real handlers in every build, including the probe: `/` is the
    // harness's health path, so it must answer 200.
    register_web_views(&mut registry).context("registering the plane.web views")?;

    #[cfg(feature = "policy-probe")]
    crate::probe::register_all(&mut registry).context("registering the policy probe handlers")?;

    let registry = registry.finish().context("finishing the DRF registry")?;
    let registered = registry.views().len();
    let redis = match settings.redis.url.as_deref() {
        Some(_) => Some(
            plane_throttle::Redis::from_settings(settings)
                .context("building the throttle Redis handle")?,
        ),
        None => None,
    };
    let runtime = Runtime::new(registry, redis).context("building the DRF runtime")?;
    tracing::info!(
        target: "plane.api.drf",
        views = registered,
        "DRF runtime ready"
    );
    Ok(runtime)
}

/// `plane/web/views.py`, byte for byte: `GET /` (Django's JSON dialect, 16
/// bytes) and `/robots.txt` (25 bytes, `text/plain`). Both function views accept
/// every method; unsafe methods are rejected by CSRF before the view runs.
fn register_web_views(registry: &mut Registry) -> Result<(), plane_drf::registry::RegistryError> {
    registry
        .function("plane.web.views.health_check")?
        .all(health)?;
    registry
        .function("plane.web.views.robots_txt")?
        .all(robots)?;
    Ok(())
}

fn health<'a>(_ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        Ok(ViewResponse::raw(
            200,
            "application/json",
            b"{\"status\": \"OK\"}".to_vec(),
        ))
    })
}

fn robots<'a>(_ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        Ok(ViewResponse::raw(
            200,
            TEXT_PLAIN,
            b"User-agent: *\nDisallow: /".to_vec(),
        ))
    })
}
