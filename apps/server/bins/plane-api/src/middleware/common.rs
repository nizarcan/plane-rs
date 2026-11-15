// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `CommonMiddleware` (`django/middleware/common.py`, HTTP_CONTRACT §6).
//!
//! Request phase: reject a `User-Agent` on `DISALLOWED_USER_AGENTS` (empty in
//! Plane) and validate `Host` via `request.get_host()`. The host check is why a
//! malformed `Host` is a 400 even with `ALLOWED_HOSTS=["*"]`.
//!
//! Response phase: turn a 404 into a 301 when the path plus `/` resolves
//! (`APPEND_SLASH`), otherwise set `Content-Length` on non-streaming responses
//! that lack one.

use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_core::error::{ApiError, TEXT_HTML};
use plane_core::py;

use crate::host;
use crate::response::Resp;
use crate::routes::RouteTable;

/// `APPEND_SLASH` is Django's default and Plane does not override it.
const APPEND_SLASH: bool = true;

/// `CommonMiddleware.process_request`.
pub fn request(settings: &Settings, ctx: &RequestContext) -> Result<(), ApiError> {
    // `DISALLOWED_USER_AGENTS` is `[]` in Plane, so the user-agent loop is a
    // no-op; keep the call-site order (user agent, then host) documented.
    host::get_host(settings, ctx.header("host"), ctx.is_secure)
        .map(|_host| ())
        .ok_or(ApiError::DisallowedHost)
}

/// `CommonMiddleware.process_response`.
pub fn response(
    _settings: &Settings,
    routes: &RouteTable,
    ctx: &RequestContext,
    mut resp: Resp,
) -> Resp {
    if resp.status == 404 && should_redirect_with_slash(routes, ctx) {
        let location = py::escape_leading_slashes(&ctx.full_path_with_slash());
        // `HttpResponsePermanentRedirect`: 301, `text/html; charset=utf-8`, no
        // body, and it is returned *before* the Content-Length step below.
        let mut redirect = Resp::empty(301, TEXT_HTML);
        redirect.set_header("Location", location);
        return redirect;
    }
    resp.set_content_length();
    resp
}

/// `CommonMiddleware.should_redirect_with_slash`: the request 404'd, its path
/// does not end in `/`, the path does not resolve, and the path plus `/` does.
/// (`resolved_view.should_append_slash` is true for every Plane view; the
/// attribute's default is `True` and nothing sets it to `False`.)
fn should_redirect_with_slash(routes: &RouteTable, ctx: &RequestContext) -> bool {
    if !APPEND_SLASH || ctx.path_info.ends_with('/') {
        return false;
    }
    routes.resolve(&ctx.path_info).is_none()
        && routes.resolve(&format!("{}/", ctx.path_info)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, Method};

    fn settings(host: &str) -> Settings {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("ALLOWED_HOSTS", host);
        Settings::from_source(&env, plane_core::config::Role::Api).expect("settings")
    }

    fn ctx(method: Method, path: &str, host: &str) -> RequestContext {
        let mut headers = HeaderMap::new();
        headers.insert("host", host.parse().unwrap());
        RequestContext::new(method, path, "", headers, None)
    }

    fn routes() -> RouteTable {
        RouteTable::load().expect("route table")
    }

    #[test]
    fn host_check() {
        let wildcard = settings("*");
        assert!(request(&wildcard, &ctx(Method::GET, "/", "plane.example.com")).is_ok());
        assert_eq!(
            request(&wildcard, &ctx(Method::GET, "/", "bad_host")),
            Err(ApiError::DisallowedHost)
        );

        let listed = settings("plane.example.com");
        assert!(request(&listed, &ctx(Method::GET, "/", "plane.example.com")).is_ok());
        assert_eq!(
            request(&listed, &ctx(Method::GET, "/", "other.example.com")),
            Err(ApiError::DisallowedHost)
        );
    }

    // HTTP_CONTRACT §6.1: the slashless path 404s, the slashed one resolves.
    #[test]
    fn append_slash_redirect() {
        let wildcard = settings("*");
        let table = routes();
        let request = ctx(Method::GET, "/api/instances", "plane.example.com");
        let resp = response(
            &wildcard,
            &table,
            &request,
            Resp::empty(404, "application/json"),
        );
        assert_eq!(resp.status.as_u16(), 301);
        assert_eq!(resp.header("location"), Some("/api/instances/"));
        assert_eq!(resp.content_type, Some(TEXT_HTML));
        assert!(resp.body.is_empty());
        // No Content-Length: the redirect is returned before that step.
        assert!(resp.header("content-length").is_none());
    }

    #[test]
    fn append_slash_keeps_the_query_string() {
        let wildcard = settings("*");
        let table = routes();
        let mut request = ctx(Method::POST, "/api/instances", "plane.example.com");
        request.query_string = "x=1&y=2".to_string();
        let resp = response(
            &wildcard,
            &table,
            &request,
            Resp::empty(404, "application/json"),
        );
        assert_eq!(resp.status.as_u16(), 301);
        assert_eq!(resp.header("location"), Some("/api/instances/?x=1&y=2"));

        // The raw path is escaped once, exactly like `get_full_path`; the
        // `str` converter accepts a space, so `/api/workspaces/a b/` resolves.
        let mut spaced = ctx(Method::GET, "/api/workspaces/a b", "plane.example.com");
        spaced.query_string = "q=a b".to_string();
        let resp = response(
            &wildcard,
            &table,
            &spaced,
            Resp::empty(404, "application/json"),
        );
        assert_eq!(
            resp.header("location"),
            Some("/api/workspaces/a%20b/?q=a%20b")
        );
    }

    #[test]
    fn no_redirect_when_the_slashed_path_does_not_resolve() {
        let wildcard = settings("*");
        let table = routes();
        let request = ctx(Method::GET, "/robots.txt/", "plane.example.com");
        let resp = response(
            &wildcard,
            &table,
            &request,
            Resp::empty(404, "application/json"),
        );
        assert_eq!(resp.status.as_u16(), 404);
        assert!(resp.header("location").is_none());
        assert_eq!(resp.header("content-length"), Some("0"));
    }

    #[test]
    fn non_404_responses_only_get_content_length() {
        let wildcard = settings("*");
        let table = routes();
        let request = ctx(Method::GET, "/", "plane.example.com");
        let resp = response(
            &wildcard,
            &table,
            &request,
            Resp::new(200, "application/json", b"{\"status\": \"OK\"}".to_vec()),
        );
        assert_eq!(resp.header("content-length"), Some("16"));
    }
}
