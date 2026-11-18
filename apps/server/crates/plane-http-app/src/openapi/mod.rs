// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `/api/schema/` — the drf-spectacular routes (Phase 7's published docs).
//!
//! Python: `plane/urls.py:27-40`. Only when `ENABLE_DRF_SPECTACULAR == "1"`
//! (`plane/settings/common.py:602-607`) does Django add:
//!
//! * `/api/schema/` -> `SpectacularAPIView`, the OpenAPI 3.0.3 document;
//! * `/api/schema/swagger-ui/` -> `SpectacularSwaggerView`, the stock template;
//! * `/api/schema/redoc/` -> `SpectacularRedocView`, the stock template.
//!
//! With the flag off the URLs do not exist and Django answers its 404
//! (`plane/app/views/error_404.py:8-9` -> `{"error": "Page not found."}`), which
//! is what these handlers return. Because the Rust route stays registered (the
//! inventory is generated with the flag on, owner `django`), that 404 goes
//! through the DRF pipeline and therefore carries `Allow` and `Vary: Accept`,
//! which Django's resolver 404 does not. The proxy does not route the group to
//! Rust while it is unflagged, so the difference only exists if the group is
//! turned on against a process with the flag off; it is not covered by a
//! scenario.
//!
//! ## Why the bodies are committed, not generated
//!
//! The document is generated from Django's URL resolver, views and serializers
//! by drf-spectacular 0.29.0 and is **DB-independent**. Phase 7's exit condition
//! is that the published docs stay accurate, so the port serves the exact bytes
//! Django produced at the pinned commit. `apps/server/tools/openapi/capture.py`
//! records them into `apps/server/deploy/openapi/`, and
//! `apps/server/tools/openapi/check.py` regenerates and compares them, so any
//! upstream view/serializer change fails a check instead of silently serving
//! stale docs.
//!
//! ## What is reproduced
//!
//! * Content negotiation over the view's four renderers
//!   (`drf_spectacular/views.py:45-49`): YAML (`application/vnd.oai.openapi`,
//!   `application/yaml`) and JSON (`application/vnd.oai.openapi+json`,
//!   `application/json`), including DRF's `?format=` filter and its 404/406
//!   errors. `plane-drf`'s dispatcher does the negotiation from
//!   `routes.json`'s `renderer_classes`, and adds `Vary: Accept` because the
//!   view offers more than one renderer (`rest_framework/views.py:443-446`).
//! * `Content-Disposition: inline; filename="The Plane REST API.<suffix>"`
//!   (`drf_spectacular/views.py:100-104`).
//! * The two HTML pages: `text/html; charset=utf-8`, and the swagger page's
//!   `Cross-Origin-Opener-Policy: unsafe-none` header
//!   (`drf_spectacular/views.py:155-158`). The `schema_url` in both pages is
//!   `/api/schema/` with `?lang=`/`?version=` appended when the request has
//!   them (`drf_spectacular/plumbing.py:1290-1303`); no translations are
//!   installed, so `lang` changes only that URL.
//!
//! ## Not ported, explicitly
//!
//! `?version=` on the **schema** route regenerates the document with a
//! different `info.version` and `Content-Disposition` filename
//! (`drf_spectacular/views.py:96-114`). The port has the committed document
//! only, so it refuses the request instead of serving the wrong bytes. No
//! Plane client uses the parameter.

use http::Method;
use plane_core::error::ApiError;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};

pub mod schema;
pub mod ui;

/// Register the three drf-spectacular views' `GET`.
///
/// `HEAD` is Django's `GET` (`View.setup`) and `OPTIONS` is DRF's metadata; the
/// runtime derives both from `routes.json`, so only `GET` is registered.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    let mut view = registry.api_view("drf_spectacular.views.SpectacularAPIView")?;
    view.method(Method::GET, schema::get)?;
    let mut view = registry.api_view("drf_spectacular.views.SpectacularSwaggerView")?;
    view.method(Method::GET, ui::swagger)?;
    let mut view = registry.api_view("drf_spectacular.views.SpectacularRedocView")?;
    view.method(Method::GET, ui::redoc)?;
    Ok(())
}

/// The three views are reachable only when the flag is on
/// (`plane/urls.py:27`): with it off the URL does not exist, so Django's URL
/// resolver answers its 404 before any view runs.
fn enabled(settings: &plane_core::config::Settings) -> Result<(), ViewError> {
    if settings.runtime.enable_drf_spectacular {
        Ok(())
    } else {
        Err(ViewError::Api(ApiError::PageNotFound))
    }
}

/// `drf_spectacular.plumbing.set_query_parameters(url="/api/schema/", lang=…,
/// version=…)` for a request with no other query parameters: `urlencode`
/// (`quote_plus`) of the non-`None` values, in `lang`, `version` order.
fn schema_url(lang: Option<&str>, version: Option<&str>) -> String {
    let mut url = String::from("/api/schema/");
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    if let Some(lang) = lang {
        pairs.push(("lang", lang));
    }
    if let Some(version) = version {
        pairs.push(("version", version));
    }
    if pairs.is_empty() {
        return url;
    }
    url.push('?');
    url.push_str(
        &pairs
            .iter()
            .map(|(key, value)| format!("{key}={}", quote_plus(value)))
            .collect::<Vec<_>>()
            .join("&"),
    );
    url
}

/// Python's `urllib.parse.quote_plus` with its default safe set (`/`).
///
/// Only ASCII alphanumerics and `_.-~` pass through unescaped; a space becomes
/// `+` (`django/utils/http.py:urlencode` uses `quote_plus`).
fn quote_plus(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' | b'/' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Django's `escapejs` filter (`django/utils/html.py:409-424`), applied to the
/// swagger page's `schema_url` (`{{ schema_url|escapejs }}` inside the included
/// `swagger_ui.js` template).
fn escapejs(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\u005C"),
            '\'' => out.push_str("\\u0027"),
            '"' => out.push_str("\\u0022"),
            '>' => out.push_str("\\u003E"),
            '<' => out.push_str("\\u003C"),
            '&' => out.push_str("\\u0026"),
            '=' => out.push_str("\\u003D"),
            '-' => out.push_str("\\u002D"),
            ';' => out.push_str("\\u003B"),
            '`' => out.push_str("\\u0060"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other => out.push(other),
        }
    }
    out
}

/// Django's `escape` (`django/utils/html.py:escape`): `&`, `<`, `>`, `"`, `'`
/// as HTML entities. The redoc template's `spec-url="{{ schema_url }}"` is
/// autoescaped, which matters for the `&` between `lang` and `version`.
fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
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

    #[test]
    fn schema_url_matches_python_urlencode() {
        assert_eq!(schema_url(None, None), "/api/schema/");
        assert_eq!(schema_url(Some("de"), None), "/api/schema/?lang=de");
        assert_eq!(
            schema_url(Some("pt-br"), Some("2.1")),
            "/api/schema/?lang=pt-br&version=2.1"
        );
        assert_eq!(schema_url(Some("a b&c"), None), "/api/schema/?lang=a+b%26c");
        // `set_query_parameters` ignores a `None` kwargs value.
        assert_eq!(schema_url(None, Some("1")), "/api/schema/?version=1");
    }

    #[test]
    fn escapejs_matches_django() {
        assert_eq!(escapejs("/api/schema/"), "/api/schema/");
        assert_eq!(
            escapejs("/api/schema/?lang=pt-br&version=1"),
            "/api/schema/?lang\\u003Dpt\\u002Dbr\\u0026version\\u003D1"
        );
    }

    #[test]
    fn escape_html_matches_django() {
        assert_eq!(escape_html("/api/schema/"), "/api/schema/");
        assert_eq!(
            escape_html("/api/schema/?lang=de&version=1"),
            "/api/schema/?lang=de&amp;version=1"
        );
    }
}
