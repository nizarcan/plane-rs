// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Raw HTTP client.
//!
//! Deliberately configured to hide nothing: no redirect following, no cookie
//! store, no automatic decompression. The harness must see the exact
//! `Location`, `Set-Cookie` and `Content-Encoding` headers Django sends, and
//! decodes the gzip body itself before comparing.

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, StatusCode};

use crate::cookies::CookieJar;
use crate::util::now_utc;

pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    /// Path and query, starting with `/`.
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub jar: String,
}

impl HttpRequest {
    pub fn new(method: impl Into<String>, path_and_query: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            path_and_query: path_and_query.into(),
            headers: Vec::new(),
            body: None,
            jar: "default".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    /// Every header, in wire order, duplicates preserved.
    pub headers: Vec<(String, String)>,
    /// Body after `Content-Encoding` was decoded.
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

pub struct HttpClient {
    client: reqwest::Client,
    base_url: String,
    user_agent: String,
    jars: BTreeMap<String, CookieJar>,
}

impl HttpClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            // Never follow redirects: the response is part of the contract.
            .redirect(reqwest::redirect::Policy::none())
            // No cookie store: each named jar is applied by hand.
            .no_proxy()
            .timeout(Duration::from_secs(60))
            .build()
            .context("building the HTTP client")?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            jars: BTreeMap::new(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn jar(&self, name: &str) -> Option<&CookieJar> {
        self.jars.get(name)
    }

    pub async fn send(&mut self, request: &HttpRequest) -> Result<HttpResponse> {
        let url = format!("{}{}", self.base_url, request.path_and_query);
        let method = Method::from_bytes(request.method.as_bytes())
            .with_context(|| format!("invalid HTTP method {:?}", request.method))?;

        let mut headers = HeaderMap::new();
        // The jar holds cookies the target set (notably `csrftoken`); an
        // explicit `Cookie` header supplies the fixture sessions, which are
        // rows rather than `Set-Cookie` responses. Merge them by cookie name,
        // the explicit header winning, so a write step can carry both.
        let jar_cookie = self
            .jars
            .get(&request.jar)
            .and_then(CookieJar::cookie_header);
        let explicit_cookie = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
            .map(|(_, value)| value.clone());
        let merged_cookie = merge_cookies(jar_cookie.as_deref(), explicit_cookie.as_deref());
        if let Some(cookie) = &merged_cookie {
            insert(&mut headers, "cookie", cookie)?;
        }
        insert(&mut headers, "accept-encoding", "gzip")?;
        insert(&mut headers, "user-agent", &self.user_agent)?;
        let mut has_content_type = false;
        for (name, value) in &request.headers {
            if name.eq_ignore_ascii_case("content-type") {
                has_content_type = true;
            }
            if name.eq_ignore_ascii_case("cookie") {
                // Already merged above.
                continue;
            }
            insert(&mut headers, name, value)?;
        }
        if request.body.is_some() && !has_content_type {
            bail!(
                "request {} {} has a body but no Content-Type header",
                request.method,
                request.path_and_query
            );
        }

        let mut builder = self.client.request(method, &url).headers(headers);
        if let Some(body) = &request.body {
            builder = builder.body(body.clone());
        }
        let response = builder
            .send()
            .await
            .with_context(|| format!("sending {} {}", request.method, url))?;
        let status = response.status();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let content_encoding = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
            .map(|(_, value)| value.to_ascii_lowercase());
        let wire = response
            .bytes()
            .await
            .with_context(|| format!("reading the body of {} {}", request.method, url))?;
        let body = decode_body(&wire, content_encoding.as_deref())?;

        let now = now_utc();
        let jar = self.jars.entry(request.jar.clone()).or_default();
        for (name, value) in &headers {
            if name.eq_ignore_ascii_case("set-cookie") {
                jar.apply_set_cookie(value, now);
            }
        }

        Ok(HttpResponse {
            status: status.as_u16(),
            headers,
            body,
        })
    }

    /// Used by the health check: no jar, no body, short timeout.
    pub async fn probe(&self, path: &str) -> Result<u16> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .client
            .get(&url)
            .header("accept-encoding", "gzip")
            .send()
            .await
            .with_context(|| format!("probing {url}"))?;
        Ok(response.status().as_u16())
    }
}

/// Merge a jar's `Cookie` header with an explicit one, by cookie name; the
/// explicit value wins for a name both carry, and the jar's order is kept.
fn merge_cookies(jar: Option<&str>, explicit: Option<&str>) -> Option<String> {
    let parse = |header: &str| -> Vec<(String, String)> {
        header
            .split(';')
            .filter_map(|item| {
                let item = item.trim();
                let (name, value) = item.split_once('=')?;
                Some((name.trim().to_string(), value.trim().to_string()))
            })
            .collect()
    };
    match (jar, explicit) {
        (None, None) => None,
        (Some(jar), None) => Some(jar.to_string()),
        (None, Some(explicit)) => Some(explicit.to_string()),
        (Some(jar), Some(explicit)) => {
            let mut pairs = parse(jar);
            for (name, value) in parse(explicit) {
                match pairs.iter_mut().find(|(existing, _)| *existing == name) {
                    Some(slot) => slot.1 = value,
                    None => pairs.push((name, value)),
                }
            }
            Some(
                pairs
                    .into_iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        }
    }
}

fn insert(headers: &mut HeaderMap, name: &str, value: &str) -> Result<()> {
    let header_name = HeaderName::from_bytes(name.as_bytes())
        .with_context(|| format!("invalid header name {name:?}"))?;
    let header_value = HeaderValue::from_str(value)
        .with_context(|| format!("invalid value for header {name:?}: {value:?}"))?;
    headers.insert(header_name, header_value);
    Ok(())
}

/// Decode a gzip body exactly once. Any other encoding is an error: the
/// harness must fail loudly rather than compare compressed bytes.
///
/// A HEAD response has no body but still advertises the `Content-Encoding` its
/// GET would use (`django/middleware/gzip.py` sets the header regardless of
/// the request method), and a 204 may carry one too. An empty wire body is not
/// a gzip stream: it decodes to itself.
pub fn decode_body(wire: &[u8], content_encoding: Option<&str>) -> Result<Vec<u8>> {
    let Some(encoding) = content_encoding else {
        return Ok(wire.to_vec());
    };
    let encoding = encoding.trim().to_ascii_lowercase();
    if encoding.is_empty() || encoding == "identity" {
        return Ok(wire.to_vec());
    }
    if encoding != "gzip" && encoding != "x-gzip" {
        bail!("unsupported Content-Encoding {encoding:?}");
    }
    if wire.is_empty() {
        return Ok(Vec::new());
    }
    let mut decoder = flate2::read::GzDecoder::new(wire);
    let mut decoded = Vec::with_capacity(wire.len() * 3);
    decoder
        .read_to_end(&mut decoded)
        .map_err(|error| anyhow!("decoding gzip response body: {error}"))?;
    Ok(decoded)
}

pub fn is_success(status: u16) -> bool {
    StatusCode::from_u16(status).is_ok_and(|status| status.is_success())
}

/// Poll a target's health path until it answers 2xx or the process exits.
pub async fn wait_healthy(
    client: &HttpClient,
    health_path: &str,
    timeout: Duration,
    child: &mut std::process::Child,
    log_path: &std::path::Path,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().context("polling the target process")? {
            bail!(
                "target exited during startup with {status}\n--- log tail ({}) ---\n{}",
                log_path.display(),
                tail_file(log_path, 4000)
            );
        }
        match client.probe(health_path).await {
            Ok(status) if is_success(status) => return Ok(()),
            Ok(status) => {
                tracing::debug!("health {health_path} returned {status}");
            }
            Err(error) => {
                tracing::debug!("health {health_path} not ready: {error}");
            }
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "target did not become healthy at {health_path} within {}s\n--- log tail ({}) ---\n{}",
                timeout.as_secs(),
                log_path.display(),
                tail_file(log_path, 4000)
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

pub fn tail_file(path: &std::path::Path, max_bytes: usize) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return "<no log>".to_string();
    };
    let start = bytes.len().saturating_sub(max_bytes);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_headers_merge_and_the_explicit_value_wins() {
        assert_eq!(merge_cookies(None, None), None);
        assert_eq!(
            merge_cookies(Some("csrftoken=abc"), None).as_deref(),
            Some("csrftoken=abc")
        );
        assert_eq!(
            merge_cookies(None, Some("session-id=s")).as_deref(),
            Some("session-id=s")
        );
        // The fixture session is an explicit header; the CSRF cookie comes from
        // the target. Both survive, and an explicit value wins by name.
        assert_eq!(
            merge_cookies(Some("csrftoken=abc"), Some("session-id=s; csrftoken=def")).as_deref(),
            Some("csrftoken=def; session-id=s")
        );
    }
    use std::io::Write;

    #[test]
    fn gzip_bodies_are_decoded() {
        let body = br#"{"hello":"world"}"#;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(body).expect("compress");
        let compressed = encoder.finish().expect("finish");
        assert_ne!(compressed, body);
        assert_eq!(
            decode_body(&compressed, Some("gzip")).expect("decode"),
            body
        );
        assert_eq!(decode_body(body, None).expect("decode"), body);
    }

    #[test]
    fn an_empty_head_body_is_not_gunzipped() {
        // HEAD advertises the GET's Content-Encoding but sends no body.
        assert_eq!(
            decode_body(&[], Some("gzip")).expect("empty body"),
            Vec::<u8>::new()
        );
        assert_eq!(
            decode_body(&[], Some("x-gzip")).expect("empty body"),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn unknown_encodings_fail_loudly() {
        assert!(decode_body(b"x", Some("br")).is_err());
    }

    #[test]
    fn success_matches_2xx_only() {
        assert!(is_success(200));
        assert!(is_success(204));
        assert!(!is_success(302));
        assert!(!is_success(404));
    }
}
