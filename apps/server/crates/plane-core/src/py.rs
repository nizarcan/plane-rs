// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Python-compatible string parsing helpers.
//!
//! `plane-core`'s configuration loader has to reproduce the *exact* parsing rules
//! Django applies at import time (`ENV.md` §0 "Python parsing helpers"): `int()`,
//! `str.strip()`, `.split(",")`, `x or d`, `urlsplit()`/`is_valid_url()`. Each
//! helper here cites the Python behaviour it reproduces, and each has
//! table-driven tests in this module.

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

// ---------------------------------------------------------------------------
// Unicode decimal digits (Python's `str.isdecimal()` / `int()`)
// ---------------------------------------------------------------------------

/// Code-point ranges whose members have the Unicode general category `Nd`
/// (decimal digit), with the value of the first code point in each range.
///
/// Python's `str.isdecimal()` is true exactly for `Nd`, and `int()` accepts any
/// `Nd` digit, mapping it to its decimal value (`int("٣") == 3`). The table was
/// generated from CPython 3.12's `unicodedata` (Unicode 15.0.0); every range
/// starts at digit 0, so a digit's value is its offset from the range start.
const DECIMAL_RANGES: &[(u32, u32)] = &[
    (0x0030, 0x0039),
    (0x0660, 0x0669),
    (0x06F0, 0x06F9),
    (0x07C0, 0x07C9),
    (0x0966, 0x096F),
    (0x09E6, 0x09EF),
    (0x0A66, 0x0A6F),
    (0x0AE6, 0x0AEF),
    (0x0B66, 0x0B6F),
    (0x0BE6, 0x0BEF),
    (0x0C66, 0x0C6F),
    (0x0CE6, 0x0CEF),
    (0x0D66, 0x0D6F),
    (0x0DE6, 0x0DEF),
    (0x0E50, 0x0E59),
    (0x0ED0, 0x0ED9),
    (0x0F20, 0x0F29),
    (0x1040, 0x1049),
    (0x1090, 0x1099),
    (0x17E0, 0x17E9),
    (0x1810, 0x1819),
    (0x1946, 0x194F),
    (0x19D0, 0x19D9),
    (0x1A80, 0x1A89),
    (0x1A90, 0x1A99),
    (0x1B50, 0x1B59),
    (0x1BB0, 0x1BB9),
    (0x1C40, 0x1C49),
    (0x1C50, 0x1C59),
    (0xA620, 0xA629),
    (0xA8D0, 0xA8D9),
    (0xA900, 0xA909),
    (0xA9D0, 0xA9D9),
    (0xA9F0, 0xA9F9),
    (0xAA50, 0xAA59),
    (0xABF0, 0xABF9),
    (0xFF10, 0xFF19),
    (0x104A0, 0x104A9),
    (0x10D30, 0x10D39),
    (0x11066, 0x1106F),
    (0x110F0, 0x110F9),
    (0x11136, 0x1113F),
    (0x111D0, 0x111D9),
    (0x112F0, 0x112F9),
    (0x11450, 0x11459),
    (0x114D0, 0x114D9),
    (0x11650, 0x11659),
    (0x116C0, 0x116C9),
    (0x11730, 0x11739),
    (0x118E0, 0x118E9),
    (0x11950, 0x11959),
    (0x11C50, 0x11C59),
    (0x11D50, 0x11D59),
    (0x11DA0, 0x11DA9),
    (0x11F50, 0x11F59),
    (0x16A60, 0x16A69),
    (0x16AC0, 0x16AC9),
    (0x16B50, 0x16B59),
    (0x1D7CE, 0x1D7FF),
    (0x1E140, 0x1E149),
    (0x1E2F0, 0x1E2F9),
    (0x1E4F0, 0x1E4F9),
    (0x1E950, 0x1E959),
    (0x1FBF0, 0x1FBF9),
];

/// `unicodedata.decimal(c)` for a single character: the digit value, if the
/// character is a Unicode decimal digit.
pub fn decimal_value(c: char) -> Option<u32> {
    let cp = c as u32;
    for &(start, end) in DECIMAL_RANGES {
        if cp >= start && cp <= end {
            return Some(cp - start);
        }
    }
    None
}

/// Python's `str.isdecimal()`: non-empty and every character is `Nd`.
pub fn is_decimal(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| decimal_value(c).is_some())
}

// ---------------------------------------------------------------------------
// int()
// ---------------------------------------------------------------------------

/// Why Python's `int(value)` would raise for a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PyIntError {
    #[error("invalid literal for int() with base 10")]
    Invalid,
    #[error("value is out of the i64 range this port supports")]
    Overflow,
}

/// Python's `int(str)`: strips surrounding whitespace, accepts one optional
/// `+`/`-`, Unicode decimal digits, and underscores *between* digits
/// (`int("1_0") == 10`, `int("_1")` and `int("1_")` raise). `int("")` raises.
///
/// Python integers are unbounded; this port returns [`PyIntError::Overflow`]
/// for values that do not fit in `i64` (see `PROGRESS.md`).
pub fn int(value: &str) -> Result<i64, PyIntError> {
    let trimmed = value.trim_matches(|c: char| c.is_whitespace());
    let mut chars = trimmed.chars().peekable();

    let negative = match chars.peek() {
        Some('-') => {
            chars.next();
            true
        }
        Some('+') => {
            chars.next();
            false
        }
        _ => false,
    };

    let mut digits: Vec<u32> = Vec::new();
    let mut last_was_digit = false;
    while let Some(c) = chars.next() {
        if let Some(digit) = decimal_value(c) {
            digits.push(digit);
            last_was_digit = true;
        } else if c == '_' {
            // An underscore is only accepted between two digits.
            if !last_was_digit || chars.peek().and_then(|next| decimal_value(*next)).is_none() {
                return Err(PyIntError::Invalid);
            }
            last_was_digit = false;
        } else {
            return Err(PyIntError::Invalid);
        }
    }
    if digits.is_empty() {
        return Err(PyIntError::Invalid);
    }

    // Accumulate in u128 so that i64::MIN, whose magnitude does not fit in an
    // i64, is representable before the sign is applied.
    let mut magnitude: u128 = 0;
    for digit in digits {
        magnitude = magnitude
            .checked_mul(10)
            .and_then(|value| value.checked_add(u128::from(digit)))
            .ok_or(PyIntError::Overflow)?;
    }
    if negative {
        let limit = (i64::MAX as u128) + 1;
        if magnitude > limit {
            return Err(PyIntError::Overflow);
        }
        if magnitude == limit {
            Ok(i64::MIN)
        } else {
            Ok(-(magnitude as i64))
        }
    } else if magnitude > i64::MAX as u128 {
        Err(PyIntError::Overflow)
    } else {
        Ok(magnitude as i64)
    }
}

// ---------------------------------------------------------------------------
// `_retention_days` (ENV.md §2.8)
// ---------------------------------------------------------------------------

/// `common.py:475`: unset → default; a value `int()` rejects → default;
/// a negative value → default; `0` is accepted.
pub fn retention_days(raw: Option<&str>, default: i64) -> i64 {
    let Some(raw) = raw else {
        return default;
    };
    match int(raw) {
        Ok(days) if days >= 0 => days,
        _ => default,
    }
}

// ---------------------------------------------------------------------------
// urlsplit()/urlparse() (Python 3.12, urllib.parse)
// ---------------------------------------------------------------------------

/// Python's `_UNSAFE_URL_BYTES_TO_REMOVE`, which `urlsplit` deletes anywhere.
const UNSAFE_URL_BYTES: &[char] = &['\t', '\r', '\n'];

/// Python's `scheme_chars` (ASCII letters, digits, `+-.`).
fn is_scheme_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')
}

/// The result of Python's `urlsplit`, with `SplitResult`'s `username`,
/// `password`, `hostname` and `port` accessors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlSplit {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

impl UrlSplit {
    /// `SplitResult._userinfo`.
    fn userinfo(&self) -> (Option<&str>, Option<&str>) {
        match self.netloc.rsplit_once('@') {
            Some((userinfo, _hostinfo)) => match userinfo.split_once(':') {
                Some((username, password)) => (Some(username), Some(password)),
                None => (Some(userinfo), None),
            },
            None => (None, None),
        }
    }

    /// `SplitResult._hostinfo`.
    fn hostinfo(&self) -> (&str, Option<&str>) {
        let hostinfo = self
            .netloc
            .rsplit_once('@')
            .map(|(_, hostinfo)| hostinfo)
            .unwrap_or(&self.netloc);
        if let Some(bracketed_start) = hostinfo.find('[') {
            let after_bracket = &hostinfo[bracketed_start + 1..];
            let (hostname, after_close) = match after_bracket.split_once(']') {
                Some((hostname, rest)) => (hostname, rest),
                // Unbalanced brackets already failed `urlsplit`, but keep the
                // accessor total.
                None => (after_bracket, ""),
            };
            let port = after_close
                .split_once(':')
                .map(|(_, port)| port)
                .filter(|port| !port.is_empty());
            (hostname, port)
        } else {
            let (hostname, port) = match hostinfo.split_once(':') {
                Some((hostname, port)) => (hostname, Some(port)),
                None => (hostinfo, None),
            };
            (hostname, port.filter(|port| !port.is_empty()))
        }
    }

    /// `SplitResult.username` (percent escapes are *not* decoded; the caller
    /// applies [`unquote`] as `dj_database_url` does).
    pub fn username(&self) -> Option<&str> {
        self.userinfo().0
    }

    /// `SplitResult.password`.
    pub fn password(&self) -> Option<&str> {
        self.userinfo().1
    }

    /// `SplitResult.hostname`: lower-cased, without brackets or port.
    pub fn hostname(&self) -> Option<String> {
        let (hostname, _) = self.hostinfo();
        if hostname.is_empty() {
            return None;
        }
        Some(hostname.to_lowercase())
    }

    /// `SplitResult.port`: raises `ValueError` for a non-numeric or
    /// out-of-range port.
    pub fn port(&self) -> Result<Option<u16>, UrlError> {
        let (_, port) = self.hostinfo();
        let Some(port) = port else {
            return Ok(None);
        };
        if !port.chars().all(|c| c.is_ascii_digit()) {
            return Err(UrlError::InvalidPort(port.to_string()));
        }
        let value: u32 = port
            .parse()
            .map_err(|_| UrlError::InvalidPort(port.to_string()))?;
        if value > u16::MAX as u32 {
            return Err(UrlError::InvalidPort(port.to_string()));
        }
        Ok(Some(value as u16))
    }
}

/// Why [`urlsplit`] rejected a string (Python raises `ValueError`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    #[error("Invalid IPv6 URL")]
    InvalidIpv6,
    #[error("Port could not be cast to integer value as '{0}'")]
    InvalidPort(String),
}

/// Python's `urllib.parse.urlsplit` for `str` input.
///
/// Differences from CPython that are not observable for configuration values:
/// the NFKC check on non-ASCII netlocs (`_checknetloc`) and the bracketed-host
/// validation (`_check_bracketed_netloc`) are not implemented; only the
/// unbalanced-bracket check is. Both would reject inputs no deployment uses.
pub fn urlsplit(url: &str) -> Result<UrlSplit, UrlError> {
    // Only lstrip: Python deliberately preserves trailing space.
    let mut url: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !UNSAFE_URL_BYTES.contains(c))
        .collect();

    let mut scheme = String::new();
    if let Some(colon) = url.find(':') {
        let candidate = &url[..colon];
        if colon > 0
            && candidate.starts_with(|c: char| c.is_ascii_alphabetic())
            && candidate.chars().all(is_scheme_char)
        {
            scheme = candidate.to_ascii_lowercase();
            url = url[colon + 1..].to_string();
        }
    }

    let mut netloc = String::new();
    if let Some(rest) = url.strip_prefix("//") {
        let end = rest
            .char_indices()
            .find(|(_, c)| matches!(c, '/' | '?' | '#'))
            .map(|(index, _)| index)
            .unwrap_or(rest.len());
        netloc = rest[..end].to_string();
        url = rest[end..].to_string();
        let opens = netloc.matches('[').count();
        let closes = netloc.matches(']').count();
        if opens != closes {
            return Err(UrlError::InvalidIpv6);
        }
    }

    let mut fragment = String::new();
    if let Some((before, after)) = url.split_once('#') {
        fragment = after.to_string();
        url.truncate(before.len());
    }
    let mut query = String::new();
    if let Some((before, after)) = url.split_once('?') {
        query = after.to_string();
        url.truncate(before.len());
    }

    Ok(UrlSplit {
        scheme,
        netloc,
        path: url,
        query,
        fragment,
    })
}

/// `plane/utils/url.py:56` `is_valid_url`: `urlparse(u)` succeeds and both
/// `scheme` and `netloc` are non-empty.
pub fn is_valid_url(value: &str) -> bool {
    match urlsplit(value) {
        Ok(split) => !split.scheme.is_empty() && !split.netloc.is_empty(),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// url decoding/encoding
// ---------------------------------------------------------------------------

/// Python's `urllib.parse.unquote`: percent-decode UTF-8, replacing invalid
/// sequences (`errors="replace"`).
pub fn unquote(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

/// `urlparse.parse_qs(query)` with Python's defaults: `keep_blank_values=False`,
/// so `a=` disappears, and keys/values are `unquote_plus`-decoded (`+` is a
/// space). Returns values in first-appearance order.
pub fn parse_qs(query: &str) -> Vec<(String, Vec<String>)> {
    let mut pairs: Vec<(String, Vec<String>)> = Vec::new();
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let (raw_key, raw_value) = match part.split_once('=') {
            Some((key, value)) => (key, value),
            None => (part, ""),
        };
        // `keep_blank_values=False`: a missing or empty value drops the pair.
        if raw_value.is_empty() {
            continue;
        }
        let key = unquote_plus(raw_key);
        let value = unquote_plus(raw_value);
        match pairs.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, values)) => values.push(value),
            None => pairs.push((key, vec![value])),
        }
    }
    pairs
}

fn unquote_plus(value: &str) -> String {
    unquote(&value.replace('+', " "))
}

/// `django.utils.encoding.escape_uri_path`: `quote(path, safe="/:@&+$,-_.!~*'()")`.
///
/// Python's `quote` always keeps `A-Za-z0-9_.-~` plus the `safe` characters;
/// `percent-encoding`'s [`AsciiSet`] is the complement, so only the bytes that
/// must be escaped are listed here.
pub fn escape_uri_path(path: &str) -> String {
    const ENCODE: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'%')
        .add(b';')
        .add(b'<')
        .add(b'=')
        .add(b'>')
        .add(b'?')
        .add(b'[')
        .add(b'\\')
        .add(b']')
        .add(b'^')
        .add(b'`')
        .add(b'{')
        .add(b'|')
        .add(b'}');
    encode_with_python_safe(path, ENCODE)
}

/// `django.utils.http.escape_leading_slashes`: a scheme-relative redirect is
/// escaped so browsers cannot read it as another host.
pub fn escape_leading_slashes(url: &str) -> String {
    match url.strip_prefix("//") {
        Some(rest) => format!("/%2F{rest}"),
        None => url.to_string(),
    }
}

/// `django.utils.encoding.iri_to_uri` for the query-string case:
/// `quote(value, safe="/#%[]=:;$&()+,!?*@'~")`.
pub fn iri_to_uri(value: &str) -> String {
    const ENCODE: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'<')
        .add(b'>')
        .add(b'\\')
        .add(b'^')
        .add(b'`')
        .add(b'{')
        .add(b'|')
        .add(b'}');
    encode_with_python_safe(value, ENCODE)
}

/// Percent-encode with Python `quote` semantics: `A-Za-z0-9_.-~` always stay,
/// every byte not listed in `safe` becomes uppercase `%XX`.
fn encode_with_python_safe(value: &str, safe: &'static AsciiSet) -> String {
    utf8_percent_encode(value, safe).to_string()
}

// ---------------------------------------------------------------------------
// Splitting helpers
// ---------------------------------------------------------------------------

/// Python's `.split(",")` keeps empty fields (`"".split(",") == [""]`).
pub fn split_comma(value: &str) -> Vec<&str> {
    value.split(',').collect()
}

/// `os.environ.get("X") or default`: `None` and `""` both yield the default.
pub fn or_default(value: Option<&str>, default: &str) -> String {
    match value {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => default.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ENV.md §0: `int(x)` strips ASCII whitespace, accepts an optional +/-
    // and underscores between digits; `int("")` and `int("abc")` raise.
    #[test]
    fn python_int_matches_cpython() {
        let cases: &[(&str, Option<i64>)] = &[
            ("0", Some(0)),
            ("1", Some(1)),
            ("+1", Some(1)),
            ("-1", Some(-1)),
            ("007", Some(7)),
            (" 12 ", Some(12)),
            ("\n12\t", Some(12)),
            ("1_0", Some(10)),
            ("1_000_000", Some(1_000_000)),
            ("", None),
            ("abc", None),
            ("1.0", None),
            ("1 ", Some(1)),
            ("1_", None),
            ("_1", None),
            ("1__0", None),
            ("+ 1", None),
            ("--1", None),
            ("٣", Some(3)),
            ("١٢", Some(12)),
            ("１２", Some(12)),
        ];
        for (input, expected) in cases {
            assert_eq!(int(input).ok(), *expected, "int({input:?})");
        }
        assert_eq!(int("9223372036854775808"), Err(PyIntError::Overflow));
        assert_eq!(int("-9223372036854775808"), Ok(i64::MIN));
    }

    // ENV.md §1.10: NUM_PROXIES accepts `٣` because `str.isdecimal()` is
    // Unicode-aware.
    #[test]
    fn python_isdecimal() {
        for value in ["0", "12", "٣", "１２"] {
            assert!(is_decimal(value), "isdecimal({value:?})");
        }
        for value in ["", "1.0", " 1", "1 ", "abc", "-1", "+1", "²"] {
            assert!(!is_decimal(value), "isdecimal({value:?})");
        }
    }

    // ENV.md §2.8: `_retention_days` helper.
    #[test]
    fn retention_helper() {
        assert_eq!(retention_days(None, 14), 14);
        assert_eq!(retention_days(Some(""), 14), 14);
        assert_eq!(retention_days(Some("abc"), 14), 14);
        assert_eq!(retention_days(Some("-1"), 14), 14);
        assert_eq!(retention_days(Some("0"), 14), 0);
        assert_eq!(retention_days(Some("30"), 14), 30);
        assert_eq!(retention_days(Some(" 30 "), 14), 30);
    }

    // ENV.md §0/§1.8: `is_valid_url` = urlparse + non-empty scheme and netloc.
    #[test]
    fn is_valid_url_rules() {
        let cases: &[(&str, bool)] = &[
            ("http://localhost", true),
            ("https://example.com/plane", true),
            ("http://x ", true),
            (" http://x", true),
            // urlparse("localhost:3000") -> scheme "localhost", netloc ""
            ("localhost:3000", false),
            ("", false),
            ("/relative", false),
            ("//host/path", false),
            ("ftp://ftp.example.com", true),
            ("http://", false),
            ("http:///path", false),
        ];
        for (value, expected) in cases {
            assert_eq!(is_valid_url(value), *expected, "is_valid_url({value:?})");
        }
    }

    #[test]
    fn urlsplit_reproduces_urlparse() {
        let split =
            urlsplit("postgresql://plane:plane@127.0.0.1:55432/plane?sslmode=require").unwrap();
        assert_eq!(split.scheme, "postgresql");
        assert_eq!(split.netloc, "plane:plane@127.0.0.1:55432");
        assert_eq!(split.path, "/plane");
        assert_eq!(split.query, "sslmode=require");
        assert_eq!(split.username(), Some("plane"));
        assert_eq!(split.password(), Some("plane"));
        assert_eq!(split.hostname().as_deref(), Some("127.0.0.1"));
        assert_eq!(split.port(), Ok(Some(55432)));

        // Percent escapes are *not* decoded by urlsplit.
        let split = urlsplit("postgres://a%40b:p%3Aw@h/db").unwrap();
        assert_eq!(split.username(), Some("a%40b"));
        assert_eq!(split.password(), Some("p%3Aw"));
        assert_eq!(unquote(split.username().unwrap()), "a@b");
        assert_eq!(unquote(split.password().unwrap()), "p:w");

        // No userinfo, no port.
        let split = urlsplit("postgres://localhost/db").unwrap();
        assert_eq!(split.username(), None);
        assert_eq!(split.hostname().as_deref(), Some("localhost"));
        assert_eq!(split.port(), Ok(None));

        // Bracketed IPv6.
        let split = urlsplit("postgres://[::1]:5432/db").unwrap();
        assert_eq!(split.hostname().as_deref(), Some("::1"));
        assert_eq!(split.port(), Ok(Some(5432)));

        // `urlsplit` tolerates surrounding whitespace (leading only) and
        // removes \t\r\n anywhere.
        let split = urlsplit("\tpostgres://h/d\nb").unwrap();
        assert_eq!(split.scheme, "postgres");
        assert_eq!(split.path, "/db");

        // Bad ports raise like SplitResult.port.
        assert_eq!(
            urlsplit("postgres://h:abc/db").unwrap().port(),
            Err(UrlError::InvalidPort("abc".to_string()))
        );
        assert_eq!(
            urlsplit("postgres://h:70000/db").unwrap().port(),
            Err(UrlError::InvalidPort("70000".to_string()))
        );
        assert_eq!(
            urlsplit("postgres://[::1/db").unwrap_err(),
            UrlError::InvalidIpv6
        );
    }

    #[test]
    fn parse_qs_drops_blank_values_and_decodes() {
        let pairs = parse_qs("a=1&b=&c=true&d=false&e=x%20y&f=1&f=2&g");
        assert_eq!(
            pairs,
            vec![
                ("a".to_string(), vec!["1".to_string()]),
                ("c".to_string(), vec!["true".to_string()]),
                ("d".to_string(), vec!["false".to_string()]),
                ("e".to_string(), vec!["x y".to_string()]),
                ("f".to_string(), vec!["1".to_string(), "2".to_string()]),
            ]
        );
    }

    // HTTP_CONTRACT §6.1: the redirect Location is `escape_uri_path(path)` and
    // `iri_to_uri(query)`, with scheme-relative URLs escaped.
    #[test]
    fn uri_escaping() {
        assert_eq!(escape_uri_path("/api/instances/"), "/api/instances/");
        assert_eq!(escape_uri_path("/a b"), "/a%20b");
        assert_eq!(escape_uri_path("/a%20b"), "/a%2520b");
        assert_eq!(escape_uri_path("/a;b=c"), "/a%3Bb%3Dc");
        assert_eq!(iri_to_uri("x=1&y=2"), "x=1&y=2");
        assert_eq!(iri_to_uri("q=a b"), "q=a%20b");
        assert_eq!(iri_to_uri("q=%20"), "q=%20");
        assert_eq!(escape_leading_slashes("//evil/x"), "/%2Fevil/x");
        assert_eq!(escape_leading_slashes("/ok"), "/ok");
    }

    #[test]
    fn splitting() {
        assert_eq!(split_comma("a,,b"), vec!["a", "", "b"]);
        assert_eq!(split_comma(""), vec![""]);
        assert_eq!(or_default(None, "*"), "*");
        assert_eq!(or_default(Some(""), "*"), "*");
        assert_eq!(or_default(Some("0"), "*"), "0");
    }
}
