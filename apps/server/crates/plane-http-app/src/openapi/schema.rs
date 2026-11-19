// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/schema/` — `drf_spectacular.views.SpectacularAPIView`.
//!
//! The document is the committed golden (`deploy/openapi/v1.yaml` /
//! `v1.json`); this module reproduces the view's observable contract:
//!
//! * `drf_spectacular/views.py:45-49` — renderer order YAML (default), YAML,
//!   JSON, JSON. The dispatcher negotiates from `routes.json` and 406s/404s
//!   exactly as DRF does before this handler runs.
//! * `drf_spectacular/views.py:100-104` — `Content-Disposition` with the
//!   renderer's format as the suffix.
//! * `drf_spectacular/views.py:82-114` — `?lang=` runs generation under
//!   `translation.override`, and `?version=` regenerates with a different
//!   `info.version` and filename. Both are applied to the committed bytes;
//!   [`crate::openapi::translations`] and `version-scalars.json` pin the two.
//!
//! `Plane` sets no `ALLOWED_VERSIONS` (`plane/settings/common.py:139-152`), so
//! `_get_version_parameter` accepts **every** non-empty value, including `abc`;
//! an empty or absent one leaves the document (and filename) untouched.

use super::enabled;
use super::translations;
use base64::Engine;
use plane_drf::error::ViewError;
use plane_drf::negotiate::{self, RendererSpec};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

/// `GET /api/schema/`.
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        enabled(ctx.settings)?;
        // The same renderer list the dispatcher negotiated with; a policy
        // without a known renderer class was already refused there.
        let renderers = negotiate::renderer_specs(&ctx.request.route.renderer_classes)
            .ok_or_else(|| not_ported(ctx))?;
        let renderer = negotiate::select_renderer_from(&renderers, ctx.request)?;

        let mut body = match renderer.media_type {
            "application/vnd.oai.openapi" | "application/yaml" => {
                include_bytes!("../../../../deploy/openapi/v1.yaml").to_vec()
            }
            "application/vnd.oai.openapi+json" | "application/json" => {
                include_bytes!("../../../../deploy/openapi/v1.json").to_vec()
            }
            other => {
                return Err(ViewError::Unknown(format!(
                    "no OpenAPI document is committed for media type {other:?}"
                )));
            }
        };

        // `if settings.USE_I18N and request.GET.get("lang")` — present but
        // empty is falsey and therefore the default language.
        if let Some(lang) = ctx.query().get("lang").filter(|value| !value.is_empty()) {
            body = translations::translate(&body, lang);
        }

        // `version = self.api_version or request.version or
        // self._get_version_parameter(request)`: the view has no
        // `api_version`/versioning class here, so the query value wins.
        let version = ctx.query().get("version").filter(|value| !value.is_empty());
        if let Some(version) = version {
            let value = format!("0.0.1 ({version})");
            body = match renderer.format {
                "yaml" => replace_yaml_version(&body, &value),
                _ => replace_json_version(&body, &value)?,
            };
        }

        let suffix = version.map_or_else(String::new, |version| format!(" ({version})"));
        let filename = format!(
            "inline; filename=\"The Plane REST API{suffix}.{}\"",
            renderer.format
        );
        Ok(ViewResponse::raw(200, content_type(renderer), body)
            .with_header("Content-Disposition", header_value(&filename)?))
    })
}

fn not_ported(ctx: &ViewCtx<'_>) -> ViewError {
    ViewError::Unknown(format!(
        "{}: renderer_classes {:?} are not ported",
        ctx.request.route.view, ctx.request.route.renderer_classes
    ))
}

/// `BaseRenderer.charset` is `None` for the JSON renderers and `"utf-8"` for
/// the YAML ones (`rest_framework/renderers.py`), so only YAML carries it.
fn content_type(renderer: &RendererSpec) -> &'static str {
    match renderer.media_type {
        "application/vnd.oai.openapi" => "application/vnd.oai.openapi; charset=utf-8",
        "application/yaml" => "application/yaml; charset=utf-8",
        other => match other {
            "application/vnd.oai.openapi+json" => "application/vnd.oai.openapi+json",
            _ => "application/json",
        },
    }
}

/// `info.version: 0.0.1` is the second line of `info:`; replace exactly that
/// scalar. PyYAML's block continuation lines carry their own indentation.
fn replace_yaml_version(body: &[u8], value: &str) -> Vec<u8> {
    const PREFIX: &[u8] = b"\n  version: ";
    let scalar = plane_drf::yaml::scalar(value, 2);
    let mut out = Vec::with_capacity(body.len() + scalar.len());
    let Some(position) = body
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)
    else {
        return body.to_vec();
    };
    out.extend_from_slice(&body[..position + PREFIX.len()]);
    out.extend_from_slice(scalar.as_bytes());
    let rest = &body[position + PREFIX.len()..];
    let end = rest
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(rest.len());
    out.extend_from_slice(&rest[end..]);
    out
}

/// `"version": "0.0.1"` in the four-space-indented JSON document.
///
/// `Json::Str` renders the string with the DRF dialect (`ensure_ascii=False`,
/// U+2028/U+2029 escaped), which is what `JSONRenderer` does.
fn replace_json_version(body: &[u8], value: &str) -> Result<Vec<u8>, ViewError> {
    const NEEDLE: &[u8] = b"\"version\": \"0.0.1\"";
    let Some(position) = body
        .windows(NEEDLE.len())
        .position(|window| window == NEEDLE)
    else {
        return Ok(body.to_vec());
    };
    let mut out = Vec::with_capacity(body.len());
    out.extend_from_slice(&body[..position]);
    out.extend_from_slice(b"\"version\": ");
    out.extend_from_slice(&plane_query::Json::Str(value.to_owned()).to_drf());
    out.extend_from_slice(&body[position + NEEDLE.len()..]);
    Ok(out)
}

/// Django encodes response header values as latin-1 and, when that fails, as
/// an RFC 2047 encoded word (`django/utils/http.py:_convert_to_charset`,
/// `mime_encode=True`). A value with a newline is `BadHeaderError` (a 500).
fn header_value(value: &str) -> Result<String, ViewError> {
    if value.contains(['\n', '\r']) {
        return Err(ViewError::Unknown(format!(
            "BadHeaderError: header values can't contain newlines (got {value:?})"
        )));
    }
    if value.is_ascii() {
        return Ok(value.to_owned());
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());
    Ok(format!("=?utf-8?b?{encoded}?="))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    /// `deploy/openapi/version-scalars.json`, recorded from PyYAML and
    /// `json.dumps` by `tools/openapi/capture.py`.
    const VERSION_SCALARS: &str = include_str!("../../../../deploy/openapi/version-scalars.json");

    #[derive(Deserialize)]
    struct VersionCorpus {
        cases: Vec<VersionCase>,
    }

    #[derive(Deserialize)]
    struct VersionCase {
        version: String,
        yaml: String,
        json: String,
    }

    fn corpus() -> VersionCorpus {
        serde_json::from_str(VERSION_SCALARS).expect("version-scalars.json")
    }

    #[test]
    fn yaml_version_scalars_match_python() {
        for case in corpus().cases {
            let value = format!("0.0.1 ({})", case.version);
            assert_eq!(
                plane_drf::yaml::scalar(&value, 2),
                case.yaml,
                "version={:?}",
                case.version
            );
        }
    }

    #[test]
    fn json_version_scalars_match_python() {
        for case in corpus().cases {
            let value = format!("0.0.1 ({})", case.version);
            assert_eq!(
                String::from_utf8(plane_query::Json::Str(value).to_drf()).expect("utf8"),
                case.json,
                "version={:?}",
                case.version
            );
        }
    }

    #[test]
    fn replacing_the_version_touches_only_info_version() {
        let body = b"openapi: 3.0.3\ninfo:\n  title: x\n  version: 0.0.1\n  description: y\n";
        let replaced = replace_yaml_version(body, "0.0.1 (1)");
        assert_eq!(
            String::from_utf8(replaced).expect("utf8"),
            "openapi: 3.0.3\ninfo:\n  title: x\n  version: 0.0.1 (1)\n  description: y\n"
        );
        let body = b"{\n    \"info\": {\n        \"title\": \"x\",\n        \"version\": \"0.0.1\"\n    }\n}";
        let replaced = replace_json_version(body, "0.0.1 (1)").expect("replace");
        assert_eq!(
            String::from_utf8(replaced).expect("utf8"),
            "{\n    \"info\": {\n        \"title\": \"x\",\n        \"version\": \"0.0.1 (1)\"\n    }\n}"
        );
    }

    #[test]
    fn non_ascii_header_values_are_rfc2047_encoded() {
        assert_eq!(header_value("inline; x").expect("ascii"), "inline; x");
        assert_eq!(
            header_value("inline; é").expect("encoded"),
            "=?utf-8?b?aW5saW5lOyDDqQ==?="
        );
        assert!(header_value("a\nb").is_err());
    }
}
