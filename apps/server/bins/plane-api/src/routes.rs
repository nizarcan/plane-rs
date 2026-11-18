// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's URL resolver, rebuilt from `apps/server/routes.json`.
//!
//! The route table is embedded at build time (`include_str!`) and compiled into
//! regexes at startup. It exists for two things only in Phase 1:
//!
//! * `CommonMiddleware`'s `APPEND_SLASH` decision: a 404 whose path plus `/`
//!   *does* resolve becomes a 301 (HTTP_CONTRACT §6.1);
//! * telling a path Django serves from one it does not: the former is answered
//!   with our explicit 501 [`plane_core::error::ApiError::NotImplemented`], the
//!   latter with Django's custom 404 (`plane/app/views/error_404.py`).
//!
//! Faithfulness notes (all verified against Django 5.2.17):
//!
//! * `path()` routes compile to `^` + `_route_to_regex(route)` + `$`; converters
//!   are `str=[^/]+`, `uuid=[0-9a-f]{8}-…` (lowercase only), `int=[0-9]+`,
//!   `slug=[-a-zA-Z0-9_]+`, `path=.+`, and DRF's registered
//!   `drf_format_suffix=\.[a-z0-9]+/?`.
//! * `re_path()` routes are regexes matched with `search()` after the parent
//!   prefix; this matcher flattens the prefix and the pattern, so a leading `^`
//!   at the start of the suffix is dropped (it anchors the *remaining* path in
//!   Django, which is the same position in the flattened string).
//! * First match in `order` wins, like Django's resolver.

use regex::Regex;
use serde::Deserialize;

/// The embedded inventory (`apps/server/routes.json`).
const ROUTES_JSON: &str = include_str!("../../../routes.json");

/// What `plane-api` currently does with a matched route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handler {
    /// `GET /` (any method) — `plane/web/views.py:health_check`.
    Health,
    /// `/robots.txt` (any method) — `plane/web/views.py:robots_txt`.
    Robots,
    /// A route Django serves that this binary has not ported yet.
    Unported,
}

/// One resolver entry.
///
/// `path_template`, `django_pattern` and `params` are kept for diagnostics and
/// tests; matching uses `regex`, capture extraction uses `capture_names`.
#[derive(Debug)]
#[allow(dead_code)]
pub struct Route {
    /// Django's first-match position.
    pub order: u32,
    /// The inventory's `path` template, for diagnostics and tests.
    pub path_template: String,
    /// The inventory's `django_pattern`, for diagnostics and tests.
    pub django_pattern: String,
    /// The dotted view path (`plane.app.views…`).
    pub view: String,
    /// `routes.json` `name` (`request.resolver_match.url_name`).
    pub url_name: Option<String>,
    /// `request.csrf_exempt`: every DRF view is exempt
    /// (`rest_framework/views.py:143-149`).
    pub csrf_exempt: bool,
    /// Converter per named parameter, as recorded by the inventory.
    pub params: std::collections::BTreeMap<String, String>,
    pub handler: Handler,
    regex: Regex,
    /// Capture group names, in pattern order.
    capture_names: Vec<Option<String>>,
}

/// The compiled route table.
#[derive(Debug)]
pub struct RouteTable {
    routes: Vec<Route>,
}

#[derive(Debug, thiserror::Error)]
pub enum RouteTableError {
    #[error("routes.json is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("route {order} ({path:?}): invalid regex {pattern:?}: {source}")]
    Regex {
        order: u32,
        path: String,
        pattern: String,
        source: regex::Error,
    },
    #[error("route {order} ({path:?}): unknown converter {converter:?}")]
    UnknownConverter {
        order: u32,
        path: String,
        converter: String,
    },
}

#[derive(Debug, Deserialize)]
struct RoutesFile {
    routes: Vec<RawRoute>,
}

#[derive(Debug, Deserialize)]
struct RawRoute {
    order: u32,
    path: String,
    django_pattern: String,
    view: String,
    #[serde(default)]
    pattern_type: String,
    #[serde(default)]
    csrf_exempt: bool,
    #[serde(default)]
    params: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    name: Option<String>,
}

impl RouteTable {
    /// Parse and compile the embedded inventory.
    pub fn load() -> Result<Self, RouteTableError> {
        Self::from_json(ROUTES_JSON)
    }

    fn from_json(json: &str) -> Result<Self, RouteTableError> {
        let file: RoutesFile = serde_json::from_str(json)?;
        let mut routes = Vec::with_capacity(file.routes.len());
        for raw in file.routes {
            let source = compile(&raw)?;
            let regex = Regex::new(&source).map_err(|source| RouteTableError::Regex {
                order: raw.order,
                path: raw.path.clone(),
                pattern: source.to_string(),
                source,
            })?;
            let handler = match raw.view.as_str() {
                "plane.web.views.health_check" => Handler::Health,
                "plane.web.views.robots_txt" => Handler::Robots,
                _ => Handler::Unported,
            };
            let capture_names = regex
                .capture_names()
                .map(|name| name.map(str::to_string))
                .collect();
            routes.push(Route {
                order: raw.order,
                path_template: raw.path,
                django_pattern: raw.django_pattern,
                view: raw.view,
                url_name: raw.name,
                csrf_exempt: raw.csrf_exempt,
                params: raw.params,
                handler,
                regex,
                capture_names,
            });
        }
        routes.sort_by_key(|route| route.order);
        Ok(Self { routes })
    }

    /// `django.urls.resolve(path)`: the first entry whose pattern matches.
    pub fn resolve(&self, path: &str) -> Option<&Route> {
        self.routes.iter().find(|route| route.regex.is_match(path))
    }

    /// `django.urls.is_valid_path(path)`.
    #[allow(dead_code)]
    pub fn is_valid_path(&self, path: &str) -> bool {
        self.resolve(path).is_some()
    }

    /// `view.kwargs`: the named captures of the resolved route, in URL order.
    ///
    /// Django's converters decode percent escapes and validate UUID/int/slug
    /// values *before* the view runs; the regexes in `compile` already reject a
    /// non-matching value, and `str` segments arrive as the path decoder left
    /// them (`RequestContext` is percent-decoded once, like ASGI's `scope`
    /// path).
    pub fn kwargs<'a>(&self, route: &'a Route, path: &'a str) -> Vec<(&'a str, &'a str)> {
        let Some(captures) = route.regex.captures(path) else {
            return Vec::new();
        };
        route
            .capture_names
            .iter()
            .skip(1)
            .filter_map(|name| name.as_deref())
            .filter_map(|name| captures.name(name).map(|value| (name, value.as_str())))
            .collect()
    }

    #[allow(dead_code)]
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    pub fn len(&self) -> usize {
        self.routes.len()
    }
}

/// Convert one inventory entry into a regex source string.
fn compile(raw: &RawRoute) -> Result<String, RouteTableError> {
    // For `re_path` routes the inventory records `prefix + pattern`, and the
    // pattern begins at the first `^`. Everything before it is a `path()`
    // prefix that must be translated (and escaped); everything after is regex.
    let (literal, suffix) = if raw.pattern_type == "regex" {
        match raw.django_pattern.find('^') {
            Some(index) => (
                &raw.django_pattern[..index],
                raw.django_pattern[index + 1..].to_string(),
            ),
            None => (raw.django_pattern.as_str(), String::new()),
        }
    } else {
        (raw.django_pattern.as_str(), String::new())
    };

    let mut source = String::with_capacity(literal.len() + suffix.len() + 8);
    // The root URLconf is wrapped in `URLResolver(RegexPattern(r"^/"), …)`, so
    // every inventory route matches against `"/" + django_pattern`.
    source.push_str("^/");
    translate_literal(raw, literal, &mut source)?;
    source.push_str(&suffix);
    // Django anchors RoutePattern with `$`; RegexPattern's `$` is already there
    // for the regex routes, and a duplicated `$` is harmless.
    source.push('$');
    Ok(source)
}

/// Escape literal text and translate `<converter:name>` placeholders.
fn translate_literal(raw: &RawRoute, text: &str, out: &mut String) -> Result<(), RouteTableError> {
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let (before, after) = rest.split_at(start);
        out.push_str(&regex::escape(before));
        let Some(end) = after.find('>') else {
            // A stray `<` is a literal in Django's route syntax only if it is
            // not part of a converter; escaping it matches Django's
            // `_route_to_regex`, which treats it literally.
            out.push_str(&regex::escape(after));
            return Ok(());
        };
        let placeholder = &after[1..end];
        let (converter, name) = placeholder.split_once(':').unwrap_or(("str", placeholder));
        // A named group, so `RouteTable::kwargs` can reproduce `view.kwargs`.
        // Django wraps the converter in a named group too; duplicate names in
        // one pattern would be a Django error before it could reach us.
        out.push_str("(?P<");
        out.push_str(name);
        out.push('>');
        out.push_str(converter_regex(raw, converter)?);
        out.push(')');
        rest = &after[end + 1..];
    }
    out.push_str(&regex::escape(rest));
    Ok(())
}

/// Django's converter regexes (`django/urls/converters.py`) plus DRF's
/// `drf_format_suffix` (`rest_framework/urlpatterns.py:19-27`).
fn converter_regex(raw: &RawRoute, converter: &str) -> Result<&'static str, RouteTableError> {
    match converter {
        // The caller wraps this in `(?P<name>…)`, like Django's
        // `_route_to_regex`. The alternation-free bodies keep the group
        // precedence trivial; a converter that grows one must wrap it itself.
        "str" => Ok("[^/]+"),
        "int" => Ok("[0-9]+"),
        "slug" => Ok("[-a-zA-Z0-9_]+"),
        "path" => Ok(".+"),
        "uuid" => Ok("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"),
        "drf_format_suffix" => Ok("\\.[a-z0-9]+/?"),
        other => Err(RouteTableError::UnknownConverter {
            order: raw.order,
            path: raw.path.clone(),
            converter: other.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> RouteTable {
        RouteTable::load().expect("routes.json compiles")
    }

    /// Every route's sample URL resolves to a route with the same pattern. Two
    /// entries can share a pattern (the `api-root` duplication recorded in
    /// `PROGRESS.md`), so the assertion is on the matched pattern, not the index.
    #[test]
    fn every_route_has_a_matching_sample_url() {
        let table = table();
        // 394 with the three drf-spectacular routes (generated with
        // ENABLE_DRF_SPECTACULAR=1, owner `django`).
        assert_eq!(table.len(), 394, "routes.json entry count");
        let mut checked = 0;
        for route in table.routes() {
            let sample = sample_path(&route.path_template, &route.params);
            let matched = table.resolve(&sample).unwrap_or_else(|| {
                panic!(
                    "route {} ({}) sample {sample} did not match",
                    route.order, route.path_template
                )
            });
            assert_eq!(
                matched.django_pattern, route.django_pattern,
                "route {} sample {sample} matched {} instead",
                route.order, matched.django_pattern
            );
            checked += 1;
        }
        assert_eq!(checked, table.len());
    }

    /// Non-matching URLs must not resolve, or APPEND_SLASH and the 404-vs-501
    /// decision would be wrong.
    #[test]
    fn non_matching_urls() {
        let table = table();
        for path in [
            "/nope/",
            "/api/nope/",
            "/api/",
            "/api",
            "/api/v1/",
            "/api/v1",
            "/robots.txt/",
            "/api/instances",
            "/api/users/me",
            "/api/users/me/extra/",
            "/a/b/c/d/e/f/g/",
            "//api/users/me/",
            "/api/users/me/../me/",
        ] {
            assert!(!table.is_valid_path(path), "{path} unexpectedly resolved");
        }
    }

    #[test]
    fn served_routes() {
        let table = table();
        assert_eq!(
            table.resolve("/").map(|route| route.handler),
            Some(Handler::Health)
        );
        assert_eq!(
            table.resolve("/robots.txt").map(|route| route.handler),
            Some(Handler::Robots)
        );
        assert_eq!(
            table.resolve("/api/instances/").map(|route| route.handler),
            Some(Handler::Unported)
        );
        // APPEND_SLASH: the slashless forms do not resolve, the slashed do.
        assert!(!table.is_valid_path("/api/instances"));
        assert!(table.is_valid_path("/api/instances/"));
    }

    #[test]
    fn uuid_converter_is_lowercase_only() {
        let table = table();
        // `/api/workspaces/{slug}/analytic-view/{pk}/` has a uuid `pk`.
        let path = |value: &str| format!("/api/workspaces/x/analytic-view/{value}/");
        assert!(table.is_valid_path(&path("0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0")));
        assert!(!table.is_valid_path(&path("0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0")));
        // Django's UUIDConverter does not accept missing dashes.
        assert!(!table.is_valid_path(&path("0f1e2d3c4b5a69788796a5b4c3d2e1f0")));
        // `str` converters accept uppercase and dots.
        assert!(table.is_valid_path("/api/workspaces/UPPER.Case/"));
    }

    #[test]
    fn format_suffixes() {
        let table = table();
        assert!(table.is_valid_path("/api/v1/workspaces/x/.json"));
        assert!(table.is_valid_path("/api/v1/workspaces/x/invitations.json"));
        assert!(table.is_valid_path("/api/v1/workspaces/x/invitations/abc.json"));
        assert!(table.is_valid_path("/api/v1/workspaces/x/stickies.json"));
        assert!(!table.is_valid_path("/api/v1/workspaces/x/invitations.JSON"));
    }

    #[test]
    fn two_params_in_one_segment() {
        let table = table();
        assert!(table.is_valid_path("/api/workspaces/x/work-items/ABC-1/"));
        assert!(table.is_valid_path("/api/v1/workspaces/x/issues/ABC-1/"));
        // Two parameters still need the separating dash.
        assert!(!table.is_valid_path("/api/workspaces/x/work-items/ABC/"));
    }

    /// Build a sample URL from the inventory's Django-style `path` template.
    fn sample_path(template: &str, params: &std::collections::BTreeMap<String, String>) -> String {
        let mut out = String::new();
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let name = &rest[start + 1..start + end];
            out.push_str(&sample_value(
                params.get(name).map(String::as_str).unwrap_or("str"),
            ));
            rest = &rest[start + end + 1..];
        }
        out.push_str(rest);
        out
    }

    fn sample_value(converter: &str) -> String {
        match converter {
            "uuid" => "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0".to_string(),
            "int" => "1".to_string(),
            "[a-z0-9]+" => "json".to_string(),
            // `[^/.]+` (the router's `pk`) and `drf_format_suffix`.
            _ => "x".to_string(),
        }
    }

    /// The regex source for one raw inventory entry (used by the sample test to
    /// prove the two-parameter segment compiles as expected).
    #[test]
    fn compiles_the_flat_django_pattern() {
        let raw = RawRoute {
            order: 102,
            path: "/api/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/".to_string(),
            django_pattern:
                "api/workspaces/<str:slug>/work-items/<str:project_identifier>-<str:issue_identifier>/"
                    .to_string(),
            view: "plane.app.views.issue.base.IssueDetailIdentifierEndpoint".to_string(),
            name: None,
            pattern_type: "path".to_string(),
            csrf_exempt: true,
            params: Default::default(),
        };
        let source = compile(&raw).expect("compiles");
        let regex = Regex::new(&source).expect("regex");
        assert!(regex.is_match("/api/workspaces/x/work-items/ABC-1/"));
        assert!(!regex.is_match("/api/workspaces/x/work-items/ABC/"));
        assert!(!regex.is_match("/api/workspaces/x/work-items/ABC-1"));
    }

    #[test]
    fn compiles_a_regex_route() {
        let raw = RawRoute {
            order: 343,
            path: "/api/v1/workspaces/{slug}/invitations/{pk}.{format}".to_string(),
            django_pattern:
                "api/v1/workspaces/<str:slug>/^invitations/(?P<pk>[^/.]+)\\.(?P<format>[a-z0-9]+)/?$"
                    .to_string(),
            view: "x".to_string(),
            name: None,
            pattern_type: "regex".to_string(),
            csrf_exempt: true,
            params: Default::default(),
        };
        let source = compile(&raw).expect("compiles");
        // The prefix is escaped and translated, the suffix stays regex, and the
        // `^` anchors the suffix in the flattened string.
        let regex = Regex::new(&source).expect("regex");
        assert!(regex.is_match("/api/v1/workspaces/x/invitations/y.json"));
        assert!(regex.is_match("/api/v1/workspaces/x/invitations/y.json/"));
        assert!(!regex.is_match("/api/v1/workspaces/x/invitations/y.json/x"));
    }
}
