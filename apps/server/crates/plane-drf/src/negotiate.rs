// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `DefaultContentNegotiation` (`rest_framework/negotiation.py`), with Plane's
//! single renderer.
//!
//! Almost every Plane route has exactly one renderer, `JSONRenderer`
//! (`plane/settings/common.py:147`), so the observable outcomes are:
//!
//! | Request | Result |
//! | --- | --- |
//! | `Accept` matches `application/json` (or `*/*`) | 200, rendered by the JSON renderer |
//! | `Accept` matches nothing | 406 `{"detail":"Could not satisfy the request Accept header."}` |
//! | `?format=xml` or `/{path}.xml` | 404 `{"detail":"Not found."}` — `filter_renderers` raises `Http404` |
//! | `?format=json` / `/{path}.json` | 200 (then the `Accept` header must still match) |
//!
//! Both errors are raised inside `initial()` **before** authentication, so an
//! anonymous request with a bad `Accept` gets 406, not 401.
//!
//! The three drf-spectacular views are the only routes with more than one
//! renderer (`drf_spectacular/views.py:45-49,122-137`): the schema view offers
//! YAML and JSON in two media types each, the two HTML pages offer
//! `text/html`. [`select_renderer_from`] is the route-aware form and
//! [`renderer_specs`] maps the class names `routes.json` records; a class that
//! is not mapped is refused by the dispatcher rather than negotiated as JSON.
//!
//! The returned *accepted media type* never reaches the wire in Plane: the
//! renderer's `Content-Type` is its own `media_type` and `charset` is `None`,
//! and DRF only adds `Vary: Accept` when there is more than one renderer. The
//! match/no-match decision and the *format* filter are therefore the whole
//! contract; the doc comments still name the Python rules for the next reader.
//!
//! Known gap: when a single precedence bucket holds two tokens that both match
//! and whose parameters differ, CPython returns whichever the `set` iterates
//! first — and `str` hashing is randomised per process, so Django itself is not
//! deterministic there. This port takes the earliest token in header order.

use std::collections::BTreeMap;

use plane_core::ApiError;

use crate::request::ViewRequest;

/// `JSONRenderer.media_type` / `.format`.
pub const JSON_MEDIA_TYPE: &str = "application/json";
pub const JSON_FORMAT: &str = "json";

/// `DefaultContentNegotiation.max_accept_header_length`.
const MAX_ACCEPT_HEADER_LENGTH: usize = 8192;
/// `DefaultContentNegotiation.max_accept_tokens`.
const MAX_ACCEPT_TOKENS: usize = 64;
/// `DefaultContentNegotiation.max_media_type_length` (and DRF's
/// `MAX_MEDIA_TYPE_LENGTH`).
pub const MAX_MEDIA_TYPE_LENGTH: usize = 256;

/// A parsed media type (`rest_framework/utils/mediatypes.py:_MediaType`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaType {
    /// The original text, returned verbatim when the client's type is more
    /// specific than the renderer's.
    pub orig: String,
    /// `main/sub` after `parse_header_parameters`, lower-cased.
    pub full_type: String,
    pub params: BTreeMap<String, String>,
    pub main_type: String,
    pub sub_type: String,
}

impl MediaType {
    pub fn parse(value: &str) -> Self {
        let (full_type, params) = parse_header_parameters(value);
        let (main_type, sub_type) = match full_type.split_once('/') {
            Some((main, sub)) => (main.to_string(), sub.to_string()),
            None => (full_type.clone(), String::new()),
        };
        Self {
            orig: value.to_string(),
            full_type,
            params,
            main_type,
            sub_type,
        }
    }

    /// `_MediaType.match` for `self = renderer`, `other = client token`.
    pub fn matches(&self, other: &MediaType) -> bool {
        for (key, value) in &self.params {
            if key == "q" {
                continue;
            }
            if other.params.get(key) != Some(value) {
                return false;
            }
        }
        if self.sub_type != "*" && other.sub_type != "*" && other.sub_type != self.sub_type {
            return false;
        }
        if self.main_type != "*" && other.main_type != "*" && other.main_type != self.main_type {
            return false;
        }
        true
    }

    /// `_MediaType.precedence`: 3 with params, 2 without, 1 `type/*`, 0 `*/*`.
    pub fn precedence(&self) -> u8 {
        if self.main_type == "*" {
            0
        } else if self.sub_type == "*" {
            1
        } else if self.params.is_empty() || self.params.keys().all(|key| key == "q") {
            2
        } else {
            3
        }
    }
}

/// `django.utils.http.parse_header_parameters`.
///
/// Splits on `;` outside quotes, lower-cases the main type and parameter names,
/// strips one layer of quotes and unescapes `\\` / `\"`. Unknown parameter
/// forms are kept as-is, exactly like Django.
pub fn parse_header_parameters(line: &str) -> (String, BTreeMap<String, String>) {
    let mut parts = split_parameters(line).into_iter();
    let full_type = parts.next().unwrap_or_default().to_ascii_lowercase();
    let mut params = BTreeMap::new();
    for part in parts {
        let Some(index) = part.find('=') else {
            continue;
        };
        let mut name = part[..index].trim().to_ascii_lowercase();
        let mut value = part[index + 1..].trim().to_string();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = value[1..value.len() - 1]
                .replace("\\\\", "\\")
                .replace("\\\"", "\"");
        }
        if let Some(stripped) = name.strip_suffix('*') {
            name = stripped.to_string();
            if value.matches('\'').count() == 2 {
                // `filename*=UTF-8''file.ext`: the port does not decode the
                // embedded-charset form (only used by multipart filenames).
                if let Some((_, rest)) = value.split_once('\'')
                    && let Some((_, tail)) = rest.split_once('\'')
                {
                    value = tail.to_string();
                }
            }
        }
        params.insert(name, value);
    }
    (full_type, params)
}

/// `_parseparam`: the text before each unquoted `;` of `";" + line`.
fn split_parameters(line: &str) -> Vec<String> {
    let mut rest = format!(";{line}");
    let mut parts = Vec::new();
    while rest.starts_with(';') {
        rest.remove(0);
        let bytes = rest.as_bytes();
        let mut quotes = 0i64;
        let mut escaped = 0i64;
        let mut end = rest.len();
        for (index, byte) in bytes.iter().enumerate() {
            match byte {
                b'"' => quotes += 1,
                b'\\' if bytes.get(index + 1) == Some(&b'"') => escaped += 1,
                b';' if (quotes - escaped) % 2 == 0 => {
                    end = index;
                    break;
                }
                _ => {}
            }
        }
        parts.push(rest[..end].trim().to_string());
        rest = rest[end..].to_string();
    }
    parts
}

/// `rest_framework.request.is_form_media_type` (`request.py:27-35`).
pub fn is_form_media_type(media_type: &str) -> bool {
    if media_type.len() > MAX_MEDIA_TYPE_LENGTH {
        return false;
    }
    let (base, _) = parse_header_parameters(media_type);
    base == "application/x-www-form-urlencoded" || base == "multipart/form-data"
}

/// `DefaultContentNegotiation.get_accept_list`.
pub fn accept_tokens(request: &ViewRequest<'_>) -> Vec<String> {
    let mut header = request.ctx.header("accept").unwrap_or("*/*").to_string();
    if header.len() > MAX_ACCEPT_HEADER_LENGTH {
        // Truncate at a separator so a truncated tail cannot read as a type.
        let prefix = &header[..MAX_ACCEPT_HEADER_LENGTH];
        header = match prefix.rfind(',') {
            Some(index) => header[..index + 1].to_string(),
            None => String::new(),
        };
    }
    let mut tokens = Vec::new();
    for token in header.split(',') {
        let token = token.trim();
        if token.is_empty() || token.len() > MAX_MEDIA_TYPE_LENGTH {
            continue;
        }
        tokens.push(token.to_string());
        if tokens.len() >= MAX_ACCEPT_TOKENS {
            break;
        }
    }
    tokens
}

/// One entry of a `routes.json` `renderer_classes` list, in the terms
/// `DefaultContentNegotiation` needs.
///
/// Plane has one renderer for almost every route (`JSONRenderer`); the three
/// drf-spectacular views under `/api/schema/` are the only exceptions, and they
/// are why negotiation has to be route-aware
/// (`drf_spectacular/views.py:45-49,122-137`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererSpec {
    /// `renderer.media_type`.
    pub media_type: &'static str,
    /// `renderer.format` (`filter_renderers` compares it with `?format=`).
    pub format: &'static str,
}

/// `JSONRenderer` (`rest_framework/renderers.py`).
pub const JSON_RENDERER: RendererSpec = RendererSpec {
    media_type: JSON_MEDIA_TYPE,
    format: JSON_FORMAT,
};

/// The renderer classes this port knows how to negotiate, as Django's
/// `renderer_classes` records them in `routes.json`.
///
/// A class missing here is a route whose rendering has not been ported; the
/// dispatcher refuses it rather than negotiating with the wrong media type.
pub fn renderer_specs(classes: &[String]) -> Option<Vec<RendererSpec>> {
    let mut specs = Vec::with_capacity(classes.len());
    for class in classes {
        specs.push(match class.as_str() {
            "rest_framework.renderers.JSONRenderer" => JSON_RENDERER,
            // `OpenApiYamlRenderer2`/`OpenApiJsonRenderer2` only override the
            // media type; the format is what `?format=` filters on.
            "drf_spectacular.renderers.OpenApiYamlRenderer"
            | "drf_spectacular.renderers.OpenApiYamlRenderer2" => RendererSpec {
                media_type: if class.ends_with('2') {
                    "application/yaml"
                } else {
                    "application/vnd.oai.openapi"
                },
                format: "yaml",
            },
            "drf_spectacular.renderers.OpenApiJsonRenderer"
            | "drf_spectacular.renderers.OpenApiJsonRenderer2" => RendererSpec {
                media_type: if class.ends_with('2') {
                    JSON_MEDIA_TYPE
                } else {
                    "application/vnd.oai.openapi+json"
                },
                format: JSON_FORMAT,
            },
            "rest_framework.renderers.TemplateHTMLRenderer" => RendererSpec {
                media_type: "text/html",
                format: "html",
            },
            _ => return None,
        });
    }
    Some(specs)
}

/// `DefaultContentNegotiation.select_renderer` for Plane's renderer set.
///
/// Returns the renderer **format** (`"json"`). The error is the exception DRF
/// raises, ready for `handle_exception`.
pub fn select_renderer(request: &ViewRequest<'_>) -> Result<&'static str, ApiError> {
    select_renderer_from(&[JSON_RENDERER], request).map(|spec| spec.format)
}

/// `DefaultContentNegotiation.select_renderer` over an explicit renderer list.
///
/// Returns the selected renderer. The order is DRF's: `?format=` (or a format
/// suffix) filters first, then the Accept tokens are bucketed by precedence
/// (3 → 0) and, within a bucket, renderers are tried in declaration order.
pub fn select_renderer_from<'a>(
    renderers: &'a [RendererSpec],
    request: &ViewRequest<'_>,
) -> Result<&'a RendererSpec, ApiError> {
    // `filter_renderers` keeps the renderers whose `format` matches; an empty
    // result is `Http404`, not 406.
    let filtered: Vec<&RendererSpec> = match request.negotiation_format() {
        Some(format) => {
            let kept: Vec<&RendererSpec> = renderers
                .iter()
                .filter(|renderer| renderer.format == format)
                .collect();
            if kept.is_empty() {
                return Err(ApiError::NotFound);
            }
            kept
        }
        None => renderers.iter().collect(),
    };

    let tokens: Vec<MediaType> = accept_tokens(request)
        .iter()
        .map(|token| MediaType::parse(token))
        .collect();

    // `order_parsed_by_precedence`: buckets 3, 2, 1, 0, empty buckets dropped.
    for precedence in (0..=3u8).rev() {
        for renderer in &filtered {
            let media_type = MediaType::parse(renderer.media_type);
            for token in tokens
                .iter()
                .filter(|token| token.precedence() == precedence)
            {
                if media_type.matches(token) {
                    // The accepted media type (`token.orig` or the renderer's
                    // type plus the token's parameters) only reaches the wire
                    // through `Content-Type`, which is always the renderer's
                    // own `media_type` plus its charset in DRF
                    // (`rest_framework/renderers.py:BaseRenderer`). It is
                    // therefore not returned.
                    return Ok(renderer);
                }
            }
        }
    }
    Err(ApiError::NotAcceptable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, Method};
    use plane_core::context::RequestContext;

    use crate::policy::PolicyTable;

    fn request<'a>(
        policy: &'a PolicyTable,
        headers: &[(&str, &str)],
        query: &str,
    ) -> ViewRequest<'a> {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                http::HeaderName::from_bytes(name.as_bytes()).expect("name"),
                http::HeaderValue::from_str(value).expect("value"),
            );
        }
        let ctx = Box::leak(Box::new(RequestContext::new(
            Method::GET,
            "/api/workspaces/x/",
            query,
            map,
            None,
        )));
        let route = policy
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.analytic.base.AnalyticsEndpoint")
            .expect("route");
        ViewRequest::new(ctx, route, Vec::new()).expect("query")
    }

    fn policy() -> PolicyTable {
        PolicyTable::load().expect("routes.json")
    }

    #[test]
    fn media_type_parsing_and_precedence() {
        let parsed = MediaType::parse("Application/JSON; Charset=\"UTF-8\"; q=0.5");
        assert_eq!(parsed.full_type, "application/json");
        assert_eq!(parsed.main_type, "application");
        assert_eq!(parsed.sub_type, "json");
        assert_eq!(parsed.params.get("charset"), Some(&"UTF-8".to_string()));
        assert_eq!(parsed.precedence(), 3);

        assert_eq!(MediaType::parse("application/json").precedence(), 2);
        assert_eq!(MediaType::parse("application/*").precedence(), 1);
        assert_eq!(MediaType::parse("*/*").precedence(), 0);
        assert_eq!(MediaType::parse("*/*;q=0.1").precedence(), 0);

        let renderer = MediaType::parse("application/json");
        assert!(renderer.matches(&MediaType::parse("*/*")));
        assert!(renderer.matches(&MediaType::parse("application/*")));
        assert!(renderer.matches(&MediaType::parse("application/json")));
        assert!(renderer.matches(&MediaType::parse("application/json; indent=4")));
        assert!(!renderer.matches(&MediaType::parse("text/html")));
        assert!(!renderer.matches(&MediaType::parse("application/xml")));
        assert!(!renderer.matches(&MediaType::parse("")));
    }

    #[test]
    fn renderer_selection() {
        let policy = policy();
        assert_eq!(select_renderer(&request(&policy, &[], "")), Ok("json"));
        assert_eq!(
            select_renderer(&request(&policy, &[("accept", "application/json")], "")),
            Ok("json")
        );
        assert_eq!(
            select_renderer(&request(&policy, &[("accept", "text/*, */*;q=0.1")], "")),
            Ok("json")
        );
        assert_eq!(
            select_renderer(&request(&policy, &[("accept", "text/html")], "")),
            Err(ApiError::NotAcceptable)
        );
        // `?format=xml` raises Http404 before the Accept header is read.
        assert_eq!(
            select_renderer(&request(
                &policy,
                &[("accept", "application/json")],
                "format=xml"
            )),
            Err(ApiError::NotFound)
        );
        // An empty `?format=` is falsy, like Python.
        assert_eq!(
            select_renderer(&request(
                &policy,
                &[("accept", "application/json")],
                "format="
            )),
            Ok("json")
        );
        // The last value wins in a QueryDict.
        assert_eq!(
            select_renderer(&request(&policy, &[], "format=xml&format=json")),
            Ok("json")
        );
    }

    #[test]
    fn accept_tokens_are_bounded() {
        let policy = policy();
        let long = "a".repeat(300);
        let long_request = request(&policy, &[("accept", &long)], "");
        assert!(accept_tokens(&long_request).is_empty());

        let many = (0..100)
            .map(|_| "application/json")
            .collect::<Vec<_>>()
            .join(",");
        let many_request = request(&policy, &[("accept", &many)], "");
        assert_eq!(accept_tokens(&many_request).len(), MAX_ACCEPT_TOKENS);

        let empty_request = request(&policy, &[("accept", "")], "");
        assert_eq!(
            select_renderer(&empty_request),
            Err(ApiError::NotAcceptable),
            "an empty Accept header matches nothing"
        );
    }

    #[test]
    fn malformed_accept_matches_nothing() {
        let parsed = MediaType::parse("not a media type");
        assert_eq!(parsed.main_type, "not a media type");
        assert_eq!(parsed.sub_type, "");
        assert!(!MediaType::parse("application/json").matches(&parsed));
    }
}
