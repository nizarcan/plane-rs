// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A response under construction, before the middleware response phases run.
//!
//! Django's `HttpResponse` has a status, case-insensitive headers and a body,
//! with `Content-Type` set at construction time. Keeping the body as bytes
//! (never a JSON tree) is what lets the middleware pipeline reproduce
//! `Content-Length` and gzip decisions: those depend on the *rendered* size.

use axum::body::Body;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::Response;
use plane_core::error::APPLICATION_JSON;
use plane_serde::Dialect;

/// A response produced by a middleware or a view.
#[derive(Debug, Clone)]
pub struct Resp {
    pub status: StatusCode,
    pub headers: HeaderMap,
    /// `Content-Type`, when the response has one. Django deletes it for an empty
    /// DRF body (`rest_framework/response.py:82-84`); nothing emits that today.
    pub content_type: Option<&'static str>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            headers: HeaderMap::new(),
            content_type: Some(content_type),
            body,
        }
    }

    pub fn empty(status: u16, content_type: &'static str) -> Self {
        Self::new(status, content_type, Vec::new())
    }

    /// Render a JSON value with one of Plane's dialects (DECISIONS.md D2).
    pub fn json(
        status: u16,
        content_type: &'static str,
        dialect: Dialect,
        value: &impl serde::Serialize,
    ) -> Result<Self, plane_serde::Error> {
        Ok(Self::new(
            status,
            content_type,
            plane_serde::to_vec(dialect, value)?,
        ))
    }

    /// `django.http.JsonResponse`.
    pub fn django_json(
        status: u16,
        value: &impl serde::Serialize,
    ) -> Result<Self, plane_serde::Error> {
        Self::json(status, APPLICATION_JSON, Dialect::Django, value)
    }

    /// A DRF response from the view runtime.
    ///
    /// A `None` content type deletes the header, which is what DRF does for an
    /// empty rendered body (`rest_framework/response.py:82-84`).
    pub fn from_drf(response: plane_drf::view::DrfResponse) -> Self {
        let mut resp = Self::empty(response.status, APPLICATION_JSON);
        resp.content_type = response.content_type;
        resp.body = response.body;
        for (name, value) in response.headers {
            let Ok(name) = axum::http::HeaderName::from_bytes(name.as_bytes()) else {
                continue;
            };
            let Ok(value) = HeaderValue::from_str(&value) else {
                continue;
            };
            resp.headers.append(name, value);
        }
        resp
    }

    /// A rendered [`ErrorBody`] (the error type decides its dialect).
    pub fn from_error_response(response: plane_core::error::ErrorResponse) -> Self {
        let mut resp = Self::new(response.status, response.content_type, response.body);
        for (name, value) in response.headers {
            if let (Ok(name), Ok(value)) = (
                axum::http::HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(&value),
            ) {
                resp.headers.insert(name, value);
            }
        }
        resp
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    pub fn set_header(&mut self, name: &'static str, value: impl Into<String>) {
        let value = value.into();
        if let (Ok(name), Ok(value)) = (
            axum::http::HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            self.headers.insert(name, value);
        }
    }

    /// `django.utils.cache.patch_vary_headers`: append to `Vary` unless the
    /// value is already present (case-insensitively), preserving order and
    /// collapsing to `*` when an asterisk is present.
    pub fn patch_vary(&mut self, value: &'static str) {
        let existing = self.header("vary").unwrap_or_default().to_string();
        let mut values: Vec<String> = existing
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
        if values.iter().any(|part| part.eq_ignore_ascii_case(value)) {
            return;
        }
        values.push(value.to_string());
        let joined = if values.iter().any(|part| part == "*") {
            "*".to_string()
        } else {
            values.join(", ")
        };
        self.set_header("Vary", joined);
    }

    /// `CommonMiddleware.process_response`: set `Content-Length` for a
    /// non-streaming response that does not have one.
    pub fn set_content_length(&mut self) {
        if self.headers.contains_key(CONTENT_LENGTH) {
            return;
        }
        self.set_header("Content-Length", self.body.len().to_string());
    }

    /// Append a header value without replacing existing ones. `Set-Cookie` is
    /// the reason this exists: Django emits one header line per cookie, in the
    /// order the cookies were set (response phase 5 before 3, so `csrftoken`
    /// precedes `session-id`).
    pub fn append_header(&mut self, name: &'static str, value: impl AsRef<str>) {
        if let (Ok(name), Ok(value)) = (
            axum::http::HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value.as_ref()),
        ) {
            self.headers.append(name, value);
        }
    }

    /// Turn the response into an axum response.
    ///
    /// Two details matter for byte-compatibility with uvicorn:
    ///
    /// * `HEAD` keeps the `Content-Length` of the body it would have sent but
    ///   sends no body, which is what the ASGI server does;
    /// * a response **without** a `Content-Length` must not get one from hyper
    ///   either. Django's `APPEND_SLASH` 301 and its `ALLOWED_HOSTS` 400 have no
    ///   `Content-Length`; uvicorn frames them with `Transfer-Encoding: chunked`.
    ///   A sized empty body would make hyper add `Content-Length: 0`, so those
    ///   responses use an unknown-size body instead.
    pub fn into_response(self, method: &Method) -> Response {
        let has_content_length = self.headers.contains_key(CONTENT_LENGTH);
        let mut builder = Response::builder().status(self.status);
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        if let Some(content_type) = self.content_type {
            builder = builder.header(CONTENT_TYPE, content_type);
        }
        let body = if *method == Method::HEAD {
            if has_content_length {
                Body::empty()
            } else {
                // A bodiless HEAD response with no `Content-Length`. hyper sends
                // neither `Content-Length` nor `Transfer-Encoding` here, while
                // uvicorn sends `Transfer-Encoding: chunked`; the difference is
                // transport framing only (see the report/PROGRESS).
                chunked_body(Vec::new())
            }
        } else if has_content_length {
            Body::from(self.body)
        } else {
            chunked_body(self.body)
        };
        builder.body(body).unwrap_or_else(|error| {
            tracing::error!(target: "plane.api", %error, "failed to build an HTTP response");
            let mut response = Response::new(Body::from(
                plane_core::error::ERROR_PAGE_500.as_bytes().to_vec(),
            ));
            *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            response
        })
    }
}

/// A body with no known size, so hyper uses `Transfer-Encoding: chunked`
/// instead of inventing a `Content-Length`.
fn chunked_body(bytes: Vec<u8>) -> Body {
    Body::from_stream(futures_util::stream::once(async move {
        Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(bytes))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vary_accumulates_in_order() {
        let mut resp = Resp::empty(200, "application/json");
        resp.patch_vary("Cookie");
        assert_eq!(resp.header("vary"), Some("Cookie"));
        resp.patch_vary("origin");
        assert_eq!(resp.header("vary"), Some("Cookie, origin"));
        // Case-insensitive duplicate is ignored.
        resp.patch_vary("cookie");
        assert_eq!(resp.header("vary"), Some("Cookie, origin"));
        resp.patch_vary("Accept-Encoding");
        assert_eq!(resp.header("vary"), Some("Cookie, origin, Accept-Encoding"));
    }

    #[test]
    fn vary_joins_an_existing_header() {
        let mut resp = Resp::empty(200, "application/json");
        resp.set_header("Vary", "Accept-Encoding");
        resp.patch_vary("Cookie");
        assert_eq!(resp.header("vary"), Some("Accept-Encoding, Cookie"));
    }

    #[test]
    fn vary_asterisk_wins() {
        let mut resp = Resp::empty(200, "application/json");
        resp.set_header("Vary", "*");
        resp.patch_vary("Cookie");
        assert_eq!(resp.header("vary"), Some("*"));
    }

    #[test]
    fn content_length_is_only_set_once() {
        let mut resp = Resp::new(200, "application/json", b"hello".to_vec());
        resp.set_content_length();
        assert_eq!(resp.header("content-length"), Some("5"));
        resp.set_header("Content-Length", "99");
        resp.set_content_length();
        assert_eq!(resp.header("content-length"), Some("99"));
    }
}
