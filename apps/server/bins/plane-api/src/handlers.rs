// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two routes Phase 1 serves, byte-identical to Django.
//!
//! `apps/api/plane/web/views.py` (whole file):
//!
//! ```python
//! def health_check(request):
//!     return JsonResponse({"status": "OK"})
//!
//! def robots_txt(request):
//!     return HttpResponse("User-agent: *\nDisallow: /", content_type="text/plain")
//! ```
//!
//! Both are plain function views: **every** method is accepted and returns the
//! same body (HTTP_CONTRACT §11.5), and `HEAD` keeps the `Content-Length` of the
//! body it would have sent.

use plane_core::error::{ERROR_PAGE_500, TEXT_HTML, TEXT_PLAIN};

use crate::response::Resp;

/// `GET /` — `plane/web/views.py:8-9`, rendered by `JsonResponse` (Django's
/// dialect: `{"status": "OK"}`, 16 bytes).
pub fn health() -> Resp {
    Resp::django_json(200, &serde_json::json!({"status": "OK"})).unwrap_or_else(|error| {
        tracing::error!(target: "plane.api", %error, "failed to render the health response");
        server_error()
    })
}

/// `/robots.txt` — `plane/web/views.py:12-13`. Content type is exactly
/// `text/plain` (no charset), body 25 bytes.
pub fn robots() -> Resp {
    Resp::new(200, TEXT_PLAIN, b"User-agent: *\nDisallow: /".to_vec())
}

/// Django's default 500 page (`django/views/defaults.py:82-99`), used when even
/// an error body cannot be rendered.
pub fn server_error() -> Resp {
    Resp::new(500, TEXT_HTML, ERROR_PAGE_500.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_is_sixteen_bytes_of_django_json() {
        let resp = health();
        assert_eq!(resp.status.as_u16(), 200);
        assert_eq!(resp.content_type, Some("application/json"));
        assert_eq!(resp.body, b"{\"status\": \"OK\"}");
        assert_eq!(resp.body.len(), 16);
    }

    #[test]
    fn robots_is_twenty_five_bytes_of_text() {
        let resp = robots();
        assert_eq!(resp.status.as_u16(), 200);
        assert_eq!(resp.content_type, Some("text/plain"));
        assert_eq!(resp.body, b"User-agent: *\nDisallow: /");
        assert_eq!(resp.body.len(), 25);
    }
}
