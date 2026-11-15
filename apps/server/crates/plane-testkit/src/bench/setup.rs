// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The one-off `setup` phase of a bench scenario.
//!
//! Setup runs through the differential harness's own HTTP client
//! (`crate::http::HttpClient`): redirects are not followed, cookies are kept in
//! named jars, gzip bodies are decoded, and captures support the same four
//! sources (`json`, `header`, `cookie`, `regex`) as a differential step. The
//! captured values and the jars' `Cookie:` headers are what the measured
//! request templates are rendered from, so an authenticated read path will be
//! benchmarked exactly the way the differential harness signs in.
//!
//! This duplicates the capture half of `runner::collect_captures` (the runner
//! keeps it private behind its `Side` type). The behaviour is intentionally
//! identical; the tests here and the differential harness's selftest both cover
//! it. Keep the two in step.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};

use crate::bench::request::{render_json_vars, render_vars};
use crate::config::{CaptureSource, Query, Step};
use crate::http::{HttpClient, HttpRequest, HttpResponse};
use crate::util::json_pointer;

/// What the setup steps left behind: the client (with its cookie jars), the
/// captured variables, and the `Cookie:` header for every jar a request
/// template names.
pub struct SetupOutcome {
    pub client: HttpClient,
    pub vars: BTreeMap<String, String>,
    pub cookies: BTreeMap<String, String>,
}

/// Run the setup steps against `base_url`. A step without `expect_status` must
/// answer below 400; a setup that quietly 401s would otherwise poison every
/// measured request.
pub async fn run_setup(
    base_url: &str,
    scenario: &crate::bench::scenario::BenchScenario,
) -> Result<SetupOutcome> {
    let mut client = HttpClient::new(base_url)?;
    let mut vars = scenario.vars.clone();
    for (index, step) in scenario.setup.iter().enumerate() {
        let number = index + 1;
        let request = build_step_request(&client, step, &vars)
            .with_context(|| format!("setup step {number} ({})", step.name))?;
        let response = client
            .send(&request)
            .await
            .with_context(|| format!("setup step {number} ({})", step.name))?;
        match step.expect_status {
            Some(expected) if response.status != expected => bail!(
                "setup step {number} ({}): expected status {expected}, got {}",
                step.name,
                response.status
            ),
            None if response.status >= 400 => bail!(
                "setup step {number} ({}): unexpected status {}; set `expect_status` if the \
                 step is meant to fail",
                step.name,
                response.status
            ),
            _ => {}
        }
        let captures = collect_captures(&client, step, &response)
            .with_context(|| format!("setup step {number} ({})", step.name))?;
        vars.extend(captures);
    }

    // One Cookie header per jar the measured templates use. A jar that was
    // never populated is an empty header, not an error: an anonymous template
    // that names a jar by habit still works.
    let jars: BTreeSet<&str> = scenario
        .requests
        .iter()
        .map(|request| request.jar.as_str())
        .collect();
    let mut cookies = BTreeMap::new();
    for jar in jars {
        let header = client
            .jar(jar)
            .and_then(|jar| jar.cookie_header())
            .unwrap_or_default();
        cookies.insert(jar.to_string(), header);
    }

    Ok(SetupOutcome {
        client,
        vars,
        cookies,
    })
}

fn build_step_request(
    client: &HttpClient,
    step: &Step,
    vars: &BTreeMap<String, String>,
) -> Result<HttpRequest> {
    let base_url = client.base_url();
    let path = render_vars(&step.path, vars, base_url)?;
    let mut path_and_query = path;
    if let Some(query) = &step.query {
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

    let mut headers = Vec::with_capacity(step.headers.len() + 1);
    let mut body = None;
    let mut content_type: Option<&'static str> = None;
    if let Some(json) = &step.json {
        let mut rendered = json.clone();
        render_json_vars(&mut rendered, vars, base_url)?;
        body = Some(serde_json::to_vec(&rendered).context("serialising the setup JSON body")?);
        content_type = Some("application/json");
    } else if let Some(form) = &step.form {
        let mut pairs = Vec::with_capacity(form.len());
        for (key, value) in form {
            pairs.push((
                render_vars(key, vars, base_url)?,
                render_vars(value, vars, base_url)?,
            ));
        }
        body = Some(form_encode(&pairs).into_bytes());
        content_type = Some("application/x-www-form-urlencoded");
    } else if let Some(raw) = &step.body {
        body = Some(render_vars(raw, vars, base_url)?.into_bytes());
        content_type = Some("text/plain;charset=UTF-8");
    }
    let has_content_type = step
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("content-type"));
    if let Some(content_type) = content_type
        && !has_content_type
    {
        headers.push(("Content-Type".to_string(), content_type.to_string()));
    }
    for (name, value) in &step.headers {
        headers.push((name.clone(), render_vars(value, vars, base_url)?));
    }

    Ok(HttpRequest {
        method: step.method.clone(),
        path_and_query,
        headers,
        body,
        jar: step.jar.clone(),
    })
}

fn collect_captures(
    client: &HttpClient,
    step: &Step,
    response: &HttpResponse,
) -> Result<BTreeMap<String, String>> {
    let mut captures = BTreeMap::new();
    for capture in &step.capture {
        let value = match capture.source {
            CaptureSource::Json => {
                let pointer = capture.pointer.as_deref().unwrap_or_default();
                let body: serde_json::Value =
                    serde_json::from_slice(&response.body).with_context(|| {
                        format!("capture {} needs a JSON response body", capture.name)
                    })?;
                let found = json_pointer(&body, pointer)
                    .with_context(|| format!("capture {}", capture.name))?;
                match found {
                    serde_json::Value::String(text) => text.clone(),
                    other => serde_json::to_string(other)?,
                }
            }
            CaptureSource::Header => {
                let name = capture.header.as_deref().unwrap_or_default();
                response.header(name).map(str::to_string).ok_or_else(|| {
                    anyhow::anyhow!("capture {}: header {name} missing", capture.name)
                })?
            }
            CaptureSource::Cookie => {
                let name = capture.cookie.as_deref().unwrap_or_default();
                client
                    .jar(&step.jar)
                    .and_then(|jar| jar.get(name))
                    .map(str::to_string)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "capture {}: cookie {name} missing from jar {}",
                            capture.name,
                            step.jar
                        )
                    })?
            }
            CaptureSource::Regex => {
                let header = capture.header.as_deref().unwrap_or_default();
                let pattern = capture.pattern.as_deref().unwrap_or_default();
                let haystack = response.header(header).ok_or_else(|| {
                    anyhow::anyhow!("capture {}: header {header} missing", capture.name)
                })?;
                let regex = regex::Regex::new(pattern).with_context(|| {
                    format!("capture {}: invalid regex {pattern:?}", capture.name)
                })?;
                let found = regex.captures(haystack).ok_or_else(|| {
                    anyhow::anyhow!(
                        "capture {}: pattern {pattern:?} did not match {header:?}={haystack:?}",
                        capture.name
                    )
                })?;
                found
                    .get(1)
                    .or_else(|| found.get(0))
                    .map(|m| m.as_str().to_string())
                    .ok_or_else(|| {
                        anyhow::anyhow!("capture {}: regex has no capture", capture.name)
                    })?
            }
        };
        captures.insert(capture.name.clone(), value);
    }
    Ok(captures)
}

/// Same encoding as `runner::url_encode` and `bench::request::url_encode`.
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
    use crate::bench::scenario::BenchScenario;
    use crate::bench::server::TrivialServer;

    #[tokio::test]
    async fn setup_runs_captures_and_reports_the_cookie_header() {
        // The trivial server answers an empty 200, so a JSON capture is only
        // possible when the pointer is not used; use a header capture.
        let server = TrivialServer::start(200, false).await.expect("server");
        let scenario: BenchScenario = serde_yaml_ng::from_str(
            r#"
name: t
setup:
  - name: warm
    path: /
requests:
  - name: r
    path: /x
    jar: user
"#,
        )
        .expect("parse");
        scenario.validate().expect("valid");
        let outcome = run_setup(&server.base_url, &scenario).await.expect("setup");
        assert_eq!(outcome.vars, BTreeMap::new());
        assert_eq!(outcome.cookies.get("user").map(String::as_str), Some(""));
        assert_eq!(outcome.client.base_url(), server.base_url);
    }

    #[tokio::test]
    async fn a_setup_step_that_fails_without_an_expectation_stops_the_run() {
        let server = TrivialServer::start(401, false).await.expect("server");
        let scenario: BenchScenario = serde_yaml_ng::from_str(
            "name: t\nsetup:\n  - name: auth\n    path: /\nrequests:\n  - name: r\n    path: /\n",
        )
        .expect("parse");
        scenario.validate().expect("valid");
        let error = run_setup(&server.base_url, &scenario)
            .await
            .err()
            .expect("must fail");
        assert!(
            error.to_string().contains("unexpected status 401"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn an_expected_failure_is_allowed() {
        let server = TrivialServer::start(401, false).await.expect("server");
        let scenario: BenchScenario = serde_yaml_ng::from_str(
            "name: t\nsetup:\n  - name: auth\n    path: /\n    expect_status: 401\nrequests:\n  - name: r\n    path: /\n",
        )
        .expect("parse");
        scenario.validate().expect("valid");
        run_setup(&server.base_url, &scenario)
            .await
            .expect("an explicit expectation is honoured");
    }
}
