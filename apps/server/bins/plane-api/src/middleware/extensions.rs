// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Documented no-op and side-effect-only layer bodies.
//!
//! Phase 1 reproduced the middleware layers that a request to `/` or
//! `/robots.txt` can observe. The remaining layers had no effect until their
//! dependencies existed (sessions, users, API tokens, a replica), so they are
//! explicit functions here rather than omitted. Each one names the Python file
//! it ports and the layer index it occupies.
//!
//! | # | Middleware | Status | What it does |
//! | --- | --- | --- | --- |
//! | 8 | `crum.CurrentRequestUserMiddleware` | no-op | request-scoped current user for audit columns |
//! | 11 | `plane.middleware.logger.APITokenLogMiddleware` | full | enqueue `process_logs` for every `X-Api-Key` request |
//! | 13 | `plane.middleware.db_routing.ReadReplicaRoutingMiddleware` | no-op | per-view `use_read_replica` routing (the DRF runtime applies it) |
//!
//! Layers 3 (session) and 5 (CSRF) live in their own modules.

#![allow(dead_code)]

use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_jobs::celery::CeleryPublisher;

use crate::response::Resp;

/// Layer 8: `crum.CurrentRequestUserMiddleware`
/// (`crum/__init__.py:86-111`). Exposes the current user to model saves so
/// `created_by`/`updated_by` are filled. In Rust this becomes a request-scoped
/// task-local or an explicit argument threaded through write paths
/// (HTTP_CONTRACT §8.2).
pub fn crum_request(_ctx: &RequestContext) {}

/// Layer 11: `APITokenLogMiddleware.process_request`
/// (`plane/middleware/logger.py:80-163`).
///
/// `APITokenLogMiddleware.__call__` reads `request.body`, calls the view, and
/// then — **whatever the response** — builds one `api_activity_logs` row and
/// publishes `plane.bgtasks.logger_task.process_logs(log_data=…)`. The row
/// construction is `plane_drf::token_log::ApiTokenLog`; this function is the
/// publish.
///
/// Two details Django's version fixes and this one reproduces:
///
/// * `.delay()` is `apply_async`, so a broker that is down costs the publish
///   retry budget (0, 200, 400 ms: `celery/app/defaults.py:278-286`,
///   `plane-jobs`' publisher) and then raises `OperationalError`. The call sits
///   inside `try: … except Exception as e: log_exception(e)`
///   (`logger.py:138,160-161`), so the request still succeeds and the row is
///   lost. Actually failing the request here would be a behaviour change.
/// * the body is the *un-gzipped* response: response phases run 12→0, so layer
///   9 (`GZipMiddleware`) has not run yet. `resp.body` is exactly what
///   `response.content` is at this point.
pub async fn api_token_log_response(
    settings: &Settings,
    ctx: &RequestContext,
    resp: &Resp,
    publisher: Option<&CeleryPublisher>,
) {
    let Some(log) =
        plane_drf::token_log::ApiTokenLog::build(settings, ctx, resp.status.as_u16(), &resp.body)
    else {
        return;
    };
    let Some(publisher) = publisher else {
        // A process built without a broker (tests). Django always has one, so
        // this is a misconfiguration worth an error rather than silence.
        tracing::error!(
            target: "plane.exception",
            path = %log.path,
            method = %log.method,
            "APITokenLogMiddleware: no Celery publisher; process_logs was not enqueued"
        );
        return;
    };
    // `log_exception(e)`: logger "plane.exception", `logger.exception(e)`, and
    // the response untouched.
    if let Err(error) = publisher.publish(&log.message()).await {
        tracing::error!(
            target: "plane.exception",
            %error,
            path = %log.path,
            method = %log.method,
            response_code = log.response_code,
            "APITokenLogMiddleware: enqueueing process_logs failed"
        );
    }
}

/// Layer 13: `ReadReplicaRoutingMiddleware.process_view`
/// (`plane/middleware/db_routing.py`). Returns whether *this* route opts into
/// the replica with `use_read_replica = True`. The DRF runtime applies it from
/// `routes.json` (`ViewCtx::db`), so the pipeline has nothing left to do.
pub fn use_read_replica(_ctx: &RequestContext) -> bool {
    false
}
