// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `SimpleMetadata` (`rest_framework/metadata.py`) for OPTIONS.
//!
//! ```json
//! {"name":"State List","description":"","renders":["application/json"],
//!  "parses":["application/json","application/x-www-form-urlencoded","multipart/form-data"]}
//! ```
//!
//! `actions` is the part that needs a serializer. `routes.json` records the
//! exact field map DRF returns for a **permitted** caller
//! (`options_metadata.actions_json`, compact `json.dumps` text, so the field
//! order survives `serde_json`'s sorted maps). At request time the permission
//! half of `determine_actions` is re-run with a request whose method is the
//! action's method:
//!
//! ```python
//! for method in {'PUT', 'POST'} & set(view.allowed_methods):
//!     view.request = clone_request(request, method)
//!     try:
//!         view.check_permissions(view.request)
//!         if method == 'PUT': view.get_object()
//!     except (APIException, PermissionDenied, Http404): pass
//!     else: actions[method] = ...
//! ```
//!
//! Known gaps, both listed in `ROUTES.md`:
//!
//! * `get_object()` for `PUT` is not reproduced. A route that allows `PUT`
//!   records `get_object_required: true`; this runtime includes the recorded
//!   actions whenever the permission check passes, so a `PUT` whose object
//!   would 404 on Django can carry `actions` here and not there.
//! * CPython iterates `{'PUT', 'POST'}` in hash order, which is randomised per
//!   process, so the key order of `actions` is not stable in Django either.
//!   This port writes `POST` before `PUT` (the order the inventory records).

use http::Method;
use plane_core::error::ApiError;
use plane_serde::Dialect;

use crate::error::ViewError;
use crate::policy::{RoutePolicy, method_from_name};

/// `SimpleMetadata.determine_metadata`'s body, before rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub name: String,
    pub description: String,
    /// `[renderer.media_type for renderer in view.renderer_classes]`.
    pub renders: Vec<String>,
    /// `[parser.media_type for parser in view.parser_classes]`.
    pub parses: Vec<String>,
    /// `actions` values in output order, with the pre-rendered JSON text.
    pub actions: Vec<(Method, String)>,
}

/// Why metadata could not be produced.
#[derive(Debug, Clone, PartialEq)]
pub enum MetadataError {
    /// `determine_actions` raised (a serializer that needs a request, say):
    /// Django answers the whole OPTIONS with the family 500 body.
    ActionsFailed(String),
    /// The inventory is missing something the renderer needs.
    Invalid(String),
}

impl From<MetadataError> for ViewError {
    fn from(error: MetadataError) -> Self {
        match error {
            MetadataError::ActionsFailed(message) => ViewError::Unknown(message),
            MetadataError::Invalid(message) => ViewError::Unknown(message),
        }
    }
}

impl Metadata {
    /// Render with the DRF JSON dialect, splicing the recorded `actions` text
    /// verbatim so its field order is preserved.
    pub fn render(&self) -> Result<Vec<u8>, plane_serde::Error> {
        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(b"{\"name\":");
        out.extend(plane_serde::to_vec(Dialect::Drf, &self.name)?);
        out.extend_from_slice(b",\"description\":");
        out.extend(plane_serde::to_vec(Dialect::Drf, &self.description)?);
        out.extend_from_slice(b",\"renders\":");
        out.extend(plane_serde::to_vec(Dialect::Drf, &self.renders)?);
        out.extend_from_slice(b",\"parses\":");
        out.extend(plane_serde::to_vec(Dialect::Drf, &self.parses)?);
        if !self.actions.is_empty() {
            out.extend_from_slice(b",\"actions\":{");
            for (index, (method, json)) in self.actions.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend(plane_serde::to_vec(Dialect::Drf, method.as_str())?);
                out.push(b':');
                out.extend_from_slice(json.as_bytes());
            }
            out.push(b'}');
        }
        out.push(b'}');
        Ok(out)
    }
}

/// Build the metadata body for a route.
///
/// `allowed` is the set of attempt methods whose permission check passed (the
/// runtime evaluates it with the cloned request's method). If
/// `options_metadata.error` is set and any attempt is allowed, Django's
/// `determine_actions` raises, so the whole OPTIONS is a family 500.
pub fn determine(route: &RoutePolicy, allowed: &[Method]) -> Result<Metadata, MetadataError> {
    let name = route
        .view_name
        .clone()
        .ok_or_else(|| MetadataError::Invalid(format!("{} has no view name", route.view)))?;
    let description = route.view_description.clone().unwrap_or_default();
    // `[renderer.media_type for renderer in view.renderer_classes]`. The
    // class → media-type map is the same one content negotiation uses, so the
    // drf-spectacular routes' OPTIONS lists their four renderers.
    let renders: Vec<String> = match crate::negotiate::renderer_specs(&route.renderer_classes) {
        Some(specs) => specs
            .iter()
            .map(|spec| spec.media_type.to_string())
            .collect(),
        None => {
            return Err(MetadataError::Invalid(format!(
                "unsupported renderer_classes {:?} on {}",
                route.renderer_classes, route.view
            )));
        }
    };
    let parses: Vec<String> = route
        .parser_classes
        .iter()
        .map(|class| {
            Ok(match class.as_str() {
                "rest_framework.parsers.JSONParser" => "application/json",
                "rest_framework.parsers.FormParser" => "application/x-www-form-urlencoded",
                "rest_framework.parsers.MultiPartParser" => "multipart/form-data",
                other => {
                    return Err(MetadataError::Invalid(format!(
                        "unsupported parser {other:?} on {}",
                        route.view
                    )));
                }
            }
            .to_string())
        })
        .collect::<Result<_, _>>()?;

    let mut actions = Vec::new();
    for method in &route.options.attempts {
        if !allowed.contains(method) {
            continue;
        }
        if let Some(error) = &route.options.error {
            return Err(MetadataError::ActionsFailed(error.clone()));
        }
        let key = match *method {
            Method::POST => "POST",
            Method::PUT => "PUT",
            _ => continue,
        };
        if let Some(json) = route.options.actions_json.get(key) {
            actions.push((method.clone(), json.clone()));
        }
    }

    Ok(Metadata {
        name,
        description,
        renders,
        parses,
        actions,
    })
}

/// The `Allow` header value for a route (`APIView.default_response_headers`).
///
/// `[m.upper() for m in self.http_method_names if hasattr(self, m)]`: the
/// inventory precomputes it from the bound methods, so this is only a lookup.
pub fn allow_header(route: &RoutePolicy) -> Option<&str> {
    route.allow.as_deref()
}

/// Parse the attempt method names the inventory records.
pub fn attempt_methods(route: &RoutePolicy) -> Vec<Method> {
    route
        .options
        .attempts
        .iter()
        .filter(|method| matches!(**method, Method::POST | Method::PUT))
        .cloned()
        .collect()
}

/// A method the runtime must never invent: the metadata model is fixed.
pub const _METADATA_CLASS: &str = "rest_framework.metadata.SimpleMetadata";

/// The `MethodNotAllowed` body (`ApiError::MethodNotAllowed`) needs the same
/// `Allow` value; this helper keeps the two in step.
pub fn method_not_allowed(route: &RoutePolicy, method: &Method) -> ApiError {
    ApiError::MethodNotAllowed {
        method: method.as_str().to_string(),
        allow: allow_header(route).unwrap_or_default().to_string(),
    }
}

/// `method_from_name` is re-exported so callers building test routes do not
/// depend on `policy` directly.
pub use crate::policy::method_from_name as method;

const _: () = {
    // `method_from_name` is used by the `attempt` filter above through
    // `policy`; keep the import exercised without a runtime branch.
    let _ = method_from_name;
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::PolicyTable;

    fn table() -> PolicyTable {
        PolicyTable::load().expect("routes.json")
    }

    /// The no-serializer OPTIONS body is exactly `determine_metadata`'s four
    /// keys, in that order, rendered by the DRF dialect.
    #[test]
    fn metadata_body_matches_the_recorded_view() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.analytic.base.AnalyticsEndpoint")
            .expect("analytics");
        assert_eq!(route.view_name.as_deref(), Some("Analytics Endpoint"));
        let metadata = determine(route, &[]).expect("metadata");
        assert_eq!(metadata.name, "Analytics Endpoint");
        assert_eq!(metadata.description, "");
        assert_eq!(metadata.renders, vec!["application/json"]);
        assert_eq!(
            metadata.parses,
            vec![
                "application/json",
                "application/x-www-form-urlencoded",
                "multipart/form-data"
            ]
        );
        let body = metadata.render().expect("render");
        assert_eq!(
            std::str::from_utf8(&body).expect("utf8"),
            r#"{"name":"Analytics Endpoint","description":"","renders":["application/json"],"parses":["application/json","application/x-www-form-urlencoded","multipart/form-data"]}"#
        );
    }

    /// A ViewSet's `view_name` carries the router suffix and its `actions` map
    /// is spliced in field order.
    #[test]
    fn actions_are_spliced_in_serializer_order() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| {
                route.view == "plane.app.views.cycle.base.CycleViewSet"
                    && route.path_template.ends_with("/cycles/")
            })
            .expect("cycle list");
        // App routes are plain `path()` + `as_view({...})`, so the router never
        // supplies a `suffix`; `get_view_name` returns the class name only.
        assert_eq!(route.view_name.as_deref(), Some("Cycle"));
        assert_eq!(route.options.attempts, vec![Method::POST]);
        let recorded = route
            .options
            .actions_json
            .get("POST")
            .expect("POST actions");
        assert!(recorded.starts_with("{\"id\":"));
        let metadata = determine(route, &[Method::POST]).expect("metadata");
        let body = String::from_utf8(metadata.render().expect("render")).expect("utf8");
        assert!(body.contains("\"actions\":{\"POST\":{\"id\":{"));
        // The recorded text is embedded verbatim, not re-encoded (which would
        // reorder fields through a sorted map).
        assert!(body.contains(recorded));
    }

    #[test]
    fn actions_need_the_permission_check() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| {
                route.view == "plane.app.views.cycle.base.CycleViewSet"
                    && route.path_template.ends_with("/cycles/")
            })
            .expect("cycle list");
        // No attempt allowed: the `actions` key is absent.
        let metadata = determine(route, &[]).expect("metadata");
        assert!(metadata.actions.is_empty());
        let body = String::from_utf8(metadata.render().expect("render")).expect("utf8");
        assert!(!body.contains("\"actions\""));
    }

    /// A view whose serializer cannot be built answers the OPTIONS with the
    /// family 500 — recorded as `options.error`.
    #[test]
    fn a_recorded_actions_error_is_a_family_500() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| route.options.error.is_some())
            .expect("a view with an actions error");
        let method = route.options.attempts.first().cloned().expect("attempt");
        let error = determine(route, &[method]).expect_err("must fail");
        assert!(matches!(error, MetadataError::ActionsFailed(_)));
    }

    #[test]
    fn the_allow_header_comes_from_the_table() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.analytic.base.AnalyticsEndpoint")
            .expect("analytics");
        assert_eq!(allow_header(route), Some("GET, HEAD, OPTIONS"));
        assert_eq!(
            method_not_allowed(route, &Method::POST),
            ApiError::MethodNotAllowed {
                method: "POST".to_string(),
                allow: "GET, HEAD, OPTIONS".to_string(),
            }
        );
    }
}
