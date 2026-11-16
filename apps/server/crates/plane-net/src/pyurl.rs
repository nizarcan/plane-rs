// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! CPython 3.12's `urllib.parse`, to the extent `plane/utils/url_security.py`
//! and `plane/utils/ip_address.py` use it.
//!
//! Porting the security checks is not enough if the URL is parsed differently:
//! `http://evil.com@127.0.0.1/`, `http://[::ffff:127.0.0.1]/`, a port that is
//! not a number, a fullwidth solidus in the authority and a trailing-dot
//! hostname all take different paths through `urlsplit`, and each decides
//! whether the request is validated at all. So this module reproduces
//! `urlsplit`, `urlparse`, `urlunsplit`, `urljoin`, `unquote` and the
//! `hostname`/`port`/`username`/`password` properties from
//! `Lib/urllib/parse.py` (3.12.13), including the errors they raise:
//!
//! * leading C0 controls and spaces are stripped, and `\t`, `\r`, `\n` are
//!   removed anywhere (`_WHATWG_C0_CONTROL_OR_SPACE`,
//!   `_UNSAFE_URL_BYTES_TO_REMOVE`);
//! * the scheme is only recognised before the first `:` when it is
//!   `[A-Za-z][A-Za-z0-9+.-]*`, and is lowercased;
//! * the netloc exists only after `//` and ends at the first `/`, `?` or `#`;
//! * bracketed hosts are validated eagerly (`_check_bracketed_netloc`), which
//!   is why `http://[127.0.0.1]/` raises `An IPv4 address cannot be in
//!   brackets` before any other check;
//! * a non-ASCII netloc is rejected when NFKC normalisation would turn one of
//!   its characters into `/`, `?`, `#`, `@` or `:` (`_checknetloc`);
//! * `hostname` lowercases everything before a `%` zone id but keeps the zone
//!   case, and `port` raises on non-digits, on unicode digits and outside
//!   0-65535.
//!
//! `url_security._split_target` reads `scheme`, `hostname`, `port`,
//! `path`+`query` and the URL-embedded credentials in that order; `Target`
//! mirrors that order so error precedence is identical.
//!
//! Anything not listed above (IDNA encoding of the hostname, `params` handling
//! outside `urljoin`) is intentionally not re-implemented; the golden corpus
//! in `tests/golden/ssrf_urls.jsonl` pins what is.

use std::fmt;

/// A `ValueError` from `urllib.parse` / `ipaddress`, message included.
///
/// Plane's callers only inspect the message and the exception *class*
/// (`webhook_send_task` treats `ValueError` as non-retryable but
/// `requests.RequestException` as retryable), so the message is the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyUrlError {
    message: String,
}

impl PyUrlError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PyUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PyUrlError {}

pub const INVALID_SCHEME: &str = "Invalid URL scheme. Only HTTP and HTTPS are allowed";
pub const NO_HOSTNAME: &str = "Invalid URL: No hostname found";
pub const INVALID_IPV6_URL: &str = "Invalid IPv6 URL";
pub const IPV4_IN_BRACKETS: &str = "An IPv4 address cannot be in brackets";
pub const IPV_FUTURE_INVALID: &str = "IPvFuture address is invalid";
pub const PORT_OUT_OF_RANGE: &str = "Port out of range 0-65535";

// `Lib/urllib/parse.py`'s scheme tables.
const USES_NETLOC: &[&str] = &[
    "", "ftp", "http", "gopher", "nntp", "telnet", "imap", "wais", "file", "mms", "https", "shttp",
    "snews", "prospero", "rtsp", "rtspu", "rsync", "svn", "svn+ssh", "sftp", "nfs", "git",
    "git+ssh", "ws", "wss",
];
const USES_RELATIVE: &[&str] = &[
    "", "ftp", "http", "gopher", "nntp", "imap", "wais", "file", "https", "shttp", "mms",
    "prospero", "rtsp", "rtspu", "sftp", "svn", "svn+ssh", "snews", "ws", "wss",
];
const USES_PARAMS: &[&str] = &[
    "", "ftp", "hdl", "http", "imap", "https", "mms", "prospero", "rtsp", "rtspu", "sftp", "shttp",
    "sip", "sips", "snews", "svn", "svn+ssh", "telnet", "wais", "ws", "wss",
];

/// Characters whose NFKC normalisation (Unicode 15.0, as shipped with CPython
/// 3.12.13) contains `/`, `?`, `#`, `@` or `:`. `_checknetloc` raises when one
/// of these appears in a non-ASCII netloc. Derived by running
/// `unicodedata.normalize("NFKC", chr(cp))` over the whole code space; see
/// `tests::nfkc_table_matches_python_observation`.
const NFKC_DELIMITER_CHARS: &[char] = &[
    '\u{2047}', '\u{2048}', '\u{2049}', '\u{2100}', '\u{2101}', '\u{2105}', '\u{2106}', '\u{2A74}',
    '\u{FE13}', '\u{FE16}', '\u{FE55}', '\u{FE56}', '\u{FE5F}', '\u{FE6B}', '\u{FF03}', '\u{FF0F}',
    '\u{FF1A}', '\u{FF1F}', '\u{FF20}',
];

/// `urllib.parse.urlsplit()`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SplitUrl {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

/// `urllib.parse.urlparse()`: `urlsplit()` plus the `;params` split for
/// schemes in `uses_params`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedUrl {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub params: String,
    pub query: String,
    pub fragment: String,
}

fn check_scheme_chars(candidate: &str) -> bool {
    candidate
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// `_splitnetloc`: the netloc ends at the first `/`, `?` or `#`.
fn split_netloc(input: &str) -> (&str, &str) {
    match input.find(['/', '?', '#']) {
        Some(index) => (&input[..index], &input[index..]),
        None => (input, ""),
    }
}

/// `urllib.parse.urlsplit()`.
pub fn urlsplit(url: &str) -> Result<SplitUrl, PyUrlError> {
    // `url.lstrip(_WHATWG_C0_CONTROL_OR_SPACE)`, then drop tab/CR/LF anywhere.
    let trimmed = url.trim_start_matches(|c: char| (c as u32) <= 0x20);
    let cleaned: String;
    let mut rest: &str = if trimmed.contains(['\t', '\r', '\n']) {
        cleaned = trimmed
            .chars()
            .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
            .collect();
        &cleaned
    } else {
        trimmed
    };

    let mut scheme = String::new();
    if let Some(index) = rest.find(':')
        && index > 0
        && rest[..index]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && check_scheme_chars(&rest[..index])
    {
        scheme = rest[..index].to_ascii_lowercase();
        rest = &rest[index + 1..];
    }

    let mut netloc = "";
    if let Some(after_slashes) = rest.strip_prefix("//") {
        let (candidate, remainder) = split_netloc(after_slashes);
        let open = candidate.contains('[');
        let close = candidate.contains(']');
        if open != close {
            return Err(PyUrlError::new(INVALID_IPV6_URL));
        }
        if open && close {
            check_bracketed_netloc(candidate)?;
        }
        netloc = candidate;
        rest = remainder;
    }

    let mut fragment = "";
    if let Some((before, after)) = rest.split_once('#') {
        rest = before;
        fragment = after;
    }
    let mut query = "";
    if let Some((before, after)) = rest.split_once('?') {
        rest = before;
        query = after;
    }

    check_netloc(netloc)?;

    Ok(SplitUrl {
        scheme,
        netloc: netloc.to_string(),
        path: rest.to_string(),
        query: query.to_string(),
        fragment: fragment.to_string(),
    })
}

/// `urllib.parse.urlparse(url, scheme)`.
pub fn urlparse_with_scheme(url: &str, scheme: &str) -> Result<ParsedUrl, PyUrlError> {
    let split = urlsplit(url)?;
    let eff_scheme = if split.scheme.is_empty() {
        scheme
            .trim_start_matches(|c: char| (c as u32) <= 0x20)
            .replace(['\t', '\r', '\n'], "")
    } else {
        split.scheme
    };
    let (path, params) = if USES_PARAMS.contains(&eff_scheme.as_str()) && split.path.contains(';') {
        let (path, params) = split_params(&split.path);
        (path.to_string(), params.to_string())
    } else {
        (split.path, String::new())
    };
    Ok(ParsedUrl {
        scheme: eff_scheme,
        netloc: split.netloc,
        path,
        params,
        query: split.query,
        fragment: split.fragment,
    })
}

/// `urllib.parse.urlparse(url)`.
pub fn urlparse(url: &str) -> Result<ParsedUrl, PyUrlError> {
    urlparse_with_scheme(url, "")
}

/// `urllib.parse._splitparams`.
fn split_params(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(last_slash) => match path[last_slash..].find(';') {
            Some(offset) => {
                let index = last_slash + offset;
                (&path[..index], &path[index + 1..])
            }
            None => (path, ""),
        },
        None => match path.find(';') {
            Some(index) => (&path[..index], &path[index + 1..]),
            None => (path, ""),
        },
    }
}

/// `_check_bracketed_netloc`.
fn check_bracketed_netloc(netloc: &str) -> Result<(), PyUrlError> {
    let after_at = match netloc.rsplit_once('@') {
        Some((_, host)) => host,
        None => netloc,
    };
    match after_at.split_once('[') {
        Some((before, bracketed)) => {
            if !before.is_empty() {
                return Err(PyUrlError::new(INVALID_IPV6_URL));
            }
            let (hostname, port) = match bracketed.split_once(']') {
                Some(parts) => parts,
                None => (bracketed, ""),
            };
            if !port.is_empty() && !port.starts_with(':') {
                return Err(PyUrlError::new(INVALID_IPV6_URL));
            }
            check_bracketed_host(hostname)
        }
        None => {
            // Reachable when the only `[` is inside userinfo and the brackets
            // are split across the last `@`; CPython then checks the bare host.
            let (hostname, _) = match after_at.split_once(':') {
                Some(parts) => parts,
                None => (after_at, ""),
            };
            check_bracketed_host(hostname)
        }
    }
}

/// `_check_bracketed_host`.
fn check_bracketed_host(hostname: &str) -> Result<(), PyUrlError> {
    if hostname.starts_with('v') {
        return if is_ipvfuture(hostname) {
            Ok(())
        } else {
            Err(PyUrlError::new(IPV_FUTURE_INVALID))
        };
    }
    match crate::ipaddr::parse_python_ip(hostname) {
        Some(std::net::IpAddr::V4(_)) => Err(PyUrlError::new(IPV4_IN_BRACKETS)),
        Some(std::net::IpAddr::V6(_)) => Ok(()),
        None => Err(PyUrlError::new(format!(
            "{} does not appear to be an IPv4 or IPv6 address",
            repr_str(hostname)
        ))),
    }
}

/// `re.match(r"\Av[a-fA-F0-9]+\..+\Z", hostname)`.
fn is_ipvfuture(hostname: &str) -> bool {
    let Some(rest) = hostname.strip_prefix('v') else {
        return false;
    };
    let Some((hex, tail)) = rest.split_once('.') else {
        return false;
    };
    !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) && !tail.is_empty()
}

/// `_checknetloc` without pulling in a Unicode normalisation crate.
fn check_netloc(netloc: &str) -> Result<(), PyUrlError> {
    if netloc.is_empty() || netloc.is_ascii() {
        return Ok(());
    }
    let suspicious = netloc
        .chars()
        .filter(|c| !matches!(c, '@' | ':' | '#' | '?'))
        .any(|c| NFKC_DELIMITER_CHARS.contains(&c));
    if suspicious {
        return Err(PyUrlError::new(format!(
            "netloc '{netloc}' contains invalid characters under NFKC normalization"
        )));
    }
    Ok(())
}

impl SplitUrl {
    /// `SplitResult._hostinfo`: `(hostname, port)`, before the `hostname`
    /// property's lowercasing and the `port` property's integer conversion.
    fn hostinfo(&self) -> (Option<&str>, Option<&str>) {
        hostinfo(&self.netloc)
    }

    /// `SplitResult.hostname`: lowercased up to a `%` zone id, which keeps its
    /// case.
    pub fn hostname(&self) -> Option<String> {
        hostname_from(self.hostinfo().0)
    }

    /// `SplitResult.port`.
    pub fn port(&self) -> Result<Option<u16>, PyUrlError> {
        port_from(self.hostinfo().1)
    }

    /// `SplitResult.username` / `SplitResult.password`.
    pub fn userinfo(&self) -> (Option<String>, Option<String>) {
        userinfo_from(&self.netloc)
    }
}

impl ParsedUrl {
    fn hostinfo(&self) -> (Option<&str>, Option<&str>) {
        hostinfo(&self.netloc)
    }

    /// `ParseResult.hostname`; identical to [`SplitUrl::hostname`].
    pub fn hostname(&self) -> Option<String> {
        hostname_from(self.hostinfo().0)
    }

    /// `ParseResult.port`.
    pub fn port(&self) -> Result<Option<u16>, PyUrlError> {
        port_from(self.hostinfo().1)
    }

    /// `ParseResult.username` / `ParseResult.password`.
    pub fn userinfo(&self) -> (Option<String>, Option<String>) {
        userinfo_from(&self.netloc)
    }
}

fn hostinfo(netloc: &str) -> (Option<&str>, Option<&str>) {
    let hostinfo = match netloc.rsplit_once('@') {
        Some((_, host)) => host,
        None => netloc,
    };
    let (hostname, port) = match hostinfo.split_once('[') {
        Some((_, bracketed)) => {
            let (hostname, rest) = match bracketed.split_once(']') {
                Some(parts) => parts,
                None => (bracketed, ""),
            };
            let port = match rest.split_once(':') {
                Some((_, port)) => port,
                None => "",
            };
            (hostname, port)
        }
        None => match hostinfo.split_once(':') {
            Some(parts) => parts,
            None => (hostinfo, ""),
        },
    };
    (
        if hostname.is_empty() {
            None
        } else {
            Some(hostname)
        },
        if port.is_empty() { None } else { Some(port) },
    )
}

fn hostname_from(hostname: Option<&str>) -> Option<String> {
    let hostname = hostname?;
    match hostname.split_once('%') {
        Some((head, zone)) => Some(format!("{}%{zone}", head.to_lowercase())),
        None => Some(hostname.to_lowercase()),
    }
}

fn port_from(port: Option<&str>) -> Result<Option<u16>, PyUrlError> {
    let Some(port) = port else {
        return Ok(None);
    };
    if !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PyUrlError::new(format!(
            "Port could not be cast to integer value as {}",
            repr_str(port)
        )));
    }
    match port.parse::<u64>() {
        Ok(value) if value <= 65_535 => Ok(Some(value as u16)),
        _ => Err(PyUrlError::new(PORT_OUT_OF_RANGE)),
    }
}

fn userinfo_from(netloc: &str) -> (Option<String>, Option<String>) {
    let Some((userinfo, _)) = netloc.rsplit_once('@') else {
        return (None, None);
    };
    match userinfo.split_once(':') {
        Some((username, password)) => (Some(username.to_string()), Some(password.to_string())),
        None => (Some(userinfo.to_string()), None),
    }
}

/// `urllib.parse.urlunsplit()`.
fn urlunsplit(scheme: &str, netloc: &str, path: &str, query: &str, fragment: &str) -> String {
    let mut url = path.to_string();
    if !netloc.is_empty() {
        if !url.is_empty() && !url.starts_with('/') {
            url.insert(0, '/');
        }
        url = format!("//{netloc}{url}");
    } else if url.starts_with("//")
        || (!scheme.is_empty()
            && USES_NETLOC.contains(&scheme)
            && (url.is_empty() || url.starts_with('/')))
    {
        // CPython has two branches here that both prefix `//`.
        url = format!("//{url}");
    }
    if !scheme.is_empty() {
        url = format!("{scheme}:{url}");
    }
    if !query.is_empty() {
        url.push('?');
        url.push_str(query);
    }
    if !fragment.is_empty() {
        url.push('#');
        url.push_str(fragment);
    }
    url
}

fn with_params(path: &str, params: &str) -> String {
    if params.is_empty() {
        path.to_string()
    } else {
        format!("{path};{params}")
    }
}

/// `urllib.parse.urljoin()`.
///
/// Used by `url_security.pinned_fetch_following_redirects` to resolve a
/// `Location` against the current hop. CPython's version is not RFC 3986: it
/// returns the *original* `url` string when the scheme is not in
/// `uses_relative`, drops empty path segments only in the middle, and appends a
/// trailing slash after a final `.` or `..`. All of that is kept.
pub fn urljoin(base: &str, url: &str) -> Result<String, PyUrlError> {
    if base.is_empty() {
        return Ok(url.to_string());
    }
    if url.is_empty() {
        return Ok(base.to_string());
    }
    let base_url = urlparse_with_scheme(base, "")?;
    let relative = urlparse_with_scheme(url, &base_url.scheme)?;

    if relative.scheme != base_url.scheme || !USES_RELATIVE.contains(&relative.scheme.as_str()) {
        return Ok(url.to_string());
    }

    let mut netloc = relative.netloc.clone();
    if USES_NETLOC.contains(&relative.scheme.as_str()) {
        if !netloc.is_empty() {
            return Ok(urlunsplit(
                &relative.scheme,
                &netloc,
                &with_params(&relative.path, &relative.params),
                &relative.query,
                &relative.fragment,
            ));
        }
        netloc = base_url.netloc.clone();
    }

    let mut path = relative.path.clone();
    let mut params = relative.params.clone();
    let mut query = relative.query.clone();
    if path.is_empty() && params.is_empty() {
        path = base_url.path.clone();
        params = base_url.params.clone();
        if query.is_empty() {
            query = base_url.query.clone();
        }
        return Ok(urlunsplit(
            &relative.scheme,
            &netloc,
            &with_params(&path, &params),
            &query,
            &relative.fragment,
        ));
    }

    let mut base_parts: Vec<&str> = base_url.path.split('/').collect();
    if base_parts.last() != Some(&"") {
        base_parts.pop();
    }
    let segments: Vec<String> = if path.starts_with('/') {
        path.split('/').map(str::to_string).collect()
    } else {
        let mut combined: Vec<String> = base_parts.iter().map(|s| s.to_string()).collect();
        combined.extend(path.split('/').map(str::to_string));
        // `segments[1:-1] = filter(None, segments[1:-1])`: only the middle is
        // filtered, and only when it is non-empty.
        if combined.len() >= 3 {
            let last = combined.pop().expect("len >= 3");
            let first = combined.remove(0);
            combined.retain(|segment| !segment.is_empty());
            combined.insert(0, first);
            combined.push(last);
        }
        combined
    };

    let mut resolved: Vec<String> = Vec::with_capacity(segments.len());
    for segment in &segments {
        match segment.as_str() {
            ".." => {
                resolved.pop();
            }
            "." => {}
            other => resolved.push(other.to_string()),
        }
    }
    if matches!(segments.last().map(String::as_str), Some(".") | Some("..")) {
        resolved.push(String::new());
    }
    let joined = if resolved.is_empty() {
        "/".to_string()
    } else {
        resolved.join("/")
    };
    Ok(urlunsplit(
        &relative.scheme,
        &netloc,
        &with_params(&joined, &params),
        &query,
        &relative.fragment,
    ))
}

/// `urllib.parse.unquote(s, encoding="utf-8", errors="replace")`.
pub fn unquote(input: &str) -> String {
    if !input.contains('%') {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `repr()` of a Python `str`, for the error messages that embed a value.
pub fn repr_str(input: &str) -> String {
    let quote = if input.contains('\'') && !input.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(input.len() + 2);
    out.push(quote);
    for c in input.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if (c as u32) > 0x7f && !is_printable(c) => {
                let code = c as u32;
                if code <= 0xff {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// A conservative stand-in for `str.isprintable()`: Python escapes C0/C1
/// controls, separators and unassigned code points. Only used for error
/// messages, so over-escaping is harmless; under-escaping would change a
/// message.
fn is_printable(c: char) -> bool {
    let code = c as u32;
    if code <= 0x1f || (0x7f..=0xa0).contains(&code) {
        return false;
    }
    // Same high-level carve-outs as Python: Zs other than space, Zl, Zp and
    // unassigned blocks are escaped. This matches for the ASCII/Latin-1 input
    // the parser can produce; see the corpus for the checked subset.
    !matches!(
        code,
        0xad | 0x1680 | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000
    ) && !(0x2000..=0x200a).contains(&code)
        && !(0x202a..=0x202e).contains(&code)
        && !(0x2060..=0x206f).contains(&code)
        && !(0xfff9..=0xfffb).contains(&code)
}

/// A URL split the way `plane.utils.url_security._split_target` splits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Lowercased by `urlsplit`; one of `http` or `https`.
    pub scheme: String,
    /// `SplitResult.hostname`: lowercased, without brackets, zone id intact,
    /// IDNA *not* applied.
    pub hostname: String,
    /// Explicit port or the scheme default.
    pub port: u16,
    /// `parts.path or "/"`, with `?query` appended when present.
    pub path: String,
    /// `(username, password)` percent-decoded, when the URL carried userinfo.
    pub auth: Option<(String, String)>,
}

impl Target {
    pub fn is_https(&self) -> bool {
        self.scheme == "https"
    }

    fn default_port(&self) -> u16 {
        if self.is_https() { 443 } else { 80 }
    }

    /// The `Host` header value `_request_to_ip` computes: the original host
    /// (bracketed if it is an IPv6 literal), with `:port` only when it is not
    /// the scheme default.
    pub fn host_header(&self) -> String {
        let label = if self.hostname.contains(':') {
            format!("[{}]", self.hostname)
        } else {
            self.hostname.clone()
        };
        if self.port == self.default_port() {
            label
        } else {
            format!("{label}:{}", self.port)
        }
    }

    /// The URL the transport connects to.
    ///
    /// Python rewrites the authority to the *validated IP* and keeps the name
    /// in `Host`/SNI. We keep the name in the URL and pin the socket address
    /// through the resolver instead, which has the same observable effect
    /// (address = validated IP, SNI and `Host` = original name) while letting
    /// rustls verify the certificate against the name. The port is always
    /// explicit, as in Python's rewritten URL.
    pub fn request_url(&self) -> String {
        let host = if self.hostname.contains(':') {
            format!("[{}]", self.hostname)
        } else {
            self.hostname.clone()
        };
        format!("{}://{host}:{}{}", self.scheme, self.port, self.path)
    }
}

/// `plane.utils.url_security._split_target`.
pub fn split_target(url: &str) -> Result<Target, PyUrlError> {
    let parts = urlsplit(url)?;
    if parts.scheme != "http" && parts.scheme != "https" {
        return Err(PyUrlError::new(INVALID_SCHEME));
    }
    let hostname = parts
        .hostname()
        .ok_or_else(|| PyUrlError::new(NO_HOSTNAME))?;
    // Python: `parts.port or (443 if https else 80)`. `or` means port 0 is
    // falsy and therefore replaced by the default, unlike `??`.
    let default_port = if parts.scheme == "https" { 443 } else { 80 };
    let port = match parts.port()? {
        Some(0) | None => default_port,
        Some(port) => port,
    };
    let mut path = if parts.path.is_empty() {
        "/".to_string()
    } else {
        parts.path.clone()
    };
    if !parts.query.is_empty() {
        path.push('?');
        path.push_str(&parts.query);
    }
    let (username, password) = parts.userinfo();
    let auth = if username.is_some() || password.is_some() {
        Some((
            unquote(username.as_deref().unwrap_or("")),
            unquote(password.as_deref().unwrap_or("")),
        ))
    } else {
        None
    };
    Ok(Target {
        scheme: parts.scheme,
        hostname,
        port,
        path,
        auth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfkc_table_matches_python_observation() {
        // The set was produced by running `unicodedata.normalize("NFKC", ...)`
        // over the code space with CPython 3.12.13; spot-check the members and
        // a few near-misses.
        for c in NFKC_DELIMITER_CHARS {
            assert!(!c.is_ascii());
        }
        assert!(NFKC_DELIMITER_CHARS.contains(&'\u{FF0F}'));
        assert!(NFKC_DELIMITER_CHARS.contains(&'\u{FF20}'));
        assert!(!NFKC_DELIMITER_CHARS.contains(&'\u{FF10}'));
        assert!(!NFKC_DELIMITER_CHARS.contains(&'\u{00E9}'));
    }

    #[test]
    fn repr_matches_python() {
        assert_eq!(repr_str("abc"), "'abc'");
        assert_eq!(repr_str("a'b"), "\"a'b\"");
        assert_eq!(repr_str("a\"b'"), "'a\"b\\''");
        assert_eq!(repr_str("a\\b"), "'a\\\\b'");
        assert_eq!(repr_str("a\nb"), "'a\\nb'");
        assert_eq!(repr_str("\u{7f}"), "'\\x7f'");
        assert_eq!(repr_str("ü"), "'ü'");
    }

    #[test]
    fn unquote_matches_python() {
        assert_eq!(unquote("p%40ss"), "p@ss");
        assert_eq!(unquote("%E2%9C%93"), "✓");
        assert_eq!(unquote("100%"), "100%");
        assert_eq!(unquote("%zz"), "%zz");
        assert_eq!(unquote("%C3%28"), "\u{fffd}(");
    }

    #[test]
    fn urljoin_matches_python() {
        assert_eq!(urljoin("https://a/b/c", "d").unwrap(), "https://a/b/d");
        assert_eq!(urljoin("https://a/b/c", "/d").unwrap(), "https://a/d");
        assert_eq!(urljoin("https://a/b/c", "../d").unwrap(), "https://a/d");
        assert_eq!(urljoin("https://a/b/c", "//x/y").unwrap(), "https://x/y");
        assert_eq!(urljoin("https://a/b/c", "?q").unwrap(), "https://a/b/c?q");
        assert_eq!(urljoin("https://a/b/", "ftp://x/y").unwrap(), "ftp://x/y");
        // Values checked against `urllib.parse.urljoin` on CPython 3.12.13.
        assert_eq!(
            urljoin("https://a/b/c?x=1", "?q=2").unwrap(),
            "https://a/b/c?q=2"
        );
        assert_eq!(
            urljoin("https://a/b/c?x=1", "#f").unwrap(),
            "https://a/b/c?x=1#f"
        );
        assert_eq!(urljoin("https://a/b/c", "").unwrap(), "https://a/b/c");
        assert_eq!(
            urljoin("https://a/b/", "x?y=1").unwrap(),
            "https://a/b/x?y=1"
        );
        assert_eq!(urljoin("https://a/b/c", ".").unwrap(), "https://a/b/");
        assert_eq!(urljoin("https://a/b/c", "..").unwrap(), "https://a/");
        assert_eq!(
            urljoin("https://a/b/c", "a/./b/../c").unwrap(),
            "https://a/b/a/c"
        );
        assert_eq!(
            urljoin("https://a/b/c#frag", "#other").unwrap(),
            "https://a/b/c#other"
        );
        // A scheme change returns the input string untouched, case included.
        assert_eq!(
            urljoin("https://a/b/c", "HTTP://X/Y").unwrap(),
            "HTTP://X/Y"
        );
        assert_eq!(
            urljoin("https://a/b/c", "mailto:x@y").unwrap(),
            "mailto:x@y"
        );
        assert_eq!(urljoin("https://a/b/c", "//x").unwrap(), "https://x");
        assert_eq!(urljoin("https://a/b/c", "///x").unwrap(), "https://a/x");
        assert_eq!(
            urljoin("https://a/b/c", "/a//b///c").unwrap(),
            "https://a/a//b///c"
        );
    }
}
