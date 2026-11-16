// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `difftest record`: a recording reverse proxy for browser sessions.
//!
//! It forwards every request unchanged and appends one scenario step per
//! request. Responses are *not* stored: replay compares the two targets, not
//! the recording. What the recorder does store is where a volatile value first
//! appeared in a response (a JSON pointer, a header regex or a cookie), so the
//! request that later reuses that value can be written as a capture plus a
//! `{{var}}` reference. That is what makes a recorded session replayable
//! against two targets whose generated ids differ.
//!
//! The proxy is a plain hyper 1 server and a `reqwest` client: one request at
//! a time (the recorder's state is behind a mutex, so the recorded order is the
//! browser's order).

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::config::{Capture, CaptureSource, Query, Scenario, Step};
use crate::http::decode_body;
use crate::timeparse::uuid_regex;

/// Headers that are recorded. Everything else is browser noise (`User-Agent`,
/// `Accept-Language`, `Sec-*`, ...) or transport state (`Host`,
/// `Content-Length`, `Cookie`, which the jar replays).
const RECORDED_HEADERS: &[&str] = &[
    "content-type",
    "x-csrftoken",
    "origin",
    "referer",
    "x-api-key",
    "accept",
];

/// A header a browser may use to ask for a separate cookie jar. It is stripped
/// before forwarding and never recorded.
const JAR_HEADER: &str = "x-difftest-jar";

/// The generic UUID pattern a header capture uses. It must match target B's
/// independently generated value, not the recorded one.
const UUID_CAPTURE_PATTERN: &str = "([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})";

#[derive(Debug, Clone)]
pub struct RecordOptions {
    pub listen: String,
    pub upstream: String,
    pub out: PathBuf,
    /// Scenario name; defaults to `recorded`.
    pub name: String,
    pub redis_db: Vec<i64>,
}

// ---------------------------------------------------------------------------
// Capture inference
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct CaptureEntry {
    raw: String,
    capture: Capture,
}

/// The map from an observed value to the capture that reproduces it.
#[derive(Debug, Default)]
pub struct CaptureIndex {
    entries: Vec<CaptureEntry>,
}

impl CaptureIndex {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Replace every known value with a `{{name}}` reference.
    pub fn rewrite(&self, text: &str) -> String {
        if self.entries.is_empty() {
            return text.to_string();
        }
        let mut out = text.to_string();
        // Longest first: a token that starts with an earlier-captured value
        // must not be half-substituted.
        let mut entries: Vec<&CaptureEntry> = self.entries.iter().collect();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.raw.len()));
        for entry in entries {
            if out.contains(entry.raw.as_str()) {
                out = out.replace(
                    entry.raw.as_str(),
                    &format!("{{{{{}}}}}", entry.capture.name),
                );
            }
        }
        out
    }

    /// Observe a JSON response body, in document order. `resource` is a
    /// human-readable noun for the endpoint (`workspace`, `project`, ...), used
    /// to name top-level `id` captures.
    pub fn observe_json(&mut self, body: &Value, resource: &str) -> Vec<Capture> {
        let mut pending = Vec::new();
        self.walk_json(body, "", resource, &mut pending);
        pending
    }

    fn walk_json(&mut self, value: &Value, pointer: &str, resource: &str, out: &mut Vec<Capture>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    let pointer = format!("{pointer}/{}", escape_pointer(key));
                    if let Value::String(text) = child
                        && let Some(base) = capture_base(key, text, resource)
                        && self.add_json_capture(&pointer, text, &base, out)
                    {
                        continue;
                    }
                    self.walk_json(child, &pointer, resource, out);
                }
            }
            // A list response is not a place where a value is first defined
            // for later use: every row would become a capture. Objects and
            // their nested objects are enough for the Plane API.
            Value::Array(_) => {}
            _ => {}
        }
    }

    fn add_json_capture(
        &mut self,
        pointer: &str,
        raw: &str,
        base: &str,
        out: &mut Vec<Capture>,
    ) -> bool {
        if raw.is_empty() || self.entries.iter().any(|entry| entry.raw == raw) {
            return false;
        }
        let capture = Capture {
            name: self.unique_name(base),
            source: CaptureSource::Json,
            pointer: Some(pointer.to_string()),
            header: None,
            cookie: None,
            pattern: None,
        };
        self.entries.push(CaptureEntry {
            raw: raw.to_string(),
            capture: capture.clone(),
        });
        out.push(capture);
        true
    }

    /// Observe a response header. Only UUID-shaped fragments are captured (a
    /// `Location` header carries a generated id, not a random secret).
    pub fn observe_header(&mut self, header: &str, value: &str, resource: &str) -> Vec<Capture> {
        let mut out = Vec::new();
        if !header.eq_ignore_ascii_case("location") {
            return out;
        }
        for matched in uuid_regex().find_iter(value) {
            let raw = matched.as_str().to_string();
            if self.entries.iter().any(|entry| entry.raw == raw) {
                continue;
            }
            let capture = Capture {
                name: self.unique_name(&format!("{resource}_id")),
                source: CaptureSource::Regex,
                pointer: None,
                header: Some("Location".to_string()),
                cookie: None,
                pattern: Some(UUID_CAPTURE_PATTERN.to_string()),
            };
            self.entries.push(CaptureEntry {
                raw,
                capture: capture.clone(),
            });
            out.push(capture);
        }
        out
    }

    /// Observe a `Set-Cookie` value. Only UUID values are captured; session and
    /// CSRF secrets are handled by the replayed cookie jar.
    pub fn observe_cookie(&mut self, name: &str, value: &str) -> Vec<Capture> {
        let mut out = Vec::new();
        if value.is_empty() || uuid_regex().find(value).map(|m| m.as_str()) != Some(value) {
            return out;
        }
        if self.entries.iter().any(|entry| entry.raw == value) {
            return out;
        }
        let capture = Capture {
            name: self.unique_name(&format!("{}_cookie", sanitize(name))),
            source: CaptureSource::Cookie,
            pointer: None,
            header: None,
            cookie: Some(name.to_string()),
            pattern: None,
        };
        self.entries.push(CaptureEntry {
            raw: value.to_string(),
            capture: capture.clone(),
        });
        out.push(capture);
        out
    }

    fn unique_name(&self, base: &str) -> String {
        let base = sanitize(base);
        if !self.entries.iter().any(|entry| entry.capture.name == base) {
            return base;
        }
        for suffix in 2.. {
            let candidate = format!("{base}_{suffix}");
            if !self
                .entries
                .iter()
                .any(|entry| entry.capture.name == candidate)
            {
                return candidate;
            }
        }
        unreachable!("an unbounded suffix search always terminates")
    }
}

/// The capture name a JSON string is worth, if any.
///
/// Generated UUIDs always are; `csrf_token`, `token`, `username` and `slug` are
/// values whose contents a target chooses, so they are captured even when they
/// happen to be deterministic.
fn capture_base(key: &str, text: &str, resource: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    if uuid_regex().find(text).map(|m| m.as_str()) == Some(text) {
        return Some(if key == "id" {
            format!("{resource}_id")
        } else {
            key.to_string()
        });
    }
    match key {
        "csrf_token" | "token" | "username" => Some(key.to_string()),
        "slug" => Some(format!("{resource}_slug")),
        _ => None,
    }
}

fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "value".to_string()
    } else if trimmed.starts_with(|c: char| c.is_ascii_digit()) {
        format!("v_{trimmed}")
    } else {
        trimmed
    }
}

/// The noun used for capture names, derived from the request path:
/// `/api/workspaces/` → `workspace`, `.../issues/<id>/comments/` → `comment`.
fn resource_from_path(path: &str) -> String {
    let mut segments = path
        .trim_end_matches('/')
        .rsplit('/')
        .filter(|segment| !segment.is_empty() && uuid_regex().find(segment).is_none());
    let mut segment = segments.next().unwrap_or("value");
    // `/api/users/me/` describes a user, not a "me": the captures it produces
    // are `user_id`, `username`, `token`.
    if matches!(segment, "me" | "self") {
        segment = segments.next().unwrap_or(segment);
    }
    let singular = if let Some(stem) = segment.strip_suffix("ies") {
        format!("{stem}y")
    } else if segment.ends_with('s') && segment.len() > 3 {
        segment[..segment.len() - 1].to_string()
    } else {
        segment.to_string()
    };
    sanitize(&singular)
}

// ---------------------------------------------------------------------------
// Recorder
// ---------------------------------------------------------------------------

/// One proxied exchange, as the recorder sees it.
pub struct Exchange<'a> {
    pub method: &'a str,
    pub path_and_query: &'a str,
    /// Request headers, in wire order.
    pub request_headers: &'a [(String, String)],
    pub request_body: &'a [u8],
    pub jar: &'a str,
    pub status: u16,
    pub response_headers: &'a [(String, String)],
    /// Response body after `Content-Encoding` was decoded.
    pub response_body: &'a [u8],
}

pub struct Recorder {
    scenario: Scenario,
    captures: CaptureIndex,
    out: PathBuf,
    origin: String,
    next_step: usize,
}

impl Recorder {
    pub fn new(out: PathBuf, name: String, origin: String, redis_db: Vec<i64>) -> Self {
        Self {
            scenario: Scenario {
                name,
                template_db: crate::config::DEFAULT_TEMPLATE_DB.to_string(),
                vars: BTreeMap::new(),
                password_vars: Vec::new(),
                redis_db,
                target_env: BTreeMap::new(),
                probe_compare_celery: false,
                steps: Vec::new(),
            },
            captures: CaptureIndex::default(),
            out,
            origin,
            next_step: 1,
        }
    }

    pub fn steps(&self) -> &[Step] {
        &self.scenario.steps
    }

    pub fn capture_names(&self) -> Vec<String> {
        self.captures
            .entries
            .iter()
            .map(|entry| entry.capture.name.clone())
            .collect()
    }

    pub fn record(&mut self, exchange: &Exchange<'_>) -> Result<()> {
        let (path, query) = split_path_and_query(exchange.path_and_query);
        let resource = resource_from_path(&path);
        let method = exchange.method.to_ascii_uppercase();

        // 1. The request, rewritten with the captures known so far.
        let mut step = Step {
            name: String::new(),
            method: method.clone(),
            path: self.rewrite(&path),
            query: query.map(|pairs| {
                Query::Map(
                    pairs
                        .into_iter()
                        .map(|(key, value)| (self.rewrite(&key), self.rewrite(&value)))
                        .collect(),
                )
            }),
            headers: BTreeMap::new(),
            json: None,
            form: None,
            body: None,
            jar: exchange.jar.to_string(),
            capture: Vec::new(),
            expect_status: Some(exchange.status),
        };
        let mut recorded_headers: BTreeMap<String, String> = BTreeMap::new();
        for (name, value) in exchange.request_headers {
            let lower = name.to_ascii_lowercase();
            if !RECORDED_HEADERS.contains(&lower.as_str()) {
                continue;
            }
            recorded_headers.insert(name.clone(), self.rewrite(value));
        }

        let content_type = exchange
            .request_headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.to_ascii_lowercase())
            .unwrap_or_default();

        // 2. The request body, in the most structured form that survives a
        //    round trip through the harness. The harness sets `Content-Type`
        //    from `json:`/`form:`, so the recorded header is redundant then.
        if method_allows_body(&method) && !exchange.request_body.is_empty() {
            if content_type.contains("application/json") {
                if let Some(json) = json_request_body(&self.captures, exchange.request_body)? {
                    step.json = Some(json);
                    recorded_headers.retain(|name, _| !name.eq_ignore_ascii_case("content-type"));
                }
            } else if content_type.contains("application/x-www-form-urlencoded") {
                step.form = Some(parse_form(&self.captures, exchange.request_body));
                recorded_headers.retain(|name, _| !name.eq_ignore_ascii_case("content-type"));
            } else if let Ok(text) = std::str::from_utf8(exchange.request_body) {
                step.body = Some(self.rewrite(text));
            }
        }
        step.headers = recorded_headers;

        // 3. Captures from the response, in document order.
        let mut captures = Vec::new();
        if let Some(json) = json_response(exchange.response_headers, exchange.response_body) {
            captures.extend(self.captures.observe_json(&json, &resource));
        }
        for (name, value) in exchange.response_headers {
            captures.extend(self.captures.observe_header(name, value, &resource));
            if name.eq_ignore_ascii_case("set-cookie")
                && let Some(cookie) = crate::cookies::parse_set_cookie(value)
            {
                captures.extend(self.captures.observe_cookie(&cookie.name, &cookie.value));
            }
        }
        step.capture = captures;
        // Name the step after the *rewritten* path, so re-recording the same
        // flow against a fresh database yields the same names instead of
        // embedding one run's generated ids.
        step.name = self.step_name(&method, &step.path);
        self.next_step += 1;
        self.scenario.steps.push(step);
        self.write()
    }

    fn rewrite(&self, text: &str) -> String {
        let replaced = text.replace(&self.origin, "{{base_url}}");
        self.captures.rewrite(&replaced)
    }

    fn step_name(&self, method: &str, path: &str) -> String {
        let slug: String = sanitize(path).chars().take(48).collect();
        format!(
            "{:02}-{}-{slug}",
            self.next_step,
            method.to_ascii_lowercase()
        )
    }

    pub fn to_yaml(&self) -> Result<String> {
        let header = "# Recorded by `difftest record`. Replay with:\n\
                      #   difftest run --scenario <this file> --a django --b django\n\
                      #\n\
                      # Request headers that matter are kept; cookies are replayed from the\n\
                      # named jar, and every value that first appeared in a response is a\n\
                      # capture plus a `{{var}}` reference. Responses are not stored: the\n\
                      # replayed targets are compared with each other, not with the recording.\n";
        let yaml = serde_yaml_ng::to_string(&yaml_scenario(&self.scenario))
            .context("serialising the recorded scenario")?;
        Ok(format!("{header}{yaml}"))
    }

    pub fn write(&self) -> Result<()> {
        if let Some(parent) = self.out.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let yaml = self.to_yaml()?;
        std::fs::write(&self.out, yaml)
            .with_context(|| format!("writing {}", self.out.display()))?;
        Ok(())
    }
}

fn method_allows_body(method: &str) -> bool {
    !matches!(method, "GET" | "HEAD" | "OPTIONS")
}

fn json_response(headers: &[(String, String)], body: &[u8]) -> Option<Value> {
    let content_type = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.to_ascii_lowercase())
        .unwrap_or_default();
    if !content_type.contains("application/json") {
        return None;
    }
    serde_json::from_slice(body).ok()
}

/// Rewrite a JSON request body with the captures. A string that *is* a
/// captured value becomes the reference; a string that only contains one (a
/// URL, a repr) is rewritten in place, exactly like a path.
fn json_request_body(captures: &CaptureIndex, body: &[u8]) -> Result<Option<Value>> {
    let Ok(text) = std::str::from_utf8(body) else {
        return Ok(None);
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    let mut value: Value = serde_json::from_str(text).context("recording a JSON request body")?;
    rewrite_json(captures, &mut value);
    Ok(Some(value))
}

fn rewrite_json(captures: &CaptureIndex, value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = captures.rewrite(text);
        }
        Value::Array(items) => {
            for item in items {
                rewrite_json(captures, item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                rewrite_json(captures, item);
            }
        }
        _ => {}
    }
}

fn parse_form(captures: &CaptureIndex, body: &[u8]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::str::from_utf8(body) else {
        return out;
    };
    for pair in text.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(
            captures.rewrite(&decode_component(key)),
            captures.rewrite(&decode_component(value)),
        );
    }
    out
}

fn decode_component(text: &str) -> String {
    percent_encoding::percent_decode_str(&text.replace('+', " "))
        .decode_utf8_lossy()
        .into_owned()
}

fn split_path_and_query(path_and_query: &str) -> (String, Option<Vec<(String, String)>>) {
    match path_and_query.split_once('?') {
        None => (path_and_query.to_string(), None),
        Some((path, query)) => {
            let pairs = query
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| match pair.split_once('=') {
                    Some((key, value)) => (decode_component(key), decode_component(value)),
                    None => (decode_component(pair), String::new()),
                })
                .collect();
            (path.to_string(), Some(pairs))
        }
    }
}

// ---------------------------------------------------------------------------
// The proxy
// ---------------------------------------------------------------------------

pub async fn run(opts: RecordOptions) -> Result<()> {
    let listen: SocketAddr = opts
        .listen
        .parse()
        .with_context(|| format!("parsing --listen {:?} (expected host:port)", opts.listen))?;
    let upstream = opts.upstream.trim_end_matches('/').to_string();
    if !upstream.starts_with("http://") {
        bail!("--upstream must be an http:// URL, got {upstream:?}");
    }
    let origin = format!("http://{listen}");
    let recorder = Arc::new(Mutex::new(Recorder::new(
        opts.out.clone(),
        opts.name.clone(),
        origin.clone(),
        opts.redis_db.clone(),
    )));
    recorder.lock().await.write()?;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("building the recording proxy's HTTP client")?;

    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("binding the recorder to {listen}"))?;
    println!(
        "difftest record: listening on http://{listen}, forwarding to {upstream}, writing {}",
        opts.out.display()
    );
    println!("send your browser or curl at http://{listen}; Ctrl-C writes the final file");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("accepting a connection")?;
                let io = TokioIo::new(stream);
                let recorder = Arc::clone(&recorder);
                let client = client.clone();
                let upstream = upstream.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request: Request<Incoming>| {
                        let recorder = Arc::clone(&recorder);
                        let client = client.clone();
                        let upstream = upstream.clone();
                        async move { handle(request, recorder, client, upstream).await }
                    });
                    if let Err(error) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .await
                    {
                        eprintln!("difftest record: connection error: {error}");
                    }
                });
            }
        }
    }

    let recorder = recorder.lock().await;
    recorder.write()?;
    println!(
        "difftest record: {} step(s), {} capture(s) written to {}",
        recorder.steps().len(),
        recorder.capture_names().len(),
        recorder.out.display()
    );
    Ok(())
}

async fn handle(
    request: Request<Incoming>,
    recorder: Arc<Mutex<Recorder>>,
    client: reqwest::Client,
    upstream: String,
) -> Result<Response<Full<Bytes>>, Infallible> {
    match forward(request, &recorder, &client, &upstream).await {
        Ok(response) => Ok(response),
        Err(error) => {
            eprintln!("difftest record: {error:#}");
            Ok(Response::builder()
                .status(502)
                .header("content-type", "text/plain; charset=utf-8")
                .body(Full::new(Bytes::from(format!(
                    "recording proxy: {error:#}"
                ))))
                .unwrap_or_else(|_| Response::new(Full::new(Bytes::new()))))
        }
    }
}

async fn forward(
    request: Request<Incoming>,
    recorder: &Mutex<Recorder>,
    client: &reqwest::Client,
    upstream: &str,
) -> Result<Response<Full<Bytes>>> {
    let (parts, body) = request.into_parts();
    let body = body
        .collect()
        .await
        .map_err(|error| anyhow!("reading the request body: {error}"))?
        .to_bytes();
    let method = parts.method.as_str().to_ascii_uppercase();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    let url = format!("{upstream}{path_and_query}");
    let jar = parts
        .headers
        .get(JAR_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("default")
        .to_string();

    let mut request_headers: Vec<(String, String)> = Vec::new();
    let mut request_builder = client.request(
        reqwest::Method::from_bytes(method.as_bytes())
            .with_context(|| format!("invalid method {method:?}"))?,
        &url,
    );
    for (name, value) in &parts.headers {
        let lower = name.as_str().to_ascii_lowercase();
        // Keep `Host`: a reverse proxy forwards the browser's Host, and
        // Django's CSRF origin check compares `Origin` against it. Dropping it
        // would make the upstream see the recorder's host instead.
        if matches!(lower.as_str(), "content-length" | "connection" | JAR_HEADER) {
            continue;
        }
        let Ok(value) = value.to_str() else { continue };
        request_headers.push((name.as_str().to_string(), value.to_string()));
        request_builder = request_builder.header(name.as_str(), value);
    }
    if !body.is_empty() {
        request_builder = request_builder.body(body.to_vec());
    }
    let response = request_builder
        .send()
        .await
        .with_context(|| format!("forwarding {method} {url}"))?;
    let status = response.status().as_u16();
    let response_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let content_encoding = response_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
        .map(|(_, value)| value.to_ascii_lowercase());
    let wire = response
        .bytes()
        .await
        .context("reading the upstream body")?;
    let decoded = decode_body(&wire, content_encoding.as_deref()).unwrap_or_else(|_| wire.to_vec());

    // Record before answering, so a crash after the response still leaves the
    // step on disk.
    recorder
        .lock()
        .await
        .record(&Exchange {
            method: &method,
            path_and_query: &path_and_query,
            request_headers: &request_headers,
            request_body: &body,
            jar: &jar,
            status,
            response_headers: &response_headers,
            response_body: &decoded,
        })
        .with_context(|| format!("recording {method} {path_and_query}"))?;

    let mut builder = Response::builder().status(status);
    for (name, value) in &response_headers {
        let lower = name.to_ascii_lowercase();
        // `content-length` is set by hyper from the body; `transfer-encoding`
        // would conflict with it; `connection` is per-hop.
        if matches!(
            lower.as_str(),
            "content-length" | "transfer-encoding" | "connection"
        ) {
            continue;
        }
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder
        .body(Full::new(wire))
        .context("building the proxied response")
}

#[derive(serde::Serialize)]
struct YamlScenario<'a> {
    name: &'a str,
    template_db: &'a str,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    vars: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    password_vars: &'a Vec<String>,
    redis_db: &'a Vec<i64>,
    steps: Vec<YamlStep<'a>>,
}

#[derive(serde::Serialize)]
struct YamlStep<'a> {
    name: &'a str,
    method: &'a str,
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<&'a Query>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    headers: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    json: Option<serde_yaml_ng::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    form: Option<&'a BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<&'a String>,
    jar: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    capture: &'a Vec<Capture>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expect_status: Option<u16>,
}

fn yaml_scenario(scenario: &Scenario) -> YamlScenario<'_> {
    YamlScenario {
        name: &scenario.name,
        template_db: &scenario.template_db,
        vars: &scenario.vars,
        password_vars: &scenario.password_vars,
        redis_db: &scenario.redis_db,
        steps: scenario
            .steps
            .iter()
            .map(|step| YamlStep {
                name: &step.name,
                method: &step.method,
                path: &step.path,
                query: step.query.as_ref(),
                headers: &step.headers,
                json: step.json.as_ref().map(json_to_yaml),
                form: step.form.as_ref(),
                body: step.body.as_ref(),
                jar: &step.jar,
                capture: &step.capture,
                expect_status: step.expect_status,
            })
            .collect(),
    }
}

/// `serde_json` is built with `arbitrary_precision`, whose `Number`
/// serialises as a private map when another format reads it. Convert the
/// tree to YAML values explicitly so `network: 2` stays a number.
fn json_to_yaml(value: &Value) -> serde_yaml_ng::Value {
    use serde_yaml_ng::Value as Y;
    match value {
        Value::Null => Y::Null,
        Value::Bool(value) => Y::Bool(*value),
        Value::Number(number) => {
            let lexeme = number.to_string();
            if let Ok(integer) = lexeme.parse::<i64>() {
                Y::Number(serde_yaml_ng::Number::from(integer))
            } else if let Ok(float) = lexeme.parse::<f64>() {
                Y::Number(serde_yaml_ng::Number::from(float))
            } else {
                Y::String(lexeme)
            }
        }
        Value::String(text) => Y::String(text.clone()),
        Value::Array(items) => Y::Sequence(items.iter().map(json_to_yaml).collect()),
        Value::Object(map) => Y::Mapping(
            map.iter()
                .map(|(key, value)| (Y::String(key.clone()), json_to_yaml(value)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn recorder() -> Recorder {
        Recorder::new(
            PathBuf::from("/tmp/difftest-record-test.yaml"),
            "test".to_string(),
            "http://127.0.0.1:18090".to_string(),
            vec![1, 2],
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn exchange<'a>(
        method: &'a str,
        path_and_query: &'a str,
        request_headers: &'a [(String, String)],
        request_body: &'a [u8],
        jar: &'a str,
        status: u16,
        response_headers: &'a [(String, String)],
        response_body: &'a [u8],
    ) -> Exchange<'a> {
        Exchange {
            method,
            path_and_query,
            request_headers,
            request_body,
            jar,
            status,
            response_headers,
            response_body,
        }
    }

    #[test]
    fn a_json_id_is_captured_and_rewritten_in_later_requests() {
        let mut recorder = recorder();
        let body = json!({"id": "3f2b8f6e-1d4a-4b7c-8e2f-0a1b2c3d4e5f", "slug": "dt-workspace"});
        let request_headers = [("Content-Type".to_string(), "application/json".to_string())];
        let response_headers = [("Content-Type".to_string(), "application/json".to_string())];
        recorder
            .record(&exchange(
                "POST",
                "/api/workspaces/",
                &request_headers,
                b"",
                "default",
                201,
                &response_headers,
                &serde_json::to_vec(&body).expect("json"),
            ))
            .expect("record");

        let step = &recorder.steps()[0];
        assert_eq!(step.path, "/api/workspaces/");
        assert_eq!(step.expect_status, Some(201));
        let names: Vec<&str> = step.capture.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["workspace_id", "workspace_slug"]);
        assert_eq!(step.capture[0].source, CaptureSource::Json);
        assert_eq!(step.capture[0].pointer.as_deref(), Some("/id"));

        recorder
            .record(&exchange(
                "GET",
                "/api/workspaces/dt-workspace/projects/?x=1",
                &[],
                b"",
                "default",
                200,
                &response_headers,
                b"{}",
            ))
            .expect("record");
        let step = &recorder.steps()[1];
        assert_eq!(step.path, "/api/workspaces/{{workspace_slug}}/projects/");
        assert_eq!(
            format!("{:?}", step.query),
            format!(
                "{:?}",
                Some(Query::Map(BTreeMap::from([(
                    "x".to_string(),
                    "1".to_string()
                )])))
            )
        );
    }

    #[test]
    fn a_location_header_is_captured_with_a_generic_regex() {
        let mut recorder = recorder();
        let uuid = "3f2b8f6e-1d4a-4b7c-8e2f-0a1b2c3d4e5f";
        let location = [("Location".to_string(), format!("/workspaces/{uuid}/"))];
        recorder
            .record(&exchange(
                "POST",
                "/api/workspaces/",
                &[],
                b"",
                "default",
                302,
                &location,
                b"",
            ))
            .expect("record");
        let capture = &recorder.steps()[0].capture[0];
        assert_eq!(capture.name, "workspace_id");
        assert_eq!(capture.source, CaptureSource::Regex);
        assert_eq!(capture.header.as_deref(), Some("Location"));
        let pattern = capture.pattern.as_deref().expect("pattern");
        assert!(
            regex::Regex::new(pattern)
                .expect("valid")
                .is_match(&format!("/w/{uuid}/"))
        );

        recorder
            .record(&exchange(
                "GET",
                &format!("/workspaces/{uuid}/"),
                &[],
                b"",
                "default",
                200,
                &[],
                b"",
            ))
            .expect("record");
        assert_eq!(recorder.steps()[1].path, "/workspaces/{{workspace_id}}/");
    }

    #[test]
    fn csrf_tokens_and_origins_are_rewritten() {
        let mut recorder = recorder();
        let masked = "auJMQXMEZbVPi6NmJTFdzXZ62SxwRmpZCdhiEYXUkT2AgNXXyQGwtMZSs02lF1Du";
        let get_headers = [
            ("Origin".to_string(), "http://127.0.0.1:18090".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        let set_cookie = [
            ("Content-Type".to_string(), "application/json".to_string()),
            (
                "Set-Cookie".to_string(),
                "csrftoken=CTIGYblqvShV8RkLZ7bt4ZaWAiFZYPoF; Path=/".to_string(),
            ),
        ];
        recorder
            .record(&exchange(
                "GET",
                "/auth/get-csrf-token/",
                &get_headers,
                b"",
                "user",
                200,
                &set_cookie,
                &serde_json::to_vec(&json!({"csrf_token": masked})).expect("json"),
            ))
            .expect("record");
        let step = &recorder.steps()[0];
        assert_eq!(step.jar, "user");
        assert_eq!(
            step.headers.get("Origin").map(String::as_str),
            Some("{{base_url}}")
        );
        assert_eq!(
            step.headers.get("Accept").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(step.capture[0].name, "csrf_token");
        // The csrftoken cookie secret is replayed by the jar, not captured.
        assert!(
            recorder
                .capture_names()
                .iter()
                .all(|name| name != "csrftoken_cookie")
        );

        let post_headers = [
            (
                "Content-Type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ),
            ("X-CSRFToken".to_string(), masked.to_string()),
        ];
        recorder
            .record(&exchange(
                "POST",
                "/auth/sign-up/",
                &post_headers,
                format!("email=a%40b.test&csrfmiddlewaretoken={masked}").as_bytes(),
                "user",
                302,
                &[],
                b"",
            ))
            .expect("record");
        let step = &recorder.steps()[1];
        assert_eq!(
            step.form.as_ref().expect("form").get("csrfmiddlewaretoken"),
            Some(&"{{csrf_token}}".to_string())
        );
        assert_eq!(
            step.form.as_ref().expect("form").get("email"),
            Some(&"a@b.test".to_string())
        );
        assert_eq!(
            step.headers.get("X-CSRFToken").map(String::as_str),
            Some("{{csrf_token}}")
        );
    }

    #[test]
    fn a_uuid_cookie_is_captured() {
        let mut recorder = recorder();
        let uuid = "3f2b8f6e-1d4a-4b7c-8e2f-0a1b2c3d4e5f";
        let set_cookie = [("Set-Cookie".to_string(), format!("device={uuid}; Path=/"))];
        recorder
            .record(&exchange(
                "GET",
                "/api/users/me/",
                &[],
                b"",
                "default",
                200,
                &set_cookie,
                b"",
            ))
            .expect("record");
        let capture = &recorder.steps()[0].capture[0];
        assert_eq!(capture.source, CaptureSource::Cookie);
        assert_eq!(capture.cookie.as_deref(), Some("device"));
    }

    #[test]
    fn names_are_unique_and_sanitised() {
        let mut index = CaptureIndex::default();
        let uuid_a = "11111111-1111-4111-8111-111111111111";
        let uuid_b = "22222222-2222-4222-8222-222222222222";
        let first = index.observe_json(&json!({"id": uuid_a, "items": [{"id": uuid_b}]}), "issue");
        assert_eq!(
            first.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["issue_id"],
            "list rows do not become captures"
        );
        let second = index.observe_json(
            &json!({"id": "33333333-3333-4333-8333-333333333333"}),
            "issue",
        );
        assert_eq!(second[0].name, "issue_id_2");
        assert_eq!(index.rewrite(uuid_a), "{{issue_id}}");
    }

    #[test]
    fn the_recorded_file_round_trips_through_the_scenario_parser() {
        let mut recorder = recorder();
        let request_headers = [("Content-Type".to_string(), "application/json".to_string())];
        let response_headers = [("Content-Type".to_string(), "application/json".to_string())];
        recorder
            .record(&exchange(
                "POST",
                "/api/workspaces/",
                &request_headers,
                br#"{"name":"Diff Test"}"#,
                "default",
                201,
                &response_headers,
                br#"{"id":"3f2b8f6e-1d4a-4b7c-8e2f-0a1b2c3d4e5f"}"#,
            ))
            .expect("record");
        let yaml = recorder.to_yaml().expect("yaml");
        let parsed: Scenario = serde_yaml_ng::from_str(&yaml).expect("parse");
        parsed.validate().expect("valid");
        assert_eq!(parsed.steps.len(), 1);
        assert_eq!(parsed.steps[0].capture[0].name, "workspace_id");
        assert_eq!(parsed.steps[0].json, Some(json!({"name": "Diff Test"})));
    }

    #[test]
    fn json_numbers_are_written_as_yaml_numbers() {
        let mut recorder = recorder();
        let request_headers = [("Content-Type".to_string(), "application/json".to_string())];
        recorder
            .record(&exchange(
                "POST",
                "/api/workspaces/acme/projects/",
                &request_headers,
                br#"{"name":"P","network":2,"ratio":0.5,"nested":{"n":3}}"#,
                "default",
                201,
                &[("Content-Type".to_string(), "application/json".to_string())],
                b"{}",
            ))
            .expect("record");
        let yaml = recorder.to_yaml().expect("yaml");
        assert!(yaml.contains("network: 2"), "{yaml}");
        assert!(!yaml.contains("$serde_json"), "{yaml}");
        let parsed: Scenario = serde_yaml_ng::from_str(&yaml).expect("parse");
        assert_eq!(
            parsed.steps[0].json,
            Some(json!({"name": "P", "network": 2, "ratio": 0.5, "nested": {"n": 3}}))
        );
    }

    #[test]
    fn resource_names_are_singular() {
        assert_eq!(resource_from_path("/api/workspaces/"), "workspace");
        assert_eq!(
            resource_from_path("/api/workspaces/acme/projects/"),
            "project"
        );
        assert_eq!(
            resource_from_path(
                "/api/workspaces/acme/projects/3f2b8f6e-1d4a-4b7c-8e2f-0a1b2c3d4e5f/issues/"
            ),
            "issue"
        );
        assert_eq!(
            resource_from_path("/auth/get-csrf-token/"),
            "get_csrf_token"
        );
        assert_eq!(
            resource_from_path(
                "/api/workspaces/acme/projects/3f2b8f6e-1d4a-4b7c-8e2f-0a1b2c3d4e5f/issues/3f2b8f6e-1d4a-4b7c-8e2f-0a1b2c3d4e5f/comments/"
            ),
            "comment"
        );
    }
}
