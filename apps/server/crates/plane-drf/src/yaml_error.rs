// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DRF error responses rendered by drf-spectacular's YAML renderer.
//!
//! DRF renders a response with the view's **first** renderer when negotiation
//! did not pick one — every error raised inside `initial()` or by a handler
//! (`rest_framework/views.py:437-442`, `perform_content_negotiation(force=True)`
//! returns `renderers[0]`). Plane's routes all use `JSONRenderer`, whose
//! `ErrorDetail` support drops the code; `/api/schema/` uses
//! `OpenApiYamlRenderer` first, and its `error_detail_representer` keeps it
//! (`drf_spectacular/renderers.py:39-41`):
//!
//! ```text
//! detail:
//!   string: Not found.
//!   code: not_found
//! ```
//!
//! That is the only observable difference for the three schema views: a 404
//! (`?format=xml`), a 406 (an `Accept` no renderer matches) and a 405 (a verb
//! the view does not implement). Their 500s are Django's HTML page and are
//! unaffected.
//!
//! PyYAML settings from `drf_spectacular/renderers.py:70-76`: `sort_keys=False`
//! (insertion order — the `detail` map is `{"string", "code"}`), block style,
//! `allow_unicode=True`, and a `str` representer that uses the `|` style when
//! the value contains a newline.

use plane_core::ApiError;

use crate::view::DrfResponse;

/// Render `error` the way the schema route's first renderer does, or `None`
/// when the error's body has a shape this module does not model.
pub fn render(error: &ApiError, media_type: &str) -> Option<DrfResponse> {
    // `BaseRenderer.media_type` + `charset` (`OpenApiYamlRenderer.charset` is
    // `"utf-8"`): served as `<media type>; charset=utf-8`.
    let content_type = match media_type {
        "application/vnd.oai.openapi" => "application/vnd.oai.openapi; charset=utf-8",
        "application/yaml" => "application/yaml; charset=utf-8",
        _ => return None,
    };
    let body = if let Some((message, code)) = error.detail_and_code() {
        // `{"detail": ErrorDetail(message, code)}`.
        format!("detail:\n  string: {}\n  code: {code}\n", scalar(&message))
    } else {
        match error {
            // `{"error_code": 5900, "error_message": "RATE_LIMIT_EXCEEDED"}`
            // (`plane-core`'s throttled body, `plane/api/views/base.py`).
            ApiError::Throttled { .. } => {
                "error_code: 5900\nerror_message: RATE_LIMIT_EXCEEDED\n".to_string()
            }
            _ => return None,
        }
    };
    let mut response = DrfResponse::new(error.status(), Some(content_type), body.into_bytes());
    for (name, value) in error.headers() {
        response.set_header(name, value);
    }
    Some(response)
}

/// A PyYAML scalar for the values DRF puts in an error body.
///
/// Messages are plain ASCII sentences; PyYAML emits them as plain scalars unless
/// they could be read as another type or contain an indicator. Only the two
/// non-plain forms Django can produce here are modelled: a single-quoted scalar
/// (for a leading indicator, a trailing space, or `: ` inside) and the `|` block
/// for a multiline string.
fn scalar(value: &str) -> String {
    if value.contains('\n') {
        let indented: Vec<String> = value.lines().map(|line| format!("  {line}")).collect();
        return format!("|-\n{}", indented.join("\n"));
    }
    let needs_quotes = value.is_empty()
        || value.trim() != value
        || value.contains(": ")
        || value.contains(" #")
        || value.chars().next().is_some_and(|first| {
            matches!(
                first,
                '-' | '?'
                    | ':'
                    | ','
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '#'
                    | '&'
                    | '*'
                    | '!'
                    | '|'
                    | '>'
                    | '\''
                    | '"'
                    | '%'
                    | '@'
                    | '`'
            )
        });
    if needs_quotes {
        format!("'{}'", value.replace('\'', "''"))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from the real view (`django.test.Client`): `GET
    /// /api/schema/?format=xml` and `GET /api/schema/` with `Accept: text/html`.
    #[test]
    fn drf_error_bodies_match_the_probed_yaml() {
        let not_found = render(&ApiError::NotFound, "application/vnd.oai.openapi")
            .expect("detail-shaped error");
        assert_eq!(not_found.status, 404);
        assert_eq!(
            not_found.content_type,
            Some("application/vnd.oai.openapi; charset=utf-8")
        );
        assert_eq!(
            String::from_utf8(not_found.body).expect("utf-8"),
            "detail:\n  string: Not found.\n  code: not_found\n"
        );

        let not_acceptable = render(&ApiError::NotAcceptable, "application/vnd.oai.openapi")
            .expect("detail-shaped error");
        assert_eq!(not_acceptable.status, 406);
        assert_eq!(
            String::from_utf8(not_acceptable.body).expect("utf-8"),
            "detail:\n  string: Could not satisfy the request Accept header.\n  code: not_acceptable\n"
        );
    }

    #[test]
    fn a_method_error_uses_drfs_code_and_message() {
        let error = ApiError::MethodNotAllowed {
            method: "POST".to_string(),
            allow: "GET, HEAD, OPTIONS".to_string(),
        };
        let response = render(&error, "application/vnd.oai.openapi").expect("rendered");
        assert_eq!(response.status, 405);
        assert_eq!(response.header("Allow"), Some("GET, HEAD, OPTIONS"));
        assert_eq!(
            String::from_utf8(response.body).expect("utf-8"),
            "detail:\n  string: Method \"POST\" not allowed.\n  code: method_not_allowed\n"
        );
    }

    #[test]
    fn object_shaped_errors_are_not_modelled() {
        assert!(render(&ApiError::ObjectDoesNotExist, "application/vnd.oai.openapi").is_none());
    }

    #[test]
    fn scalars_quote_like_pyyaml() {
        assert_eq!(scalar("Not found."), "Not found.");
        assert_eq!(scalar("a: b"), "'a: b'");
        assert_eq!(scalar(""), "''");
        assert_eq!(scalar("it's"), "it's");
        assert_eq!(scalar("'quoted'"), "'''quoted'''");
        assert_eq!(scalar("two\nlines"), "|-\n  two\n  lines");
    }
}
