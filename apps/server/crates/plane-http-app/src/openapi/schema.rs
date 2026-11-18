// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/schema/` — `drf_spectacular.views.SpectacularAPIView`.
//!
//! The document is the committed golden (`deploy/openapi/v1.yaml` /
//! `v1.json`); this module only reproduces the view's observable contract:
//!
//! * `drf_spectacular/views.py:45-49` — renderer order YAML (default), YAML,
//!   JSON, JSON. The dispatcher negotiates from `routes.json` and 406s/404s
//!   exactly as DRF does before this handler runs.
//! * `drf_spectacular/views.py:100-104` — `Content-Disposition` with the
//!   renderer's format as the suffix.
//! * `drf_spectacular/views.py:96-114` — `?version=` and `?lang=` regenerate
//!   the document; not ported (see the module docs), refused with an explicit
//!   error.

use plane_drf::error::ViewError;
use plane_drf::negotiate::{self, RendererSpec};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::enabled;

/// `GET /api/schema/`.
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        enabled(ctx.settings)?;
        // `SpectacularAPIView._get_version_parameter` has no allow-list in
        // Plane (`ALLOWED_VERSIONS` is unset), so any `?version=` reaches the
        // generator. The committed document was generated without one.
        if ctx.query().contains_key("version") {
            return Err(ViewError::Unknown(
                "/api/schema/?version= is not ported: the served document is the committed \
                 artefact, and a version changes its info.version and Content-Disposition"
                    .to_string(),
            ));
        }
        // `SpectacularAPIView.get` wraps the generation in
        // `translation.override(lang)` (`drf_spectacular/views.py:82-88`), which
        // translates DRF's *own* strings (a `lang=de` document differs from the
        // default by a few hundred bytes), so the committed artefact is not the
        // answer for it.
        if ctx.query().contains_key("lang") {
            return Err(ViewError::Unknown(
                "/api/schema/?lang= is not ported: the served document is the committed \
                 default-language artefact, and Django translates DRF's own strings"
                    .to_string(),
            ));
        }
        // The same renderer list the dispatcher negotiated with; a policy
        // without a known renderer class was already refused there.
        let renderers = negotiate::renderer_specs(&ctx.request.route.renderer_classes)
            .ok_or_else(|| not_ported(ctx))?;
        let renderer = negotiate::select_renderer_from(&renderers, ctx.request)?;
        serve(renderer)
    })
}

fn not_ported(ctx: &ViewCtx<'_>) -> ViewError {
    ViewError::Unknown(format!(
        "{}: renderer_classes {:?} are not ported",
        ctx.request.route.view, ctx.request.route.renderer_classes
    ))
}

/// The body and `Content-Type`/`Content-Disposition` for a selected renderer.
fn serve(renderer: &RendererSpec) -> Result<ViewResponse, ViewError> {
    // `BaseRenderer.charset` is `None` for the JSON renderers and `"utf-8"` for
    // the YAML ones (`rest_framework/renderers.py`), so only YAML carries it.
    let (content_type, body): (&'static str, &'static [u8]) = match renderer.media_type {
        "application/vnd.oai.openapi" => (
            "application/vnd.oai.openapi; charset=utf-8",
            include_bytes!("../../../../deploy/openapi/v1.yaml"),
        ),
        "application/yaml" => (
            "application/yaml; charset=utf-8",
            include_bytes!("../../../../deploy/openapi/v1.yaml"),
        ),
        "application/vnd.oai.openapi+json" => (
            "application/vnd.oai.openapi+json",
            include_bytes!("../../../../deploy/openapi/v1.json"),
        ),
        "application/json" => (
            "application/json",
            include_bytes!("../../../../deploy/openapi/v1.json"),
        ),
        other => {
            return Err(ViewError::Unknown(format!(
                "no OpenAPI document is committed for media type {other:?}"
            )));
        }
    };
    Ok(
        ViewResponse::raw(200, content_type, body.to_vec()).with_header(
            "Content-Disposition",
            // `_get_filename`: `{TITLE}.{renderer.format}`
            // (`plane/settings/openapi.py:17`, `drf_spectacular/views.py:107-112`).
            format!(
                "inline; filename=\"The Plane REST API.{}\"",
                renderer.format
            ),
        ),
    )
}
