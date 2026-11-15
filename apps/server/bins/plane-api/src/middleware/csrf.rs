// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `CsrfViewMiddleware` (`django/middleware/csrf.py`, HTTP_CONTRACT §7).
//!
//! This is the *Django* CSRF check, which applies only to views that are not
//! `csrf_exempt`. Every DRF view is exempt (`rest_framework/views.py:143-149`),
//! so in Plane it applies to the plain `django.views.View` classes under
//! `/auth/` and to the two function views in `plane.web.views` — including
//! `POST /` and `POST /robots.txt`, which is exactly what the skeleton scenario
//! exercises. The DRF-side origin check (`BaseSessionAuthentication`) is a
//! different mechanism and belongs to Phase 2.
//!
//! Two behaviours are easy to get wrong:
//!
//! * a rejection is **HTTP 200** with the `csrf_failure.html` template
//!   (`plane/authentication/views/common.py:39-45`), not a 403;
//! * the response is produced in `process_view`, so it passes through every
//!   response middleware (logger, gzip, session), which is why it carries
//!   `Vary: Accept-Encoding, Cookie, origin` (HTTP_CONTRACT §7.5).
//!
//! The template is embedded from `apps/api/templates/csrf_failure.html` at build
//! time, so upstream edits are picked up on rebuild instead of drifting.
//!
//! Not implemented in Phase 1 (Phase 2 owns the cookie writer): writing a
//! replacement `csrftoken` cookie when the request carried a *malformed* one
//! (`_add_new_csrf_cookie` + `_set_csrf_cookie`), and multipart form fields for
//! `csrfmiddlewaretoken`. Both only change responses for requests that already
//! carry a malformed cookie or a multipart body.

use axum::http::Method;
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_core::error::TEXT_HTML;
use plane_core::py;

use crate::host;
use crate::response::Resp;

/// `CSRF_SECRET_LENGTH`.
const SECRET_LENGTH: usize = 32;
/// `CSRF_TOKEN_LENGTH = 2 * CSRF_SECRET_LENGTH`.
const TOKEN_LENGTH: usize = 64;
/// `CSRF_ALLOWED_CHARS`.
const ALLOWED_CHARS: &[u8; 62] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

const REASON_BAD_ORIGIN: &str = "Origin checking failed - %s does not match any trusted origins.";
const REASON_NO_REFERER: &str = "Referer checking failed - no Referer.";
const REASON_BAD_REFERER: &str = "Referer checking failed - %s does not match any trusted origins.";
const REASON_NO_CSRF_COOKIE: &str = "CSRF cookie not set.";
const REASON_CSRF_TOKEN_MISSING: &str = "CSRF token missing.";
const REASON_MALFORMED_REFERER: &str = "Referer checking failed - Referer is malformed.";
const REASON_INSECURE_REFERER: &str =
    "Referer checking failed - Referer is insecure while host is secure.";
const REASON_INCORRECT_LENGTH: &str = "has incorrect length";
const REASON_INVALID_CHARACTERS: &str = "has invalid characters";

/// `_get_token_source`'s header-name rendering: `HTTP_X_CSRFTOKEN` →
/// `X-Csrftoken` (`django/http/request.py`, `HttpHeaders.parse_header_name`).
const HEADER_SOURCE: &str = "the 'X-Csrftoken' HTTP header";

/// The template Django renders for a rejection.
const CSRF_FAILURE_TEMPLATE: &str =
    include_str!("../../../../../../apps/api/templates/csrf_failure.html");

/// `CsrfViewMiddleware.process_response`: only the `Vary` effect for Phase 1.
pub fn response(ctx: &RequestContext, mut resp: Resp) -> Resp {
    if ctx.csrf_cookie_needs_update {
        resp.patch_vary("Cookie");
    }
    resp
}

/// `CsrfViewMiddleware.process_view`. `false` means "no opinion, continue to
/// the view"; `Some(response)` is a rejection.
///
/// `matched` is false for an unmatched path: Django raises `Resolver404` before
/// any `process_view` hook, so no CSRF check runs for a 404.
pub fn process_view(settings: &Settings, ctx: &RequestContext, matched: bool) -> Option<Resp> {
    if !matched || ctx.csrf_exempt {
        return None;
    }
    // Safe methods are always accepted.
    if matches!(
        ctx.method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    ) {
        return None;
    }

    if ctx.headers.contains_key("origin") {
        let origin = ctx.header("origin").unwrap_or("");
        if !origin_verified(settings, ctx, origin) {
            return Some(rejection(
                settings,
                &REASON_BAD_ORIGIN.replace("%s", origin),
            ));
        }
    } else if ctx.is_secure
        && let Err(reason) = check_referer(settings, ctx)
    {
        return Some(rejection(settings, &reason));
    }

    match check_token(settings, ctx) {
        Ok(()) => None,
        Err(reason) => Some(rejection(settings, &reason)),
    }
}

/// `_origin_verified`.
fn origin_verified(settings: &Settings, ctx: &RequestContext, request_origin: &str) -> bool {
    if let Some(host) = host::get_host(settings, ctx.header("host"), ctx.is_secure) {
        let scheme = if ctx.is_secure { "https" } else { "http" };
        if request_origin == format!("{scheme}://{host}") {
            return true;
        }
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
                && trust_domain_matches(&parsed.netloc, trusted.netloc.trim_start_matches('*'))
        })
}

/// `django.utils.http.is_same_domain`, applied to a netloc that may include a
/// port. Django does the same string comparison here, so the port is part of
/// the domain string.
fn trust_domain_matches(host: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = pattern.to_lowercase();
    (pattern.starts_with('.') && (host.ends_with(&pattern) || host == &pattern[1..]))
        || pattern == host
}

/// `_check_referer`.
fn check_referer(settings: &Settings, ctx: &RequestContext) -> Result<(), String> {
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

    let trusted_hosts = settings
        .cookies
        .csrf_trusted_origins
        .iter()
        .filter_map(|origin| py::urlsplit(origin).ok())
        .map(|origin| origin.netloc.trim_start_matches('*').to_string())
        .collect::<Vec<_>>();
    if trusted_hosts
        .iter()
        .any(|host| trust_domain_matches(&parsed.netloc, host))
    {
        return Ok(());
    }

    let good_referer = match &settings.cookies.csrf_cookie_domain {
        Some(domain) => {
            let port = settings.rust.bind.port();
            if port != 443 && port != 80 {
                format!("{domain}:{port}")
            } else {
                domain.clone()
            }
        }
        None => match host::get_host(settings, ctx.header("host"), ctx.is_secure) {
            Some(host) => host,
            None => return Err(REASON_BAD_REFERER.replace("%s", referer)),
        },
    };
    if !trust_domain_matches(&parsed.netloc, &good_referer) {
        return Err(REASON_BAD_REFERER.replace("%s", referer));
    }
    Ok(())
}

/// `_check_token`.
fn check_token(settings: &Settings, ctx: &RequestContext) -> Result<(), String> {
    let secret = match ctx.cookie(settings.cookies.csrf_cookie_name) {
        None => return Err(REASON_NO_CSRF_COOKIE.to_string()),
        Some(cookie) => {
            if let Err(reason) = check_token_format(cookie) {
                return Err(format!("CSRF cookie {reason}."));
            }
            unmask_if_needed(cookie)
        }
    };

    let mut token_source = TokenSource::Post;
    let mut request_token = String::new();
    if ctx.method == Method::POST
        && let Some(body) = &ctx.body
        && let Some(form_token) = form_field(
            ctx.header("content-type").unwrap_or(""),
            body,
            "csrfmiddlewaretoken",
        )
    {
        request_token = form_token;
    }
    if request_token.is_empty() {
        token_source = TokenSource::Header;
        match ctx.header("x-csrftoken") {
            // Present but empty falls through to the format check, which
            // reports "incorrect length" (Django reads META directly).
            Some(header) => request_token = header.to_string(),
            None => return Err(REASON_CSRF_TOKEN_MISSING.to_string()),
        }
    }

    if let Err(reason) = check_token_format(&request_token) {
        return Err(bad_token_message(reason, token_source));
    }
    if !does_token_match(&request_token, &secret) {
        return Err(bad_token_message("is incorrect", token_source));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenSource {
    Post,
    Header,
}

fn bad_token_message(reason: &str, source: TokenSource) -> String {
    match source {
        TokenSource::Post => format!("CSRF token from POST {reason}."),
        TokenSource::Header => format!("CSRF token from {HEADER_SOURCE} {reason}."),
    }
}

/// `_check_token_format`: length is 32 or 64 and every character is in the
/// alphabet.
fn check_token_format(token: &str) -> Result<(), &'static str> {
    let length = token.chars().count();
    if length != TOKEN_LENGTH && length != SECRET_LENGTH {
        return Err(REASON_INCORRECT_LENGTH);
    }
    if !token.bytes().all(is_allowed_char) {
        return Err(REASON_INVALID_CHARACTERS);
    }
    Ok(())
}

fn is_allowed_char(byte: u8) -> bool {
    ALLOWED_CHARS.contains(&byte)
}

fn char_index(byte: u8) -> u8 {
    ALLOWED_CHARS
        .iter()
        .position(|candidate| *candidate == byte)
        .unwrap_or(0) as u8
}

/// `_unmask_cipher_token` for a 64-character token; shorter tokens are already
/// unmasked secrets.
fn unmask_if_needed(token: &str) -> String {
    if token.len() != TOKEN_LENGTH {
        return token.to_string();
    }
    let bytes = token.as_bytes();
    let mut unmasked = String::with_capacity(SECRET_LENGTH);
    for index in 0..SECRET_LENGTH {
        let cipher = char_index(bytes[SECRET_LENGTH + index]);
        let mask = char_index(bytes[index]);
        let value = (i32::from(cipher) - i32::from(mask)).rem_euclid(62) as usize;
        unmasked.push(ALLOWED_CHARS[value] as char);
    }
    unmasked
}

/// `_does_token_match`.
fn does_token_match(request_token: &str, secret: &str) -> bool {
    let candidate = unmask_if_needed(request_token);
    constant_time_eq(candidate.as_bytes(), secret.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// `request.POST.get(name)` for an `application/x-www-form-urlencoded` body.
fn form_field(content_type: &str, body: &[u8], name: &str) -> Option<String> {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if media_type != "application/x-www-form-urlencoded" {
        return None;
    }
    let body = std::str::from_utf8(body).ok()?;
    py::parse_qs(body)
        .into_iter()
        .find(|(key, values)| key == name && !values.is_empty())
        .map(|(_, mut values)| values.swap_remove(0))
}

/// `csrf_failure` (`plane/authentication/views/common.py:39-45`): render the
/// template with `root_url = base_host(request)`.
fn rejection(settings: &Settings, reason: &str) -> Resp {
    // `settings.WEB_URL or settings.APP_BASE_URL`; `base_host` raises
    // ImproperlyConfigured when both are unset, which Django turns into its 500
    // page.
    let root_url = match settings
        .urls
        .web_url
        .as_deref()
        .filter(|url| !url.is_empty())
    {
        Some(url) => url.to_string(),
        None => match &settings.urls.app_base_url {
            Some(url) => url.clone(),
            None => {
                tracing::error!(
                    target: "plane.exception",
                    "ImproperlyConfigured: APP_BASE_URL or WEB_URL is not set"
                );
                return crate::handlers::server_error();
            }
        },
    };
    tracing::warn!(
        target: "django.security.csrf",
        "Forbidden ({reason})"
    );
    let body = CSRF_FAILURE_TEMPLATE.replace("{{ root_url }}", &html_escape(&root_url));
    Resp::new(200, TEXT_HTML, body.into_bytes())
}

/// Django's HTML auto-escaping for `{{ root_url }}`.
fn html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue, Method};

    fn settings(web_url: Option<&str>, cors: &str) -> Settings {
        let mut env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("CORS_ALLOWED_ORIGINS", cors);
        if let Some(url) = web_url {
            env = env.set("WEB_URL", url);
        }
        Settings::from_source(&env, plane_core::config::Role::Api).expect("settings")
    }

    fn ctx(method: Method, headers: &[(&str, &str)], body: Option<&str>) -> RequestContext {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        let mut request = RequestContext::new(method, "/", "", map, None);
        request.body = body.map(|body| body.as_bytes().to_vec());
        request
    }

    // HTTP_CONTRACT Appendix B.2, VECTOR 1.
    #[test]
    fn masked_token_vector() {
        let secret = "0123456789abcdefghijklmnopqrstuv";
        let masked = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefqsuwyACEGIKMOQSUWY02468aceqsuwyA";
        assert_eq!(unmask_if_needed(masked), secret);
        assert!(does_token_match(masked, secret));
        assert!(does_token_match(secret, secret));
        assert!(!does_token_match(
            secret,
            "other-secret-0123456789abcdefghi"
        ));
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

    // HTTP_CONTRACT §7.3: a POST without a cookie is rejected with the failure
    // page, status 200, `text/html`.
    #[test]
    fn rejects_a_post_without_a_cookie() {
        let settings = settings(Some("http://127.0.0.1:18080"), "");
        let request = ctx(Method::POST, &[("host", "127.0.0.1:18101")], None);
        let response = process_view(&settings, &request, true).expect("rejection");
        assert_eq!(response.status.as_u16(), 200);
        assert_eq!(response.content_type, Some(TEXT_HTML));
        let body = String::from_utf8(response.body).expect("utf8");
        assert!(body.contains("CSRF Verification Failed"));
        assert!(body.contains("href=\"http://127.0.0.1:18080\""));
        assert!(!body.contains("{{ root_url }}"));
        assert_eq!(body.len(), 1750);
    }

    // Safe methods and exempt views are not checked.
    #[test]
    fn safe_methods_and_exempt_views_pass() {
        let settings = settings(Some("http://127.0.0.1:18080"), "");
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            let request = ctx(method.clone(), &[], None);
            assert!(
                process_view(&settings, &request, true).is_none(),
                "{method}"
            );
        }
        let mut post = ctx(Method::POST, &[], None);
        post.csrf_exempt = true;
        assert!(process_view(&settings, &post, true).is_none());
        // An unmatched path never reaches process_view.
        let post = ctx(Method::POST, &[], None);
        assert!(process_view(&settings, &post, false).is_none());
    }

    // A matching Origin does not replace the token check (§7.4 order).
    #[test]
    fn origin_is_checked_before_the_token() {
        let settings = settings(Some("http://127.0.0.1:18080"), "");
        let request = ctx(
            Method::POST,
            &[
                ("host", "plane.example.com"),
                ("origin", "http://evil.example"),
            ],
            None,
        );
        let response = process_view(&settings, &request, true).expect("rejection");
        let body = String::from_utf8(response.body).expect("utf8");
        assert!(body.contains("CSRF Verification Failed"));

        // A same-host Origin passes the origin check and then fails on the
        // missing token.
        let request = ctx(
            Method::POST,
            &[
                ("host", "plane.example.com"),
                ("origin", "http://plane.example.com"),
            ],
            None,
        );
        let response = process_view(&settings, &request, true).expect("rejection");
        assert!(
            String::from_utf8(response.body)
                .expect("utf8")
                .contains("CSRF Verification Failed")
        );
    }

    // §7.4 step 6: the header fallback, and the exact reason wording.
    #[test]
    fn token_reason_wording() {
        let settings = settings(Some("http://127.0.0.1:18080"), "");
        let secret = "0123456789abcdefghijklmnopqrstuv";
        let request = ctx(
            Method::POST,
            &[("cookie", &format!("csrftoken={secret}"))],
            None,
        );
        assert_eq!(
            check_token(&settings, &request),
            Err("CSRF token missing.".to_string())
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
            Err("CSRF token from the 'X-Csrftoken' HTTP header is incorrect.".to_string())
        );

        // The correct secret passes.
        let request = ctx(
            Method::POST,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("x-csrftoken", secret),
            ],
            None,
        );
        assert_eq!(check_token(&settings, &request), Ok(()));

        // A malformed cookie is reported with the cookie wording.
        let request = ctx(Method::POST, &[("cookie", "csrftoken=short")], None);
        assert_eq!(
            check_token(&settings, &request),
            Err("CSRF cookie has incorrect length.".to_string())
        );
    }

    // The POST form field is preferred over the header.
    #[test]
    fn form_token_is_used_for_post() {
        let settings = settings(Some("http://127.0.0.1:18080"), "");
        let secret = "0123456789abcdefghijklmnopqrstuv";
        let body = format!("csrfmiddlewaretoken={secret}&other=1");
        let request = ctx(
            Method::POST,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("content-type", "application/x-www-form-urlencoded"),
            ],
            Some(&body),
        );
        assert_eq!(check_token(&settings, &request), Ok(()));

        // A masked form token also works.
        let masked = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefqsuwyACEGIKMOQSUWY02468aceqsuwyA";
        let body = format!("csrfmiddlewaretoken={masked}");
        let request = ctx(
            Method::POST,
            &[
                ("cookie", &format!("csrftoken={secret}")),
                ("content-type", "application/x-www-form-urlencoded"),
            ],
            Some(&body),
        );
        assert_eq!(check_token(&settings, &request), Ok(()));
    }

    #[test]
    fn trusted_origins() {
        let settings = settings(
            Some("http://127.0.0.1:18080"),
            "https://plane.example.com, https://*.internal.example.com",
        );
        let request = ctx(
            Method::POST,
            &[
                ("host", "127.0.0.1:18101"),
                ("origin", "https://plane.example.com"),
            ],
            None,
        );
        // Origin accepted; the token is still missing.
        assert_eq!(
            check_token(&settings, &request),
            Err("CSRF cookie not set.".to_string())
        );

        let subdomain = ctx(
            Method::POST,
            &[
                ("host", "127.0.0.1:18101"),
                ("origin", "https://a.internal.example.com"),
            ],
            None,
        );
        assert!(origin_verified(
            &settings,
            &subdomain,
            "https://a.internal.example.com"
        ));

        let evil = ctx(
            Method::POST,
            &[
                ("host", "127.0.0.1:18101"),
                ("origin", "https://evil.example.com"),
            ],
            None,
        );
        assert!(!origin_verified(
            &settings,
            &evil,
            "https://evil.example.com"
        ));
    }

    #[test]
    fn secure_requests_require_a_referer() {
        let settings = settings(Some("http://127.0.0.1:18080"), "");
        let mut request = ctx(
            Method::POST,
            &[
                ("host", "plane.example.com"),
                ("x-forwarded-proto", "https"),
            ],
            None,
        );
        request.is_secure = true;
        let response = process_view(&settings, &request, true).expect("rejection");
        let body = String::from_utf8(response.body).expect("utf8");
        assert!(body.contains("CSRF Verification Failed"));

        // The check run directly yields the "no Referer" reason.
        assert_eq!(
            check_referer(&settings, &request),
            Err(REASON_NO_REFERER.to_string())
        );

        request.headers.insert(
            "referer",
            HeaderValue::from_static("http://plane.example.com/sign-in/"),
        );
        assert_eq!(
            check_referer(&settings, &request),
            Err(REASON_INSECURE_REFERER.to_string())
        );

        request.headers.insert(
            "referer",
            HeaderValue::from_static("https://plane.example.com/sign-in/"),
        );
        assert_eq!(check_referer(&settings, &request), Ok(()));
    }

    #[test]
    fn html_escaping_matches_django() {
        assert_eq!(
            html_escape("http://x/?a=1&b=<2>\"'"),
            "http://x/?a=1&amp;b=&lt;2&gt;&quot;&#x27;"
        );
    }

    #[test]
    fn template_is_the_upstream_file() {
        // A future upstream edit that removes the placeholder must fail this
        // test instead of silently shipping a literal `{{ root_url }}`.
        assert!(CSRF_FAILURE_TEMPLATE.contains("{{ root_url }}"));
        assert!(CSRF_FAILURE_TEMPLATE.contains("CSRF Verification Failed"));
    }
}
