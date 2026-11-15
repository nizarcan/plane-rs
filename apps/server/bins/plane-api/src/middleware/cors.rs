// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `CorsMiddleware` (`corsheaders/middleware.py`, HTTP_CONTRACT §2).
//!
//! Plane's configuration: `CORS_ALLOW_CREDENTIALS=True` unconditionally,
//! `CORS_URLS_REGEX=^.*$` (every path), and either an allow-list or
//! `CORS_ALLOW_ALL_ORIGINS=True` when `CORS_ALLOWED_ORIGINS` is empty.
//!
//! The request phase answers a preflight (`OPTIONS` +
//! `Access-Control-Request-Method`) with an empty 200 before *any* other
//! middleware runs; its response phase then adds the CORS headers to that
//! response, exactly like `CorsMiddleware.__call__`.

use axum::http::Method;
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_core::error::TEXT_HTML;
use plane_core::py::{self, UrlSplit};

use crate::response::Resp;

/// `CorsMiddleware.check_preflight`.
pub fn request(ctx: &RequestContext) -> Option<Resp> {
    if ctx.method == Method::OPTIONS && ctx.headers.contains_key("access-control-request-method") {
        // `HttpResponse(headers={"content-length": "0"})` — Django's default
        // content type is `text/html; charset=utf-8`.
        let mut resp = Resp::empty(200, TEXT_HTML);
        resp.set_header("content-length", "0");
        Some(resp)
    } else {
        None
    }
}

/// `CorsMiddleware.add_response_headers`.
pub fn response(settings: &Settings, ctx: &RequestContext, mut resp: Resp) -> Resp {
    // `patch_vary_headers(response, ("origin",))` runs before the Origin check,
    // so it applies even to a request without an Origin header.
    resp.patch_vary("origin");

    let Some(origin) = ctx.header("origin").filter(|origin| !origin.is_empty()) else {
        return resp;
    };
    // `urlsplit` raises ValueError for an unbalanced IPv6 bracket; Django
    // returns without CORS headers in that case.
    let Ok(parsed) = py::urlsplit(origin) else {
        return resp;
    };

    if !settings.cors.allow_all_origins && !origin_found_in_whitelist(settings, origin, &parsed) {
        return resp;
    }

    // With credentials allowed, the request Origin is echoed verbatim (never
    // `*`) even in allow-all mode.
    if settings.cors.allow_all_origins && !settings.cors.allow_credentials {
        resp.set_header("access-control-allow-origin", "*");
    } else {
        resp.set_header("access-control-allow-origin", origin);
    }
    if settings.cors.allow_credentials {
        resp.set_header("access-control-allow-credentials", "true");
    }
    if !settings.cors.expose_headers.is_empty() {
        resp.set_header(
            "access-control-expose-headers",
            settings.cors.expose_headers.join(", "),
        );
    }
    // `request.method == "OPTIONS"`, not "was a preflight": a plain OPTIONS
    // request that reached a view gets these too (HTTP_CONTRACT §2).
    if ctx.method == Method::OPTIONS {
        resp.set_header(
            "access-control-allow-headers",
            settings.cors.allow_headers.join(", "),
        );
        resp.set_header(
            "access-control-allow-methods",
            settings.cors.allow_methods.join(", "),
        );
        if settings.cors.preflight_max_age != 0 {
            resp.set_header(
                "access-control-max-age",
                settings.cors.preflight_max_age.to_string(),
            );
        }
    }
    resp
}

/// `CorsMiddleware.origin_found_in_white_lists` + `_url_in_whitelist`: exact
/// scheme+netloc comparison, plus the literal `"null"` origin when configured.
fn origin_found_in_whitelist(settings: &Settings, origin: &str, parsed: &UrlSplit) -> bool {
    if origin == "null"
        && settings
            .cors
            .allowed_origins
            .iter()
            .any(|candidate| candidate == "null")
    {
        return true;
    }
    settings
        .cors
        .allowed_origins
        .iter()
        .any(|candidate| match py::urlsplit(candidate) {
            Ok(candidate) => candidate.scheme == parsed.scheme && candidate.netloc == parsed.netloc,
            Err(_) => false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    fn ctx(method: Method, headers: &[(&str, &str)]) -> RequestContext {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        RequestContext::new(method, "/", "", map, None)
    }

    fn settings(origins: &str) -> Settings {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("CORS_ALLOWED_ORIGINS", origins);
        Settings::from_source(&env, plane_core::config::Role::Api).expect("settings")
    }

    /// HTTP_CONTRACT §A.1.3: allow-all echoes the origin and adds credentials.
    #[test]
    fn allow_all_echoes_the_origin() {
        let config = settings("");
        let request = ctx(Method::GET, &[("origin", "http://localhost:3000")]);
        let resp = response(&config, &request, Resp::empty(200, "application/json"));
        assert_eq!(resp.header("vary"), Some("origin"));
        assert_eq!(
            resp.header("access-control-allow-origin"),
            Some("http://localhost:3000")
        );
        assert_eq!(
            resp.header("access-control-allow-credentials"),
            Some("true")
        );
        assert!(resp.header("access-control-allow-methods").is_none());

        // No Origin: Vary is still added, nothing else.
        let resp = response(
            &config,
            &ctx(Method::GET, &[]),
            Resp::empty(200, "application/json"),
        );
        assert_eq!(resp.header("vary"), Some("origin"));
        assert!(resp.header("access-control-allow-origin").is_none());
    }

    /// HTTP_CONTRACT §A.1.3: a plain OPTIONS that reached the view gets the
    /// allow-headers/methods/max-age trio.
    #[test]
    fn options_gets_the_full_set() {
        let config = settings("");
        let request = ctx(Method::OPTIONS, &[("origin", "http://localhost:3000")]);
        let resp = response(&config, &request, Resp::empty(200, "application/json"));
        assert_eq!(
            resp.header("access-control-allow-headers"),
            Some(
                "accept, authorization, content-type, user-agent, x-csrftoken, x-requested-with, X-API-Key"
            )
        );
        assert_eq!(
            resp.header("access-control-allow-methods"),
            Some("DELETE, GET, OPTIONS, PATCH, POST, PUT")
        );
        assert_eq!(resp.header("access-control-max-age"), Some("86400"));
    }

    /// HTTP_CONTRACT §A.1.4: allow-list mode rejects other origins after Vary.
    #[test]
    fn allow_list_mode() {
        let config = settings("http://localhost:3000, https://plane.example.com");
        let allowed = ctx(Method::GET, &[("origin", "https://plane.example.com")]);
        let resp = response(&config, &allowed, Resp::empty(200, "application/json"));
        assert_eq!(
            resp.header("access-control-allow-origin"),
            Some("https://plane.example.com")
        );

        // Scheme is part of the comparison: http://plane.example.com is not in
        // the list even though the host is.
        let wrong_scheme = ctx(Method::GET, &[("origin", "http://plane.example.com")]);
        let resp = response(&config, &wrong_scheme, Resp::empty(200, "application/json"));
        assert_eq!(resp.header("vary"), Some("origin"));
        assert!(resp.header("access-control-allow-origin").is_none());

        // A different port is a different origin.
        let wrong_port = ctx(Method::GET, &[("origin", "http://localhost:3001")]);
        let resp = response(&config, &wrong_port, Resp::empty(200, "application/json"));
        assert!(resp.header("access-control-allow-origin").is_none());
    }

    #[test]
    fn preflight_request_short_circuit() {
        let request = ctx(
            Method::OPTIONS,
            &[
                ("origin", "http://localhost:3000"),
                ("access-control-request-method", "POST"),
            ],
        );
        let resp = request_preflight(&request);
        assert_eq!(resp.status.as_u16(), 200);
        assert_eq!(resp.header("content-length"), Some("0"));
        assert_eq!(resp.content_type, Some(TEXT_HTML));
        assert!(resp.body.is_empty());
    }

    fn request_preflight(ctx: &RequestContext) -> Resp {
        request(ctx).expect("preflight response")
    }

    #[test]
    fn preflight_needs_the_request_method_header() {
        assert!(request(&ctx(Method::OPTIONS, &[])).is_none());
        assert!(
            request(&ctx(
                Method::GET,
                &[("access-control-request-method", "POST")]
            ))
            .is_none()
        );
    }
}
