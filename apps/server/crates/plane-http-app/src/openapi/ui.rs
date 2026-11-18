// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/schema/swagger-ui/` and `/api/schema/redoc/` — the two HTML pages.
//!
//! Python: `drf_spectacular/views.py:122-186,246-284` plus the stock templates
//! `drf_spectacular/templates/drf_spectacular/{swagger_ui,redoc}.html`. Both
//! pages are static given Plane's settings (title "The Plane REST API", the
//! CDN asset URLs, `SWAGGER_UI_SETTINGS={deepLinking: true}`, and
//! `REDOC_UI_SETTINGS={}` which takes the `<redoc spec-url=...>` branch), so
//! `tools/openapi/capture.py` records them and the only per-request value is
//! `schema_url`.
//!
//! `schema_url` is `/api/schema/` (`get_relative_url` strips scheme and host)
//! with `?lang=`/`?version=` appended when present
//! (`drf_spectacular/plumbing.py:1290-1303`). The swagger template escapes it
//! with `escapejs`, the redoc template with HTML autoescaping, so the two pages
//! need different substitutions of the same placeholder.

use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::{enabled, escape_html, escapejs, schema_url};

/// `SpectacularSwaggerView.template_name`'s rendering.
const SWAGGER: &[u8] = include_bytes!("../../../../deploy/openapi/swagger-ui.html");
/// `SpectacularRedocView.template_name`'s rendering.
const REDOC: &[u8] = include_bytes!("../../../../deploy/openapi/redoc.html");

/// The placeholder both captured pages carry for `{{ schema_url }}`.
const PLACEHOLDER: &str = "/api/schema/";

/// The placeholder the captured swagger page carries for `{{ csrf_token }}`.
const CSRF_PLACEHOLDER: &str = "__CSRF_TOKEN__";

/// `GET /api/schema/swagger-ui/`.
pub fn swagger<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        enabled(ctx.settings)?;
        let url = escapejs(&requested_schema_url(ctx));
        let body = render(SWAGGER, &url)?;
        // `{{ csrf_token }}` in the included `swagger_ui.js` runs
        // `get_token(request)`: a fresh random mask on every call
        // (`django/middleware/csrf.py:96-114`), which is also why the committed
        // page stores a placeholder instead of a literal token. The shared
        // `CsrfSlot` is the same state `CsrfViewMiddleware` read and will write
        // back as `Set-Cookie: csrftoken=…` on the response.
        let token = ctx.csrf()?.lock().await.get_token();
        let text = String::from_utf8(body)
            .map_err(|error| ViewError::Unknown(format!("OpenAPI page is not UTF-8: {error}")))?;
        if text.matches(CSRF_PLACEHOLDER).count() != 1 {
            return Err(ViewError::Unknown(format!(
                "committed Swagger page has {} occurrences of {CSRF_PLACEHOLDER:?}, expected 1",
                text.matches(CSRF_PLACEHOLDER).count()
            )));
        }
        let body = text.replacen(CSRF_PLACEHOLDER, &token, 1).into_bytes();
        // `drf_spectacular/views.py:155-158` sets this on the response, before
        // `SecurityMiddleware` (which only adds its own value when the header is
        // absent) runs.
        Ok(ViewResponse::raw(200, "text/html; charset=utf-8", body)
            .with_header("Cross-Origin-Opener-Policy", "unsafe-none"))
    })
}

/// `GET /api/schema/redoc/`.
pub fn redoc<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        enabled(ctx.settings)?;
        let url = escape_html(&requested_schema_url(ctx));
        Ok(ViewResponse::raw(
            200,
            "text/html; charset=utf-8",
            render(REDOC, &url)?,
        ))
    })
}

/// `_get_schema_url` for a request to one of the two pages.
fn requested_schema_url(ctx: &ViewCtx<'_>) -> String {
    schema_url(ctx.query().get("lang"), ctx.query().get("version"))
}

/// Replace the captured page's single `schema_url` with the request's.
///
/// The templates contain the placeholder exactly once; a second occurrence
/// would mean the committed page is not the one this module expects, so it is
/// an explicit error rather than a silent no-op.
fn render(template: &'static [u8], url: &str) -> Result<Vec<u8>, ViewError> {
    let text = std::str::from_utf8(template).map_err(|error| {
        ViewError::Unknown(format!("committed OpenAPI page is not UTF-8: {error}"))
    })?;
    if text.matches(PLACEHOLDER).count() != 1 {
        return Err(ViewError::Unknown(format!(
            "committed OpenAPI page has {} occurrences of {PLACEHOLDER:?}, expected 1",
            text.matches(PLACEHOLDER).count()
        )));
    }
    Ok(text.replacen(PLACEHOLDER, url, 1).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_captured_pages_carry_one_placeholder() {
        for page in [SWAGGER, REDOC] {
            let text = std::str::from_utf8(page).expect("utf-8");
            assert_eq!(text.matches(PLACEHOLDER).count(), 1);
        }
    }

    #[test]
    fn only_the_swagger_page_carries_a_csrf_placeholder() {
        let text = std::str::from_utf8(SWAGGER).expect("utf-8");
        assert_eq!(text.matches(CSRF_PLACEHOLDER).count(), 1);
        let text = std::str::from_utf8(REDOC).expect("utf-8");
        assert_eq!(text.matches(CSRF_PLACEHOLDER).count(), 0);
    }

    #[test]
    fn the_default_substitution_is_byte_identical() {
        assert_eq!(render(SWAGGER, "/api/schema/").expect("render"), SWAGGER);
        assert_eq!(render(REDOC, "/api/schema/").expect("render"), REDOC);
    }
}
