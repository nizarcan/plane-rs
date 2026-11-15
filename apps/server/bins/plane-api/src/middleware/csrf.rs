// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `CsrfViewMiddleware` (`django/middleware/csrf.py`), HTTP_CONTRACT §7.
//!
//! This is the *Django* CSRF check, which applies only to views that are not
//! `csrf_exempt`. Every DRF view is exempt (`rest_framework/views.py:143-149`),
//! so in Plane it applies to the plain `django.views.View` classes under
//! `/auth/` and to the two function views in `plane.web.views` — including
//! `POST /` and `POST /robots.txt`, which is exactly what the skeleton scenario
//! exercises. The DRF-side origin check (`BaseSessionAuthentication`) is a
//! different mechanism, lives in `plane_auth::origin`, and never looks at a
//! token.
//!
//! Two behaviours are easy to get wrong:
//!
//! * a rejection is **HTTP 200** with the `csrf_failure.html` template
//!   (`plane/authentication/views/common.py:39-45`), not a 403;
//! * the response is produced in `process_view`, so it passes through every
//!   response middleware (logger, gzip, session), which is why it carries
//!   `Vary: Accept-Encoding, Cookie, origin` (HTTP_CONTRACT §7.5).
//!
//! The token machinery itself (masking, `get_token`, `rotate_token`, the
//! `request.POST`/multipart lookup and the reason strings) is
//! `plane_auth::csrf`; this module is the middleware's request/response wiring.
//!
//! The template is embedded from `apps/api/templates/csrf_failure.html` at build
//! time, so upstream edits are picked up on rebuild instead of drifting.

use axum::http::Method;
use plane_auth::csrf::{self, CsrfState, REASON_BAD_ORIGIN, REASON_BAD_REFERER};
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_core::error::TEXT_HTML;

use crate::host;
use crate::response::Resp;

/// The template Django renders for a rejection.
const CSRF_FAILURE_TEMPLATE: &str =
    include_str!("../../../../../../apps/api/templates/csrf_failure.html");

/// `CsrfViewMiddleware.process_request`: adopt a valid `csrftoken` cookie or
/// replace a malformed one (`csrf.py:400-414`).
pub fn request(settings: &Settings, ctx: &RequestContext, csrf_state: &mut CsrfState) {
    csrf_state.process_request(settings, ctx);
}

/// `CsrfViewMiddleware.process_response`: write the `csrftoken` cookie when
/// `get_token`/`rotate_token` asked for it, and patch `Vary: Cookie`
/// (`csrf.py:505-520`, `253-269`).
pub fn response(settings: &Settings, csrf_state: &mut CsrfState, mut resp: Resp) -> Resp {
    if let Some(cookie) = csrf_state.response_cookie(settings) {
        resp.append_header("Set-Cookie", cookie);
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

    let request_host = host::get_host(settings, ctx.header("host"), ctx.is_secure);

    if ctx.headers.contains_key("origin") {
        let origin = ctx.header("origin").unwrap_or("");
        // `_origin_verified` skips the same-origin branch when `get_host`
        // raises `DisallowedHost`; layer 4 has already rejected that case, so
        // `request_host` is normally `Some`.
        let verified = match &request_host {
            Some(host) => csrf::origin_verified(settings, origin, host, ctx.is_secure),
            None => false,
        };
        if !verified {
            return Some(rejection(
                settings,
                &REASON_BAD_ORIGIN.replace("%s", origin),
            ));
        }
    } else if ctx.is_secure
        && let Some(host) = &request_host
        && let Err(reason) =
            csrf::check_referer(settings, ctx, host, &settings.rust.bind.port().to_string())
    {
        return Some(rejection(settings, &reason));
    } else if ctx.is_secure && request_host.is_none() {
        return Some(rejection(
            settings,
            &REASON_BAD_REFERER.replace("%s", ctx.header("referer").unwrap_or("")),
        ));
    }

    match csrf::check_token(settings, ctx) {
        Ok(()) => None,
        Err(reason) => Some(rejection(settings, &reason)),
    }
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
    use axum::http::{HeaderMap, HeaderValue};
    use plane_auth::csrf::{
        CSRF_SECRET_LENGTH, CSRF_TOKEN_LENGTH, check_token, check_token_format, token_matches,
        unmask_token,
    };

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
        assert_eq!(unmask_token(masked), secret);
        assert!(token_matches(masked, secret));
        assert!(token_matches(secret, secret));
        assert!(!token_matches(secret, "other-secret-0123456789abcdefghi"));
        assert_eq!(CSRF_SECRET_LENGTH, 32);
        assert_eq!(CSRF_TOKEN_LENGTH, 64);
    }

    #[test]
    fn token_format_rules() {
        assert!(check_token_format(&"a".repeat(32)).is_ok());
        assert!(check_token_format(&"a".repeat(64)).is_ok());
        assert_eq!(
            check_token_format(&"a".repeat(63)),
            Err(csrf::REASON_INCORRECT_LENGTH)
        );
        assert_eq!(check_token_format(""), Err(csrf::REASON_INCORRECT_LENGTH));
        assert_eq!(
            check_token_format(&format!("{}_", "a".repeat(31))),
            Err(csrf::REASON_INVALID_CHARACTERS)
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

    // §7.4 step 6: the header fallback, and the exact reason wording. Django's
    // `_bad_token_message("incorrect", …)` produces "… header incorrect."
    // without "is"; the skeleton's test had "is incorrect", taken from the
    // HTTP_CONTRACT §7.4 paraphrase.
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
            Err("CSRF token from the 'X-Csrftoken' HTTP header incorrect.".to_string())
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
        assert!(csrf::origin_verified(
            &settings,
            "https://a.internal.example.com",
            "127.0.0.1:18101",
            false
        ));
        assert!(process_view(&settings, &subdomain, true).is_some());

        let evil = ctx(
            Method::POST,
            &[
                ("host", "127.0.0.1:18101"),
                ("origin", "https://evil.example.com"),
            ],
            None,
        );
        assert!(!csrf::origin_verified(
            &settings,
            "https://evil.example.com",
            "127.0.0.1:18101",
            false
        ));
        let response = process_view(&settings, &evil, true).expect("rejection");
        assert_eq!(response.status.as_u16(), 200);
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
            csrf::check_referer(&settings, &request, "plane.example.com", "18101"),
            Err(csrf::REASON_NO_REFERER.to_string())
        );

        request.headers.insert(
            "referer",
            HeaderValue::from_static("http://plane.example.com/sign-in/"),
        );
        assert_eq!(
            csrf::check_referer(&settings, &request, "plane.example.com", "18101"),
            Err(csrf::REASON_INSECURE_REFERER.to_string())
        );

        request.headers.insert(
            "referer",
            HeaderValue::from_static("https://plane.example.com/sign-in/"),
        );
        assert_eq!(
            csrf::check_referer(&settings, &request, "plane.example.com", "18101"),
            Ok(())
        );

        // MALFORMED_REFERER for an unparseable value.
        request
            .headers
            .insert("referer", HeaderValue::from_static("not a url"));
        assert_eq!(
            csrf::check_referer(&settings, &request, "plane.example.com", "18101"),
            Err(csrf::REASON_MALFORMED_REFERER.to_string())
        );
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

    #[test]
    fn the_response_writes_the_cookie_and_varies() {
        let settings = settings(Some("http://127.0.0.1:18080"), "");
        let mut state = CsrfState::default();
        let token = state.get_token();
        assert_eq!(token.len(), 64);
        let response = response(&settings, &mut state, Resp::empty(200, "application/json"));
        let cookie = response.header("set-cookie").expect("cookie");
        assert!(cookie.starts_with("csrftoken="));
        assert!(cookie.contains("HttpOnly"));
        assert_eq!(response.header("vary"), Some("Cookie"));
        assert!(!state.needs_update);
    }
}
