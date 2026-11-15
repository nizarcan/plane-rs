// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `APITokenLogMiddleware` (`plane/middleware/logger.py:80-163`), layer 11.
//!
//! For **every** request carrying `X-Api-Key` — whatever the response, on any
//! mount — the middleware builds one `api_activity_logs` row and enqueues
//! `plane.bgtasks.logger_task.process_logs(log_data=…)` on Celery. The row has:
//!
//! | Column | Value |
//! | --- | --- |
//! | `token_identifier` | `hmac.new(SECRET_KEY, api_key, sha256).hexdigest()` |
//! | `path` | `request.path` (no query string) |
//! | `method` | `request.method` |
//! | `query_params` | `request.META["QUERY_STRING"]` |
//! | `headers` | Python `dict.__repr__` of the lower-cased headers, with `x-api-key`, `authorization`, `cookie` replaced by `[REDACTED]` |
//! | `body` / `response_body` | `_safe_decode_body`, below |
//! | `response_code` | the status |
//! | `ip_address` | `get_client_ip`: the first `X-Forwarded-For` entry, else `REMOTE_ADDR` (it ignores `NUM_PROXIES`) |
//! | `user_agent` | `User-Agent`, `NULL` when absent |
//!
//! `_safe_decode_body` returns `None` for no body, `"[Binary Content]"` for a
//! PNG/JPEG/PDF signature, the UTF-8 text otherwise and
//! `"[Could not decode content]"` on a decode error.
//!
//! ## Why `/api/v1/` cannot be registered in Phase 2
//!
//! The row is written by a Celery task, and Phase 4 owns the Celery publisher
//! (DECISIONS.md D7). Until it exists, a Rust request would silently drop the
//! `api_activity_logs` row — a real difference, and one the differential harness
//! would show as a drained `process_logs` message that only Django published.
//! [`crate::registry::Registry`] therefore refuses every `/api/v1/` view with
//! [`crate::registry::RegistryError::ApiTokenLogUnavailable`], and the policy
//! probe covers the other mounts only. The builder fills the gap:
//!
//! * [`ApiTokenLog::build`] is the complete Python data construction, tested;
//! * [`TokenLogSink`] is the one method the Phase 4 publisher has to implement;
//! * `plane-api` calls the sink for any request with `X-Api-Key` once one is
//!   installed.
//!
//! Residual gap, recorded in `ROUTES.md`: a request carrying `X-Api-Key` to
//! `/api/`, `/api/public/` or `/api/instances/` also produces this log in
//! Django (those mounts ignore the header for authentication but the middleware
//! does not). Until a sink exists, no row or message is produced here. The
//! probe never sends the header on those routes.

use aws_lc_rs::hmac;
use plane_core::config::Settings;
use plane_core::context::RequestContext;

/// One `api_activity_logs` row, as Python builds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiTokenLog {
    pub token_identifier: String,
    pub path: String,
    pub method: String,
    pub query_params: String,
    /// Python's `dict.__repr__` text, not JSON.
    pub headers: String,
    pub body: Option<String>,
    pub response_body: Option<String>,
    pub response_code: u16,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl ApiTokenLog {
    /// `APITokenLogMiddleware.process_request`, minus the Celery call.
    ///
    /// Returns `None` when the request has no `X-Api-Key`, which is when Python
    /// returns before building anything.
    pub fn build(
        settings: &Settings,
        ctx: &RequestContext,
        response_status: u16,
        response_body: &[u8],
    ) -> Option<Self> {
        let api_key = ctx.header("x-api-key")?;
        if api_key.is_empty() {
            return None;
        }
        let request_body = ctx.body.as_deref();
        Some(Self {
            token_identifier: token_identifier(settings.secret_key.value(), api_key),
            path: ctx.path.clone(),
            method: ctx.method.as_str().to_string(),
            query_params: ctx.query_string.clone(),
            headers: redacted_headers(ctx),
            body: safe_decode_body(request_body),
            response_body: safe_decode_body(Some(response_body)),
            response_code: response_status,
            ip_address: ctx.client_ip(),
            user_agent: ctx.header("user-agent").map(str::to_string),
        })
    }

    /// The task name and kwargs `process_logs.delay(log_data=…)` publishes.
    pub fn task(&self) -> (&'static str, &Self) {
        ("plane.bgtasks.logger_task.process_logs", self)
    }
}

/// `hmac.new(SECRET_KEY.encode(), api_key.encode(), sha256).hexdigest()`.
pub fn token_identifier(secret_key: &str, api_key: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret_key.as_bytes());
    let tag = hmac::sign(&key, api_key.as_bytes());
    tag.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `_safe_decode_body`.
pub fn safe_decode_body(content: Option<&[u8]>) -> Option<String> {
    let content = content?;
    if content.is_empty() {
        return None;
    }
    if content.starts_with(b"\x89PNG")
        || content.starts_with(b"\xff\xd8\xff")
        || content.starts_with(b"%PDF")
    {
        return Some("[Binary Content]".to_string());
    }
    match std::str::from_utf8(content) {
        Ok(text) => Some(text.to_string()),
        Err(_) => Some("[Could not decode content]".to_string()),
    }
}

/// `_redacted_headers`: `str({key: "[REDACTED]" if key.lower() in … else value
/// for key, value in request.headers.items()})`.
///
/// Django's `request.headers` iterates in `META` insertion order and exposes
/// lower-case names. `HeaderMap` does not preserve the wire order, so the text
/// is the same but the key order can differ from Django's. The column is only
/// written on `/api/v1/` requests, which cannot be registered yet (see the
/// module docs).
fn redacted_headers(ctx: &RequestContext) -> String {
    const SENSITIVE: [&str; 3] = ["x-api-key", "authorization", "cookie"];
    let mut out = String::from("{");
    for (index, (name, value)) in ctx.headers.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        let name = name.as_str().to_ascii_lowercase();
        let value = if SENSITIVE.contains(&name.as_str()) {
            "[REDACTED]"
        } else {
            value.to_str().unwrap_or_default()
        };
        out.push('\'');
        out.push_str(&name.replace('\\', "\\\\").replace('\'', "\\'"));
        out.push_str("': '");
        out.push_str(&value.replace('\\', "\\\\").replace('\'', "\\'"));
        out.push('\'');
    }
    out.push('}');
    out
}

/// Where the Phase 4 Celery publisher plugs in.
pub trait TokenLogSink: Send + Sync {
    /// Publish `plane.bgtasks.logger_task.process_logs(log_data=…)`.
    ///
    /// Python swallows every exception here (`logger.py:160-161`), so a failure
    /// must be logged and **not** turned into a response error.
    fn publish(&self, log: &ApiTokenLog) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, HeaderValue, Method};

    fn ctx(headers: &[(&str, &str)], body: Option<&[u8]>) -> RequestContext {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                http::HeaderName::from_bytes(name.as_bytes()).expect("name"),
                HeaderValue::from_str(value).expect("value"),
            );
        }
        let mut ctx = RequestContext::new(Method::POST, "/api/v1/x/", "a=1&b=2", map, None);
        ctx.body = body.map(<[u8]>::to_vec);
        ctx
    }

    #[test]
    fn safe_decode_matches_python() {
        assert_eq!(safe_decode_body(None), None);
        assert_eq!(safe_decode_body(Some(b"")), None);
        assert_eq!(
            safe_decode_body(Some(b"\x89PNG\r\n")),
            Some("[Binary Content]".to_string())
        );
        assert_eq!(
            safe_decode_body(Some(b"\xff\xd8\xff\xe0")),
            Some("[Binary Content]".to_string())
        );
        assert_eq!(
            safe_decode_body(Some(b"%PDF-1.7")),
            Some("[Binary Content]".to_string())
        );
        assert_eq!(safe_decode_body(Some(b"hello")), Some("hello".to_string()));
        assert_eq!(
            safe_decode_body(Some(&[0xff, 0xfe])),
            Some("[Could not decode content]".to_string())
        );
    }

    #[test]
    fn only_requests_with_an_api_key_are_logged() {
        assert!(
            ApiTokenLog::build(
                &plane_core::config::Settings::from_source(
                    &plane_core::config::EnvMap::new().set("REDIS_URL", "redis://127.0.0.1:6379/0"),
                    plane_core::config::Role::Api
                )
                .expect("settings"),
                &ctx(&[("host", "x")], None),
                200,
                b"{}"
            )
            .is_none()
        );
    }

    #[test]
    fn the_row_matches_the_contract() {
        let settings = plane_core::config::Settings::from_source(
            &plane_core::config::EnvMap::new()
                .set("REDIS_URL", "redis://127.0.0.1:6379/0")
                .set("SECRET_KEY", "secret"),
            plane_core::config::Role::Api,
        )
        .expect("settings");
        let request = ctx(
            &[
                ("host", "api.example.com"),
                ("x-api-key", "token"),
                ("origin", "https://web.example.com"),
                ("authorization", "Bearer x"),
                ("user-agent", "test-agent"),
                ("x-forwarded-for", "203.0.113.7, 10.0.0.1"),
            ],
            Some(b"{\"a\":1}"),
        );
        let log = ApiTokenLog::build(&settings, &request, 201, b"{\"ok\":true}").expect("log");
        assert_eq!(
            log.token_identifier,
            token_identifier("secret", "token"),
            "the identifier is a keyed HMAC, not a bare hash"
        );
        assert_eq!(log.path, "/api/v1/x/");
        assert_eq!(log.method, "POST");
        assert_eq!(log.query_params, "a=1&b=2");
        assert_eq!(log.body.as_deref(), Some("{\"a\":1}"));
        assert_eq!(log.response_body.as_deref(), Some("{\"ok\":true}"));
        assert_eq!(log.response_code, 201);
        assert_eq!(log.ip_address.as_deref(), Some("203.0.113.7"));
        assert_eq!(log.user_agent.as_deref(), Some("test-agent"));
        assert!(log.headers.contains("'x-api-key': '[REDACTED]'"));
        assert!(log.headers.contains("'authorization': '[REDACTED]'"));
        assert!(log.headers.contains("'host': 'api.example.com'"));
        assert!(!log.headers.contains("token"));
        assert_eq!(log.task().0, "plane.bgtasks.logger_task.process_logs");
    }
}
