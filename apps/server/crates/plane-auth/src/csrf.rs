// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's `CsrfViewMiddleware`, `get_token` and `rotate_token`
//! (`django/middleware/csrf.py` 5.2.17), plus the cookie state the pipeline
//! threads through a request.
//!
//! The token scheme (HTTP_CONTRACT §7.2, Appendix B.2):
//!
//! * the **secret** is 32 characters from `[A-Za-z0-9]`;
//! * a **masked token** is `mask + cipher`, 64 characters, where
//!   `cipher[i] = CHARS[(CHARS.index(secret[i]) + CHARS.index(mask[i])) % 62]`;
//! * the cookie stores the secret; the value in `X-CSRFToken`,
//!   `csrfmiddlewaretoken` or `/auth/get-csrf-token/` may be either the secret or
//!   a masked token.
//!
//! Two mechanisms are easy to confuse and are kept apart: this module is
//! Django's *token* check, which only applies to non-`csrf_exempt` views (Plane's
//! plain `django.views.View` classes under `/auth/`). The DRF origin check is
//! [`crate::origin`].
//!
//! `CSRF_USE_SESSIONS` is `False` in Plane (`api:plane/settings/common.py`), so
//! the secret lives in the cookie, never in the session.

use http::Method;
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_core::py;

use crate::cookies;
use crate::random::{CSRF_ALLOWED_CHARS, constant_time_compare, get_random_string};

/// `CSRF_SECRET_LENGTH`.
pub const CSRF_SECRET_LENGTH: usize = 32;
/// `CSRF_TOKEN_LENGTH`.
pub const CSRF_TOKEN_LENGTH: usize = 2 * CSRF_SECRET_LENGTH;
/// `CSRF_COOKIE_NEEDS_UPDATE`, the flag `get_token`/`rotate_token` set.
pub const REASON_BAD_ORIGIN: &str =
    "Origin checking failed - %s does not match any trusted origins.";
pub const REASON_NO_REFERER: &str = "Referer checking failed - no Referer.";
pub const REASON_BAD_REFERER: &str =
    "Referer checking failed - %s does not match any trusted origins.";
pub const REASON_NO_CSRF_COOKIE: &str = "CSRF cookie not set.";
pub const REASON_CSRF_TOKEN_MISSING: &str = "CSRF token missing.";
pub const REASON_MALFORMED_REFERER: &str = "Referer checking failed - Referer is malformed.";
pub const REASON_INSECURE_REFERER: &str =
    "Referer checking failed - Referer is insecure while host is secure.";
pub const REASON_INCORRECT_LENGTH: &str = "has incorrect length";
pub const REASON_INVALID_CHARACTERS: &str = "has invalid characters";

/// `_get_token_source`'s header-name rendering: `HTTP_X_CSRFTOKEN` →
/// `X-Csrftoken` (`django/http/request.py`, `HttpHeaders.parse_header_name`).
const HEADER_SOURCE: &str = "the 'X-Csrftoken' HTTP header";

/// `_get_new_csrf_string()`.
pub fn new_secret() -> String {
    get_random_string(CSRF_SECRET_LENGTH, CSRF_ALLOWED_CHARS)
}

/// `_check_token_format`: length 32 or 64, all characters in the alphabet.
pub fn check_token_format(token: &str) -> Result<(), &'static str> {
    let length = token.chars().count();
    if length != CSRF_TOKEN_LENGTH && length != CSRF_SECRET_LENGTH {
        return Err(REASON_INCORRECT_LENGTH);
    }
    if !token
        .bytes()
        .all(|byte| CSRF_ALLOWED_CHARS.as_bytes().contains(&byte))
    {
        return Err(REASON_INVALID_CHARACTERS);
    }
    Ok(())
}

fn char_index(byte: u8) -> u8 {
    CSRF_ALLOWED_CHARS
        .bytes()
        .position(|candidate| candidate == byte)
        .unwrap_or(0) as u8
}

/// `_mask_cipher_secret(secret)`.
pub fn mask_secret(secret: &str) -> String {
    let mask = new_secret();
    let secret = secret.as_bytes();
    let mask_bytes = mask.as_bytes();
    let mut masked = String::with_capacity(CSRF_TOKEN_LENGTH);
    masked.push_str(&mask);
    for index in 0..CSRF_SECRET_LENGTH.min(secret.len()) {
        let value =
            (u16::from(char_index(secret[index])) + u16::from(char_index(mask_bytes[index]))) % 62;
        masked.push(CSRF_ALLOWED_CHARS.as_bytes()[usize::from(value)] as char);
    }
    masked
}

/// `_unmask_cipher_token(token)`; a 32-character token is already a secret.
pub fn unmask_token(token: &str) -> String {
    if token.len() != CSRF_TOKEN_LENGTH {
        return token.to_string();
    }
    let bytes = token.as_bytes();
    let mut unmasked = String::with_capacity(CSRF_SECRET_LENGTH);
    for index in 0..CSRF_SECRET_LENGTH {
        let cipher = i32::from(char_index(bytes[CSRF_SECRET_LENGTH + index]));
        let mask = i32::from(char_index(bytes[index]));
        let value = (cipher - mask).rem_euclid(62) as usize;
        unmasked.push(CSRF_ALLOWED_CHARS.as_bytes()[value] as char);
    }
    unmasked
}

/// `_does_token_match(request_token, secret)`.
pub fn token_matches(request_token: &str, secret: &str) -> bool {
    let candidate = unmask_token(request_token);
    constant_time_compare(candidate.as_bytes(), secret.as_bytes())
}

/// `request.META["CSRF_COOKIE"]` and `CSRF_COOKIE_NEEDS_UPDATE`.
///
/// `process_request` fills this at layer 5; `get_token` (the `/auth/get-csrf-token/`
/// view) and `rotate_token` (inside `login()`) update it; `process_response`
/// writes the cookie and clears the flag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CsrfState {
    /// The **unmasked** secret.
    pub cookie: Option<String>,
    pub needs_update: bool,
}

impl CsrfState {
    /// `CsrfViewMiddleware.process_request` (`csrf.py:400-414`):
    ///
    /// * a malformed cookie is replaced by a fresh secret in every response;
    /// * a valid cookie is adopted, unmasked if a pre-4.0 client sent the
    ///   masked form;
    /// * no cookie means no secret yet — nothing is written until `get_token`
    ///   or a rejection calls for it.
    pub fn process_request(&mut self, settings: &Settings, ctx: &RequestContext) {
        let Some(cookie) = ctx.cookie(settings.cookies.csrf_cookie_name) else {
            return;
        };
        match check_token_format(cookie) {
            Ok(()) => self.cookie = Some(unmask_token(cookie)),
            Err(_) => {
                self.cookie = Some(new_secret());
                self.needs_update = true;
            }
        }
    }

    /// `get_token(request)` (`csrf.py:96-114`): create a secret if needed, mark
    /// the cookie for refresh (so the expiry slides), and return the **masked**
    /// token.
    pub fn get_token(&mut self) -> String {
        let secret = match &self.cookie {
            Some(secret) => secret.clone(),
            None => {
                let secret = new_secret();
                self.cookie = Some(secret.clone());
                secret
            }
        };
        self.needs_update = true;
        mask_secret(&secret)
    }

    /// `rotate_token(request)` (`csrf.py:116-121`): a new secret, always written
    /// on the response. `django.contrib.auth.login` calls it.
    pub fn rotate(&mut self) {
        self.cookie = Some(new_secret());
        self.needs_update = true;
    }

    /// `CsrfViewMiddleware._set_csrf_cookie` (`csrf.py:253-269`), when
    /// `CSRF_COOKIE_NEEDS_UPDATE` is set. Returns the `Set-Cookie` value and
    /// clears the flag.
    pub fn response_cookie(&mut self, settings: &Settings) -> Option<String> {
        if !self.needs_update {
            return None;
        }
        self.needs_update = false;
        self.cookie
            .as_deref()
            .map(|secret| cookies::csrf_cookie(settings, secret))
    }
}

/// `CsrfViewMiddleware._get_secret`/`_check_token`'s reading of the cookie:
/// `request.COOKIES[CSRF_COOKIE_NAME]`, validated, unmasked if necessary.
fn request_secret(settings: &Settings, ctx: &RequestContext) -> Result<Option<String>, String> {
    let Some(cookie) = ctx.cookie(settings.cookies.csrf_cookie_name) else {
        return Ok(None);
    };
    if let Err(reason) = check_token_format(cookie) {
        return Err(format!("CSRF cookie {reason}."));
    }
    Ok(Some(unmask_token(cookie)))
}

/// `_bad_token_message` (`csrf.py:342-348`).
fn bad_token_message(reason: &str, from_post: bool) -> String {
    if from_post {
        format!("CSRF token from POST {reason}.")
    } else {
        format!("CSRF token from {HEADER_SOURCE} {reason}.")
    }
}

/// `_check_token` (`csrf.py:349-399`). The error string is the `reason` Django
/// passes to the failure view.
pub fn check_token(settings: &Settings, ctx: &RequestContext) -> Result<(), String> {
    let secret = match request_secret(settings, ctx)? {
        Some(secret) => secret,
        None => return Err(REASON_NO_CSRF_COOKIE.to_string()),
    };

    let mut request_token = String::new();
    let mut from_post = false;
    if ctx.method == Method::POST
        && let Some(body) = &ctx.body
        && let Some(field) = form_field(
            ctx.header("content-type").unwrap_or(""),
            body,
            "csrfmiddlewaretoken",
        )
    {
        request_token = field;
        from_post = true;
    }
    if request_token.is_empty() {
        from_post = false;
        match ctx.header("x-csrftoken") {
            Some(header) => request_token = header.to_string(),
            None => return Err(REASON_CSRF_TOKEN_MISSING.to_string()),
        }
    }

    if let Err(reason) = check_token_format(&request_token) {
        return Err(bad_token_message(reason, from_post));
    }
    if !token_matches(&request_token, &secret) {
        return Err(bad_token_message("incorrect", from_post));
    }
    Ok(())
}

/// `request.POST.get("csrfmiddlewaretoken", "")` for the two body types that can
/// carry it: urlencoded and multipart. The **last** occurrence wins, which is
/// what `QueryDict.get` returns.
pub fn form_field(content_type: &str, body: &[u8], name: &str) -> Option<String> {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match media_type.as_str() {
        "application/x-www-form-urlencoded" => {
            let body = std::str::from_utf8(body).ok()?;
            // `QueryDict`: `parse_qsl(keep_blank_values=True)`, `get()`
            // returns the last value.
            parse_qsl(body)
                .into_iter()
                .rfind(|(key, _)| key == name)
                .map(|(_, value)| value)
        }
        "multipart/form-data" => multipart_field(content_type, body, name),
        _ => None,
    }
}

/// `MultipartParser`: find the part whose `Content-Disposition` names `name`,
/// decode its body as UTF-8 (Django's `request.POST` encoding).
fn multipart_field(content_type: &str, body: &[u8], name: &str) -> Option<String> {
    let boundary = content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("boundary="))
        .map(|value| value.trim_matches('"'))?;
    if boundary.is_empty() {
        return None;
    }
    let delimiter = format!("--{boundary}");
    let mut found: Option<String> = None;
    // Every part starts after a delimiter line; the final `--<boundary>--`
    // terminates the body.
    let text = String::from_utf8_lossy(body).into_owned();
    let mut rest = text.as_str();
    while let Some(start) = rest.find(&delimiter) {
        rest = &rest[start + delimiter.len()..];
        if rest.starts_with("--") {
            break;
        }
        rest = rest.strip_prefix("\r\n").unwrap_or(rest);
        let Some(header_end) = rest.find("\r\n\r\n") else {
            break;
        };
        let headers = &rest[..header_end];
        let content = &rest[header_end + 4..];
        let content_end = content
            .find(&format!("\r\n{delimiter}"))
            .unwrap_or(content.len());
        let value = &content[..content_end];
        if content_disposition_name(headers).as_deref() == Some(name) {
            found = Some(value.to_string());
        }
        rest = &content[content_end..];
    }
    found
}

/// `urllib.parse.parse_qsl(query, keep_blank_values=True)`: `+` is a space,
/// `%XX` is UTF-8 (invalid sequences become U+FFFD), and blank values are kept.
fn parse_qsl(query: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let (key, value) = match part.split_once('=') {
            Some((key, value)) => (key, value),
            None => (part, ""),
        };
        pairs.push((unquote_plus(key), unquote_plus(value)));
    }
    pairs
}

fn unquote_plus(value: &str) -> String {
    let replaced = value.replace('+', " ");
    percent_encoding::percent_decode_str(&replaced)
        .decode_utf8_lossy()
        .into_owned()
}

fn content_disposition_name(headers: &str) -> Option<String> {
    for line in headers.split("\r\n") {
        let Some((header, value)) = line.split_once(':') else {
            continue;
        };
        if !header.trim().eq_ignore_ascii_case("content-disposition") {
            continue;
        }
        for part in value.split(';').map(str::trim) {
            if let Some(rest) = part.strip_prefix("name=") {
                return Some(rest.trim_matches('"').to_string());
            }
        }
    }
    None
}

/// `CsrfViewMiddleware._origin_verified` (`csrf.py:271-295`).
pub fn origin_verified(
    settings: &Settings,
    request_origin: &str,
    request_host: &str,
    is_secure: bool,
) -> bool {
    let good_origin = format!(
        "{}://{}",
        if is_secure { "https" } else { "http" },
        request_host
    );
    if request_origin == good_origin {
        return true;
    }
    if settings
        .cookies
        .csrf_trusted_origins
        .iter()
        .any(|origin| !origin.contains('*') && origin == request_origin)
    {
        return true;
    }
    let Ok(parsed) = py::urlsplit(request_origin) else {
        return false;
    };
    if parsed.scheme.is_empty() || parsed.netloc.is_empty() {
        return false;
    }
    settings
        .cookies
        .csrf_trusted_origins
        .iter()
        .filter(|origin| origin.contains('*'))
        .filter_map(|origin| py::urlsplit(origin).ok())
        .any(|trusted| {
            trusted.scheme == parsed.scheme
                && crate::host::is_same_domain(
                    &parsed.netloc,
                    trusted.netloc.trim_start_matches('*'),
                )
        })
}

/// `CsrfViewMiddleware._check_referer` (`csrf.py:297-340`).
///
/// `server_port` is Django's `request.get_port()`, which under ASGI is
/// `scope["server"][1]` — the port the server is bound to, not the `Host`
/// header's port. `plane-api` passes its bind port for the same value.
pub fn check_referer(
    settings: &Settings,
    ctx: &RequestContext,
    request_host: &str,
    server_port: &str,
) -> Result<(), String> {
    let Some(referer) = ctx.header("referer") else {
        return Err(REASON_NO_REFERER.to_string());
    };
    let Ok(parsed) = py::urlsplit(referer) else {
        return Err(REASON_MALFORMED_REFERER.to_string());
    };
    if parsed.scheme.is_empty() || parsed.netloc.is_empty() {
        return Err(REASON_MALFORMED_REFERER.to_string());
    }
    if parsed.scheme != "https" {
        return Err(REASON_INSECURE_REFERER.to_string());
    }

    let trusted_hosts: Vec<String> = settings
        .cookies
        .csrf_trusted_origins
        .iter()
        .filter_map(|origin| py::urlsplit(origin).ok())
        .map(|origin| origin.netloc.trim_start_matches('*').to_string())
        .collect();
    if trusted_hosts
        .iter()
        .any(|host| crate::host::is_same_domain(&parsed.netloc, host))
    {
        return Ok(());
    }

    let good_referer = match &settings.cookies.csrf_cookie_domain {
        Some(domain) => {
            if server_port != "443" && server_port != "80" {
                format!("{domain}:{server_port}")
            } else {
                domain.clone()
            }
        }
        None => request_host.to_string(),
    };
    if !crate::host::is_same_domain(&parsed.netloc, &good_referer) {
        return Err(REASON_BAD_REFERER.replace("%s", referer));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, HeaderValue};

    fn settings() -> Settings {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("WEB_URL", "http://127.0.0.1:18080");
        Settings::from_source(&env, plane_core::config::Role::Api).expect("settings")
    }

    fn ctx(method: Method, headers: &[(&str, &str)], body: Option<&[u8]>) -> RequestContext {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                http::HeaderName::from_bytes(name.as_bytes()).expect("header"),
                HeaderValue::from_str(value).expect("value"),
            );
        }
        let mut request = RequestContext::new(method, "/auth/sign-in/", "", map, None);
        request.body = body.map(<[u8]>::to_vec);
        request
    }

    // HTTP_CONTRACT Appendix B.2, VECTOR 1.
    #[test]
    fn masked_token_vector() {
        let secret = "0123456789abcdefghijklmnopqrstuv";
        let masked = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefqsuwyACEGIKMOQSUWY02468aceqsuwyA";
        assert_eq!(unmask_token(masked), secret);
        assert!(token_matches(masked, secret));
        assert!(token_matches(secret, secret));
        assert!(!token_matches(secret, "other-secret-0123456789abcdefghi"));
    }

    #[test]
    fn masking_round_trips() {
        for _ in 0..50 {
            let secret = new_secret();
            let masked = mask_secret(&secret);
            assert_eq!(masked.len(), 64);
            assert_eq!(unmask_token(&masked), secret);
            assert!(token_matches(&masked, &secret));
        }
    }

    #[test]
    fn token_format_rules() {
        assert!(check_token_format(&"a".repeat(32)).is_ok());
        assert!(check_token_format(&"a".repeat(64)).is_ok());
        assert_eq!(
            check_token_format(&"a".repeat(63)),
            Err(REASON_INCORRECT_LENGTH)
        );
        assert_eq!(check_token_format(""), Err(REASON_INCORRECT_LENGTH));
        assert_eq!(
            check_token_format(&format!("{}_", "a".repeat(31))),
            Err(REASON_INVALID_CHARACTERS)
        );
    }

    #[test]
    fn get_token_marks_the_cookie_for_refresh() {
        let mut state = CsrfState::default();
        let token = state.get_token();
        assert_eq!(token.len(), 64);
        assert!(state.needs_update);
        let secret = state.cookie.clone().expect("secret");
        assert_eq!(unmask_token(&token), secret);

        // A second call keeps the same secret and still refreshes the expiry.
        let second = state.get_token();
        assert_eq!(state.cookie.as_deref(), Some(secret.as_str()));
        assert_ne!(second, token); // a fresh mask
        assert_eq!(unmask_token(&second), secret);
    }

    #[test]
    fn rotate_token_replaces_the_secret() {
        let mut state = CsrfState::default();
        let token = state.get_token();
        let old = state.cookie.clone().expect("secret");
        state.needs_update = false;
        state.rotate();
        assert!(state.needs_update);
        assert_ne!(state.cookie.as_deref(), Some(old.as_str()));
        assert_eq!(unmask_token(&token), old);
        let header = state.response_cookie(&settings()).expect("cookie");
        assert!(header.starts_with("csrftoken="));
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("Max-Age=31449600"));
        assert!(!state.needs_update);
    }

    #[test]
    fn process_request_adopts_or_replaces() {
        let settings = settings();
        let secret = "0123456789abcdefghijklmnopqrstuv";

        let request = ctx(
            Method::GET,
            &[("cookie", &format!("csrftoken={secret}"))],
            None,
        );
        let mut state = CsrfState::default();
        state.process_request(&settings, &request);
        assert_eq!(state.cookie.as_deref(), Some(secret));
        assert!(!state.needs_update);

        // A masked cookie (pre-4.0) is unmasked.
        let masked = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefqsuwyACEGIKMOQSUWY02468aceqsuwyA";
        let request = ctx(
            Method::GET,
            &[("cookie", &format!("csrftoken={masked}"))],
            None,
        );
        let mut state = CsrfState::default();
        state.process_request(&settings, &request);
        assert_eq!(state.cookie.as_deref(), Some(secret));

        // A malformed cookie is replaced and the replacement is always written.
        let request = ctx(Method::GET, &[("cookie", "csrftoken=short")], None);
        let mut state = CsrfState::default();
        state.process_request(&settings, &request);
        assert!(state.needs_update);
        assert_ne!(state.cookie.as_deref(), Some("short"));

        // No cookie: no secret yet, nothing written.
        let request = ctx(Method::GET, &[], None);
        let mut state = CsrfState::default();
        state.process_request(&settings, &request);
        assert!(state.cookie.is_none());
        assert!(!state.needs_update);
    }

    #[test]
    fn check_token_reason_wording() {
        let settings = settings();
        let secret = "0123456789abcdefghijklmnopqrstuv";
        let request = ctx(
            Method::POST,
            &[("cookie", &format!("csrftoken={secret}"))],
            None,
        );
        assert_eq!(
            check_token(&settings, &request),
            Err(REASON_CSRF_TOKEN_MISSING.to_string())
        );

        let request = ctx(
            Method::POST,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("x-csrftoken", "short"),
            ],
            None,
        );
        assert_eq!(
            check_token(&settings, &request),
            Err("CSRF token from the 'X-Csrftoken' HTTP header has incorrect length.".to_string())
        );

        let wrong = "b".repeat(32);
        let request = ctx(
            Method::POST,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("x-csrftoken", &wrong),
            ],
            None,
        );
        assert_eq!(
            check_token(&settings, &request),
            Err("CSRF token from the 'X-Csrftoken' HTTP header incorrect.".to_string())
        );

        let request = ctx(
            Method::POST,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("x-csrftoken", secret),
            ],
            None,
        );
        assert_eq!(check_token(&settings, &request), Ok(()));

        let request = ctx(Method::POST, &[("cookie", "csrftoken=short")], None);
        assert_eq!(
            check_token(&settings, &request),
            Err("CSRF cookie has incorrect length.".to_string())
        );

        let request = ctx(Method::POST, &[], None);
        assert_eq!(
            check_token(&settings, &request),
            Err(REASON_NO_CSRF_COOKIE.to_string())
        );
    }

    #[test]
    fn urlencoded_form_token_is_used_for_post() {
        let settings = settings();
        let secret = "0123456789abcdefghijklmnopqrstuv";
        for token in [
            secret.to_string(),
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefqsuwyACEGIKMOQSUWY02468aceqsuwyA".to_string(),
        ] {
            let body = format!("csrfmiddlewaretoken={token}&other=1");
            let request = ctx(
                Method::POST,
                &[
                    ("cookie", &format!("csrftoken={secret}")),
                    ("content-type", "application/x-www-form-urlencoded"),
                ],
                Some(body.as_bytes()),
            );
            assert_eq!(check_token(&settings, &request), Ok(()), "{token}");
        }
        // The last value wins.
        let body = format!("csrfmiddlewaretoken=wrong&csrfmiddlewaretoken={secret}");
        let request = ctx(
            Method::POST,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("content-type", "application/x-www-form-urlencoded"),
            ],
            Some(body.as_bytes()),
        );
        assert_eq!(check_token(&settings, &request), Ok(()));

        // A form token on PUT is ignored (Django only reads request.POST for
        // POST), so the header is required.
        let body = format!("csrfmiddlewaretoken={secret}");
        let request = ctx(
            Method::PUT,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("content-type", "application/x-www-form-urlencoded"),
            ],
            Some(body.as_bytes()),
        );
        assert_eq!(
            check_token(&settings, &request),
            Err(REASON_CSRF_TOKEN_MISSING.to_string())
        );
    }

    #[test]
    fn multipart_form_token_is_used_for_post() {
        let settings = settings();
        let secret = "0123456789abcdefghijklmnopqrstuv";
        let body = format!(
            "--BOUND\r\nContent-Disposition: form-data; name=\"email\"\r\n\r\na@example.com\r\n\
             --BOUND\r\nContent-Disposition: form-data; name=\"csrfmiddlewaretoken\"\r\n\r\n{secret}\r\n\
             --BOUND--\r\n"
        );
        let request = ctx(
            Method::POST,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("content-type", "multipart/form-data; boundary=BOUND"),
            ],
            Some(body.as_bytes()),
        );
        assert_eq!(check_token(&settings, &request), Ok(()));

        // A quoted boundary and a filename parameter on another part.
        let body = format!(
            "--b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\n\r\nhello\r\n\
             --b\r\nContent-Disposition: form-data; name=\"csrfmiddlewaretoken\"\r\n\r\n{secret}\r\n--b--\r\n"
        );
        let request = ctx(
            Method::POST,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("content-type", "multipart/form-data; boundary=\"b\""),
            ],
            Some(body.as_bytes()),
        );
        assert_eq!(check_token(&settings, &request), Ok(()));
    }

    #[test]
    fn origin_and_referer_checks() {
        let settings = settings();
        assert!(origin_verified(
            &settings,
            "http://plane.example.com",
            "plane.example.com",
            false
        ));
        // HTTPS sees a different good origin.
        assert!(!origin_verified(
            &settings,
            "http://plane.example.com",
            "plane.example.com",
            true
        ));
        assert!(origin_verified(
            &settings,
            "https://plane.example.com",
            "plane.example.com",
            true
        ));
        assert!(!origin_verified(
            &settings,
            "https://evil.example",
            "plane.example.com",
            false
        ));

        let request = ctx(
            Method::POST,
            &[
                ("host", "plane.example.com"),
                ("referer", "http://plane.example.com/x"),
            ],
            None,
        );
        assert_eq!(
            check_referer(&settings, &request, "plane.example.com", "80"),
            Err(REASON_INSECURE_REFERER.to_string())
        );
        let request = ctx(
            Method::POST,
            &[
                ("host", "plane.example.com"),
                ("referer", "https://plane.example.com/x"),
            ],
            None,
        );
        assert_eq!(
            check_referer(&settings, &request, "plane.example.com", "443"),
            Ok(())
        );
        let request = ctx(Method::POST, &[("host", "plane.example.com")], None);
        assert_eq!(
            check_referer(&settings, &request, "plane.example.com", "443"),
            Err(REASON_NO_REFERER.to_string())
        );
    }

    #[test]
    fn trusted_origin_subdomains() {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set(
                "CORS_ALLOWED_ORIGINS",
                "https://plane.example.com, https://*.internal.example.com",
            );
        let settings =
            Settings::from_source(&env, plane_core::config::Role::Api).expect("settings");
        assert!(origin_verified(
            &settings,
            "https://plane.example.com",
            "127.0.0.1:18101",
            false
        ));
        assert!(origin_verified(
            &settings,
            "https://a.internal.example.com",
            "127.0.0.1:18101",
            false
        ));
        assert!(!origin_verified(
            &settings,
            "https://evil.example.com",
            "127.0.0.1:18101",
            false
        ));
    }
}
