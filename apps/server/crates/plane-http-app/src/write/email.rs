// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Plane's `contains_url` (`plane/utils/url.py:15-53`) and the re-exported
//! Django `validate_email` the user write paths share with the auth adapters.
//!
//! * `validate_email` lives in `plane-auth` now
//!   (`plane_auth::auth_flow::email`): the `/auth/` adapters need it and
//!   `plane-http-app` depends on `plane-auth`, not the other way round.
//! * `contains_url` is `UserSerializer`'s
//!   `validate_first_name`/`validate_last_name`
//!   (`plane/app/serializers/user.py:16-24`).
//!
//! Both are oracle-tested against the real Python in
//! `tests/golden/email-validator.json`; regenerate it with
//! `tools/inventory/email_corpus.py`.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::Regex;

pub use plane_auth::auth_flow::email::validate_email;

/// CPython's `re.IGNORECASE` compares with the Unicode **simple** lower-case
/// mapping (`Py_UNICODE_TOLOWER`), not CaseFolding.txt's simple fold. The two
/// disagree for characters whose UnicodeData lower-case mapping differs from
/// their case-fold mapping; `U+0130` (LATIN CAPITAL LETTER I WITH DOT ABOVE)
/// is the one that can appear here: Python's `[a-z]` matches it, Rust's
/// `(?i)` does not. Map it before a regex match (one code point to one, so
/// lengths and label boundaries are unchanged).
fn python_ignore_case(value: &str) -> Cow<'_, str> {
    if value.contains('\u{0130}') {
        Cow::Owned(value.replace('\u{0130}', "i"))
    } else {
        Cow::Borrowed(value)
    }
}

const URL_PATTERN: &str = concat!(
    r"(?i)(?:https?://[^\s]+",
    r"|www\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*",
    r"|(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,6}",
    r"|(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?))",
);

fn url_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(URL_PATTERN).expect("the URL regex is valid"))
}

/// `plane.utils.url.contains_url(value)` (`plane/utils/url.py:32-53`).
///
/// The length caps are code points, like Python's `len`/slicing: a value over
/// 1000 characters is `False` outright, and each `\n`-separated line is
/// truncated to its first 500 characters before the search.
#[must_use]
pub fn contains_url(value: &str) -> bool {
    if value.chars().count() > 1000 {
        return false;
    }
    value.split('\n').any(|line| {
        let line: String = line.chars().take(500).collect();
        url_regex().is_match(&python_ignore_case(&line))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// `tests/golden/email-validator.json`, recorded from the real
    /// `django.core.validators.validate_email` and
    /// `plane.utils.url.contains_url` by `tools/inventory/email_corpus.py`.
    const CORPUS: &str = include_str!("../../tests/golden/email-validator.json");

    #[derive(Deserialize)]
    struct Case {
        input: String,
        valid: bool,
        contains_url: bool,
    }

    #[derive(Deserialize)]
    struct Corpus {
        cases: Vec<Case>,
    }

    #[test]
    fn validate_email_matches_django_and_contains_url_matches_plane() {
        let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
        assert!(corpus.cases.len() >= 200, "corpus shrank");
        let mut valid = 0;
        let mut urls = 0;
        for case in &corpus.cases {
            assert_eq!(
                validate_email(&case.input),
                case.valid,
                "validate_email({:?})",
                case.input
            );
            assert_eq!(
                contains_url(&case.input),
                case.contains_url,
                "contains_url({:?})",
                case.input
            );
            valid += usize::from(case.valid);
            urls += usize::from(case.contains_url);
        }
        // Both answers must be exercised on both sides, or a broken branch
        // could pass by returning a constant.
        assert!(valid > 30 && valid < corpus.cases.len() - 30);
        assert!(urls > 30 && urls < corpus.cases.len() - 30);
    }

    #[test]
    fn the_call_sites_messages_are_the_documented_branches() {
        // `_validate_new_email` (`user/base.py:120-126`).
        assert!(validate_email("new@example.com"));
        assert!(!validate_email("not-an-email"));
        assert!(!validate_email("new@example"));
        assert!(!validate_email(&format!("{}@example.com", "a".repeat(320))));
        // A quoted local part and a bracketed literal are both accepted.
        // (A quoted space is *not*: Django's class starts at `!`.)
        assert!(validate_email(r#""a!b"@example.com"#));
        assert!(!validate_email(r#""a b"@example.com"#));
        assert!(validate_email("user@[127.0.0.1]"));
        assert!(validate_email("user@[::1]"));
        // `validate_first_name`/`validate_last_name`.
        assert!(contains_url("see https://example.com"));
        assert!(contains_url("www.example.com"));
        assert!(contains_url("example.com"));
        assert!(contains_url("1.2.3.4"));
        assert!(!contains_url("Ada"));
        assert!(!contains_url(&"a".repeat(1001)));
    }
}
