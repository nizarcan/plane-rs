// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `SecurityMiddleware` (`django/middleware/security.py`, HTTP_CONTRACT §3).
//!
//! Plane does not override any `SECURE_*` setting, so the resolved values are
//! Django's global defaults:
//!
//! | Setting | Value | Consequence |
//! | --- | --- | --- |
//! | `SECURE_CONTENT_TYPE_NOSNIFF` | `True` | `X-Content-Type-Options: nosniff` |
//! | `SECURE_REFERRER_POLICY` | `"same-origin"` | `Referrer-Policy: same-origin` |
//! | `SECURE_CROSS_ORIGIN_OPENER_POLICY` | `"same-origin"` | `Cross-Origin-Opener-Policy` |
//! | `SECURE_HSTS_SECONDS` | `0` | no `Strict-Transport-Security` |
//! | `SECURE_SSL_REDIRECT` | `False` | no https redirect |
//!
//! None of these are environment-driven, so they are constants here rather than
//! settings. `request.is_secure()` honours `SECURE_PROXY_SSL_HEADER`
//! (`production.py:15`), which is why the HSTS branch can fire on a request
//! that only carries `X-Forwarded-Proto: https` — with the default of 0 it does
//! not, but the logic is kept so a future change is a one-line diff.

use plane_core::context::RequestContext;

use crate::response::Resp;

/// `SECURE_HSTS_SECONDS` (Django's global default; Plane does not override it).
const SECURE_HSTS_SECONDS: u64 = 0;
const SECURE_HSTS_INCLUDE_SUBDOMAINS: bool = false;
const SECURE_HSTS_PRELOAD: bool = false;

/// `SecurityMiddleware.process_response`.
pub fn response(ctx: &RequestContext, mut resp: Resp) -> Resp {
    if SECURE_HSTS_SECONDS != 0
        && ctx.is_secure
        && resp.header("strict-transport-security").is_none()
    {
        let mut value = format!("max-age={SECURE_HSTS_SECONDS}");
        if SECURE_HSTS_INCLUDE_SUBDOMAINS {
            value.push_str("; includeSubDomains");
        }
        if SECURE_HSTS_PRELOAD {
            value.push_str("; preload");
        }
        resp.set_header("Strict-Transport-Security", value);
    }

    // `headers.setdefault`.
    if resp.header("x-content-type-options").is_none() {
        resp.set_header("X-Content-Type-Options", "nosniff");
    }
    if resp.header("referrer-policy").is_none() {
        resp.set_header("Referrer-Policy", "same-origin");
    }
    if resp.header("cross-origin-opener-policy").is_none() {
        resp.set_header("Cross-Origin-Opener-Policy", "same-origin");
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, Method};
    use plane_core::context::RequestContext;

    fn ctx(secure: bool) -> RequestContext {
        let mut headers = HeaderMap::new();
        if secure {
            headers.insert("x-forwarded-proto", "https".parse().unwrap());
        }
        RequestContext::new(Method::GET, "/", "", headers, None)
    }

    #[test]
    fn baseline_headers() {
        let resp = response(&ctx(false), Resp::empty(200, "application/json"));
        assert_eq!(resp.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(resp.header("referrer-policy"), Some("same-origin"));
        assert_eq!(
            resp.header("cross-origin-opener-policy"),
            Some("same-origin")
        );
        assert!(resp.header("strict-transport-security").is_none());
    }

    #[test]
    fn setdefault_does_not_overwrite() {
        let mut original = Resp::empty(200, "application/json");
        original.set_header("X-Content-Type-Options", "custom");
        let resp = response(&ctx(false), original);
        assert_eq!(resp.header("x-content-type-options"), Some("custom"));
    }
}
