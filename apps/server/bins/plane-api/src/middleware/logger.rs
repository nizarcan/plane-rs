// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `RequestLoggerMiddleware` (`plane/middleware/logger.py:26-77`,
//! HTTP_CONTRACT §9.3).
//!
//! Two responsibilities, both observable:
//!
//! * the log line itself, with the fields operators depend on (`path`, `method`,
//!   `status_code`, `duration_ms`, `remote_addr`, `user_agent`, `user_id`);
//! * touching `request.user`, which loads the session and is why almost every
//!   response carries `Vary: Cookie` (HTTP_CONTRACT §1.1). `GET /` is the one
//!   route that skips logging (`logger.py:35-36`) and therefore the one response
//!   without it.
//!
//! The status recorded is the one the response has *before* the outer
//! middlewares run, so an `APPEND_SLASH` redirect is logged as the 404 that
//! caused it — exactly what Django does, because #12's `process_response` runs
//! before #4's.

use std::time::Instant;

use axum::http::Method;
use plane_core::clock;
use plane_core::context::RequestContext;

use crate::response::Resp;

/// `RequestLoggerMiddleware.process_response`.
pub fn response(ctx: &mut RequestContext, resp: &Resp, started: Instant) {
    // `_should_log_route`: only `GET /` is skipped.
    if ctx.method == Method::GET && ctx.path == "/" {
        return;
    }

    // `getattr(request, "user")` / `is_authenticated`: evaluating the lazy user
    // is what marks the session accessed.
    let user_id = ctx.touch_user();
    let duration_ms = clock::millis_between(started, Instant::now());
    let remote_addr = ctx.client_ip().unwrap_or_default();
    let user_agent = ctx.header("user-agent").unwrap_or("").to_string();
    let full_path = ctx.full_path();
    let status_code = resp.status.as_u16();
    let message = format!("{} {} {}", ctx.method, full_path, status_code);

    // Field names match `logger.py:68-78` (`extra={...}`); the target matches
    // the `plane.api.request` logger from `production.py:61-65`.
    match user_id {
        Some(user_id) => tracing::info!(
            target: "plane.api.request",
            path = %ctx.path,
            method = %ctx.method,
            status_code,
            duration_ms,
            remote_addr = %remote_addr,
            user_agent = %user_agent,
            user_id = %user_id,
            "{message}"
        ),
        None => tracing::info!(
            target: "plane.api.request",
            path = %ctx.path,
            method = %ctx.method,
            status_code,
            duration_ms,
            remote_addr = %remote_addr,
            user_agent = %user_agent,
            user_id = tracing::field::Empty,
            "{message}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use plane_core::ids::UserId;
    use uuid::Uuid;

    fn ctx(method: Method, path: &str) -> RequestContext {
        RequestContext::new(method, path, "x=1", HeaderMap::new(), None)
    }

    #[test]
    fn health_check_is_not_logged_but_is_logged_for_other_methods() {
        // The side effect we can observe without a subscriber is `accessed`.
        let mut get = ctx(Method::GET, "/");
        response(
            &mut get,
            &Resp::empty(200, "application/json"),
            Instant::now(),
        );
        assert!(!get.session_accessed);

        let mut head = ctx(Method::HEAD, "/");
        response(
            &mut head,
            &Resp::empty(200, "application/json"),
            Instant::now(),
        );
        assert!(head.session_accessed);

        let mut options = ctx(Method::OPTIONS, "/");
        response(
            &mut options,
            &Resp::empty(200, "application/json"),
            Instant::now(),
        );
        assert!(options.session_accessed);

        let mut other = ctx(Method::GET, "/robots.txt");
        response(&mut other, &Resp::empty(200, "text/plain"), Instant::now());
        assert!(other.session_accessed);
    }

    #[test]
    fn an_authenticated_user_is_still_marked_accessed() {
        let mut request = ctx(Method::GET, "/api/users/me/");
        request.user = Some(UserId(Uuid::nil()));
        response(
            &mut request,
            &Resp::empty(401, "application/json"),
            Instant::now(),
        );
        assert!(request.session_accessed);
    }
}
