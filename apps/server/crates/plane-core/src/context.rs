// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Per-request state shared by the middleware pipeline.
//!
//! Django's middlewares communicate through attributes on the request object:
//! `request.session.accessed`, `request.META["CSRF_COOKIE_NEEDS_UPDATE"]`,
//! `request.csrf_exempt` (set by `APIView.as_view`'s `csrf_exempt` wrapper),
//! `request.user` (a lazy object that loads the session when touched). The
//! pipeline reproduces those flags here so that response headers match what
//! Django emits (HTTP_CONTRACT §1.1 is the list of observable consequences).

use std::net::{IpAddr, SocketAddr};

use http::{HeaderMap, Method};

use crate::ids::UserId;

/// Everything a middleware can look at, plus the flags later phases fill in.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub method: Method,
    /// `request.path_info`: percent-decoded, used for routing and APPEND_SLASH.
    pub path_info: String,
    /// `request.path`: the same decoded value in Django 5.2 (`ASGIRequest.path =
    /// scope["path"]`), used by the session middleware and the request log.
    pub path: String,
    /// `request.META["QUERY_STRING"]`, without the leading `?`.
    pub query_string: String,
    pub headers: HeaderMap,
    /// `REMOTE_ADDR`, when the server exposes it.
    pub peer_addr: Option<SocketAddr>,
    /// `request.is_secure()`: true when `X-Forwarded-Proto: https`, because
    /// `production.py:15` sets `SECURE_PROXY_SSL_HEADER`.
    pub is_secure: bool,

    // ---- flags written by middlewares, read by later ones ---------------
    /// `request.session.accessed`: set when anything touches the session
    /// (in practice, when the request logger evaluates `request.user`).
    pub session_accessed: bool,
    /// `request.session.modified`.
    pub session_modified: bool,
    /// `request.META["CSRF_COOKIE_NEEDS_UPDATE"]`.
    pub csrf_cookie_needs_update: bool,
    /// `request.csrf_exempt`: every DRF view sets it (`rest_framework/views.py:143-149`).
    pub csrf_exempt: bool,
    /// `request.user`, when an authentication layer resolved one. Phase 2 fills
    /// this; touching it marks the session accessed, exactly like Django's
    /// `SimpleLazyObject`.
    pub user: Option<UserId>,

    /// The raw request body, read by `RequestBodySizeLimitMiddleware` (§9.1) and
    /// reused by later layers (Django caches `request.body`).
    pub body: Option<Vec<u8>>,
}

impl RequestContext {
    pub fn new(
        method: Method,
        path_info: impl Into<String>,
        query_string: impl Into<String>,
        headers: HeaderMap,
        peer_addr: Option<SocketAddr>,
    ) -> Self {
        let path_info = path_info.into();
        let is_secure = header_eq(&headers, "x-forwarded-proto", "https");
        Self {
            method,
            path: path_info.clone(),
            path_info,
            query_string: query_string.into(),
            headers,
            peer_addr,
            is_secure,
            session_accessed: false,
            session_modified: false,
            csrf_cookie_needs_update: false,
            csrf_exempt: false,
            user: None,
            body: None,
        }
    }

    /// A case-insensitive header lookup that returns the value as `&str`
    /// (non-UTF-8 values are treated as absent, as Django's `request.headers`
    /// decodes with Latin-1 and can never produce an invalid string).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// `request.get_full_path()`: `escape_uri_path(path)` plus
    /// `?` + `iri_to_uri(query)` when the query is non-empty.
    pub fn full_path(&self) -> String {
        let path = crate::py::escape_uri_path(&self.path);
        if self.query_string.is_empty() {
            path
        } else {
            format!("{}?{}", path, crate::py::iri_to_uri(&self.query_string))
        }
    }

    /// `request.get_full_path(force_append_slash=True)` with the query string
    /// escaped like `iri_to_uri` (HTTP_CONTRACT §6.1). The caller applies
    /// `escape_leading_slashes`.
    pub fn full_path_with_slash(&self) -> String {
        let mut path = crate::py::escape_uri_path(&self.path);
        if !path.ends_with('/') {
            path.push('/');
        }
        if self.query_string.is_empty() {
            path
        } else {
            format!("{}?{}", path, crate::py::iri_to_uri(&self.query_string))
        }
    }

    /// `crum`/`AuthenticationMiddleware`: reading `request.user` evaluates the
    /// lazy object, which reads the session and marks it accessed.
    pub fn touch_user(&mut self) -> Option<UserId> {
        self.session_accessed = true;
        self.user
    }

    /// `plane/utils/ip_address.py:199-205` `get_client_ip`: the first
    /// `X-Forwarded-For` entry verbatim, else `REMOTE_ADDR`. It deliberately
    /// ignores `NUM_PROXIES` (ENV.md §2.6).
    pub fn client_ip(&self) -> Option<String> {
        match self.header("x-forwarded-for") {
            Some(forwarded) if !forwarded.is_empty() => {
                Some(forwarded.split(',').next().unwrap_or(forwarded).to_string())
            }
            _ => self.remote_addr(),
        }
    }

    /// `REMOTE_ADDR` as text, without the port.
    pub fn remote_addr(&self) -> Option<String> {
        self.peer_addr.map(|address| address.ip().to_string())
    }

    /// `REMOTE_ADDR` as an `IpAddr`.
    pub fn remote_ip(&self) -> Option<IpAddr> {
        self.peer_addr.map(|address| address.ip())
    }

    /// The cookie named `name` from the `Cookie` header, if present.
    ///
    /// This ports `django.http.cookie.parse_cookie`: morsels are split on `;`,
    /// keys and values are stripped, and a double-quoted value is unquoted.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        let header = self.header("cookie")?;
        for morsel in header.split(';') {
            let Some((key, value)) = morsel.split_once('=') else {
                continue;
            };
            if key.trim() != name {
                continue;
            }
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value);
            return Some(value);
        }
        None
    }
}

fn header_eq(headers: &HeaderMap, name: &str, expected: &str) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value == expected)
        .unwrap_or(false)
}

/// DRF's `BaseThrottle.get_ident` (HTTP_CONTRACT §12.2, ENV.md §2.6).
///
/// * `num_proxies == None`: all `X-Forwarded-For` entries concatenated with all
///   whitespace removed (the foot-gun: one bucket for every proxied client);
/// * `0`, or no header: the peer address;
/// * `n`: the n-th address from the right, trimmed.
pub fn throttle_ident(
    x_forwarded_for: Option<&str>,
    remote_addr: Option<&str>,
    num_proxies: Option<u16>,
) -> Option<String> {
    let remote = remote_addr.map(str::to_string);
    match num_proxies {
        None => match x_forwarded_for {
            Some(forwarded) if !forwarded.is_empty() => {
                Some(forwarded.split_whitespace().collect::<String>())
            }
            _ => remote,
        },
        Some(0) => remote,
        Some(num_proxies) => match x_forwarded_for {
            None => remote,
            Some(forwarded) => {
                let addresses: Vec<&str> = forwarded.split(',').collect();
                let index = addresses.len().saturating_sub(usize::from(num_proxies));
                addresses
                    .get(index)
                    .map(|address| address.trim().to_string())
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(headers: &[(&str, &str)]) -> RequestContext {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                http::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                http::header::HeaderValue::from_str(value).unwrap(),
            );
        }
        RequestContext::new(Method::GET, "/", "", map, None)
    }

    #[test]
    fn paths() {
        let context = RequestContext::new(Method::GET, "/a b", "x=1&y=%20", HeaderMap::new(), None);
        assert_eq!(context.path, "/a b");
        assert_eq!(context.full_path(), "/a%20b?x=1&y=%20");
        assert_eq!(context.full_path_with_slash(), "/a%20b/?x=1&y=%20");

        let bare = RequestContext::new(Method::POST, "/api/instances", "", HeaderMap::new(), None);
        assert_eq!(bare.full_path_with_slash(), "/api/instances/");
    }

    #[test]
    fn secure_comes_from_the_proxy_header() {
        assert!(!ctx(&[]).is_secure);
        assert!(ctx(&[("x-forwarded-proto", "https")]).is_secure);
        assert!(!ctx(&[("x-forwarded-proto", "HTTPS")]).is_secure);
        assert!(!ctx(&[("x-forwarded-proto", "http")]).is_secure);
    }

    #[test]
    fn cookies() {
        let context = ctx(&[("cookie", "session-id=abc; other=\"x y\"; broken")]);
        assert_eq!(context.cookie("session-id"), Some("abc"));
        assert_eq!(context.cookie("other"), Some("x y"));
        assert_eq!(context.cookie("broken"), None);
        assert_eq!(context.cookie("missing"), None);
        assert_eq!(ctx(&[]).cookie("session-id"), None);
    }

    // HTTP_CONTRACT §12.2: the exact identity table.
    #[test]
    fn throttle_identities() {
        assert_eq!(
            throttle_ident(None, Some("203.0.113.7"), None).as_deref(),
            Some("203.0.113.7")
        );
        assert_eq!(
            throttle_ident(None, Some("203.0.113.7"), Some(1)).as_deref(),
            Some("203.0.113.7")
        );
        assert_eq!(
            throttle_ident(Some("1.1.1.1, 2.2.2.2"), Some("10.0.0.1"), None).as_deref(),
            Some("1.1.1.1,2.2.2.2")
        );
        assert_eq!(
            throttle_ident(Some("1.1.1.1, 2.2.2.2"), Some("10.0.0.1"), Some(1)).as_deref(),
            Some("2.2.2.2")
        );
        assert_eq!(
            throttle_ident(Some("1.1.1.1, 2.2.2.2"), Some("10.0.0.1"), Some(2)).as_deref(),
            Some("1.1.1.1")
        );
        assert_eq!(
            throttle_ident(Some(" 1.1.1.1 ,  2.2.2.2 "), Some("10.0.0.1"), Some(2)).as_deref(),
            Some("1.1.1.1")
        );
        assert_eq!(
            throttle_ident(Some("1.1.1.1, 2.2.2.2"), Some("10.0.0.1"), Some(0)).as_deref(),
            Some("10.0.0.1")
        );
        // More hops than configured: the left-most entry wins.
        assert_eq!(
            throttle_ident(Some("1.1.1.1, 2.2.2.2"), Some("10.0.0.1"), Some(5)).as_deref(),
            Some("1.1.1.1")
        );
        assert_eq!(
            throttle_ident(Some("1.1.1.1"), None, Some(1)).as_deref(),
            Some("1.1.1.1")
        );
    }
}
