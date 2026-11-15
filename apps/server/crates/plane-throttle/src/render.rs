// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two ways a throttled request is answered.
//!
//! **DRF views.** `Throttled` (`exceptions.py:226-244`) is rendered by DRF's
//! handler and then rewritten by Plane's
//! (`plane/authentication/adapter/exception.py:17-33`): the body becomes
//! `{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}`, the status is
//! forced to 429, and the `Retry-After` header the base handler computed is
//! kept. That applies to every DRF view in the project — `/api/`, `/api/v1/`,
//! `/api/public/`, `/api/instances/` and the `/auth/` APIViews — because
//! `EXCEPTION_HANDLER` is global (`plane/settings/common.py:149`).
//!
//! DRF's own detail string, which never reaches the client here but is part of
//! the exception's contract, is
//! `Request was throttled. Expected available in <n> second[s].` with
//! `<n> = math.ceil(wait)`, and `Retry-After: <n>` is only added when
//! `exc.wait` is truthy (`views.py:87-100`) — so a wait that rounds to zero, or
//! no wait at all, drops the header. `plane/app/views/user/base.py:162` calls
//! `self.throttled(request, None)` and is the one place with no wait estimate.
//!
//! **The redirect flow.** The plain `django.views.View` auth endpoints cannot
//! run DRF throttles, so `@throttle_auth_redirect` checks
//! `AuthenticationThrottle` by hand and answers with a 302 to the app's error
//! page carrying the same error dict in the query string
//! (`plane/authentication/rate_limit.py:62-95`).

use plane_core::error::{APPLICATION_JSON, ErrorResponse};
use plane_core::py;
use plane_core::{ApiError, Object};
use plane_serde::{Dialect, to_vec};

use crate::error::Error;

/// `AuthenticationException(RATE_LIMIT_EXCEEDED).get_error_dict()`.
pub const RATE_LIMIT_EXCEEDED_CODE: i64 = 5900;
/// The `error_message` Plane sends.
pub const RATE_LIMIT_EXCEEDED_MESSAGE: &str = "RATE_LIMIT_EXCEEDED";

/// A refusal: the `wait` DRF would carry, already through `math.ceil`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThrottleRejection {
    /// `Throttled.wait` after `math.ceil`: `None` when the caller raised
    /// `Throttled(None)` (only `plane/app/views/user/base.py:162` does).
    pub wait: Option<u64>,
}

impl ThrottleRejection {
    /// Apply `math.ceil` to a raw `SimpleRateThrottle.wait()` result.
    ///
    /// A throttle failure always leaves at least one entry in the history, so
    /// `wait()` is strictly positive there and the clamp never fires; it only
    /// keeps a hand-built rejection from turning into a negative header.
    pub fn from_wait(wait: Option<f64>) -> Self {
        Self {
            wait: wait.map(|seconds| seconds.ceil().max(0.0) as u64),
        }
    }

    /// DRF's `Throttled` detail, verbatim (`exceptions.py:228-242`).
    pub fn detail(&self) -> String {
        const DETAIL: &str = "Request was throttled.";
        match self.wait {
            None => DETAIL.to_string(),
            Some(wait) => {
                // `ngettext(..., wait)` picks the singular for 1.
                let unit = if wait == 1 { "second" } else { "seconds" };
                format!("{DETAIL} Expected available in {wait} {unit}.")
            }
        }
    }

    /// `if getattr(exc, 'wait', None): headers['Retry-After'] = '%d' % exc.wait`
    /// (`rest_framework/views.py:91-92`).
    pub fn retry_after(&self) -> Option<u64> {
        self.wait.filter(|wait| *wait != 0)
    }

    /// The exact 429 DRF plus Plane's handler produce.
    ///
    /// Built here rather than through [`ApiError::Throttled`] because that
    /// variant always emits `Retry-After`, while DRF omits it when the wait is
    /// absent or zero.
    pub fn render(&self) -> Result<ErrorResponse, Error> {
        let body = rate_limit_exceeded_object();
        let body = to_vec(Dialect::Drf, &body)?;
        let headers = match self.retry_after() {
            Some(wait) => vec![("Retry-After", wait.to_string())],
            None => Vec::new(),
        };
        Ok(ErrorResponse {
            status: 429,
            headers,
            content_type: APPLICATION_JSON,
            body,
        })
    }

    /// The same response as an [`ApiError`], for pipelines that speak
    /// `ApiError` only.
    ///
    /// `ApiError::Throttled` cannot express "no `Retry-After`", so this is
    /// exact for every throttle failure that came from `check_throttles`
    /// (`wait >= 1`); prefer [`ThrottleRejection::render`] for the
    /// `Throttled(None)` call site.
    pub fn api_error(&self) -> ApiError {
        ApiError::Throttled {
            retry_after_seconds: self.wait.unwrap_or(0),
        }
    }
}

/// `{"error_code": 5900, "error_message": "RATE_LIMIT_EXCEEDED"}`, in the key
/// order `AuthenticationException.get_error_dict` builds.
fn rate_limit_exceeded_object() -> Object {
    Object::new()
        .field("error_code", RATE_LIMIT_EXCEEDED_CODE)
        .field("error_message", RATE_LIMIT_EXCEEDED_MESSAGE)
}

/// The URL `@throttle_auth_redirect` redirects to when the per-IP budget is
/// exhausted (`rate_limit.py:80-92`).
///
/// This is `get_safe_redirect_url(base_url=base_host(...), next_path=
/// request.POST.get("next_path"), params=exc.get_error_dict())`
/// (`plane/utils/path_validator.py:157-201`) with the rate-limit params:
///
/// * `base_url` is `base_host(request, is_app=..., is_space=..., is_admin=...)`
///   — the caller resolves it from `WEB_URL`/`APP_BASE_URL`/`ADMIN_BASE_URL`/
///   `SPACE_BASE_URL`, which this crate does not read;
/// * `allowed_hosts` is `get_allowed_hosts()`: the netloc of each configured
///   base URL. The redirect falls back to the bare query string when the
///   constructed URL is not on one of them.
///
/// `next_path` is *not* percent-encoded by Plane (it is validated first), and
/// that is reproduced here.
///
/// `plane/utils/path_validator.py` is also needed by the sign-in flows; when
/// `plane-auth` ports it, this function should delegate to that copy rather
/// than keep a second one.
pub fn rate_limit_redirect_url(base_url: &str, next_path: &str, allowed_hosts: &[&str]) -> String {
    let base = base_url.trim_end_matches('/');
    let encoded_params = "error_code=5900&error_message=RATE_LIMIT_EXCEEDED";
    let validated = validate_next_path(next_path);

    let mut query = String::with_capacity(validated.len() + encoded_params.len() + 16);
    if !validated.is_empty() {
        query.push_str("next_path=");
        query.push_str(&validated);
        query.push('&');
    }
    query.push_str(encoded_params);

    let url = format!("{base}/?{query}");
    if url_has_allowed_host_and_scheme(&url, allowed_hosts) {
        url
    } else {
        format!("{base}?{encoded_params}")
    }
}

/// `plane/utils/path_validator.py:104-155`: reject absolute, authority-relative,
/// traversal and script-ish `next_path` values.
pub fn validate_next_path(next_path: &str) -> String {
    if next_path.is_empty() {
        return String::new();
    }
    if next_path.chars().count() > 500 {
        return String::new();
    }
    // Browsers treat backslashes as slashes; remove them all.
    let mut path: String = next_path.replace('\\', "");
    // Browsers strip tab/CR/LF from a URL before parsing it.
    path.retain(|character| !matches!(character, '\t' | '\r' | '\n'));

    if let Ok(split) = py::urlsplit(&path)
        && (!split.scheme.is_empty() || !split.netloc.is_empty())
    {
        path = split.path.clone();
    }

    if path.is_empty() || !path.starts_with('/') {
        return String::new();
    }
    if path.starts_with("//") {
        return String::new();
    }
    if path.contains("..") {
        return String::new();
    }
    if contains_suspicious_pattern(&path) {
        return String::new();
    }
    path
}

/// `plane/utils/path_validator.py:64-92`.
fn contains_suspicious_pattern(path: &str) -> bool {
    const PATTERNS: [&str; 16] = [
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
        "onclick=",
    ];
    let lowered = path.to_lowercase();
    PATTERNS.iter().any(|pattern| lowered.contains(pattern))
}

/// `django.utils.http.url_has_allowed_host_and_scheme`
/// (`django/utils/http.py:244-304`), with `require_https=False`.
pub fn url_has_allowed_host_and_scheme(url: &str, allowed_hosts: &[&str]) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Chrome treats `\` as `/` in paths but it can also be part of credentials,
    // so Django checks both spellings.
    check_one(trimmed, allowed_hosts) && check_one(&trimmed.replace('\\', "/"), allowed_hosts)
}

/// `django.utils.http.MAX_URL_LENGTH`.
const MAX_URL_LENGTH: usize = 2048;

fn check_one(url: &str, allowed_hosts: &[&str]) -> bool {
    if url.starts_with("///") || url.chars().count() > MAX_URL_LENGTH {
        return false;
    }
    let Ok(split) = py::urlsplit(url) else {
        return false;
    };
    // `http:///example.com` — a scheme with no host.
    if split.netloc.is_empty() && !split.scheme.is_empty() {
        return false;
    }
    // Chrome ignores some control characters at the start of a URL and could
    // read it as scheme-relative.
    if url.chars().next().is_some_and(is_control_category) {
        return false;
    }
    let scheme = if split.scheme.is_empty() && !split.netloc.is_empty() {
        // URLs without a scheme (`//example.com/p`) are treated as http.
        "http".to_string()
    } else {
        split.scheme.clone()
    };
    (split.netloc.is_empty() || allowed_hosts.contains(&split.netloc.as_str()))
        && (scheme.is_empty() || matches!(scheme.as_str(), "http" | "https"))
}

/// Python's `unicodedata.category(character)[0] == "C"`, for the characters
/// that can realistically start a URL.
fn is_control_category(character: char) -> bool {
    character.is_control() || matches!(character, '\u{7f}'..='\u{9f}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_core::ApiError;

    #[test]
    fn detail_matches_drf_ngettext() {
        assert_eq!(
            ThrottleRejection::from_wait(Some(59.2)).detail(),
            "Request was throttled. Expected available in 60 seconds."
        );
        assert_eq!(
            ThrottleRejection::from_wait(Some(0.5)).detail(),
            "Request was throttled. Expected available in 1 second."
        );
        assert_eq!(
            ThrottleRejection { wait: None }.detail(),
            "Request was throttled."
        );
    }

    #[test]
    fn retry_after_is_omitted_when_falsy() {
        assert_eq!(
            ThrottleRejection::from_wait(Some(1.2)).retry_after(),
            Some(2)
        );
        assert_eq!(ThrottleRejection::from_wait(Some(0.0)).retry_after(), None);
        assert_eq!(ThrottleRejection { wait: None }.retry_after(), None);
        assert_eq!(ThrottleRejection::from_wait(None).retry_after(), None);
    }

    #[test]
    fn renders_planes_429_body_and_headers() {
        let response = ThrottleRejection::from_wait(Some(60.0)).render().unwrap();
        assert_eq!(response.status, 429);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(
            std::str::from_utf8(&response.body).unwrap(),
            r#"{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}"#
        );
        assert_eq!(response.headers, vec![("Retry-After", "60".to_string())]);

        // The `Throttled(None)` call site: same body, no header.
        let response = ThrottleRejection { wait: None }.render().unwrap();
        assert_eq!(
            std::str::from_utf8(&response.body).unwrap(),
            r#"{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}"#
        );
        assert!(response.headers.is_empty());

        // ... and the ApiError shortcut agrees with ApiError::Throttled.
        assert_eq!(
            ThrottleRejection::from_wait(Some(60.0)).api_error(),
            ApiError::Throttled {
                retry_after_seconds: 60
            }
        );
    }

    #[test]
    fn redirect_url_matches_get_safe_redirect_url() {
        let allowed = ["app.plane.so"];
        assert_eq!(
            rate_limit_redirect_url("https://app.plane.so", "", &allowed),
            "https://app.plane.so/?error_code=5900&error_message=RATE_LIMIT_EXCEEDED"
        );
        // `base_url.rstrip("/")` then `"/?..."`.
        assert_eq!(
            rate_limit_redirect_url("https://app.plane.so/", "", &allowed),
            "https://app.plane.so/?error_code=5900&error_message=RATE_LIMIT_EXCEEDED"
        );
        // `next_path` is appended raw, before the params.
        assert_eq!(
            rate_limit_redirect_url("https://app.plane.so", "/workspace/1/", &allowed),
            "https://app.plane.so/?next_path=/workspace/1/&error_code=5900&error_message=RATE_LIMIT_EXCEEDED"
        );
        // An unsafe next_path is dropped by `validate_next_path`. (Note that
        // `//evil.com/` is *not* unsafe by this function's rules: `urlparse`
        // moves the host into `netloc` and the remaining path is `/`, exactly
        // as `get_safe_redirect_url` behaves.)
        assert_eq!(
            rate_limit_redirect_url("https://app.plane.so", "/a/../b", &allowed),
            "https://app.plane.so/?error_code=5900&error_message=RATE_LIMIT_EXCEEDED"
        );
        assert_eq!(
            rate_limit_redirect_url("https://app.plane.so", "//evil.com/", &allowed),
            "https://app.plane.so/?next_path=/&error_code=5900&error_message=RATE_LIMIT_EXCEEDED"
        );
        // A base URL that is not on the allowed list falls back to the bare
        // query string without the trailing slash.
        assert_eq!(
            rate_limit_redirect_url("https://other.example", "", &allowed),
            "https://other.example?error_code=5900&error_message=RATE_LIMIT_EXCEEDED"
        );
        // Relative base URL (WEB_URL unset): no netloc, always allowed.
        assert_eq!(
            rate_limit_redirect_url("", "", &[]),
            "/?error_code=5900&error_message=RATE_LIMIT_EXCEEDED"
        );
    }

    #[test]
    fn next_path_validation_matches_python() {
        assert_eq!(validate_next_path("/a/b"), "/a/b");
        assert_eq!(validate_next_path("/a/../b"), "");
        assert_eq!(validate_next_path("//evil.com"), "");
        assert_eq!(validate_next_path("https://evil.com/a"), "/a");
        assert_eq!(validate_next_path("  /a "), "");
        assert_eq!(validate_next_path("/a\t\r\nb"), "/ab");
        assert_eq!(validate_next_path("javascript:alert(1)"), "");
        assert_eq!(validate_next_path(&"a".repeat(501)), "");
        assert_eq!(validate_next_path(""), "");
    }

    #[test]
    fn allowed_host_and_scheme_check() {
        assert!(url_has_allowed_host_and_scheme("/x", &[]));
        assert!(url_has_allowed_host_and_scheme(
            "https://app.plane.so/x",
            &["app.plane.so"]
        ));
        assert!(!url_has_allowed_host_and_scheme(
            "https://evil.example/x",
            &["app.plane.so"]
        ));
        assert!(!url_has_allowed_host_and_scheme(
            "ftp://app.plane.so/x",
            &["app.plane.so"]
        ));
        assert!(!url_has_allowed_host_and_scheme(
            "///app.plane.so/x",
            &["app.plane.so"]
        ));
        assert!(!url_has_allowed_host_and_scheme("", &[]));
        // `//host/path` is treated as http.
        assert!(url_has_allowed_host_and_scheme(
            "//app.plane.so/x",
            &["app.plane.so"]
        ));
    }
}
