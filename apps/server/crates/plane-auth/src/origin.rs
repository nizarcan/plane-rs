// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Plane's origin check for cookie-authenticated DRF writes:
//! `plane/authentication/session.py`.
//!
//! This is **not** Django's CSRF token check. `/api/`, `/api/public/` and
//! `/api/instances/` authenticate with `BaseSessionAuthentication`
//! (`plane/app/views/base.py:74,284`, `plane/space/views/base.py:69,159`,
//! `plane/license/api/views/base.py:47`), whose `enforce_csrf` only compares
//! the request's `Origin` against the request host and Plane's own configured
//! origins — no token, no cookie (`api:plane/authentication/session.py:38-63`).
//!
//! Quirks ported as they are (PROGRESS.md "Suspected upstream bugs"):
//!
//! * only `host[:port]` is compared for the same-origin case, not the scheme;
//! * `normalize_origin` lower-cases the whole `scheme://netloc`, so the path,
//!   query and fragment of a configured base URL are dropped;
//! * an `Origin` that does not parse, including the opaque `"null"`, is denied.

use std::collections::HashSet;

use http::Method;
use plane_core::config::Settings;
use plane_core::py;

/// `SAFE_METHODS` (`plane/authentication/session.py:11`).
pub fn is_safe_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

/// The exact `PermissionDenied` detail (`plane/authentication/session.py:63`).
pub const ORIGIN_NOT_ALLOWED: &str = "CSRF Failed: origin not allowed";

/// `normalize_origin(url)`: `scheme://netloc`, lower-cased, or `None` when
/// either part is missing (`plane/authentication/session.py:14-23`).
pub fn normalize_origin(url: &str) -> Option<String> {
    let split = py::urlsplit(url.trim()).ok()?;
    if split.scheme.is_empty() || split.netloc.is_empty() {
        return None;
    }
    Some(format!("{}://{}", split.scheme, split.netloc).to_lowercase())
}

/// `allowed_origins()` (`plane/authentication/session.py:26-35`).
///
/// Django caches the frozenset in an unbounded `lru_cache`; the inputs cannot
/// change while the process runs, and computing this is a handful of string
/// operations, so no cache is needed.
pub fn allowed_origins(settings: &Settings) -> HashSet<String> {
    let mut urls: Vec<&str> = Vec::with_capacity(8);
    for value in [
        settings.urls.web_url.as_deref(),
        settings.urls.app_base_url.as_deref(),
        settings.urls.admin_base_url.as_deref(),
        settings.urls.space_base_url.as_deref(),
        settings.urls.live_base_url.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        urls.push(value);
    }
    urls.extend(settings.cors.allowed_origins.iter().map(String::as_str));
    urls.extend(
        settings
            .cookies
            .csrf_trusted_origins
            .iter()
            .map(String::as_str),
    );
    urls.into_iter()
        .filter(|url| !url.is_empty())
        .filter_map(normalize_origin)
        .collect()
}

/// `is_allowed_origin(request)` (`plane/authentication/session.py:38-53`).
///
/// `request_host` is `request.get_host()` — the value `CommonMiddleware` already
/// validated at layer 4, so it cannot raise here.
pub fn is_allowed_origin(
    origin: Option<&str>,
    request_host: &str,
    allowed: &HashSet<String>,
) -> bool {
    let Some(origin) = origin else {
        // Browsers attach Origin to unsafe methods; requests without it come
        // from non-browser clients such as the live server.
        return true;
    };
    let Some(normalized) = normalize_origin(origin) else {
        // Includes the opaque "null" origin.
        return false;
    };
    let request_host = request_host.to_lowercase();
    if let Some((_, host)) = normalized.split_once("://")
        && host == request_host
    {
        return true;
    }
    allowed.contains(&normalized)
}

/// `BaseSessionAuthentication.enforce_csrf`.
///
/// `Ok(())` when the request may proceed; `Err(ORIGIN_NOT_ALLOWED)` is the
/// `PermissionDenied` the DRF runtime must render.
pub fn enforce_csrf(
    method: &Method,
    origin: Option<&str>,
    request_host: &str,
    settings: &Settings,
) -> Result<(), &'static str> {
    if is_safe_method(method) {
        return Ok(());
    }
    if is_allowed_origin(origin, request_host, &allowed_origins(settings)) {
        Ok(())
    } else {
        Err(ORIGIN_NOT_ALLOWED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("WEB_URL", "https://web.plane.so/")
            .set("ADMIN_BASE_URL", "https://admin.plane.so")
            .set("LIVE_BASE_URL", "https://live.plane.so/app");
        Settings::from_source(&env, plane_core::config::Role::Api).expect("settings")
    }

    #[test]
    fn normalizing() {
        assert_eq!(
            normalize_origin("HTTPS://Web.Plane.SO/path?q=1"),
            Some("https://web.plane.so".to_string())
        );
        assert_eq!(
            normalize_origin("  https://x.example  "),
            Some("https://x.example".into())
        );
        assert_eq!(normalize_origin("null"), None);
        assert_eq!(normalize_origin("web.plane.so"), None);
        assert_eq!(normalize_origin(""), None);
    }

    #[test]
    fn allowed_origin_set() {
        let allowed = allowed_origins(&settings());
        assert!(allowed.contains("https://web.plane.so"));
        assert!(allowed.contains("https://admin.plane.so"));
        assert!(allowed.contains("https://live.plane.so"));
        assert_eq!(allowed.len(), 3);
    }

    #[test]
    fn same_host_ignores_scheme() {
        // The bug ported as-is: https page, http request, same host:port.
        assert!(is_allowed_origin(
            Some("https://plane.example.com"),
            "plane.example.com",
            &HashSet::new()
        ));
        assert!(is_allowed_origin(
            Some("https://plane.example.com:8443"),
            "plane.example.com:8443",
            &HashSet::new()
        ));
        assert!(!is_allowed_origin(
            Some("https://plane.example.com:8443"),
            "plane.example.com",
            &HashSet::new()
        ));
    }

    #[test]
    fn no_origin_is_allowed_and_null_is_not() {
        assert!(is_allowed_origin(
            None,
            "plane.example.com",
            &HashSet::new()
        ));
        assert!(!is_allowed_origin(
            Some("null"),
            "plane.example.com",
            &HashSet::new()
        ));
    }

    #[test]
    fn an_unparsable_origin_is_denied() {
        for origin in ["garbage", "://x", "http://", "", "web.plane.so"] {
            assert!(
                !is_allowed_origin(Some(origin), "plane.example.com", &HashSet::new()),
                "origin {origin:?} must be denied"
            );
        }
    }

    #[test]
    fn configured_origins_are_allowed() {
        let allowed = allowed_origins(&settings());
        assert!(is_allowed_origin(
            Some("https://web.plane.so"),
            "api.plane.so",
            &allowed
        ));
        assert!(!is_allowed_origin(
            Some("https://evil.example"),
            "api.plane.so",
            &allowed
        ));
    }

    #[test]
    fn enforce_csrf_skips_safe_methods_and_checks_writes() {
        let settings = settings();
        assert_eq!(
            enforce_csrf(&Method::GET, Some("https://evil.example"), "x", &settings),
            Ok(())
        );
        assert_eq!(
            enforce_csrf(&Method::POST, Some("https://evil.example"), "x", &settings),
            Err(ORIGIN_NOT_ALLOWED)
        );
        assert_eq!(enforce_csrf(&Method::POST, None, "x", &settings), Ok(()));
        assert_eq!(
            enforce_csrf(
                &Method::DELETE,
                Some("https://web.plane.so"),
                "x",
                &settings
            ),
            Ok(())
        );
    }
}
