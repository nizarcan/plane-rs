// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `django.utils.http.is_same_domain`, used by the CSRF referer check and the
//! trusted-origin subdomain match.

/// `.example.com` matches `example.com` and every subdomain; anything else is
/// an exact match. `host` may carry a port, which is compared as text — Django
/// does the same string comparison here.
pub fn is_same_domain(host: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = pattern.to_lowercase();
    (pattern.starts_with('.') && (host.ends_with(&pattern) || host == &pattern[1..]))
        || pattern == host
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_domain_rules() {
        assert!(is_same_domain("example.com", "example.com"));
        assert!(is_same_domain("example.com", ".example.com"));
        assert!(is_same_domain("a.example.com", ".example.com"));
        assert!(!is_same_domain("badexample.com", ".example.com"));
        assert!(!is_same_domain("example.com", ""));
        assert!(!is_same_domain("example.com", "other.com"));
        // Ports are part of the comparison.
        assert!(!is_same_domain("example.com:8000", "example.com"));
        assert!(is_same_domain("example.com:8000", "example.com:8000"));
    }
}
