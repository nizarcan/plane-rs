// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's `URLValidator`, ported once for every serializer that runs it.
//!
//! Callers today are `IssueLinkSerializer.validate_url`
//! (`plane/app/serializers/issue.py:585-592`) and
//! `ModuleLinkSerializer.validate_url`
//! (`plane/app/serializers/module.py:178-186`); both also run DRF's
//! `URLField`, whose validator is the same `django.core.validators
//! .URLValidator`, so a handler that needs the exact accept/reject decision
//! can call this function directly (the field error is DRF's `"Enter a valid
//! URL."`).

// ---------------------------------------------------------------------------
// Django's URLValidator
// ---------------------------------------------------------------------------

/// `URLValidator.__call__` (`django/core/validators.py:107-223`, Django 5.2).
///
/// The Python is three checks and two regexes:
///
/// 1. `len(value) > 2048` (`MAX_URL_LENGTH`) or a `\t`/`\r`/`\n` anywhere is
///    invalid; the scheme (`value.split("://")[0].lower()`) must be one of
///    `http`, `https`, `ftp`, `ftps`;
/// 2. `^(?:[a-z0-9.+-]*)://(?:userinfo@)?(ipv4|ipv6|host)(:port)?([/?#]\S*)?$`
///    (`re.IGNORECASE`);
/// 3. the IPv6 literal (inside brackets) must parse, and
///    `urlsplit(value).hostname` must be non-empty and at most 253 characters.
///
/// The host grammar is `DomainNameValidator`'s: a `hostname` label, any number
/// of `domain` labels, then a `tld` of at least two letters/hyphens (or
/// `xn--` + alphanumerics) that may end with a dot, with the Unicode-letter
/// range `U+00A1..=U+FFFF` allowed anywhere in a label and no label starting or
/// ending with `-`. `localhost` is the one single-label host.
///
/// The Rust port cannot use a regex: the Python patterns contain lookbehind
/// (`(?<!-)`), which the `regex` crate does not support. The grammar is parsed
/// by hand instead; the test below pins it against cases recorded from Django.
pub fn django_url_valid(value: &str) -> bool {
    /// `django.utils.http.MAX_URL_LENGTH`.
    const MAX_URL_LENGTH: usize = 2048;
    // `unsafe_chars = frozenset("\t\r\n")`.
    if value.chars().count() > MAX_URL_LENGTH || value.contains(['\t', '\r', '\n']) {
        return false;
    }
    // `scheme = value.split("://")[0].lower()`.
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if !matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "ftp" | "ftps"
    ) {
        return false;
    }
    // `^(?:[a-z0-9.+-]*)://`
    if !scheme
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '+' | '-'))
    {
        return false;
    }
    // `(?:[/?#][^\s]*)?\Z`: the first path delimiter ends the authority, and
    // a path may not contain whitespace.
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(index) => (&rest[..index], Some(&rest[index..])),
        None => (rest, None),
    };
    if path.is_some_and(|path| path.chars().any(char::is_whitespace)) {
        return false;
    }
    if !valid_authority(authority) {
        return false;
    }
    // The `urlsplit(value).hostname` length check; the regex already rejected
    // an empty host.
    let hostname = hostname_of(authority);
    !hostname.is_empty() && hostname.chars().count() <= 253
}

/// `(?:userinfo@)?host(:port)?`.
fn valid_authority(authority: &str) -> bool {
    if authority.is_empty() {
        return false;
    }
    // The userinfo alternative: `[^\s:@/]+(?::[^\s:@/]*)?@`. Try every `@` as
    // the separator (the regex's backtracking does the same).
    for (index, _) in authority.match_indices('@') {
        if valid_userinfo(&authority[..index]) && valid_host_port(&authority[index + 1..]) {
            return true;
        }
    }
    valid_host_port(authority)
}

/// `[^\s:@/]+(?::[^\s:@/]*)?`.
fn valid_userinfo(userinfo: &str) -> bool {
    let forbidden =
        |character: char| character.is_whitespace() || matches!(character, ':' | '@' | '/');
    match userinfo.split_once(':') {
        Some((user, password)) => {
            !user.is_empty() && !user.chars().any(forbidden) && !password.chars().any(forbidden)
        }
        None => !userinfo.is_empty() && !userinfo.chars().any(forbidden),
    }
}

/// `(ipv4|ipv6|host)(:[0-9]{1,5})?`.
fn valid_host_port(host_port: &str) -> bool {
    if let Some(rest) = host_port.strip_prefix('[') {
        // `ipv6_re = \[[0-9a-f:.]+\]`, then Python's `validate_ipv6_address`.
        let Some((inside, after)) = rest.split_once(']') else {
            return false;
        };
        if inside.is_empty()
            || !inside
                .chars()
                .all(|character| character.is_ascii_hexdigit() || matches!(character, ':' | '.'))
        {
            return false;
        }
        if inside.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        return after.is_empty() || after.strip_prefix(':').is_some_and(valid_port);
    }
    match host_port.rsplit_once(':') {
        Some((host, port)) if valid_port(port) => valid_host(host),
        Some(_) => false,
        None => valid_host(host_port),
    }
}

/// `[0-9]{1,5}`.
fn valid_port(port: &str) -> bool {
    (1..=5).contains(&port.len()) && port.bytes().all(|byte| byte.is_ascii_digit())
}

/// `(?:ipv4|host_re)`.
fn valid_host(host: &str) -> bool {
    valid_ipv4(host) || host.eq_ignore_ascii_case("localhost") || valid_domain(host)
}

/// `ipv4_re`: four decimal octets with no leading zero.
fn valid_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            if part.is_empty() || part.len() > 3 {
                return false;
            }
            if part.len() > 1 && part.starts_with('0') {
                return false;
            }
            part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u16>().is_ok_and(|value| value <= 255)
        })
}

/// `host_re = (hostname_re domain_re tld_re | localhost)`.
fn valid_domain(host: &str) -> bool {
    // `tld_re`'s trailing `\.?`.
    let host = host.strip_suffix('.').unwrap_or(host);
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|label| label.is_empty()) {
        return false;
    }
    let (first, rest) = labels.split_first().expect("at least two labels");
    // `hostname_re`: `[a-zUL0-9](?:[-a-zUL0-9]{0,61}[a-zUL0-9])?`.
    if first.chars().count() > 63 || !label_characters(first) {
        return false;
    }
    if first.starts_with('-') || first.ends_with('-') {
        return false;
    }
    // `domain_re`: `(?:\.(?!-)[-a-zUL0-9]{1,63}(?<!-))*`.
    let (tld, middle) = rest.split_last().expect("at least one domain label");
    for label in middle {
        if label.chars().count() > 63
            || !label_characters(label)
            || label.starts_with('-')
            || label.ends_with('-')
        {
            return false;
        }
    }
    // `tld_re`: `\.(?!-)(?:[a-zUL-]{2,63}|xn--[a-z0-9]{1,59})(?<!-)\.?`.
    let length = tld.chars().count();
    if tld.starts_with('-') || tld.ends_with('-') {
        return false;
    }
    if let Some(rest) = tld
        .to_ascii_lowercase()
        .strip_prefix("xn--")
        .map(str::to_owned)
    {
        return (1..=59).contains(&rest.chars().count())
            && rest
                .chars()
                .all(|character| character.is_ascii_alphanumeric());
    }
    (2..=63).contains(&length)
        && tld.chars().all(|character| {
            character == '-' || character.is_ascii_alphabetic() || unicode_letter(character)
        })
}

/// `[a-zUL0-9-]` (`re.IGNORECASE`): ASCII letters, digits, hyphen and the
/// `U+00A1..=U+FFFF` range.
fn label_characters(label: &str) -> bool {
    label.chars().all(|character| {
        character.is_ascii_alphanumeric() || character == '-' || unicode_letter(character)
    })
}

fn unicode_letter(character: char) -> bool {
    ('\u{a1}'..='\u{ffff}').contains(&character)
}

/// `urlsplit(authority).hostname`: the host without the brackets of an IPv6
/// literal or the port, lowercased, `""` when there is no host.
fn hostname_of(authority: &str) -> String {
    let authority = match authority.split_once('@') {
        Some((_, rest)) => rest,
        None => authority,
    };
    if let Some(rest) = authority.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((inside, _)) => inside.to_ascii_lowercase(),
            None => String::new(),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            host.to_ascii_lowercase()
        }
        _ => authority.to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::django_url_valid;

    /// Expected values recorded from Django 5.2's `URLValidator` on the same
    /// inputs (`apps/api/.venv`, CPython 3.12.13).
    #[test]
    fn url_validator_matches_django() {
        let cases: &[(&str, bool)] = &[
            ("https://example.com", true),
            ("http://example.com", true),
            ("http://localhost", true),
            ("http://localhost:8000", true),
            ("http://127.0.0.1", true),
            ("http://127.0.0.1:65536", true),
            ("http://[::1]", true),
            ("http://[::1]:8080", true),
            ("http://[zz]", false),
            ("http://[::1", false),
            ("http://example", false),
            ("http://example.c", false),
            ("http://example.com.", true),
            ("http://example..com", false),
            ("http://-example.com", false),
            ("http://exa mple.com", false),
            ("http://example.com:123456", false),
            ("http://user:pass@example.com", true),
            ("http://user@example.com", true),
            ("http://@example.com", false),
            ("ftp://example.com", true),
            ("ftps://example.com", true),
            ("file://example.com", false),
            ("http://例え.jp", true),
            ("http://example.123", false),
            ("http://example.xn--p1ai", true),
            ("http://xn--e1afmkfd.xn--p1ai", true),
            ("http://example.com/path?q=1#frag", true),
            ("http://example.com/a b", false),
            ("http://example.com\\path", false),
            ("http://not a url", false),
            ("http://javascript:alert(1)", false),
            ("http://example.com:0", true),
            ("http://192.168.1.1", true),
            ("http://256.1.1.1", false),
            ("http://01.2.3.4", false),
            ("http://example.com#", true),
            ("http://example.com?x=1", true),
            ("http://例子.测试", true),
            ("http://example.com/ä", true),
            ("http://example.com/a\tb", false),
            ("HTTP://EXAMPLE.COM", true),
            ("http://example.com:abc", false),
            ("http://a.b.c.d.example.com", true),
            ("http://example.com/", true),
            ("http://example.com:80/x", true),
            ("http://example.com:1234567", false),
            ("http://user:@example.com", true),
            ("http://user:pa@ss@example.com", false),
            ("http://example.com..", false),
            ("http://ex-ample.com", true),
            ("http://example.co-m", true),
            ("http://example-.com", false),
            ("http://.example.com", false),
            ("http://example.com./x", true),
            ("http://EXAMPLE.COM", true),
            ("http://example.com/%20", true),
            ("http://example.com:x", false),
            ("https://sub.domain.example.museum", true),
            ("http://example.com?", true),
            ("http://example.com#frag x", false),
        ];
        for (value, expected) in cases {
            assert_eq!(django_url_valid(value), *expected, "input {value:?}");
        }
    }

    #[test]
    fn the_length_limit_counts_characters() {
        let mut at_limit = String::from("http://example.com/");
        at_limit.push_str(&"a".repeat(2048 - at_limit.chars().count()));
        assert_eq!(at_limit.chars().count(), 2048);
        assert!(django_url_valid(&at_limit));
        at_limit.push('a');
        assert!(!django_url_valid(&at_limit));
    }
}
