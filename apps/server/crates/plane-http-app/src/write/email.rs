// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's `validate_email` and Plane's `contains_url`, for the user write
//! paths.
//!
//! * `validate_email` (`django/core/validators.py:210-283`, Django 5.2.17) is
//!   the model field's validator, but Plane calls it directly in
//!   `UserEndpoint._validate_new_email` (`plane/app/views/user/base.py:120-126`)
//!   and `InstanceAdminSignInEndpoint.post`
//!   (`plane/license/api/views/admin.py:306-310`). Only the boolean is
//!   observable: the caller answers `{"error": "Invalid email format"}` (or
//!   its own message) instead of the validator's text, so the port returns
//!   `bool`, not a `ValidationError` value.
//! * `contains_url` (`plane/utils/url.py:15-53`) is `UserSerializer`'s
//!   `validate_first_name`/`validate_last_name`
//!   (`plane/app/serializers/user.py:16-24`).
//!
//! Both are oracle-tested against the real Python in
//! `tests/golden/email-validator.json`; regenerate it with
//! `tools/inventory/email_corpus.py`.

use std::borrow::Cow;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::OnceLock;

use regex::Regex;

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

/// `EmailValidator.user_regex` (`validators.py:217-224`): dot-atom or
/// quoted-string local part.
///
/// Python's `\001`.. escapes are written as `\x01`; `\Z` is Python 3.12's
/// absolute end of string, which Rust spells `\z`. `re.IGNORECASE` is `(?i)`.
const USER_PATTERN: &str = concat!(
    r#"(?i)(^[-!#$%&'*+/=?^_`{}|~0-9A-Z]+(\.[-!#$%&'*+/=?^_`{}|~0-9A-Z]+)*\z"#,
    r#"|^"([\x01-\x08\x0b\x0c\x0e-\x1f!#-\[\]-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])*"\z)"#,
);

/// `DomainNameValidator.hostname_re` (`validators.py:73-75`), per label.
const HOSTNAME_LABEL_PATTERN: &str =
    r"(?i)^[a-z\x{a1}-\x{ffff}0-9](?:[a-z\x{a1}-\x{ffff}0-9-]{0,61}[a-z\x{a1}-\x{ffff}0-9])?\z";

/// The middle labels of `DomainNameValidator.domain_re` (`:77`): the same
/// character set, 1-63 characters. Python's `(?!-)`/`(?<!-)` are checked in
/// [`valid_domain`], because Rust's `regex` has no look-around.
const DOMAIN_LABEL_PATTERN: &str = r"(?i)^[a-z\x{a1}-\x{ffff}0-9-]{1,63}\z";

/// `DomainNameValidator.tld_no_fqdn_re`'s general alternative (`:79-85`):
/// letters (no digits) and dashes, 2-63 characters.
const TLD_LABEL_PATTERN: &str = r"(?i)^[a-z\x{a1}-\x{ffff}-]{2,63}\z";

/// The punycode alternative: `xn--` plus 1-59 letters/digits.
const TLD_XN_PATTERN: &str = r"(?i)^xn--[a-z0-9]{1,59}\z";

/// `EmailValidator.literal_regex` (`validators.py:229-233`): the bracketed
/// address is validated separately as IPv4 or IPv6.
const LITERAL_PATTERN: &str = r"(?i)^\[([A-F0-9:.]+)\]\z";

/// Plane's `URL_PATTERN` (`plane/utils/url.py:10-29`), verbatim.
const URL_PATTERN: &str = concat!(
    r"(?i)(?:https?://[^\s]+",
    r"|www\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*",
    r"|(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,6}",
    r"|(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?))",
);

fn user_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(USER_PATTERN).expect("the user regex is valid"))
}

fn hostname_label_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(HOSTNAME_LABEL_PATTERN).expect("the hostname regex is valid"))
}

fn domain_label_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(DOMAIN_LABEL_PATTERN).expect("the domain regex is valid"))
}

fn tld_label_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(TLD_LABEL_PATTERN).expect("the TLD regex is valid"))
}

fn tld_xn_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(TLD_XN_PATTERN).expect("the punycode regex is valid"))
}

/// `EmailValidator.domain_regex` (`validators.py:225-228`): a hostname label,
/// zero or more `domain_re` labels, then `tld_no_fqdn_re`.
///
/// The two negative look-arounds apply to every label:
/// `(?!-)` forbids a leading dash and `(?<!-)` a trailing one
/// (`DomainNameValidator.domain_re`/`:79-85`). The TLD's first character can
/// therefore not be a dash either.
fn valid_domain(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let (hostname, rest) = labels.split_first().expect("at least two labels");
    let (tld, middle) = rest.split_last().expect("at least two labels");
    if !hostname_label_regex().is_match(&python_ignore_case(hostname)) {
        return false;
    }
    for label in middle {
        if !domain_label_regex().is_match(&python_ignore_case(label))
            || label.starts_with('-')
            || label.ends_with('-')
        {
            return false;
        }
    }
    if tld.starts_with('-') || tld.ends_with('-') {
        return false;
    }
    let tld = python_ignore_case(tld);
    tld_label_regex().is_match(&tld) || tld_xn_regex().is_match(&tld)
}

fn literal_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(LITERAL_PATTERN).expect("the literal regex is valid"))
}

fn url_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(URL_PATTERN).expect("the URL regex is valid"))
}

/// `django.core.validators.validate_email(value)`, as a boolean.
///
/// The checks are `EmailValidator.__call__`'s, in its order:
/// truthiness, an `@`, the 320-character limit, the local part, then the
/// domain (the `localhost` allowlist, the domain regex, or a bracketed
/// IPv4/IPv6 literal).
///
/// `value` is always a `str` at the call sites (the view calls
/// `request.data.get(...).strip().lower()` first), so Python's
/// `TypeError`/`AttributeError` for a non-string cannot arise here.
#[must_use]
pub fn validate_email(value: &str) -> bool {
    // `len(value)` counts code points, not bytes.
    if value.is_empty() || !value.contains('@') || value.chars().count() > 320 {
        return false;
    }
    // `user_part, domain_part = value.rsplit("@", 1)`.
    let Some((user, domain)) = value.rsplit_once('@') else {
        return false;
    };
    if !user_regex().is_match(&python_ignore_case(user)) {
        return false;
    }
    if domain == "localhost" || valid_domain(domain) {
        return true;
    }
    let Some(literal) = literal_regex().captures(domain) else {
        return false;
    };
    let Some(address) = literal.get(1) else {
        return false;
    };
    is_valid_ipv46_address(address.as_str())
}

/// `django.core.validators.validate_ipv46_address` (`validators.py:324-330`):
/// IPv4 first, then `django.utils.ipv6.is_valid_ipv6_address`.
fn is_valid_ipv46_address(value: &str) -> bool {
    if value.parse::<Ipv4Addr>().is_ok() {
        return true;
    }
    // `MAX_IPV6_ADDRESS_LENGTH` (`django/utils/ipv6.py:7`) then
    // `ipaddress.IPv6Address(value)`; Rust's std parser agrees with CPython
    // 3.12's on every literal the address regex can pass (4-digit hextet
    // limit, embedded IPv4 with no leading zeros).
    value.chars().count() <= 39 && value.parse::<Ipv6Addr>().is_ok()
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
