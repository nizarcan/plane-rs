// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `HttpRequest.build_absolute_uri(location=None)`
//! (`django/http/request.py:256-302`) — one implementation for the two callers:
//!
//! * **no argument** — [`absolute_uri`] / [`absolute_uri_of`]. Django's page
//!   cache hashes `request.build_absolute_uri().encode("ascii")`
//!   (`django/utils/cache.py:350-364`), so the Rust page key is built from the
//!   same string;
//! * **a relative location** — [`build_absolute_uri`]. DRF's
//!   `FileField.to_representation` writes
//!   `request.build_absolute_uri(value.url)` (`rest_framework/fields.py:1252-1265`),
//!   and `S3Storage.url()` returns the object name unchanged
//!   (`plane/settings/storage.py:23-24`), so an asset's `asset` field is the
//!   request path with the asset key appended.
//!
//! The rules Django applies, in order:
//!
//! * the scheme is `https` when `SECURE_PROXY_SSL_HEADER` matches
//!   (`production.py:15` sets `("HTTP_X_FORWARDED_PROTO", "https")`), else the
//!   connection's scheme. [`RequestContext::is_secure`] already holds that
//!   decision;
//! * the host is `HTTP_HOST` verbatim — `get_host()` only strips a port when it
//!   is the scheme's default, and `CommonMiddleware` has already validated it
//!   against `ALLOWED_HOSTS` (`django/middleware/common.py:88-112`);
//! * the location (or `"//" + request.get_full_path()`) is split with
//!   `urlsplit`. An absolute location is converted with `iri_to_uri` and
//!   returned; a location whose path starts with `/` and holds no `/./` or
//!   `/../` segment replaces the request's whole path
//!   (`location.removeprefix("//")`); anything else is `urljoin`ed against
//!   `scheme://host + request.path`, which is what makes a relative asset name
//!   follow the request path when that path ends in `/`;
//! * the whole string is passed through `iri_to_uri` once, which is idempotent
//!   for everything `get_full_path()` already escaped because `%` is in
//!   Python `quote`'s safe set.

use plane_core::context::RequestContext;
use plane_core::py;
use plane_drf::view::ViewCtx;

/// `request.build_absolute_uri(location)` for a handler's request.
///
/// `location` is the relative value a serializer field holds (an asset name);
/// a root-relative or absolute location is handled the way Django does.
#[must_use]
pub fn build_absolute_uri(ctx: &ViewCtx<'_>, location: &str) -> String {
    build(
        scheme(ctx.ctx),
        ctx.ctx.header("host").unwrap_or_default(),
        ctx.path(),
        &ctx.ctx.full_path(),
        Some(location),
    )
}

/// `request.build_absolute_uri()` for a handler's request.
#[must_use]
pub fn absolute_uri(ctx: &ViewCtx<'_>) -> String {
    absolute_uri_of(ctx.ctx)
}

/// The same, for the pipeline layers (which have no `ViewCtx`).
#[must_use]
pub fn absolute_uri_of(ctx: &RequestContext) -> String {
    build(
        scheme(ctx),
        ctx.header("host").unwrap_or_default(),
        &ctx.path,
        &ctx.full_path(),
        None,
    )
}

/// `request.is_secure()`: `https` only when `SECURE_PROXY_SSL_HEADER` matched.
fn scheme(ctx: &RequestContext) -> &'static str {
    if ctx.is_secure { "https" } else { "http" }
}

/// The algorithm, without a request, so the URL rules are testable.
///
/// `path` is `request.path` (decoded) and `full_path`
/// `request.get_full_path()` (escaped); `location` is `None` for
/// `build_absolute_uri()`'s no-argument form.
fn build(scheme: &str, host: &str, path: &str, full_path: &str, location: Option<&str>) -> String {
    let scheme_host = format!("{scheme}://{host}");
    // `if location is None: location = "//%s" % self.get_full_path()`
    let location = match location {
        Some(location) => location.to_string(),
        None => format!("//{full_path}"),
    };

    let resolved = match plane_net::pyurl::urlsplit(&location) {
        Ok(bits) if !bits.scheme.is_empty() && !bits.netloc.is_empty() => location.clone(),
        Ok(bits)
            if bits.scheme.is_empty()
                && bits.netloc.is_empty()
                && bits.path.starts_with('/')
                && !bits.path.contains("/./")
                && !bits.path.contains("/../") =>
        {
            // `location.removeprefix("//")` keeps the request's authority.
            format!(
                "{scheme_host}{}",
                location.strip_prefix("//").unwrap_or(&location)
            )
        }
        _ => {
            let base = format!("{scheme_host}{path}");
            plane_net::pyurl::urljoin(&base, &location).unwrap_or(location)
        }
    };
    py::iri_to_uri(&resolved)
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
        // A dot segment is normalised by `urljoin`, as Django's does — the
        // router never routes such a path, so no page-cache key can hold one.
        assert_eq!(uri("/a/./b/", "", &[("host", "h")]), "http://h/a/b/");
        assert_eq!(
            uri("/api/unsplash/", "query=a%20b", &[("host", "h")]),
            "http://h/api/unsplash/?query=a%20b"
        );
    }

    /// `request.build_absolute_uri(name)` for the shapes an asset name can have.
    /// The interesting ones are the filenames with a space and a non-ASCII
    /// character: `iri_to_uri` escapes them here, and the presigned URL's
    /// `Content-Disposition` is percent-encoded separately by
    /// `crates/plane-storage` (whose golden corpus pins that side).
    #[test]
    fn absolute_uri_matches_django_for_a_relative_location() {
        let path = "/api/workspaces/file-assets/ws/47ef-spec.pdf/";
        // A relative name against a path that ends in `/`: the whole path stays,
        // because `urljoin` treats the last segment as a directory.
        assert_eq!(
            build(
                "http",
                "plane-fixture.test",
                path,
                "",
                Some("ws/47ef-spec.pdf")
            ),
            "http://plane-fixture.test/api/workspaces/file-assets/ws/47ef-spec.pdf/ws/47ef-spec.pdf"
        );
        // `iri_to_uri` keeps `()` — Python's `quote` safe set includes them.
        assert_eq!(
            build("http", "h", "/a/b/", "", Some("ws/1-my file (1).pdf")),
            "http://h/a/b/ws/1-my%20file%20(1).pdf"
        );
        assert_eq!(
            build("http", "h", "/a/b/", "", Some("ws/1-caf\u{e9}.png")),
            "http://h/a/b/ws/1-caf%C3%A9.png"
        );
        // A root-relative location replaces the path (`location.startswith("/")`).
        assert_eq!(
            build("https", "h:8443", "/a/b/", "", Some("/static/x.png")),
            "https://h:8443/static/x.png"
        );
        // An absolute location is returned unchanged (after `iri_to_uri`).
        assert_eq!(
            build("http", "h", "/a/b/", "", Some("https://cdn.test/x.png")),
            "https://cdn.test/x.png"
        );
        // A base path without a trailing slash loses its last segment, which is
        // `urljoin`'s rule.
        assert_eq!(
            build("http", "h", "/a/b", "", Some("ws/x.png")),
            "http://h/a/ws/x.png"
        );
        // Every case above is Django's own output for the same request
        // (`RequestFactory().get(...)` with `HTTP_HOST` set).
    }

    /// The no-argument form is Django's `"//" + get_full_path()` through the
    /// same code path; with one implementation the two callers cannot drift.
    #[test]
    fn the_two_forms_share_one_implementation() {
        let headers = [("host", "plane.test")];
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                http::HeaderName::from_bytes(name.as_bytes()).expect("name"),
                HeaderValue::from_str(value).expect("value"),
            );
        }
        let ctx = RequestContext::new(Method::GET, "/api/unsplash/", "page=2", map, None);
        assert_eq!(
            absolute_uri_of(&ctx),
            build(
                "http",
                "plane.test",
                "/api/unsplash/",
                "/api/unsplash/?page=2",
                None
            )
        );
        // A `location` equal to the full path (root-relative) resolves to the
        // same string, as it does in Django.
        assert_eq!(
            build(
                "http",
                "plane.test",
                "/api/unsplash/",
                "/api/unsplash/?page=2",
                Some("/api/unsplash/?page=2")
            ),
            "http://plane.test/api/unsplash/?page=2"
        );
    }
}
