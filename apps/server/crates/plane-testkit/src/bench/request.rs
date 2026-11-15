// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Pre-rendered request templates for the load generator.
//!
//! Everything that can be computed once is computed once: the path, the query
//! string, every header (including the `Cookie:` header captured by the setup
//! steps), and the body. The measured loop then clones a `Request` and sends
//! it; the only allocation per request is the header map clone and the
//! response body's frames, which the client-headroom measurement in `BENCH.md`
//! bounds.
//!
//! Rendering matches the differential harness's `{{variable}}` syntax exactly
//! (`runner::render_vars`), including the implicit `{{base_url}}`, so a request
//! copied from a differential scenario into a bench scenario keeps working.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use http_body_util::Full;
use hyper::header::{HeaderMap, HeaderName, HeaderValue};
use hyper::{Method, Request};

use crate::bench::scenario::{BenchRequest, BenchScenario};
use crate::config::Query;
use crate::http::DEFAULT_USER_AGENT;

/// One prepared request. `expect_status` is per template because a 404 probe
/// is a legitimate part of the mix and must not count as an error.
#[derive(Debug, Clone)]
pub struct RequestBlueprint {
    pub name: String,
    pub method: Method,
    pub path_and_query: String,
    pub expect_status: u16,
    request: Request<Full<Bytes>>,
}

impl RequestBlueprint {
    /// A request with no body and a `Host` header, for the headroom server.
    pub fn simple(base_url: &str, path: &str) -> Result<Self> {
        let (authority, prefix) = authority_and_prefix(base_url)?;
        let path_and_query = format!("{prefix}{path}");
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("host"),
            HeaderValue::from_str(&authority)
                .context("base URL authority is not a header value")?,
        );
        let request = Request::builder()
            .method(Method::GET)
            .uri(path_and_query.as_str())
            .body(Full::new(Bytes::new()))
            .context("building the headroom request")?;
        let mut request = request;
        *request.headers_mut() = headers;
        Ok(Self {
            name: "headroom".to_string(),
            method: Method::GET,
            path_and_query,
            expect_status: 200,
            request,
        })
    }

    /// The request to send. `Request<Full<Bytes>>` is `Clone`: the body is a
    /// refcounted `Bytes` and the header map is a small clone.
    pub fn request(&self) -> Request<Full<Bytes>> {
        self.request.clone()
    }
}

/// Render every request template against the scenario variables and the
/// values the setup steps captured, plus the cookie header of each named jar.
pub fn build_blueprints(
    scenario: &BenchScenario,
    base_url: &str,
    vars: &BTreeMap<String, String>,
    cookies: &BTreeMap<String, String>,
) -> Result<Vec<RequestBlueprint>> {
    let (authority, prefix) = authority_and_prefix(base_url)?;
    let mut blueprints = Vec::with_capacity(scenario.requests.len());
    for template in &scenario.requests {
        blueprints.push(build_one(
            template, &authority, &prefix, base_url, vars, cookies,
        )?);
    }
    Ok(blueprints)
}

fn build_one(
    template: &BenchRequest,
    authority: &str,
    prefix: &str,
    base_url: &str,
    vars: &BTreeMap<String, String>,
    cookies: &BTreeMap<String, String>,
) -> Result<RequestBlueprint> {
    let where_ = format!("request template {:?}", template.name);
    let mut path_and_query = format!("{prefix}{}", render_vars(&template.path, vars, base_url)?);
    if let Some(query) = &template.query {
        let encoded = match query {
            Query::Map(map) => {
                let mut pairs = Vec::with_capacity(map.len());
                for (key, value) in map {
                    pairs.push((
                        render_vars(key, vars, base_url)?,
                        render_vars(value, vars, base_url)?,
                    ));
                }
                form_encode(&pairs)
            }
            Query::Raw(raw) => render_vars(raw, vars, base_url)?,
        };
        if !encoded.is_empty() {
            path_and_query.push(if path_and_query.contains('?') {
                '&'
            } else {
                '?'
            });
            path_and_query.push_str(&encoded);
        }
    }

    let method = Method::from_bytes(template.method.as_bytes())
        .with_context(|| format!("{where_}: invalid method {:?}", template.method))?;

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("host"),
        HeaderValue::from_str(authority)
            .with_context(|| format!("{where_}: base URL authority is not a header value"))?,
    );
    if !template
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("accept-encoding"))
    {
        headers.insert(
            HeaderName::from_static("accept-encoding"),
            HeaderValue::from_static("gzip"),
        );
    }
    if !template
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("user-agent"))
    {
        headers.insert(
            HeaderName::from_static("user-agent"),
            HeaderValue::from_static(DEFAULT_USER_AGENT),
        );
    }
    let mut explicit_cookie = false;
    for (name, value) in &template.headers {
        if name.eq_ignore_ascii_case("cookie") {
            explicit_cookie = true;
        }
        headers.insert(
            HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("{where_}: invalid header name {name:?}"))?,
            HeaderValue::from_str(&render_vars(value, vars, base_url)?)
                .with_context(|| format!("{where_}: invalid value for header {name:?}"))?,
        );
    }
    if !explicit_cookie
        && let Some(cookie) = cookies.get(&template.jar)
        && !cookie.is_empty()
    {
        headers.insert(
            HeaderName::from_static("cookie"),
            HeaderValue::from_str(cookie).with_context(|| {
                format!(
                    "{where_}: the {:?} cookie jar is not a header value",
                    template.jar
                )
            })?,
        );
    }

    let (body, content_type) = match (&template.json, &template.form, &template.body) {
        (Some(json), _, _) => {
            let mut rendered = json.clone();
            render_json_vars(&mut rendered, vars, base_url)?;
            (
                serde_json::to_vec(&rendered)
                    .with_context(|| format!("{where_}: serialising JSON"))?,
                Some("application/json"),
            )
        }
        (_, Some(form), _) => {
            let mut pairs = Vec::with_capacity(form.len());
            for (key, value) in form {
                pairs.push((
                    render_vars(key, vars, base_url)?,
                    render_vars(value, vars, base_url)?,
                ));
            }
            (
                form_encode(&pairs).into_bytes(),
                Some("application/x-www-form-urlencoded"),
            )
        }
        (_, _, Some(raw)) => (
            render_vars(raw, vars, base_url)?.into_bytes(),
            Some("text/plain;charset=UTF-8"),
        ),
        _ => (Vec::new(), None),
    };
    if let Some(content_type) = content_type
        && !headers.contains_key("content-type")
    {
        headers.insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static(content_type),
        );
    }

    let request = Request::builder()
        .method(method.clone())
        .uri(path_and_query.as_str())
        .body(Full::new(Bytes::from(body)))
        .with_context(|| format!("{where_}: building the request for {path_and_query:?}"))?;
    let mut request = request;
    *request.headers_mut() = headers;

    Ok(RequestBlueprint {
        name: template.name.clone(),
        method,
        path_and_query,
        expect_status: template.expect_status,
        request,
    })
}

/// `http://host:port[/prefix]` into its `Host` value and path prefix.
pub fn authority_and_prefix(base_url: &str) -> Result<(String, String)> {
    let rest = base_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .ok_or_else(|| anyhow::anyhow!("base URL {base_url:?} has no scheme"))?;
    let (authority, prefix) = match rest.split_once('/') {
        Some((authority, prefix)) => (authority, format!("/{}", prefix.trim_end_matches('/'))),
        None => (rest, String::new()),
    };
    if authority.is_empty() {
        bail!("base URL {base_url:?} has no host");
    }
    let prefix = if prefix == "/" { String::new() } else { prefix };
    Ok((authority.to_string(), prefix))
}

fn template_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"\{\{\s*([A-Za-z0-9_.-]+)\s*\}\}").expect("static template regex")
    })
}

/// The differential harness's variable rendering, byte for byte
/// (`runner::render_vars`): `{{name}}` with optional spaces, and `base_url`
/// always available.
pub fn render_vars(
    template: &str,
    vars: &BTreeMap<String, String>,
    base_url: &str,
) -> Result<String> {
    let mut missing: Option<String> = None;
    let rendered = template_regex()
        .replace_all(template, |captures: &regex::Captures<'_>| {
            let name = captures
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or_default()
                .to_string();
            if name == "base_url" {
                return base_url.to_string();
            }
            match vars.get(&name) {
                Some(value) => value.clone(),
                None => {
                    missing = Some(name.clone());
                    String::new()
                }
            }
        })
        .into_owned();
    if let Some(name) = missing {
        bail!("undefined scenario variable {name:?} in {template:?}");
    }
    Ok(rendered)
}

pub(crate) fn render_json_vars(
    value: &mut serde_json::Value,
    vars: &BTreeMap<String, String>,
    base_url: &str,
) -> Result<()> {
    match value {
        serde_json::Value::String(text) => {
            *text = render_vars(text, vars, base_url)?;
        }
        serde_json::Value::Array(items) => {
            for item in items {
                render_json_vars(item, vars, base_url)?;
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                render_json_vars(item, vars, base_url)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Same encoding as `runner::url_encode`: unreserved characters stay, every
/// other byte becomes `%XX` (space is `%20`, not `+`).
fn url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn form_encode(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", url_encode(key), url_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(text: &str) -> BenchScenario {
        let scenario: BenchScenario = serde_yaml_ng::from_str(text).expect("parse");
        scenario.validate().expect("valid");
        scenario
    }

    fn header(blueprint: &RequestBlueprint, name: &str) -> Option<String> {
        blueprint
            .request()
            .headers()
            .get(name)
            .map(|value| value.to_str().unwrap_or_default().to_string())
    }

    #[test]
    fn variables_are_rendered_and_missing_ones_fail() {
        let vars = BTreeMap::from([("slug".to_string(), "ws".to_string())]);
        assert_eq!(
            render_vars("/api/workspaces/{{ slug }}/", &vars, "http://127.0.0.1:1")
                .expect("render"),
            "/api/workspaces/ws/"
        );
        assert_eq!(
            render_vars("{{base_url}}/x", &vars, "http://127.0.0.1:1").expect("render"),
            "http://127.0.0.1:1/x"
        );
        let error = render_vars("{{nope}}", &vars, "http://x").expect_err("missing");
        assert!(error.to_string().contains("\"nope\""));
    }

    #[test]
    fn a_blueprint_carries_host_query_and_cookie() {
        let scenario = scenario(
            r#"
name: t
requests:
  - name: issues
    path: /api/workspaces/{{slug}}/issues/
    query:
      cursor: "{{cursor}}"
    jar: user
"#,
        );
        let vars = BTreeMap::from([
            ("slug".to_string(), "acme".to_string()),
            ("cursor".to_string(), "1:0:0".to_string()),
        ]);
        let cookies = BTreeMap::from([("user".to_string(), "session-id=abc".to_string())]);
        let blueprints =
            build_blueprints(&scenario, "http://127.0.0.1:18100", &vars, &cookies).expect("build");
        assert_eq!(blueprints.len(), 1);
        let blueprint = &blueprints[0];
        assert_eq!(
            blueprint.path_and_query,
            "/api/workspaces/acme/issues/?cursor=1%3A0%3A0"
        );
        assert_eq!(
            header(blueprint, "host").as_deref(),
            Some("127.0.0.1:18100")
        );
        assert_eq!(
            header(blueprint, "cookie").as_deref(),
            Some("session-id=abc")
        );
        // The defaults a browser would send are present unless overridden.
        assert_eq!(
            header(blueprint, "accept-encoding").as_deref(),
            Some("gzip")
        );
        assert_eq!(
            header(blueprint, "user-agent").as_deref(),
            Some(DEFAULT_USER_AGENT)
        );
    }

    #[test]
    fn a_base_url_prefix_is_kept_and_the_host_header_is_the_authority() {
        let scenario = scenario("name: t\nrequests:\n  - name: r\n    path: /x\n");
        let blueprints = build_blueprints(
            &scenario,
            "http://127.0.0.1:18080/api-prefix",
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .expect("build");
        assert_eq!(blueprints[0].path_and_query, "/api-prefix/x");
        assert_eq!(
            header(&blueprints[0], "host").as_deref(),
            Some("127.0.0.1:18080")
        );
    }

    #[test]
    fn a_json_body_is_rendered_recursively_and_gets_a_content_type() {
        let scenario = scenario(
            r#"
name: t
requests:
  - name: create
    method: POST
    path: /api/things/
    json: {name: "{{name}}", nested: ["{{name}}"]}
"#,
        );
        let vars = BTreeMap::from([("name".to_string(), "x".to_string())]);
        let blueprints =
            build_blueprints(&scenario, "http://h:1", &vars, &BTreeMap::new()).expect("build");
        assert_eq!(
            header(&blueprints[0], "content-type").as_deref(),
            Some("application/json")
        );
        assert_eq!(blueprints[0].method, Method::POST);
    }

    #[test]
    fn an_explicit_header_wins_over_the_default_cookie_and_content_type() {
        let scenario = scenario(
            r#"
name: t
requests:
  - name: r
    method: POST
    path: /x
    jar: user
    headers:
      Cookie: "session-id=explicit"
      Content-Type: application/json
    body: "{}"
"#,
        );
        let cookies = BTreeMap::from([("user".to_string(), "session-id=jar".to_string())]);
        let blueprints =
            build_blueprints(&scenario, "http://h:1", &BTreeMap::new(), &cookies).expect("build");
        assert_eq!(
            header(&blueprints[0], "cookie").as_deref(),
            Some("session-id=explicit")
        );
        assert_eq!(
            header(&blueprints[0], "content-type").as_deref(),
            Some("application/json")
        );
    }

    #[test]
    fn an_invalid_path_is_reported_at_build_time() {
        let scenario = scenario("name: t\nrequests:\n  - name: r\n    path: /two words\n");
        let error = build_blueprints(&scenario, "http://h:1", &BTreeMap::new(), &BTreeMap::new())
            .expect_err("must reject");
        assert!(
            error.to_string().contains("building the request"),
            "{error}"
        );
    }

    #[test]
    fn a_blueprint_is_cloneable_and_keeps_its_expectation() {
        let scenario = scenario(
            "name: t\nrequests:\n  - name: missing\n    path: /nope\n    expect_status: 404\n",
        );
        let blueprints =
            build_blueprints(&scenario, "http://h:1", &BTreeMap::new(), &BTreeMap::new())
                .expect("build");
        assert_eq!(blueprints[0].expect_status, 404);
        let cloned = blueprints[0].request();
        assert_eq!(cloned.uri().path(), "/nope");
    }
}
