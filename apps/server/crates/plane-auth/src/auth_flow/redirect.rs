// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/path_validator.py` and Django's
//! `url_has_allowed_host_and_scheme`, for every `/auth/` redirect.
//!
//! Three quirks are load-bearing and pinned by
//! `tests/golden/redirect.json`, recorded from the real functions:
//!
//! * the validated path is interpolated **unencoded**
//!   (`path_validator.py:182`): `next_path=/a&error_code=1` forges a second
//!   query parameter on an allowed host;
//! * an empty allow-list rejects every URL with a netloc
//!   (`django/utils/http.py:303`), so a deployment with no base URLs redirects
//!   to the bare base and drops the error parameters;
//! * `get_redirection_path` returns bare slugs (`redirection_path.py:14,30,
//!   46`), and `validate_next_path` drops them for lack of a leading `/`
//!   (`path_validator.py:132-133`), so "redirect to the last workspace" never
//!   works: users land on the app root.
//!
//! The URL parser is `plane-net`'s port of CPython 3.12's `urlsplit` /
//! `urlparse` / `urljoin`, including the bracketed-host and NFKC netloc checks
//! Django's `url_has_allowed_host_and_scheme` relies on (it catches the
//! `ValueError` and answers `False`). `validate_next_path` does not catch it:
//! a malformed `next_path` is a 500 there, so [`validate_next_path`] returns
//! the error instead of swallowing it.

use std::fmt;

use plane_core::config::Settings;
use plane_net::pyurl::{self, PyUrlError};

/// `django.utils.http.MAX_URL_LENGTH`.
pub const MAX_URL_LENGTH: usize = 2048;
/// `validate_next_path`'s own input cap (`path_validator.py:111-112`).
pub const MAX_NEXT_PATH_LENGTH: usize = 500;

/// A Python `ValueError`, message included.
///
/// `urlparse` raises for an unbalanced or non-IP bracketed host
/// (`urllib/parse.py:510-516`) and for an NFKC-expanded delimiter in a
/// non-ASCII netloc (`:421-436`); `validate_next_path` and `get_allowed_hosts`
/// do not catch it, so the view is a 500 in Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyValueError {
    message: String,
}

impl PyValueError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PyValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PyValueError {}

impl From<PyUrlError> for PyValueError {
    fn from(error: PyUrlError) -> Self {
        Self::new(error.message())
    }
}

/// `_contains_suspicious_patterns` (`path_validator.py:54-88`), in its order.
///
/// `path.lower()` is Python's Unicode lower-case; the patterns are all ASCII,
/// so a Unicode-aware `to_lowercase` is what makes `/JAVASCRIPT:` match like
/// Python's does.
#[must_use]
pub fn contains_suspicious_patterns(path: &str) -> bool {
    const PATTERNS: [&str; 15] = [
        "javascript:",
        "data:",
        "vbscript:",
        "file:",
        "ftp:",
        "%2e%2e",
        "%2f%2f",
        "%5c%5c",
        "<script",
        "<iframe",
        "<object",
        "<embed",
        "<form",
        "onload=",
        "onerror=",
    ];
    let lower = path.to_lowercase();
    PATTERNS.iter().any(|pattern| lower.contains(pattern)) || lower.contains("onclick=")
}

/// `validate_next_path(next_path)` (`path_validator.py:105-154`).
///
/// `None` is Python's `None`, `""` its empty string; both return `""`. The
/// returned string is *unencoded* (see the module note).
///
/// # Errors
///
/// [`PyValueError`] when `urllib.parse.urlparse` raises, which Python does not
/// catch here.
pub fn validate_next_path(next_path: Option<&str>) -> Result<String, PyValueError> {
    let Some(next_path) = next_path.filter(|value| !value.is_empty()) else {
        return Ok(String::new());
    };
    // `len()` counts code points.
    if next_path.chars().count() > MAX_NEXT_PATH_LENGTH {
        return Ok(String::new());
    }
    // "Browsers interpret backslashes as forward slashes. Remove all
    // backslashes." (`:114`)
    let mut path = next_path.replace('\\', "");
    // `str.maketrans("", "", "\t\r\n")` (`:122`).
    path.retain(|character| !matches!(character, '\t' | '\r' | '\n'));

    let parsed = pyurl::urlparse(&path)?;
    if !parsed.scheme.is_empty() || !parsed.netloc.is_empty() {
        // "Block absolute URLs or anything with scheme/netloc" (`:127-128`).
        path = parsed.path;
    }

    if path.is_empty() || !path.starts_with('/') {
        return Ok(String::new());
    }
    // "Reject authority-relative paths (//, ///, ...)" (`:137-140`).
    if path.starts_with("//") {
        return Ok(String::new());
    }
    if path.contains("..") {
        return Ok(String::new());
    }
    if contains_suspicious_patterns(&path) {
        return Ok(String::new());
    }
    Ok(path)
}

/// `get_allowed_hosts()` (`path_validator.py:91-102`).
///
/// `(WEB_URL, APP_BASE_URL, ADMIN_BASE_URL, SPACE_BASE_URL)` in that order,
/// `urlparse(setting).netloc` for each truthy value, duplicates dropped. A
/// malformed bracketed host raises, as it does in Python.
///
/// # Errors
///
/// [`PyValueError`] from `urlparse`.
pub fn allowed_hosts(
    web_url: Option<&str>,
    app_base_url: Option<&str>,
    admin_base_url: Option<&str>,
    space_base_url: Option<&str>,
) -> Result<Vec<String>, PyValueError> {
    let mut hosts: Vec<String> = Vec::new();
    for setting in [web_url, app_base_url, admin_base_url, space_base_url] {
        let Some(setting) = setting.filter(|value| !value.is_empty()) else {
            continue;
        };
        let host = pyurl::urlparse(setting)?.netloc;
        if !host.is_empty() && !hosts.iter().any(|existing| existing == &host) {
            hosts.push(host);
        }
    }
    Ok(hosts)
}

/// [`allowed_hosts`] for the process settings.
///
/// `plane-core` validates `APP_BASE_URL`, `ADMIN_BASE_URL` and
/// `SPACE_BASE_URL` with `is_valid_url` at startup, so a malformed value is
/// `None` here where Django (which validates only the last two) would pass it
/// to `urlparse`. `WEB_URL` is raw, exactly as Django reads it.
///
/// # Errors
///
/// [`PyValueError`] from `urlparse("WEB_URL")`.
pub fn settings_allowed_hosts(settings: &Settings) -> Result<Vec<String>, PyValueError> {
    allowed_hosts(
        settings.urls.web_url.as_deref(),
        settings.urls.app_base_url.as_deref(),
        settings.urls.admin_base_url.as_deref(),
        settings.urls.space_base_url.as_deref(),
    )
}

/// Django's `url_has_allowed_host_and_scheme` (`django/utils/http.py:244-304`).
///
/// `url.strip()` (Python whitespace, which includes U+001C..U+001F), the
/// `///` and 2048-character guards, a scheme without a netloc, a first
/// character whose Unicode category is `C`, then the host/scheme comparison —
/// twice, once with `\` rewritten to `/`, because Chrome treats a backslash as
/// a separator (`:259-264`).
#[must_use]
pub fn url_has_allowed_host_and_scheme(url: &str, allowed_hosts: &[String]) -> bool {
    url_has_allowed_host_and_scheme_https(url, allowed_hosts, false)
}

/// [`url_has_allowed_host_and_scheme`] with Django's `require_https`.
#[must_use]
pub fn url_has_allowed_host_and_scheme_https(
    url: &str,
    allowed_hosts: &[String],
    require_https: bool,
) -> bool {
    let url = python_strip(url);
    if url.is_empty() {
        return false;
    }
    check_host_and_scheme(url, allowed_hosts, require_https)
        && check_host_and_scheme(&url.replace('\\', "/"), allowed_hosts, require_https)
}

fn check_host_and_scheme(url: &str, allowed_hosts: &[String], require_https: bool) -> bool {
    // `url.startswith("///") or len(url) > MAX_URL_LENGTH` (`:273-279`).
    if url.starts_with("///") || url.chars().count() > MAX_URL_LENGTH {
        return false;
    }
    let Ok(split) = pyurl::urlsplit(url) else {
        // `except ValueError: return False` (invalid IPv6, bad bracketed host,
        // NFKC netloc).
        return false;
    };
    // "Forbid URLs like http:///example.com" (`:287-290`).
    if split.netloc.is_empty() && !split.scheme.is_empty() {
        return false;
    }
    // "Forbid URLs that start with control characters" (`:291-295`).
    if url.chars().next().is_some_and(is_unicode_category_c) {
        return false;
    }
    let scheme = if split.scheme.is_empty() && !split.netloc.is_empty() {
        // "Consider URLs without a scheme (e.g. //example.com/p) to be http."
        "http"
    } else {
        split.scheme.as_str()
    };
    // `not scheme or scheme in valid_schemes` (`http.py:303-305`): a URL with
    // neither a scheme nor a netloc passes even under `require_https`.
    let scheme_allowed = if require_https {
        scheme.is_empty() || scheme == "https"
    } else {
        scheme.is_empty() || scheme == "http" || scheme == "https"
    };
    let host_allowed =
        split.netloc.is_empty() || allowed_hosts.iter().any(|host| host == &split.netloc);
    host_allowed && scheme_allowed
}

/// Whether CPython's `unicodedata.category(character)` starts with `C`.
///
/// `Cc` (control), `Cf` (format, `regex`'s `\p{Cf}`), `Co` (private use) and
/// `Cs` (surrogate, impossible in a Rust `char`). `Cn` (unassigned) depends on
/// the Unicode database version; `regex` ships the same categories as
/// CPython 3.12's Unicode 15.0 tables, so this agrees for assigned characters
/// and for the control/format/private-use blocks a URL can start with.
fn is_unicode_category_c(character: char) -> bool {
    use std::sync::OnceLock;
    static OTHER: OnceLock<regex::Regex> = OnceLock::new();
    let regex = OTHER.get_or_init(|| {
        regex::Regex::new(r"^\p{C}$").expect("the general-category class is valid")
    });
    regex.is_match(character.encode_utf8(&mut [0; 4]))
}

/// Python's `str.strip()`: `str.isspace()` includes U+001C..U+001F, which
/// `char::is_whitespace` (Unicode `White_Space`) does not.
fn python_strip(value: &str) -> &str {
    value.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '\u{1c}'..='\u{1f}')
    })
}

/// `urllib.parse.urlencode(params)` with a dict of strings/ints and
/// `quote_via=quote_plus` (`urllib/parse.py:1008-1041`).
///
/// `str(None)` is `"None"`; the caller passes already-stringified values
/// ([`crate::auth_flow::error::AuthError::redirect_params`]).
#[must_use]
pub fn urlencode(params: &[(String, String)]) -> String {
    let mut encoded = String::new();
    for (index, (key, value)) in params.iter().enumerate() {
        if index > 0 {
            encoded.push('&');
        }
        quote_plus_into(key, &mut encoded);
        encoded.push('=');
        quote_plus_into(value, &mut encoded);
    }
    encoded
}

/// `urllib.parse.quote_plus(value)` with `safe=""` and UTF-8.
fn quote_plus_into(value: &str, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(*byte));
            }
            b' ' => out.push('+'),
            other => {
                out.push('%');
                out.push(char::from(HEX[usize::from(other >> 4)]));
                out.push(char::from(HEX[usize::from(other & 0x0f)]));
            }
        }
    }
}

/// `get_safe_redirect_url(base_url, next_path, params)`
/// (`path_validator.py:157-201`).
///
/// The `next_path=` value is **not** urlencoded (`:182`). When the assembled
/// URL fails the host check the fallback is `base_url + "?" + encoded_params`,
/// i.e. the next path is dropped but the error parameters survive
/// (`:199-201`).
///
/// # Errors
///
/// [`PyValueError`] from `validate_next_path` or `get_allowed_hosts`.
pub fn get_safe_redirect_url(
    base_url: &str,
    next_path: Option<&str>,
    params: &[(String, String)],
    allowed_hosts: &[String],
) -> Result<String, PyValueError> {
    let validated_path = validate_next_path(next_path)?;
    let base_url = base_url.trim_end_matches('/');
    let mut query_parts: Vec<String> = Vec::new();
    let mut encoded_params = String::new();
    if !validated_path.is_empty() {
        query_parts.push(format!("next_path={validated_path}"));
    }
    if !params.is_empty() {
        encoded_params = urlencode(params);
        query_parts.push(encoded_params.clone());
    }
    let url = if query_parts.is_empty() {
        base_url.to_string()
    } else {
        format!("{base_url}/?{}", query_parts.join("&"))
    };
    if url_has_allowed_host_and_scheme(&url, allowed_hosts) {
        return Ok(url);
    }
    Ok(if encoded_params.is_empty() {
        base_url.to_string()
    } else {
        format!("{base_url}?{encoded_params}")
    })
}

/// The space views' redirect (`views/space/email.py:98-103` and siblings):
/// `base.rstrip("/") + validate_next_path(next_path)`, checked with
/// `url_has_allowed_host_and_scheme`; on failure the bare base host.
///
/// Returns the final URL. The space views **never** call
/// `get_safe_redirect_url` for a successful login (only for errors), so the
/// next path is not passed through `urlencode` here either.
///
/// # Errors
///
/// [`PyValueError`] from `validate_next_path`.
pub fn space_redirect_url(
    base_url: &str,
    next_path: Option<&str>,
    allowed_hosts: &[String],
) -> Result<String, PyValueError> {
    let validated_path = validate_next_path(next_path)?;
    let base_url = base_url.trim_end_matches('/');
    let url = format!("{base_url}{validated_path}");
    if url_has_allowed_host_and_scheme(&url, allowed_hosts) {
        return Ok(url);
    }
    Ok(base_url.to_string())
}

/// `urllib.parse.urljoin`, for Gitea's `urljoin(host, "?" + urlencode(...))`
/// and `urljoin(host, path)` (`views/app/gitea.py:45,76,101`).
///
/// # Errors
///
/// [`PyValueError`] when Python's `urljoin` would raise (a bad port, an
/// invalid bracketed host, an NFKC netloc).
pub fn urljoin(base: &str, relative: &str) -> Result<String, PyValueError> {
    Ok(pyurl::urljoin(base, relative)?)
}

/// `get_redirection_path(user)`'s answer (`redirection_path.py:8-47`), for a
/// caller that has run its three queries.
///
/// The function returns a **bare slug**, and the login views pass it straight
/// to [`validate_next_path`], which drops anything without a leading `/`. So
/// every variant ends at the app root once validated; the enum exists so the
/// caller's `path` variable and the (dead) validation match Python, and so a
/// future decision to fix the redirect is one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectionPath<'a> {
    /// `"onboarding"` (`redirection_path.py:14`).
    Onboarding,
    /// `f"{workspace.slug}"` (`:30,39`).
    Workspace(&'a str),
    /// `"invitations"` (`:43`).
    Invitations,
    /// `"create-workspace"` (`:46`).
    CreateWorkspace,
}

impl<'a> RedirectionPath<'a> {
    /// The bare value the Python function returns, before validation.
    #[must_use]
    pub fn slug(self) -> &'a str {
        match self {
            Self::Onboarding => "onboarding",
            Self::Workspace(slug) => slug,
            Self::Invitations => "invitations",
            Self::CreateWorkspace => "create-workspace",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_list(hosts: &[&str]) -> Vec<String> {
        hosts.iter().map(|host| (*host).to_string()).collect()
    }

    #[test]
    fn validate_next_path_keeps_only_local_paths() {
        let valid = |value: &str| validate_next_path(Some(value)).expect("no parse error");
        assert_eq!(valid("/my-workspace/"), "/my-workspace/");
        assert_eq!(valid("/a?b=c"), "/a?b=c");
        // Backslashes are removed, not turned into slashes (`:114`).
        assert_eq!(valid("/\\/evil.com"), "");
        assert_eq!(valid("/\\evil.com"), "/evil.com");
        // Absolute URLs keep only the path, params included (`urlparse`).
        assert_eq!(valid("http://localhost:3000/ok"), "/ok");
        assert_eq!(valid("http://localhost:3000/a;b?c=d"), "/a");
        // Bare slugs are dropped: `get_redirection_path`'s answer never lands.
        assert_eq!(valid("onboarding"), "");
        assert_eq!(valid("my-workspace"), "");
        // Suspicious patterns, case-insensitively.
        assert_eq!(valid("/JavaScript:alert(1)"), "");
        assert_eq!(valid("/x onload=alert(1)"), "");
        assert_eq!(valid("/%2E%2E/etc"), "");
        assert_eq!(valid("/a/../b"), "");
        // Length caps: 500 passes, 501 is dropped.
        assert_eq!(valid(&format!("/{}", "a".repeat(499))).len(), 500);
        assert_eq!(valid(&format!("/{}", "a".repeat(500))), "");
        assert_eq!(validate_next_path(None).expect("none"), "");
        assert_eq!(validate_next_path(Some("")).expect("empty"), "");
    }

    #[test]
    fn a_malformed_bracketed_host_is_a_python_value_error() {
        // `urlparse("http://[bad")` is `Invalid IPv6 URL`; a valid IPv4 in
        // brackets is `An IPv4 address cannot be in brackets`; neither is
        // caught by `validate_next_path`.
        assert!(validate_next_path(Some("http://[bad")).is_err());
        assert!(validate_next_path(Some("http://[127.0.0.1]/x")).is_err());
        assert!(validate_next_path(Some("http://[::1]/x")).is_ok());
    }

    #[test]
    fn the_unencoded_next_path_forges_parameters() {
        let hosts = host_list(&["localhost:3000"]);
        let url = get_safe_redirect_url(
            "http://localhost:3000",
            Some("/my-workspace&error_code=9999"),
            &[
                ("error_code".to_string(), "5005".to_string()),
                ("error_message".to_string(), "INVALID_EMAIL".to_string()),
            ],
            &hosts,
        )
        .expect("redirect");
        assert_eq!(
            url,
            "http://localhost:3000/?next_path=/my-workspace&error_code=9999&error_code=5005&error_message=INVALID_EMAIL"
        );
    }

    #[test]
    fn an_empty_allow_list_drops_the_parameters() {
        // The base has no netloc check to fail only when the assembled URL has
        // one; with no allowed hosts every netloc is rejected, so the fallback
        // keeps `?params` and drops `next_path`.
        let url = get_safe_redirect_url(
            "http://localhost:3000/",
            Some("/my-workspace"),
            &[("error_code".to_string(), "5000".to_string())],
            &[],
        )
        .expect("redirect");
        assert_eq!(url, "http://localhost:3000?error_code=5000");
    }

    #[test]
    fn no_parameters_and_no_next_path_returns_the_stripped_base() {
        let url = get_safe_redirect_url("http://localhost:3000///", Some("garbage"), &[], &[])
            .expect("redirect");
        assert_eq!(url, "http://localhost:3000");
    }

    #[test]
    fn urlencode_is_python_quote_plus() {
        assert_eq!(
            urlencode(&[
                ("email".to_string(), "a b+c@example.com".to_string()),
                ("emoji".to_string(), "😀".to_string()),
                ("url".to_string(), "http://x/y?z=1&w=2".to_string()),
            ]),
            "email=a+b%2Bc%40example.com&emoji=%F0%9F%98%80&url=http%3A%2F%2Fx%2Fy%3Fz%3D1%26w%3D2"
        );
        assert_eq!(urlencode(&[]), "");
    }

    #[test]
    fn host_checks_match_djangos_two_passes() {
        let hosts = host_list(&["localhost:3000"]);
        assert!(url_has_allowed_host_and_scheme("/path", &hosts));
        assert!(url_has_allowed_host_and_scheme(
            "http://localhost:3000/x",
            &hosts
        ));
        assert!(!url_has_allowed_host_and_scheme(
            "http://evil.com/x",
            &hosts
        ));
        assert!(!url_has_allowed_host_and_scheme(
            "///localhost:3000/x",
            &hosts
        ));
        assert!(!url_has_allowed_host_and_scheme(
            "http:///localhost:3000/x",
            &hosts
        ));
        assert!(!url_has_allowed_host_and_scheme(
            "javascript:alert(1)",
            &hosts
        ));
        assert!(url_has_allowed_host_and_scheme("/x", &[]));
        assert!(!url_has_allowed_host_and_scheme("//localhost:3000/x", &[]));
        // The backslash pass: Chrome reads `\` as `/`.
        assert!(!url_has_allowed_host_and_scheme("\\\\evil.com/x", &hosts));
        // A leading format character is category C.
        assert!(!url_has_allowed_host_and_scheme(
            "\u{200b}http://localhost:3000/x",
            &hosts
        ));
        assert!(!url_has_allowed_host_and_scheme(
            "\u{e000}http://localhost:3000/x",
            &hosts
        ));
        assert!(url_has_allowed_host_and_scheme(
            " http://localhost:3000/x ",
            &hosts
        ));
    }

    #[test]
    fn space_redirect_keeps_or_drops_the_path() {
        let hosts = host_list(&["localhost:3002"]);
        assert_eq!(
            space_redirect_url("http://localhost:3002/", Some("/my-workspace/"), &hosts)
                .expect("space"),
            "http://localhost:3002/my-workspace/"
        );
        // The path is dropped for a bare slug, so the base survives.
        assert_eq!(
            space_redirect_url("http://localhost:3002", Some("onboarding"), &hosts).expect("space"),
            "http://localhost:3002"
        );
        // An empty allow-list rejects the assembled URL with a netloc.
        assert_eq!(
            space_redirect_url("http://localhost:3002", Some("/x"), &[]).expect("space"),
            "http://localhost:3002"
        );
    }

    #[test]
    fn allowed_hosts_dedupes_in_setting_order() {
        let hosts = allowed_hosts(
            Some("https://plane.example"),
            Some("https://plane.example"),
            Some("https://plane.example/"),
            Some("https://other.example/x?y=1"),
        )
        .expect("hosts");
        assert_eq!(hosts, vec!["plane.example", "other.example"]);
        assert_eq!(
            allowed_hosts(None, Some(""), None, None).expect("hosts"),
            Vec::<String>::new()
        );
        // A value that is not a URL contributes nothing.
        assert_eq!(
            allowed_hosts(Some("not a url"), Some("/relative"), None, None).expect("hosts"),
            Vec::<String>::new()
        );
        // A malformed bracketed host raises, as Django's `urlparse` does.
        assert!(allowed_hosts(Some("http://[bad"), None, None, None).is_err());
    }

    #[test]
    fn urljoin_matches_the_gitea_shapes() {
        assert_eq!(
            urljoin("http://localhost:3000", "?error_code=5005").expect("join"),
            "http://localhost:3000?error_code=5005"
        );
        assert_eq!(
            urljoin("http://localhost:3000/app/", "my-workspace").expect("join"),
            "http://localhost:3000/app/my-workspace"
        );
        assert_eq!(
            urljoin("http://localhost:3000", "onboarding").expect("join"),
            "http://localhost:3000/onboarding"
        );
    }
}
