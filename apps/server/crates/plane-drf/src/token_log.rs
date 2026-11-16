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
//! | `headers` | Python `dict.__repr__` of `request.headers`, with `x-api-key`, `authorization`, `cookie` replaced by `[REDACTED]` |
//! | `body` / `response_body` | `_safe_decode_body`, below |
//! | `response_code` | the status |
//! | `ip_address` | `get_client_ip`: the first `X-Forwarded-For` entry, else `REMOTE_ADDR` (it ignores `NUM_PROXIES`) |
//! | `user_agent` | `User-Agent`, `NULL` when absent |
//!
//! `_safe_decode_body` returns `None` for no body, `"[Binary Content]"` for a
//! PNG/JPEG/PDF signature, the UTF-8 text otherwise and
//! `"[Could not decode content]"` on a decode error.
//!
//! ## Where the row goes, and what a failure does
//!
//! `process_logs.delay(log_data=log_data)` (`logger.py:158`) calls Celery's
//! `apply_async`, whose `task_publish_retry` policy retries three times and then
//! raises `OperationalError` when the broker is unreachable
//! (`celery/app/task.py:897-989`, TASKS.md §2.6). **That exception never
//! reaches the client**: the call sits inside
//! `try: … except Exception as e: log_exception(e)` (`logger.py:138,160-161`),
//! which logs to `plane.exception` and returns the response untouched. So a
//! broker outage loses the log row (the same as Django) and the request still
//! succeeds. [`crate::registry::Registry`] therefore registers `/api/v1/` views
//! normally: the publisher is part of the process and this layer swallows its
//! failures.
//!
//! The middleware runs for *every* mount, not just `/api/v1/`: a request with
//! `X-Api-Key` to `/api/`, `/api/public/` or `/api/instances/` also produces a
//! row in Django. The pipeline's layer 11 calls [`ApiTokenLog::build`] for every
//! request that has the header, exactly as `__call__` does.
//!
//! ## Rendering `headers` byte for byte
//!
//! `str(redacted)` is a Python dict literal, so three Django details decide the
//! bytes:
//!
//! 1. `request.headers` is a `CaseInsensitiveMapping` over `META` in insertion
//!    order (`django/http/request.py:509-532`), i.e. the order the ASGI server
//!    received the headers;
//! 2. names are `header.replace("_", "-").title()` — `"HTTP_X_API_KEY"` becomes
//!    `"X-Api-Key"`, not `"x-api-key"`;
//! 3. an ASGI header name containing `_` is dropped before it reaches `META`
//!    (`django/core/handlers/asgi.py:89-105`), and a repeated name is joined
//!    with `","` at its first position.
//!
//! The Celery message is compared byte for byte by the differential harness,
//! including Celery's `kwargsrepr` of the `log_data` dict, so all three matter.

use aws_lc_rs::hmac;
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_jobs::celery::Message;
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::process_logs;

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

    /// The `log_data` argument `process_logs.delay(log_data=…)` sends.
    ///
    /// A Python `dict`, so the key order is the literal's (and therefore the
    /// order Celery's `kwargsrepr` renders); `None` stays `None`, and
    /// `response_code` stays an `int` (`APIActivityLog.response_code` is an
    /// `IntegerField`).
    #[must_use]
    pub fn task_arg(&self) -> TaskArg {
        let optional = |value: Option<&str>| match value {
            Some(value) => TaskArg::str(value),
            None => TaskArg::Null,
        };
        TaskArg::map([
            ("token_identifier", TaskArg::str(&self.token_identifier)),
            ("path", TaskArg::str(&self.path)),
            ("method", TaskArg::str(&self.method)),
            ("query_params", TaskArg::str(&self.query_params)),
            ("headers", TaskArg::str(&self.headers)),
            ("body", optional(self.body.as_deref())),
            ("response_body", optional(self.response_body.as_deref())),
            ("response_code", TaskArg::Int(i64::from(self.response_code))),
            ("ip_address", optional(self.ip_address.as_deref())),
            ("user_agent", optional(self.user_agent.as_deref())),
        ])
    }

    /// The Celery message `process_logs.delay(log_data=…)` publishes
    /// (`plane/bgtasks/logger_task.py:26-42`).
    #[must_use]
    pub fn message(&self) -> Message {
        process_logs(self.task_arg())
    }
}

/// `hmac.new(SECRET_KEY.encode(), api_key.encode(), sha256).hexdigest()`.
pub fn token_identifier(secret_key: &str, api_key: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret_key.as_bytes());
    let tag = hmac::sign(&key, api_key.as_bytes());
    let mut out = String::with_capacity(tag.as_ref().len() * 2);
    for byte in tag.as_ref() {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
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
fn redacted_headers(ctx: &RequestContext) -> String {
    const SENSITIVE: [&str; 3] = ["x-api-key", "authorization", "cookie"];

    // `request.headers` order: META (wire) order, underscore-named headers
    // dropped, repeats joined with "," at the first position.
    let mut entries: Vec<(String, String)> = Vec::new();
    for (name, value) in ctx.headers.iter() {
        let name = name.as_str();
        if name.contains('_') {
            continue;
        }
        let value = latin1(value.as_bytes());
        match entries
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(name))
        {
            Some((_, joined)) => {
                joined.push(',');
                joined.push_str(&value);
            }
            None => entries.push((py_title(name), value)),
        }
    }

    let mut out = String::with_capacity(2 + entries.len() * 32);
    out.push('{');
    for (index, (name, value)) in entries.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        py_string_repr(&mut out, name);
        out.push_str(": ");
        if SENSITIVE.contains(&name.to_ascii_lowercase().as_str()) {
            py_string_repr(&mut out, "[REDACTED]");
        } else {
            py_string_repr(&mut out, value);
        }
    }
    out.push('}');
    out
}

/// ASGI decodes header bytes as latin-1 (`django/core/handlers/asgi.py:97`), so
/// a UTF-8 `é` reaches the log as `Ã©`.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| *byte as char).collect()
}

/// Python's `str.title()` on an ASCII header name. `str.title()` uppercases a
/// letter that follows a non-letter, so `x-2fa` becomes `X-2Fa`.
fn py_title(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut previous_cased = false;
    for character in value.chars() {
        if character.is_alphabetic() {
            if previous_cased {
                out.extend(character.to_lowercase());
            } else {
                out.extend(character.to_uppercase());
            }
            previous_cased = true;
        } else {
            out.push(character);
            previous_cased = false;
        }
    }
    out
}

/// CPython's `repr(str)`: single quotes unless the value contains `'` and no
/// `"`, `\\`, `\n`, `\r`, `\t` escapes, `\xXX` for non-printables.
fn py_string_repr(out: &mut String, value: &str) {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    out.push(quote);
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character == quote => {
                out.push('\\');
                out.push(character);
            }
            character if character < ' ' || ('\u{7f}'..='\u{a0}').contains(&character) => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\x{:02x}", character as u32);
            }
            character => out.push(character),
        }
    }
    out.push(quote);
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

    fn settings(secret: &str) -> Settings {
        Settings::from_source(
            &plane_core::config::EnvMap::new()
                .set("REDIS_URL", "redis://127.0.0.1:6379/0")
                .set("SECRET_KEY", secret),
            plane_core::config::Role::Api,
        )
        .expect("settings")
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
                &settings("secret"),
                &ctx(&[("host", "x")], None),
                200,
                b"{}"
            )
            .is_none()
        );
        // An empty header is falsy in Python (`if not api_key: return`).
        assert!(
            ApiTokenLog::build(
                &settings("secret"),
                &ctx(&[("x-api-key", "")], None),
                200,
                b"{}"
            )
            .is_none()
        );
    }

    #[test]
    fn the_row_matches_the_contract() {
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
        let log =
            ApiTokenLog::build(&settings("secret"), &request, 201, b"{\"ok\":true}").expect("log");
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
        assert!(log.headers.contains("'X-Api-Key': '[REDACTED]'"));
        assert!(log.headers.contains("'Authorization': '[REDACTED]'"));
        assert!(log.headers.contains("'Host': 'api.example.com'"));
        assert!(!log.headers.contains("token"));
    }

    /// `str({...})` with Django's title-cased names, wire order, `", "`
    /// separators and single quotes.
    #[test]
    fn the_headers_column_is_python_dict_repr() {
        let request = ctx(
            &[
                ("host", "127.0.0.1:18100"),
                ("accept-encoding", "gzip"),
                ("cookie", "session-id=abc"),
                ("x-api-key", "secret"),
                ("x-forwarded-for", "203.0.113.7"),
            ],
            None,
        );
        let log = ApiTokenLog::build(&settings("s"), &request, 200, b"").expect("log");
        assert_eq!(
            log.headers,
            "{'Host': '127.0.0.1:18100', 'Accept-Encoding': 'gzip', \
             'Cookie': '[REDACTED]', 'X-Api-Key': '[REDACTED]', \
             'X-Forwarded-For': '203.0.113.7'}"
        );
    }

    #[test]
    fn header_names_are_title_cased_like_django() {
        assert_eq!(py_title("x-api-key"), "X-Api-Key");
        assert_eq!(py_title("accept-encoding"), "Accept-Encoding");
        assert_eq!(py_title("x-2fa"), "X-2Fa");
        assert_eq!(py_title(""), "");
    }

    #[test]
    fn python_repr_quoting_follows_cpython() {
        let mut out = String::new();
        py_string_repr(&mut out, "plain");
        assert_eq!(out, "'plain'");
        let mut out = String::new();
        py_string_repr(&mut out, "it's");
        assert_eq!(out, "\"it's\"");
        // A value with `'` and no `"` uses double quotes (CPython's choice).
        let mut out = String::new();
        py_string_repr(&mut out, "a\\b'c");
        assert_eq!(out, "\"a\\\\b'c\"");
        // Both quote kinds: single quotes, the `'` escaped.
        let mut out = String::new();
        py_string_repr(&mut out, "a\\b'c\"");
        assert_eq!(out, "'a\\\\b\\'c\"'");
        let mut out = String::new();
        py_string_repr(&mut out, "tab\there");
        assert_eq!(out, "'tab\\there'");
        let mut out = String::new();
        py_string_repr(&mut out, "\u{1}");
        assert_eq!(out, "'\\x01'");
    }

    /// The `log_data` dict, in the literal's order, with `None` and `int`
    /// preserved.
    #[test]
    fn the_log_data_argument_is_a_dict_in_call_site_order() {
        let request = ctx(&[("x-api-key", "token")], None);
        let log = ApiTokenLog::build(&settings("secret"), &request, 404, b"").expect("log");
        let TaskArg::Map(entries) = log.task_arg() else {
            panic!("log_data must be a dict");
        };
        let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "token_identifier",
                "path",
                "method",
                "query_params",
                "headers",
                "body",
                "response_body",
                "response_code",
                "ip_address",
                "user_agent",
            ]
        );
        assert_eq!(entries[5].1, TaskArg::Null, "no request body is None");
        assert_eq!(entries[7].1, TaskArg::Int(404));
        assert_eq!(
            log.message().task(),
            "plane.bgtasks.logger_task.process_logs"
        );
        assert_eq!(log.message().kwargs().len(), 1);
        assert_eq!(log.message().kwargs()[0].0, "log_data");
    }
}
