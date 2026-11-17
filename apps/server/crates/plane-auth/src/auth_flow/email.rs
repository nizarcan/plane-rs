// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's `validate_email` (`django/core/validators.py:210-283`, Django
//! 5.2.17), for the auth adapters.
//!
//! `Adapter.sanitize_email` (`plane/authentication/adapter/base.py:62-86`)
//! lower-cases and strips the address and then calls it; the only observable
//! answer is the boolean, because the caller raises `INVALID_EMAIL` instead of
//! the validator's text.
//!
//! This module was moved from `plane-http-app`'s `write::email` so the auth
//! flows and the user write paths share one implementation (they did, until
//! the auth crate needed it); `plane_http_app::write::email::validate_email`
//! re-exports it. The oracle corpus and its golden test stay there.

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
