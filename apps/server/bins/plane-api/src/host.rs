// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ALLOWED_HOSTS` validation, ported from `django/http/request.py` and
//! `django/http/request.py:35-37` (`host_validation_re`).
//!
//! `CommonMiddleware.process_request` calls `request.get_host()` for every
//! request (`django/middleware/common.py:48`). A malformed `Host` header is a
//! `DisallowedHost` (a `SuspiciousOperation`) even when `ALLOWED_HOSTS=["*"]`,
//! because the syntax check runs first (HTTP_CONTRACT §11.3).

use regex::Regex;
use std::sync::LazyLock;

use plane_core::config::Settings;

/// `host_validation_re = ^([a-z0-9.-]+|\[[a-f0-9]*:[a-f0-9.:]+\])(?::([0-9]+))?$`
static HOST_VALIDATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([a-z0-9.-]+|\[[a-f0-9]*:[a-f0-9.:]+\])(?::([0-9]+))?$")
        .expect("the ALLOWED_HOSTS regex is a compile-time constant")
});

/// `django.utils.http.split_domain_port`: returns `(domain, port)` or
/// `("", "")` when the host is invalid.
pub fn split_domain_port(host: &str) -> (String, String) {
    match HOST_VALIDATION.captures(&host.to_lowercase()) {
        Some(captures) => {
            let domain = captures.get(1).map(|value| value.as_str()).unwrap_or("");
            let port = captures.get(2).map(|value| value.as_str()).unwrap_or("");
            // Remove a trailing dot from the domain.
            (domain.trim_end_matches('.').to_string(), port.to_string())
        }
        None => (String::new(), String::new()),
    }
}

/// `django.utils.http.is_same_domain`: `.example.com` matches `example.com` and
/// every subdomain; anything else is an exact match.
pub fn is_same_domain(host: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = pattern.to_lowercase();
    (pattern.starts_with('.') && (host.ends_with(&pattern) || host == &pattern[1..]))
        || pattern == host
}

/// `django.http.request.validate_host`.
pub fn validate_host(host: &str, allowed_hosts: &[String]) -> bool {
    allowed_hosts
        .iter()
        .any(|pattern| pattern == "*" || is_same_domain(host, pattern))
}

/// The effective `ALLOWED_HOSTS`, including Django's DEBUG fallback for an empty
/// list (`request.py:187-190`; unreachable from Plane's `.split(",")` parsing but
/// kept for fidelity).
fn effective_allowed_hosts(settings: &Settings) -> Vec<String> {
    if settings.debug && settings.allowed_hosts.is_empty() {
        vec![
            ".localhost".to_string(),
            "127.0.0.1".to_string(),
            "[::1]".to_string(),
        ]
    } else {
        settings.allowed_hosts.clone()
    }
}

/// The `Host` header Django would read: `HTTP_HOST` when present, otherwise the
/// PEP 333 reconstruction from `SERVER_NAME`/`SERVER_PORT` (the bind address,
/// which is what uvicorn puts in the ASGI scope).
pub fn raw_host(settings: &Settings, host_header: Option<&str>, is_secure: bool) -> String {
    if let Some(host) = host_header {
        return host.to_string();
    }
    let name = settings.rust.bind.ip().to_string();
    let port = settings.rust.bind.port();
    let default_port = if is_secure { 443 } else { 80 };
    if port == default_port {
        name
    } else {
        format!("{name}:{port}")
    }
}

/// `request.get_host()`: validate the raw host and return it, or `None` for a
/// `DisallowedHost`.
pub fn get_host(settings: &Settings, host_header: Option<&str>, is_secure: bool) -> Option<String> {
    let host = raw_host(settings, host_header, is_secure);
    let (domain, _port) = split_domain_port(&host);
    if !domain.is_empty() && validate_host(&domain, &effective_allowed_hosts(settings)) {
        Some(host)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(allowed: &[&str]) -> Settings {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set(
                "ALLOWED_HOSTS",
                &allowed
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
        plane_core::config::Settings::from_source(&env, plane_core::config::Role::Api)
            .expect("settings")
    }

    #[test]
    fn split_domain_port_rules() {
        assert_eq!(
            split_domain_port("Example.COM:8080"),
            ("example.com".to_string(), "8080".to_string())
        );
        assert_eq!(
            split_domain_port("example.com."),
            ("example.com".to_string(), String::new())
        );
        assert_eq!(
            split_domain_port("[::1]:8000"),
            ("[::1]".to_string(), "8000".to_string())
        );
        assert_eq!(
            split_domain_port("[::1]"),
            ("[::1]".to_string(), String::new())
        );
        // "-" is syntactically valid for Django's `[a-z0-9.-]+` pattern; only
        // the shapes below fail the fullmatch.
        assert_eq!(split_domain_port("-"), ("-".to_string(), String::new()));
        for invalid in ["", "bad_host", "foo/bar", "foo:bar", ":8080", "a b"] {
            assert_eq!(
                split_domain_port(invalid),
                (String::new(), String::new()),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn same_domain_rules() {
        assert!(is_same_domain("example.com", "example.com"));
        assert!(is_same_domain("example.com", ".example.com"));
        assert!(is_same_domain("a.example.com", ".example.com"));
        assert!(!is_same_domain("badexample.com", ".example.com"));
        assert!(!is_same_domain("example.com", ""));
        assert!(!is_same_domain("example.com", "other.com"));
    }

    #[test]
    fn wildcard_allows_any_valid_host() {
        let settings = settings(&["*"]);
        assert!(get_host(&settings, Some("anything.example.com"), false).is_some());
        assert!(get_host(&settings, Some("anything.example.com:9999"), false).is_some());
        // Malformed hosts are rejected even with "*".
        assert!(get_host(&settings, Some("bad_host"), false).is_none());
        assert!(get_host(&settings, Some(""), false).is_none());
        assert!(get_host(&settings, Some("foo/bar"), false).is_none());
    }

    #[test]
    fn explicit_list() {
        let settings = settings(&["plane.example.com", ".internal.example.com"]);
        assert!(get_host(&settings, Some("plane.example.com"), false).is_some());
        assert!(get_host(&settings, Some("plane.example.com:8000"), false).is_some());
        assert!(get_host(&settings, Some("other.example.com"), false).is_none());
        assert!(get_host(&settings, Some("a.internal.example.com"), false).is_some());
        assert!(get_host(&settings, Some("internal.example.com"), false).is_some());
        assert!(get_host(&settings, Some("notinternal.example.com"), false).is_none());
    }

    #[test]
    fn set_but_empty_allowed_hosts_rejects_everything() {
        // ENV.md §1.1: ALLOWED_HOSTS="" gives [""], which rejects every host.
        let settings = settings(&[""]);
        assert_eq!(settings.allowed_hosts, vec![String::new()]);
        assert!(get_host(&settings, Some("plane.example.com"), false).is_none());
    }

    #[test]
    fn missing_host_header_uses_the_bind_address() {
        let settings = {
            let env = plane_core::config::EnvMap::new()
                .set("REDIS_URL", "redis://127.0.0.1:6379/0")
                .set("PLANE_RS_BIND", "127.0.0.1:18101");
            plane_core::config::Settings::from_source(&env, plane_core::config::Role::Api)
                .expect("settings")
        };
        assert_eq!(
            get_host(&settings, None, false).as_deref(),
            Some("127.0.0.1:18101")
        );
    }
}
