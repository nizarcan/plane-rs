// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `request.build_absolute_uri()` (`django/http/request.py:256-302`).
//!
//! The one caller is Django's page cache: `_generate_cache_key` hashes
//! `request.build_absolute_uri().encode("ascii")` (`django/utils/cache.py:350-364`),
//! so the Rust page key must be built from the same string.
//!
//! The rules it applies, in order:
//!
//! * the scheme is `https` when `SECURE_PROXY_SSL_HEADER` matches
//!   (`production.py:15` sets `("HTTP_X_FORWARDED_PROTO", "https")`), else the
//!   connection's scheme. [`RequestContext::is_secure`] already holds that
//!   decision;
//! * the host is `HTTP_HOST` verbatim — `get_host()` only strips a port when it
//!   is the scheme's default, and `CommonMiddleware` has already validated it
//!   against `ALLOWED_HOSTS` (`django/middleware/common.py:88-112`);
//! * the path+query is `request.get_full_path()` (escaped path,
//!   `iri_to_uri(query)`);
//! * the whole string is then passed through `iri_to_uri` once more, which is
//!   idempotent for everything `get_full_path()` already escaped because `%`
//!   is in Python `quote`'s safe set.

use plane_core::context::RequestContext;
use plane_core::py;
use plane_drf::view::ViewCtx;

/// `request.build_absolute_uri()` for a handler's request.
#[must_use]
pub fn absolute_uri(ctx: &ViewCtx<'_>) -> String {
    absolute_uri_of(ctx.ctx)
}

/// The same, for the pipeline layers (which have no `ViewCtx`).
#[must_use]
pub fn absolute_uri_of(ctx: &RequestContext) -> String {
    let scheme = if ctx.is_secure { "https" } else { "http" };
    let host = ctx.header("host").unwrap_or_default();
    let full_path = py::escape_uri_path(&ctx.path);
    let mut uri = String::with_capacity(scheme.len() + host.len() + full_path.len() + 24);
    uri.push_str(scheme);
    uri.push_str("://");
    uri.push_str(host);
    uri.push_str(&full_path);
    if !ctx.query_string.is_empty() {
        uri.push('?');
        uri.push_str(&py::iri_to_uri(&ctx.query_string));
    }
    py::iri_to_uri(&uri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, HeaderValue, Method};

    fn uri(path: &str, query: &str, headers: &[(&str, &str)]) -> String {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                http::HeaderName::from_bytes(name.as_bytes()).expect("name"),
                HeaderValue::from_str(value).expect("value"),
            );
        }
        absolute_uri_of(&RequestContext::new(Method::GET, path, query, map, None))
    }

    #[test]
    fn absolute_uri_matches_django() {
        assert_eq!(
            uri("/api/timezones/", "", &[("host", "127.0.0.1:18001")]),
            "http://127.0.0.1:18001/api/timezones/"
        );
        // The query string keeps its escaping; `%` is never re-encoded.
        assert_eq!(
            uri(
                "/api/workspaces/acme/",
                "q=caf%C3%A9&x=1",
                &[("host", "plane.test")]
            ),
            "http://plane.test/api/workspaces/acme/?q=caf%C3%A9&x=1"
        );
        // A space in the decoded path is escaped once.
        assert_eq!(
            uri("/a b/", "", &[("host", "plane.test")]),
            "http://plane.test/a%20b/"
        );
        // `X-Forwarded-Proto: https` selects the https scheme
        // (`SECURE_PROXY_SSL_HEADER`).
        assert_eq!(
            uri(
                "/x/",
                "",
                &[("host", "plane.test"), ("x-forwarded-proto", "https")]
            ),
            "https://plane.test/x/"
        );
        // No `Host`: the pipeline rejected the request at layer 4, so this is
        // only reachable in a unit test; it must not panic.
        assert_eq!(uri("/x/", "", &[]), "http:///x/");
    }
}
