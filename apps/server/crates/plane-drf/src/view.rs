// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! What a handler receives and returns.

use std::future::Future;
use std::pin::Pin;

use chrono_tz::Tz;
use http::{HeaderMap, Method};
use plane_auth::{RequestUser, SessionSlot};
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_core::error::{APPLICATION_JSON, TEXT_HTML};
use plane_db::DbPools;
use plane_serde::Dialect;

use crate::db::Conn;
use crate::error::ViewError;
use crate::parsers::Data;
use crate::policy::MethodPolicy;
use crate::request::{QueryDict, ViewRequest};

/// A handler's future. Boxed because the registry stores `fn` pointers.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The handler signature.
///
/// ```ignore
/// use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
/// use plane_drf::error::ViewError;
///
/// pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
///     Box::pin(async move {
///         let project_id = ctx.kwarg("project_id").unwrap_or_default();
///         Ok(ViewResponse::json(200, serde_json::json!({ "project": project_id })))
///     })
/// }
/// ```
pub type HandlerFn = for<'a> fn(&'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>>;

/// The request as a handler sees it.
///
/// Everything is borrowed: the dispatch loop owns the `ViewRequest`, the
/// resolved user, the timezone and the (lazy) connection, and a handler cannot
/// outlive them. There is deliberately no "authenticated" or "permission
/// passed" flag here — a handler is only ever called after the runtime ran
/// authentication, the permission classes, the throttles, the family `initial`
/// hooks and `allow_permission`, so the presence of a `ViewCtx` *is* the proof.
pub struct ViewCtx<'a> {
    pub request: &'a ViewRequest<'a>,
    /// `request.user`, after DRF's authenticators.
    pub user: &'a RequestUser,
    /// `request.session`: the same store the middleware built at layer 3 and
    /// will save at response time. A handler that signs a user out calls
    /// [`SessionSlot::flush`]; the middleware then deletes the cookie and adds
    /// `Vary: Cookie`, exactly as Django's `logout()` + `SessionMiddleware`
    /// do.
    ///
    /// Shared (`&SessionSlot`, not `&mut SessionStore`) because the response
    /// phase needs it after the handler returns and the dispatcher's future
    /// stays `Send`.
    pub session: &'a SessionSlot,
    /// `view.action` for a ViewSet, `None` for an APIView.
    pub action: Option<&'a str>,
    /// The dotted view path (`routes.json`).
    pub view: &'a str,
    /// `TimezoneMixin.initial`: the user's timezone, when activated.
    pub timezone: Option<&'a Tz>,
    /// The read connection the runtime already checked out, when it needed one.
    pub db: Option<&'a deadpool_postgres::Client>,
    pub pools: &'a DbPools,
    pub settings: &'a Settings,
    /// The raw `RequestContext` (path, headers, body, `REMOTE_ADDR`).
    pub ctx: &'a RequestContext,
    /// `django.core.cache.cache`: the process-wide django-redis handle, built
    /// once from `Settings` and shared with the throttles (both use the same
    /// cache alias in Django). The connection itself is opened on first use.
    pub redis: Option<&'a plane_cache::Redis>,
    /// The Celery publisher the process was started with (DECISIONS.md D7).
    /// One per process; publishing is what connects it, so the first enqueue
    /// pays the AMQP handshake and later ones reuse the channel.
    pub publisher: Option<&'a plane_jobs::celery::CeleryPublisher>,
}

impl<'a> ViewCtx<'a> {
    /// `request.session`, for a handler that signs the caller out or reads the
    /// store directly. Most handlers only need the middleware's effects.
    pub fn session(&self) -> &'a SessionSlot {
        self.session
    }

    /// `django.core.cache.cache`, as the Django client (`:1:` keys, the raw-int
    /// value rule, protocol-4 pickles).
    ///
    /// `None` only in a process built without Redis (tests); a handler that
    /// needs the cache should go through `plane_http_app::common::cache`,
    /// which turns that into the 500 Django would produce.
    pub fn cache(&self) -> Option<plane_cache::Client<'a>> {
        self.redis.map(plane_cache::Client::new)
    }

    /// The Celery publisher, or `None` in a build/test without a broker.
    pub fn celery(&self) -> Option<&'a plane_jobs::celery::CeleryPublisher> {
        self.publisher
    }

    pub fn method(&self) -> &Method {
        &self.request.method
    }

    pub fn path(&self) -> &str {
        // Django's `request.path`; in 5.2 it equals `path_info` (ASGI decodes
        // the path once), and the view API uses the `request.path` spelling.
        &self.request.ctx.path
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.request.ctx.headers
    }

    pub fn query(&self) -> &QueryDict {
        &self.request.query
    }

    /// `view.kwargs.get(name)`.
    pub fn kwarg(&self, name: &str) -> Option<&'a str> {
        self.request.kwarg(name)
    }

    /// `view.fields` (`?fields=a,b`).
    pub fn fields(&self) -> Option<Vec<&'a str>> {
        self.request.fields()
    }

    /// `view.expand`.
    pub fn expand(&self) -> Option<Vec<&'a str>> {
        self.request.expand()
    }

    /// `request.data`, parsed by DRF's parsers on first access.
    pub fn data(&self) -> Result<&Data, ViewError> {
        self.request.data()
    }

    /// The `MethodPolicy` for this request.
    pub fn policy(&self) -> Option<&'a MethodPolicy> {
        self.request.method_policy()
    }
}

/// A DRF `Response` before rendering.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewResponse {
    /// `Response(data, status=..)`, rendered by `JSONRenderer`.
    Json {
        status: u16,
        data: Option<serde_json::Value>,
        headers: Vec<(String, String)>,
    },
    /// A non-DRF response body (`HttpResponse`, `HttpResponseRedirect`, a CSV
    /// attachment). `content_type` is exactly what the view set.
    Raw {
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
        headers: Vec<(String, String)>,
        /// `HttpResponseBase.streaming` (`django/http/response.py:453`): the body
        /// is a `StreamingHttpResponse`'s stream, so it has **no**
        /// `Content-Length` and `GZipMiddleware` compresses it whatever its
        /// length (`django/middleware/gzip.py:27-30,52-56`). The handler still
        /// supplies the whole body at once because the Rust pipeline renders it
        /// into one buffer; only the framing differs.
        streaming: bool,
    },
}

impl ViewResponse {
    pub fn json(status: u16, data: serde_json::Value) -> Self {
        Self::Json {
            status,
            data: Some(data),
            headers: Vec::new(),
        }
    }

    pub fn empty(status: u16) -> Self {
        Self::Json {
            status,
            data: None,
            headers: Vec::new(),
        }
    }

    pub fn raw(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self::Raw {
            status,
            content_type,
            body,
            headers: Vec::new(),
            streaming: false,
        }
    }

    /// `StreamingHttpResponse(body, content_type=…)`: the same bytes, framed
    /// without a `Content-Length`.
    pub fn streaming_raw(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self::Raw {
            status,
            content_type,
            body,
            headers: Vec::new(),
            streaming: true,
        }
    }

    /// `HttpResponseBase.streaming`; always false for a JSON response.
    pub fn is_streaming(&self) -> bool {
        match self {
            Self::Json { .. } => false,
            Self::Raw { streaming, .. } => *streaming,
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::Json { status, .. } | Self::Raw { status, .. } => *status,
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let entry = (name.into(), value.into());
        match &mut self {
            Self::Json { headers, .. } | Self::Raw { headers, .. } => headers.push(entry),
        }
        self
    }

    /// Append a list of headers, preserving their order.
    pub fn with_headers(mut self, values: Vec<(String, String)>) -> Self {
        match &mut self {
            Self::Json { headers, .. } | Self::Raw { headers, .. } => {
                headers.extend(values);
            }
        }
        self
    }
}

/// A rendered response, ready for the middleware pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrfResponse {
    pub status: u16,
    /// Extra headers (`Allow`, `X-RateLimit-*`, `Retry-After`, …), in the order
    /// the view set them. Multi-valued headers are repeated.
    pub headers: Vec<(String, String)>,
    /// `Content-Type`; `None` deletes it, as DRF does for an empty body
    /// (`rest_framework/response.py:82-84`).
    pub content_type: Option<&'static str>,
    pub body: Vec<u8>,
    /// `HttpResponseBase.streaming`; carried through rendering so the pipeline
    /// can frame the body without a `Content-Length` (`bins/plane-api`'s
    /// `Resp`).
    pub streaming: bool,
}

impl DrfResponse {
    pub fn new(status: u16, content_type: Option<&'static str>, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            content_type,
            body,
            streaming: false,
        }
    }

    pub fn json(status: u16, data: &serde_json::Value) -> Result<Self, plane_serde::Error> {
        Ok(Self::new(
            status,
            Some(APPLICATION_JSON),
            plane_serde::to_vec(Dialect::Drf, data)?,
        ))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn set_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        if let Some(slot) = self
            .headers
            .iter_mut()
            .find(|(header, _)| header.eq_ignore_ascii_case(&name))
        {
            slot.1 = value;
        } else {
            self.headers.push((name, value));
        }
    }

    pub fn append_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.headers.push((name.into(), value.into()));
    }

    /// `django.utils.cache.patch_vary_headers`: append unless present, keep
    /// order, collapse to `*` when an asterisk appears.
    pub fn patch_vary(&mut self, value: &str) {
        let existing = self.header("Vary").unwrap_or_default().to_string();
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

    /// Render a `ViewResponse` exactly as DRF's `JSONRenderer` does.
    pub fn render(response: ViewResponse) -> Result<Self, plane_serde::Error> {
        match response {
            ViewResponse::Json {
                status,
                data,
                headers,
            } => {
                let (content_type, body) = match data {
                    // `JSONRenderer.render(None)` returns `b""`, and
                    // `Response.rendered_content` then deletes `Content-Type`.
                    None => (None, Vec::new()),
                    Some(data) => (
                        Some(APPLICATION_JSON),
                        plane_serde::to_vec(Dialect::Drf, &data)?,
                    ),
                };
                let mut rendered = Self::new(status, content_type, body);
                rendered.headers = headers;
                Ok(rendered)
            }
            ViewResponse::Raw {
                status,
                content_type,
                body,
                headers,
                streaming,
            } => {
                let mut rendered = Self::new(status, Some(content_type), body);
                rendered.headers = headers;
                rendered.streaming = streaming;
                Ok(rendered)
            }
        }
    }
}

/// `django.views.generic.base.View.http_method_not_allowed`'s
/// `HttpResponseNotAllowed`: text/html, empty body, `Allow`.
pub fn http_response_not_allowed(allow: &str) -> DrfResponse {
    let mut response = DrfResponse::new(405, Some(TEXT_HTML), Vec::new());
    response.set_header("Allow", allow.to_string());
    response
}

/// The connection handle a handler may keep for its own reads.
pub type HandlerConn<'a> = Conn<'a>;

#[cfg(test)]
mod tests {
    use super::*;

    /// `StreamingHttpResponse` (`django/http/response.py:444-459`): the flag
    /// has to survive rendering so the pipeline frames the body without a
    /// `Content-Length` and `GZipMiddleware` compresses it at any size.
    #[test]
    fn a_streaming_raw_response_stays_streaming_after_rendering() {
        let response = ViewResponse::streaming_raw(200, "application/octet-stream", b"x".to_vec());
        assert!(response.is_streaming());
        let rendered = DrfResponse::render(response).expect("render");
        assert!(rendered.streaming);
        assert_eq!(rendered.content_type, Some("application/octet-stream"));
        assert_eq!(rendered.body, b"x");

        let plain = ViewResponse::raw(200, APPLICATION_JSON, b"{}".to_vec());
        assert!(!plain.is_streaming());
        assert!(!DrfResponse::render(plain).expect("render").streaming);
        assert!(!ViewResponse::json(200, serde_json::json!({})).is_streaming());
    }
}
