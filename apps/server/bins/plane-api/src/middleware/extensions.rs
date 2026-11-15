// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Documented no-op extension points.
//!
//! Phase 1 reproduces the middleware layers that a request to `/` or
//! `/robots.txt` can observe. The remaining layers have no effect until their
//! dependencies exist (sessions, users, API tokens, a replica), so they are
//! explicit functions here rather than omitted. Each one names the Python file
//! it ports and the layer index it occupies, so Phase 2 can fill it in without
//! rediscovering the ordering.
//!
//! | # | Middleware | Status | What it will do |
//! | --- | --- | --- | --- |
//! | 6 | `django.contrib.auth.middleware.AuthenticationMiddleware` | no-op | resolve `request.user` from the session |
//! | 8 | `crum.CurrentRequestUserMiddleware` | no-op | request-scoped current user for audit columns |
//! | 11 | `plane.middleware.logger.APITokenLogMiddleware` | no-op | enqueue `process_logs` for every `X-Api-Key` request |
//! | 13 | `plane.middleware.db_routing.ReadReplicaRoutingMiddleware` | no-op | per-view `use_read_replica` routing |
//!
//! Layers 3 (session) and 5 (CSRF) live in their own modules: their observable
//! response behaviour is implemented, and only the parts that need a session
//! store or the CSRF cookie writer are left for Phase 2.
//!
//! The functions are intentionally unused until Phase 2; the allow keeps the
//! "fill me in" markers visible in the file instead of deleting them.

#![allow(dead_code)]

use plane_core::config::Settings;
use plane_core::context::RequestContext;

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
/// The data construction is `plane_drf::token_log::ApiTokenLog::build`, which
/// reproduces the HMAC token identifier, the redacted Python `dict` repr of the
/// headers, `_safe_decode_body` and the client IP. The row is written by the
/// Celery task `plane.bgtasks.logger_task.process_logs`; the publisher is
/// Phase 4's (DECISIONS.md D7), so no sink is installed yet and a request with
/// `X-Api-Key` only logs the would-be row at debug level.
///
/// This is why the registry refuses `/api/v1/` routes: those are the views
/// whose authentication depends on the header, so every one of their requests
/// would produce the log. A request carrying `X-Api-Key` to another mount also
/// logs in Django; that residual gap is recorded in `ROUTES.md`, and the
/// differential probe never sends the header outside `/api/v1/`.
pub fn api_token_log_response(settings: &Settings, ctx: &RequestContext, resp: &Resp) {
    let Some(log) =
        plane_drf::token_log::ApiTokenLog::build(settings, ctx, resp.status.as_u16(), &resp.body)
    else {
        return;
    };
    tracing::debug!(
        target: "plane.api.token_log",
        token_identifier = %log.token_identifier,
        path = %log.path,
        method = %log.method,
        response_code = log.response_code,
        "APITokenLogMiddleware row built; no Celery publisher is installed yet \
         (Phase 4), so it is not enqueued"
    );
}

/// Layer 13: `ReadReplicaRoutingMiddleware.process_view`
/// (`plane/middleware/db_routing.py`). Returns whether *this* route opts into
/// the replica with `use_read_replica = True`. Until the route table records
/// that flag (Phase 3), every read stays on the primary.
pub fn use_read_replica(_ctx: &RequestContext) -> bool {
    false
}
