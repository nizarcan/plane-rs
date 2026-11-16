// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Timestamp parsing and the regular expressions used to find UUIDs and
//! timestamps embedded in strings.
//!
//! Formats seen in Plane responses and database dumps:
//! - DRF's `DateTimeField`: RFC 3339 with `Z` or `+00:00`
//! - Postgres `to_jsonb(timestamp)`: `2026-11-15T20:16:00.123456+00:00`
//! - Session cookie `Expires`: RFC 2822 with `GMT`
//! - Python `repr` / Celery `argsrepr`: ISO 8601 with or without an offset

use std::sync::OnceLock;

use chrono::{DateTime, NaiveDateTime, Utc};
use regex::Regex;

pub fn uuid_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // `\b` keeps the match from starting inside a longer hex run (the
        // random 32-character `users.username` must not be treated as a UUID).
        Regex::new(
            r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
        )
        .expect("static UUID regex")
    })
}

/// `'epoch': <int>` inside a Celery `kwargsrepr` (the Python dict repr of the
/// task's kwargs). The value is `int(timezone.now().timestamp())`, so two
/// targets handling the same step in different seconds differ there.
pub fn celery_epoch_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"'epoch': (-?\d{9,11})").expect("static celery epoch regex"))
}

pub fn timestamp_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:?\d{2})?\b",
        )
        .expect("static timestamp regex")
    })
}

/// Parse a timestamp string, assuming UTC when no offset is present.
pub fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    let text = text.trim();
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(parsed.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            return Some(naive.and_utc());
        }
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f%:z", "%Y-%m-%d %H:%M:%S%.f%:z"] {
        if let Ok(parsed) = DateTime::parse_from_str(text, format) {
            return Some(parsed.with_timezone(&Utc));
        }
    }
    None
}

/// Parse an HTTP date, the format Django uses for `Set-Cookie; Expires`.
pub fn parse_http_date(text: &str) -> Option<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc2822(text.trim()) {
        return Some(parsed.with_timezone(&Utc));
    }
    for format in ["%a, %d %b %Y %H:%M:%S GMT", "%a, %d %b %Y %H:%M:%S %z"] {
        if let Ok(naive) = DateTime::parse_from_str(text.trim(), format) {
            return Some(naive.with_timezone(&Utc));
        }
    }
    None
}

/// True when `text` is exactly one UUID.
pub fn is_uuid(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_formats_we_expect() {
        let expected = "2026-11-15T20:16:00Z"
            .parse::<DateTime<Utc>>()
            .expect("parse");
        for text in [
            "2026-11-15T20:16:00Z",
            "2026-11-15T20:16:00+00:00",
            "2026-11-15T20:16:00.000000Z",
            "2026-11-15T20:16:00",
            "2026-11-15 20:16:00",
            "2026-11-15T20:16:00.000000+00:00",
        ] {
            assert_eq!(parse_timestamp(text), Some(expected), "{text}");
        }
    }

    #[test]
    fn rejects_dates_and_prose() {
        assert_eq!(parse_timestamp("2026-11-15"), None);
        assert_eq!(parse_timestamp("yesterday"), None);
        assert_eq!(parse_timestamp(""), None);
    }

    #[test]
    fn parses_django_cookie_expiry() {
        let parsed = parse_http_date("Sun, 22 Nov 2026 20:00:00 GMT").expect("parse");
        assert_eq!(parsed.to_rfc3339(), "2026-11-22T20:00:00+00:00");
    }

    #[test]
    fn uuid_matching_does_not_swallow_hex_usernames() {
        let uuid = "3f2b8f6e-1d4a-4b7c-8e2f-0a1b2c3d4e5f";
        assert!(uuid_regex().is_match(uuid));
        assert!(!uuid_regex().is_match("3f2b8f6e1d4a4b7c8e2f0a1b2c3d4e5f"));
    }

    #[test]
    fn timestamp_matching_finds_embedded_values() {
        let text = "created at 2026-11-15T20:16:00.123456+00:00 by user";
        let found = timestamp_regex().find(text).expect("match").as_str();
        assert_eq!(found, "2026-11-15T20:16:00.123456+00:00");
        assert!(parse_timestamp(found).is_some());
    }
}
