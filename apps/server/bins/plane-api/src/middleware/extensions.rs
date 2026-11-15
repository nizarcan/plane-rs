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

use plane_core::context::RequestContext;

use crate::response::Resp;

/// Layer 8: `crum.CurrentRequestUserMiddleware`
/// (`crum/__init__.py:86-111`). Exposes the current user to model saves so
/// `created_by`/`updated_by` are filled. In Rust this becomes a request-scoped
/// task-local or an explicit argument threaded through write paths
/// (HTTP_CONTRACT §8.2).
pub fn crum_request(_ctx: &RequestContext) {}

/// Layer 11: `APITokenLogMiddleware.process_request`
/// (`plane/middleware/logger.py:80-163`). Builds the `api_activity_logs` row
/// (hashed token identifier, path, method, query, redacted headers, bodies,
/// status, client IP, user agent) and enqueues `process_logs`; failures are
/// swallowed so logging never breaks a request.
pub fn api_token_log_response(_ctx: &RequestContext, _resp: &Resp) {}

/// Layer 13: `ReadReplicaRoutingMiddleware.process_view`
/// (`plane/middleware/db_routing.py`). Returns whether *this* route opts into
/// the replica with `use_read_replica = True`. Until the route table records
/// that flag (Phase 3), every read stays on the primary.
pub fn use_read_replica(_ctx: &RequestContext) -> bool {
    false
}
