// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Build the DRF runtime and the handlers registered in this binary.
//!
//! The binary registers three things: the two `plane.web` function views, the
//! ported `/api/` handlers from `plane-http-app`, and (in a `policy-probe`
//! build) a probe handler for every other route method. A route that is not
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

    // Every ported `/api/` handler, one module per route group
    // (`crates/plane-http-app`). A group with nothing ported registers nothing
    // and keeps answering 501.
    //
    // Not in the `policy-probe` build: the probe registers a handler for *every*
    // non-v1 route method, ported or not, and registering one twice is a
    // `RegistryError`. The probe build therefore behaves exactly as it did
    // before this crate existed, and `policy-probe.yaml` still measures the
    // policy of every route. (`register_with_probe` in `probe.rs` could instead
    // skip the views already registered here, which would additionally exercise
    // the real handlers; the probe is a policy oracle, so it does not need to.)
    #[cfg(not(feature = "policy-probe"))]
    plane_http_app::register_all(&mut registry).context("registering the /api/ handlers")?;

    // Every ported `/api/v1/` handler (`crates/plane-http-public`). The crate
    // and its `common/` module land with P0; the four route sets fill it. An
    // empty `register_all` is intentional and leaves every v1 route on Django.
    #[cfg(not(feature = "policy-probe"))]
    plane_http_public::register_all(&mut registry).context("registering the /api/v1/ handlers")?;

    #[cfg(feature = "policy-probe")]
    crate::probe::register_all(&mut registry).context("registering the policy probe handlers")?;

    let registry = registry.finish().context("finishing the DRF registry")?;
    let registered = registry.views().len();

    // The process's one django-redis handle. `Settings` requires `REDIS_URL`
    // for the api role (`plane-core::config`), so a missing one has already
    // aborted startup; building the handle opens no connection (the
    // `ConnectionManager` connects on the first command, like django-redis).
    //
    // Throttles and response caching share it: both go through
    // `django.core.cache.cache` in Plane, so they must see the same keyspace.
    let redis = Some(
        plane_cache::Redis::from_settings(settings).context("building the Redis cache handle")?,
    );

    // The Celery publisher (DECISIONS.md D7). Built from the broker settings,
    // parsed now so a malformed `AMQP_URL` is a startup error (kombu parses it
    // at import too); it connects on the first publish, so startup does not
    // wait for RabbitMQ.
    let celery = plane_jobs::celery::CeleryPublisher::from_settings(settings)
        .context("building the Celery publisher (AMQP_URL / RABBITMQ_*)")?;

    let runtime =
        Runtime::new(registry, redis, Some(celery)).context("building the DRF runtime")?;
    tracing::info!(
        target: "plane.api.drf",
        views = registered,
        "DRF runtime ready (cache and Celery publisher are lazy)"
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
