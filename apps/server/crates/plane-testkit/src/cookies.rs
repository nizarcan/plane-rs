// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `Set-Cookie` parsing and a per-target cookie jar.
//!
//! The harness deliberately uses no HTTP cookie store: it must see every raw
//! `Set-Cookie` header so the comparator can check attributes, and it keeps
//! named jars so a scenario can hold a normal user session and an instance
//! admin session at the same time.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::timeparse::parse_http_date;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCookie {
    pub name: String,
    pub value: String,
    /// Attributes in wire order; `Set-Cookie` has no attribute, always
    /// `(name, "")`.
    pub attributes: Vec<(String, String)>,
}

/// Parse a `Set-Cookie` value. Returns `None` when the header is malformed
/// (no `name=value` pair), which the comparator then reports verbatim.
pub fn parse_set_cookie(raw: &str) -> Option<ParsedCookie> {
    let mut parts = raw.split(';');
    let first = parts.next()?.trim();
    let (name, value) = first.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut attributes = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once('=') {
            Some((key, value)) => {
                attributes.push((key.trim().to_string(), value.trim().to_string()))
            }
            None => attributes.push((part.to_string(), String::new())),
        }
    }
    Some(ParsedCookie {
        name: name.to_string(),
        value: value.trim().to_string(),
        attributes,
    })
}

impl ParsedCookie {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Serialise a parsed cookie in a canonical form for comparison: the value as
/// given and the attributes sorted by lower-case name.
pub fn canonical_set_cookie(cookie: &ParsedCookie, value: &str) -> String {
    let mut attributes: Vec<String> = cookie
        .attributes
        .iter()
        .map(|(key, attribute_value)| {
            if attribute_value.is_empty() {
                key.to_ascii_lowercase()
            } else {
                format!("{}={attribute_value}", key.to_ascii_lowercase())
            }
        })
        .collect();
    attributes.sort();
    let mut out = format!("{}={value}", cookie.name);
    for attribute in attributes {
        out.push_str("; ");
        out.push_str(&attribute);
    }
    out
}

/// A cookie jar keyed by name. Values are kept exactly as received; a later
/// `Set-Cookie` for the same name (same path) overwrites it, matching browser
/// behaviour for this harness's purposes.
#[derive(Debug, Clone, Default)]
pub struct CookieJar {
    values: BTreeMap<String, String>,
}

impl CookieJar {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn cookie_header(&self) -> Option<String> {
        if self.values.is_empty() {
            return None;
        }
        Some(
            self.values
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    /// Apply one `Set-Cookie` header at wall-clock time `now`.
    pub fn apply_set_cookie(&mut self, raw: &str, now: DateTime<Utc>) {
        let Some(cookie) = parse_set_cookie(raw) else {
            return;
        };
        if is_expired(&cookie, now) {
            self.values.remove(&cookie.name);
            return;
        }
        self.values.insert(cookie.name, cookie.value);
    }
}

fn is_expired(cookie: &ParsedCookie, now: DateTime<Utc>) -> bool {
    if cookie.value.is_empty() {
        return true;
    }
    if let Some(max_age) = cookie.attribute("Max-Age")
        && max_age.parse::<i64>().is_ok_and(|seconds| seconds <= 0)
    {
        return true;
    }
    if let Some(expires) = cookie.attribute("Expires")
        && let Some(at) = parse_http_date(expires)
    {
        return at <= now;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().expect("valid time")
    }

    #[test]
    fn parses_value_and_attributes() {
        let cookie = parse_set_cookie(
            "session-id=abc; expires=Sun, 22 Nov 2026 20:00:00 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax",
        )
        .expect("parse");
        assert_eq!(cookie.name, "session-id");
        assert_eq!(cookie.value, "abc");
        assert_eq!(cookie.attribute("Path"), Some("/"));
        assert_eq!(cookie.attribute("Max-Age"), Some("604800"));
        assert_eq!(cookie.attribute("HttpOnly"), Some(""));
        assert_eq!(
            canonical_set_cookie(&cookie, "abc"),
            "session-id=abc; expires=Sun, 22 Nov 2026 20:00:00 GMT; httponly; max-age=604800; path=/; samesite=Lax"
        );
    }

    #[test]
    fn attributes_are_order_insensitive() {
        let a = parse_set_cookie("a=1; Path=/; HttpOnly; SameSite=Lax").expect("parse");
        let b = parse_set_cookie("a=1; SameSite=Lax; HttpOnly; Path=/").expect("parse");
        assert_eq!(canonical_set_cookie(&a, "1"), canonical_set_cookie(&b, "1"));
    }

    #[test]
    fn deletions_remove_the_cookie() {
        let mut jar = CookieJar::default();
        jar.apply_set_cookie("session-id=abc; Path=/", at(1_000));
        assert_eq!(jar.get("session-id"), Some("abc"));
        jar.apply_set_cookie(
            "session-id=; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/",
            at(1_000),
        );
        assert_eq!(jar.get("session-id"), None);
        assert_eq!(jar.cookie_header(), None);
    }

    #[test]
    fn cookie_header_is_stable() {
        let mut jar = CookieJar::default();
        jar.apply_set_cookie("b=2; Path=/", at(1_000));
        jar.apply_set_cookie("a=1; Path=/", at(1_000));
        assert_eq!(jar.cookie_header().as_deref(), Some("a=1; b=2"));
    }
}
