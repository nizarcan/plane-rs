// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The DRF-compatible error type.
//!
//! Every variant maps to something `apps/api` can produce: DRF's exception set
//! as `plane/authentication/adapter/exception.py` and DRF's base handler render
//! it (HTTP_CONTRACT §10.6), Django's own error responses (§11), Plane's
//! request-body limit (§9.1), and our own explicit "route not ported yet" 501.
//!
//! Bodies are kept as [`ErrorBody`] values rather than bytes: DRF data must be
//! rendered with the DRF JSON dialect and `JsonResponse` data with Django's
//! (DECISIONS.md D2), and the renderer lives in `plane-serde` so both backends
//! use one formatter.

use serde_json::Value;

use crate::json::Object;

/// The default `PermissionDenied` detail DRF uses (`exceptions.py:41`).
pub const PERMISSION_DENIED_DETAIL: &str = "You do not have permission to perform this action.";
/// `NotAuthenticated.default_detail`.
pub const NOT_AUTHENTICATED_DETAIL: &str = "Authentication credentials were not provided.";
/// DRF `NotFound.default_detail` (and `filter_renderers`).
pub const NOT_FOUND_DETAIL: &str = "Not found.";
/// DRF `NotAcceptable.default_detail`.
pub const NOT_ACCEPTABLE_DETAIL: &str = "Could not satisfy the request Accept header.";
/// `plane/app/views/base.py:222-226` (also `plane/api/views/base.py:212-223`).
pub const OBJECT_DOES_NOT_EXIST_DETAIL: &str = "The required object does not exist.";
/// `plane/api/views/base.py:93-97`.
pub const RESOURCE_DOES_NOT_EXIST_DETAIL: &str = "The requested resource does not exist.";
/// Our explicit marker for a route Django serves that `plane-api` has not ported.
pub const NOT_IMPLEMENTED_DETAIL: &str = "Not implemented by plane-api.";

/// Which Django route surface produced an error, for the two catch-all wordings
/// that differ between `/api/` and `/api/v1/` (§11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSurface {
    /// `/api/`, `/api/public/`, `/api/instances/`.
    App,
    /// `/api/v1/`.
    V1,
    /// `/api/v1/` viewsets use the "requested resource" wording.
    V1ViewSet,
}

/// Django's default `ERROR_PAGE_TEMPLATE` (`django/views/defaults.py:16-26`)
/// with the 400 title. 143 bytes.
pub const ERROR_PAGE_400: &str = "\n<!doctype html>\n<html lang=\"en\">\n<head>\n  <title>Bad Request (400)</title>\n</head>\n<body>\n  <h1>Bad Request (400)</h1><p></p>\n</body>\n</html>\n";

/// The same template with the 403 title. 135 bytes.
///
/// (HTTP_CONTRACT.md §11.4 says 141; that count is wrong: "403 Forbidden" is
/// four characters shorter than "Bad Request (400)" and appears twice, so the
/// page is 143 - 8 = 135 bytes. Reported in the session notes.)
pub const ERROR_PAGE_403: &str = "\n<!doctype html>\n<html lang=\"en\">\n<head>\n  <title>403 Forbidden</title>\n</head>\n<body>\n  <h1>403 Forbidden</h1><p></p>\n</body>\n</html>\n";

/// The same template with the 500 title. 145 bytes.
pub const ERROR_PAGE_500: &str = "\n<!doctype html>\n<html lang=\"en\">\n<head>\n  <title>Server Error (500)</title>\n</head>\n<body>\n  <h1>Server Error (500)</h1><p></p>\n</body>\n</html>\n";

/// The `text/html; charset=utf-8` Django uses for error pages and redirects.
pub const TEXT_HTML: &str = "text/html; charset=utf-8";
/// The DRF renderer's content type: no charset, no space (§10.1).
pub const APPLICATION_JSON: &str = "application/json";
/// `HttpResponse`'s default content type (`text/plain` for `/robots.txt`).
pub const TEXT_PLAIN: &str = "text/plain";

/// An error the HTTP layer can turn into a response.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// 401, forced to 401 by Plane's handler even though DRF would use 403.
    NotAuthenticated,
    /// DRF `AuthenticationFailed`: 403 when the authenticator has no
    /// `authenticate_header`, 401 with `WWW-Authenticate` when it has one.
    AuthenticationFailed {
        detail: String,
        authenticate_header: Option<String>,
    },
    /// `PermissionDenied` with a custom detail (e.g. `CSRF Failed: …`).
    PermissionDenied(String),
    /// DRF `Http404` inside a view: `{"detail":"Not found."}`.
    NotFound,
    /// `/api/`'s `ObjectDoesNotExist` catch-all (§11.2).
    ObjectDoesNotExist,
    /// `/api/v1/`'s `ObjectDoesNotExist` catch-all (§11.2).
    ResourceDoesNotExist,
    /// `IntegrityError` in a Plane family `handle_exception`
    /// (`plane/app/views/base.py:210-214`): 400 `The payload is not valid`.
    InvalidPayload,
    /// Django's `ValidationError` in a Plane family `handle_exception`
    /// (`plane/app/views/base.py:216-220`): 400 `Please provide valid detail`.
    InvalidDetail,
    /// `KeyError` in a Plane family `handle_exception`
    /// (`plane/app/views/base.py:228-233`): 400 `The required key does not exist.`
    MissingKey,
    /// Any other exception in a Plane family `handle_exception`
    /// (`plane/app/views/base.py:235-239`): 500 with Plane's JSON body.
    FamilyServerError,
    /// DRF `MethodNotAllowed`, with the `Allow` header DRF copies onto every
    /// response (`views.py:157-165`).
    MethodNotAllowed { method: String, allow: String },
    /// DRF `NotAcceptable`.
    NotAcceptable,
    /// DRF `UnsupportedMediaType`, carrying the offending Content-Type.
    UnsupportedMediaType(String),
    /// DRF `ParseError` (JSON syntax errors and the "must be an object" rule).
    ParseError(String),
    /// DRF `ValidationError`: the body is the serializer's error structure.
    ValidationError(Value),
    /// DRF `Throttled`, after Plane replaces the body (§12.5).
    Throttled { retry_after_seconds: u64 },
    /// `DisallowedHost` from `CommonMiddleware.process_request` (§11.3).
    DisallowedHost,
    /// Any other `SuspiciousOperation` (§11.3).
    SuspiciousOperation,
    /// Plane's 413 (`plane/middleware/request_body_size.py:22-28`, §9.1).
    RequestBodyTooLarge,
    /// Django's default 500 page (`django/views/defaults.py:82-99`).
    InternalServerError,
    /// Unmatched path: `handler404` = `custom_404_view` (§11.1).
    PageNotFound,
    /// A route Django serves that `plane-api` has not ported (501, our own).
    NotImplemented,
}

impl ApiError {
    /// The default `PermissionDenied` detail.
    pub fn permission_denied() -> Self {
        Self::PermissionDenied(PERMISSION_DENIED_DETAIL.to_string())
    }

    /// `NotImplemented` for a route that exists in `routes.json`.
    pub fn not_implemented() -> Self {
        Self::NotImplemented
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::NotAuthenticated => 401,
            Self::AuthenticationFailed {
                authenticate_header,
                ..
            } => {
                if authenticate_header.is_some() {
                    401
                } else {
                    403
                }
            }
            Self::PermissionDenied(_) => 403,
            Self::NotFound | Self::ObjectDoesNotExist | Self::ResourceDoesNotExist => 404,
            Self::InvalidPayload | Self::InvalidDetail | Self::MissingKey => 400,
            Self::FamilyServerError => 500,
            Self::MethodNotAllowed { .. } => 405,
            Self::NotAcceptable => 406,
            Self::UnsupportedMediaType(_) => 415,
            Self::ParseError(_) | Self::ValidationError(_) => 400,
            Self::Throttled { .. } => 429,
            Self::DisallowedHost | Self::SuspiciousOperation => 400,
            Self::RequestBodyTooLarge => 413,
            Self::InternalServerError => 500,
            Self::PageNotFound => 404,
            Self::NotImplemented => 501,
        }
    }

    /// Extra response headers this error requires.
    pub fn headers(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::MethodNotAllowed { allow, .. } => vec![("Allow", allow.clone())],
            Self::Throttled {
                retry_after_seconds,
            } => vec![("Retry-After", retry_after_seconds.to_string())],
            Self::AuthenticationFailed {
                authenticate_header: Some(header),
                ..
            } => vec![("WWW-Authenticate", header.clone())],
            _ => Vec::new(),
        }
    }

    /// The response body, tagged with the JSON dialect that must render it.
    ///
    /// Multi-key objects use [`Object`] so key order matches Python's dict
    /// insertion order; a `serde_json::Value` would sort them (see
    /// `crate::json`).
    pub fn body(&self) -> ErrorBody {
        match self {
            Self::NotAuthenticated => ErrorBody::drf(detail(NOT_AUTHENTICATED_DETAIL)),
            Self::AuthenticationFailed { detail, .. } | Self::PermissionDenied(detail) => {
                ErrorBody::drf(detail_body(detail))
            }
            Self::NotFound => ErrorBody::drf(detail(NOT_FOUND_DETAIL)),
            Self::ObjectDoesNotExist => {
                ErrorBody::drf(Object::new().field("error", OBJECT_DOES_NOT_EXIST_DETAIL))
            }
            Self::ResourceDoesNotExist => {
                ErrorBody::drf(Object::new().field("error", RESOURCE_DOES_NOT_EXIST_DETAIL))
            }
            Self::InvalidPayload => {
                ErrorBody::drf(Object::new().field("error", "The payload is not valid"))
            }
            Self::InvalidDetail => {
                ErrorBody::drf(Object::new().field("error", "Please provide valid detail"))
            }
            Self::MissingKey => {
                ErrorBody::drf(Object::new().field("error", "The required key does not exist."))
            }
            Self::FamilyServerError => ErrorBody::drf(
                Object::new().field("error", "Something went wrong please try again later"),
            ),
            Self::MethodNotAllowed { method, .. } => {
                ErrorBody::drf(detail_body(&format!("Method \"{method}\" not allowed.")))
            }
            Self::NotAcceptable => ErrorBody::drf(detail(NOT_ACCEPTABLE_DETAIL)),
            Self::UnsupportedMediaType(content_type) => ErrorBody::drf(detail_body(&format!(
                "Unsupported media type \"{content_type}\" in request."
            ))),
            Self::ParseError(detail) => ErrorBody::drf(detail_body(detail)),
            // Serializer errors are a structure the caller builds; a bare
            // ValidationError("boom") is a JSON array, not an object.
            Self::ValidationError(value) => ErrorBody::DrfValue(value.clone()),
            Self::Throttled { .. } => ErrorBody::drf(
                Object::new()
                    .field("error_code", 5900)
                    .field("error_message", "RATE_LIMIT_EXCEEDED"),
            ),
            Self::DisallowedHost | Self::SuspiciousOperation => ErrorBody::Static {
                content_type: TEXT_HTML,
                body: ERROR_PAGE_400.as_bytes(),
            },
            Self::RequestBodyTooLarge => ErrorBody::django(
                Object::new()
                    .field("error", "REQUEST_BODY_TOO_LARGE")
                    .field(
                        "detail",
                        "The size of the request body exceeds the maximum allowed size.",
                    ),
            ),
            Self::InternalServerError => ErrorBody::Static {
                content_type: TEXT_HTML,
                body: ERROR_PAGE_500.as_bytes(),
            },
            Self::PageNotFound => {
                ErrorBody::django(Object::new().field("error", "Page not found."))
            }
            Self::NotImplemented => ErrorBody::drf(detail(NOT_IMPLEMENTED_DETAIL)),
        }
    }

    /// The content type of the rendered response.
    pub fn content_type(&self) -> &'static str {
        match self.body() {
            ErrorBody::Drf(_) | ErrorBody::DrfValue(_) | ErrorBody::Django(_) => APPLICATION_JSON,
            ErrorBody::Static { content_type, .. } => content_type,
        }
    }

    /// Render the error into the exact bytes and headers Django sends.
    ///
    /// DRF data uses [`Dialect::Drf`], `JsonResponse` data uses
    /// [`Dialect::Django`] (DECISIONS.md D2), and HTML error pages are static.
    pub fn render(&self) -> Result<ErrorResponse, plane_serde::Error> {
        let body = self.body();
        let bytes = match &body {
            ErrorBody::Drf(object) => plane_serde::to_vec(plane_serde::Dialect::Drf, object)?,
            ErrorBody::DrfValue(value) => plane_serde::to_vec(plane_serde::Dialect::Drf, value)?,
            ErrorBody::Django(object) => plane_serde::to_vec(plane_serde::Dialect::Django, object)?,
            ErrorBody::Static { body, .. } => body.to_vec(),
        };
        Ok(ErrorResponse {
            status: self.status(),
            headers: self.headers(),
            content_type: self.content_type(),
            body: bytes,
        })
    }

    pub fn surface(&self) -> Option<ErrorSurface> {
        match self {
            Self::ObjectDoesNotExist => Some(ErrorSurface::App),
            Self::ResourceDoesNotExist => Some(ErrorSurface::V1),
            _ => None,
        }
    }
}

/// A fully rendered error response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorResponse {
    pub status: u16,
    /// Extra headers the error requires (`Allow`, `Retry-After`, `WWW-Authenticate`).
    pub headers: Vec<(&'static str, String)>,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// A response body, tagged with the formatter that must render it.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorBody {
    /// A DRF-rendered object (`rest_framework.renderers.JSONRenderer`,
    /// DECISIONS.md D2, `Dialect::Drf`), in insertion order.
    Drf(Object),
    /// Arbitrary DRF-rendered data (a `ValidationError`'s structure).
    DrfValue(Value),
    /// A `JsonResponse`/`DjangoJSONEncoder` object (`Dialect::Django`).
    Django(Object),
    /// Already-encoded bytes with a fixed content type.
    Static {
        content_type: &'static str,
        body: &'static [u8],
    },
}

impl ErrorBody {
    fn drf(object: Object) -> Self {
        Self::Drf(object)
    }

    fn django(object: Object) -> Self {
        Self::Django(object)
    }
}

fn detail(value: &str) -> Object {
    Object::new().field("detail", value)
}

fn detail_body(value: &str) -> Object {
    detail(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn error_pages_are_byte_exact() {
        // HTTP_CONTRACT §11.2-§11.4: 145 and 143 bytes (both probed), and 135
        // for the 403 page (the document's 141 is an arithmetic slip).
        assert_eq!(ERROR_PAGE_500.len(), 145);
        assert_eq!(ERROR_PAGE_400.len(), 143);
        assert_eq!(ERROR_PAGE_403.len(), 135);
        assert!(ERROR_PAGE_500.contains("<title>Server Error (500)</title>"));
        assert!(ERROR_PAGE_400.contains("<title>Bad Request (400)</title>"));
        assert!(ERROR_PAGE_403.contains("<title>403 Forbidden</title>"));
    }

    #[test]
    fn statuses_and_headers() {
        assert_eq!(ApiError::NotAuthenticated.status(), 401);
        assert_eq!(
            ApiError::AuthenticationFailed {
                detail: "Given API token is not valid".to_string(),
                authenticate_header: None
            }
            .status(),
            403
        );
        assert_eq!(
            ApiError::AuthenticationFailed {
                detail: "x".to_string(),
                authenticate_header: Some("Basic realm=\"api\"".to_string())
            }
            .status(),
            401
        );
        assert_eq!(ApiError::permission_denied().status(), 403);
        assert_eq!(ApiError::NotFound.status(), 404);
        assert_eq!(ApiError::PageNotFound.status(), 404);
        assert_eq!(
            ApiError::MethodNotAllowed {
                method: "POST".to_string(),
                allow: "GET, HEAD, OPTIONS".to_string()
            }
            .status(),
            405
        );
        assert_eq!(
            ApiError::MethodNotAllowed {
                method: "POST".to_string(),
                allow: "GET, HEAD, OPTIONS".to_string()
            }
            .headers(),
            vec![("Allow", "GET, HEAD, OPTIONS".to_string())]
        );
        assert_eq!(ApiError::NotAcceptable.status(), 406);
        assert_eq!(
            ApiError::UnsupportedMediaType("text/plain".into()).status(),
            415
        );
        assert_eq!(ApiError::ParseError("x".into()).status(), 400);
        assert_eq!(ApiError::ValidationError(json!({})).status(), 400);
        assert_eq!(
            ApiError::Throttled {
                retry_after_seconds: 60
            }
            .headers(),
            vec![("Retry-After", "60".to_string())]
        );
        assert_eq!(ApiError::DisallowedHost.status(), 400);
        assert_eq!(ApiError::RequestBodyTooLarge.status(), 413);
        assert_eq!(ApiError::InternalServerError.status(), 500);
        assert_eq!(ApiError::NotImplemented.status(), 501);
        assert_eq!(ApiError::InvalidPayload.status(), 400);
        assert_eq!(ApiError::InvalidDetail.status(), 400);
        assert_eq!(ApiError::MissingKey.status(), 400);
        assert_eq!(ApiError::FamilyServerError.status(), 500);
    }

    #[test]
    fn bodies_match_the_contract() {
        // HTTP_CONTRACT §10.6, §11.1-§11.3. `Object` keeps Python's insertion
        // order; the rendered bytes are asserted in the `render` test below.
        let detail = |value: &str| Object::new().field("detail", value);
        assert_eq!(
            ApiError::NotAuthenticated.body(),
            ErrorBody::Drf(detail("Authentication credentials were not provided."))
        );
        assert_eq!(
            ApiError::NotFound.body(),
            ErrorBody::Drf(detail("Not found."))
        );
        assert_eq!(
            ApiError::PageNotFound.body(),
            ErrorBody::Django(Object::new().field("error", "Page not found."))
        );
        assert_eq!(
            ApiError::ObjectDoesNotExist.body(),
            ErrorBody::Drf(Object::new().field("error", "The required object does not exist."))
        );
        assert_eq!(
            ApiError::ResourceDoesNotExist.body(),
            ErrorBody::Drf(Object::new().field("error", "The requested resource does not exist."))
        );
        assert_eq!(
            ApiError::UnsupportedMediaType("text/plain".into()).body(),
            ErrorBody::Drf(detail("Unsupported media type \"text/plain\" in request."))
        );
        assert_eq!(
            ApiError::MethodNotAllowed {
                method: "POST".to_string(),
                allow: "GET".to_string()
            }
            .body(),
            ErrorBody::Drf(detail("Method \"POST\" not allowed."))
        );
        assert_eq!(
            ApiError::RequestBodyTooLarge.body(),
            ErrorBody::Django(
                Object::new()
                    .field("error", "REQUEST_BODY_TOO_LARGE")
                    .field(
                        "detail",
                        "The size of the request body exceeds the maximum allowed size."
                    )
            )
        );
        assert_eq!(
            ApiError::NotImplemented.body(),
            ErrorBody::Drf(detail("Not implemented by plane-api."))
        );
        assert_eq!(
            ApiError::Throttled {
                retry_after_seconds: 1
            }
            .body(),
            ErrorBody::Drf(
                Object::new()
                    .field("error_code", 5900)
                    .field("error_message", "RATE_LIMIT_EXCEEDED")
            )
        );
        // Bare ValidationError("boom") is a JSON array, not an object.
        assert_eq!(
            ApiError::ValidationError(json!(["boom"])).body(),
            ErrorBody::DrfValue(json!(["boom"]))
        );
    }

    #[test]
    fn rendered_bytes_are_exact() {
        // HTTP_CONTRACT §11.2: 28 bytes, Django dialect.
        let response = ApiError::PageNotFound.render().expect("render");
        assert_eq!(response.body, b"{\"error\": \"Page not found.\"}");
        assert_eq!(response.body.len(), 28);
        assert_eq!(response.status, 404);
        assert_eq!(response.content_type, "application/json");

        // §9.1: 111 bytes, keys in insertion order.
        let response = ApiError::RequestBodyTooLarge.render().expect("render");
        assert_eq!(
            response.body,
            b"{\"error\": \"REQUEST_BODY_TOO_LARGE\", \"detail\": \"The size of the request body exceeds the maximum allowed size.\"}"
        );
        assert_eq!(response.body.len(), 111);

        // §10.6: compact Drf bytes, no spaces.
        let response = ApiError::NotAuthenticated.render().expect("render");
        assert_eq!(
            response.body,
            b"{\"detail\":\"Authentication credentials were not provided.\"}"
        );
        assert_eq!(response.body.len(), 58);

        // §12.5: the 429 body and its Retry-After header.
        let response = ApiError::Throttled {
            retry_after_seconds: 60,
        }
        .render()
        .expect("render");
        assert_eq!(
            response.body,
            b"{\"error_code\":5900,\"error_message\":\"RATE_LIMIT_EXCEEDED\"}"
        );
        assert_eq!(response.headers, vec![("Retry-After", "60".to_string())]);

        // §11.3: the 400 page is static bytes.
        let response = ApiError::DisallowedHost.render().expect("render");
        assert_eq!(response.body.len(), 143);
        assert_eq!(response.body, ERROR_PAGE_400.as_bytes());

        // §11.2: the 500 page.
        let response = ApiError::InternalServerError.render().expect("render");
        assert_eq!(response.body.len(), 145);
        assert_eq!(response.body, ERROR_PAGE_500.as_bytes());

        // Plane family `handle_exception` bodies (§11.2), DRF dialect.
        assert_eq!(
            ApiError::InvalidPayload.render().expect("render").body,
            b"{\"error\":\"The payload is not valid\"}"
        );
        assert_eq!(
            ApiError::InvalidDetail.render().expect("render").body,
            b"{\"error\":\"Please provide valid detail\"}"
        );
        assert_eq!(
            ApiError::MissingKey.render().expect("render").body,
            b"{\"error\":\"The required key does not exist.\"}"
        );
        assert_eq!(
            ApiError::FamilyServerError.render().expect("render").body,
            b"{\"error\":\"Something went wrong please try again later\"}"
        );

        // ValidationError keeps the array shape.
        let response = ApiError::ValidationError(json!(["boom"]))
            .render()
            .expect("render");
        assert_eq!(response.body, b"[\"boom\"]");
    }

    #[test]
    fn content_types() {
        assert_eq!(ApiError::NotFound.content_type(), "application/json");
        assert_eq!(ApiError::PageNotFound.content_type(), "application/json");
        assert_eq!(
            ApiError::RequestBodyTooLarge.content_type(),
            "application/json"
        );
        assert_eq!(ApiError::DisallowedHost.content_type(), TEXT_HTML);
        assert_eq!(ApiError::InternalServerError.content_type(), TEXT_HTML);
    }
}
