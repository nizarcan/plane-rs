// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The outbound transport: `plane/utils/url_security.py`'s two public
//! functions, on top of `reqwest`.
//!
//! # Pinning, without a second DNS lookup
//!
//! Python rewrites the request URL to the validated IP literal and passes the
//! original name as the `Host` header and (through a custom urllib3 pool key)
//! as the TLS SNI, so urllib3 has nothing left to resolve. reqwest has no
//! per-request SNI override, so this crate pins one level lower: the request
//! URL keeps the hostname, and the client is built with a
//! [`reqwest::dns::Resolve`] implementation that answers **only** the names in
//! its pin map and fails closed for anything else. hyper connects to the pinned
//! `SocketAddr`, rustls sends SNI and verifies the certificate against the
//! URL's host (the original name), and the `Host` header is set explicitly to
//! Python's value. There is no code path that could perform a second lookup:
//! the resolver is the only source of addresses and it holds a fixed list.
//!
//! This also makes the rebinding proof observable: the pin comes from
//! validation, so changing the resolver's answer between validation and
//! connect cannot move the connection (see `tests/live_https.rs`).
//!
//! One caveat: the URL parser (`url`) normalises the authority (IDNA to
//! punycode, default-port removal), so the name rustls verifies against can be
//! a normalised form of the validated name. The *address* is always the
//! validated one, which is what the SSRF guarantee is about.
//!
//! # Redirects
//!
//! `pinned_fetch` never follows a redirect: the 3xx is returned. When a preset
//! allows redirects, [`HttpEngine::fetch`] follows them itself, resolving,
//! validating and pinning every hop, exactly like
//! `pinned_fetch_following_redirects`. The client's own redirect policy is
//! always `none()`, so a `Location` is never acted on by reqwest.
//!
//! # Per-request cost
//!
//! A `reqwest::Client` is built per hop, as Python builds a `requests.Session`
//! per hop. The TLS configuration is shared through an `Arc` (so the root store
//! and provider are not rebuilt), and building a client is a hash-map insert
//! plus a connector. `cargo run --release -p plane-net --example cost` measures
//! it: 0.42 µs p50 on the development machine, against 0.54 µs p50 for the
//! validation itself.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use base64::Engine;
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect;
use reqwest::{Client, ClientBuilder, Url};

use crate::dns::SharedResolver;
use crate::error::FetchError;
use crate::headers::HeaderList;
use crate::preset::{Preset, ProxyMode};
use crate::pyurl::{self, Target};
use crate::ssrf::{SsrfPolicy, ValidatedHop, validate_hop};
use crate::tls::{TlsSettings, TlsTrust};

/// A request body and the headers a call site adds.
#[derive(Debug, Clone, Default)]
pub struct OutboundRequest {
    /// Extra headers, in the order Python's dict would hold them. They override
    /// the preset's headers on a name collision (as a per-request `headers=`
    /// argument does in `requests`).
    pub headers: Vec<(String, String)>,
    /// Body bytes. A JSON body must already be encoded exactly as Python's
    /// `json.dumps` would encode it (webhook delivery uses `plane-serde`'s
    /// Python JSON dialect); this crate does not serialise.
    pub body: Option<Vec<u8>>,
    /// Overrides [`Preset::max_body_bytes`] when set.
    pub max_body_bytes: Option<usize>,
}

impl OutboundRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn max_body_bytes(mut self, limit: usize) -> Self {
        self.max_body_bytes = Some(limit);
        self
    }
}

/// A response, read into memory (or capped, for the streaming presets).
#[derive(Debug, Clone)]
pub struct PinnedResponse {
    pub status: u16,
    pub headers: HeaderList,
    /// The URL of this response: the request URL for `fetch_once`, the last hop
    /// for `fetch`.
    pub url: String,
    body: Vec<u8>,
}

impl PinnedResponse {
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    pub fn is_redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308)
    }

    pub fn location(&self) -> Option<&str> {
        self.header("Location")
    }

    /// `response.text`: the encoding rules of `requests.utils.get_encoding_from_headers`
    /// followed by a decode with `errors="replace"`.
    ///
    /// `requests` falls back to `charset_normalizer`/`chardet` when the headers
    /// name no encoding; neither library is installed in `apps/api`, so
    /// `apparent_encoding` is the string `"utf-8"`. This function therefore
    /// decodes as UTF-8 with replacement in that case, which is exactly what
    /// Python does on the pinned dependency set. Installing chardet would change
    /// Python's answer for non-UTF-8 bodies and would have to be ported too.
    pub fn text(&self) -> String {
        if self.body.is_empty() {
            return String::new();
        }
        match encoding_from_headers(&self.headers) {
            Some(encoding) => decode_with(&encoding, &self.body)
                .unwrap_or_else(|| String::from_utf8_lossy(&self.body).into_owned()),
            None => String::from_utf8_lossy(&self.body).into_owned(),
        }
    }
}

/// `requests.utils.get_encoding_from_headers`.
fn encoding_from_headers(headers: &HeaderList) -> Option<String> {
    let content_type = headers.get("content-type")?;
    if content_type.is_empty() {
        return None;
    }
    let (media_type, params) = parse_content_type_header(content_type);
    if let Some(charset) = params.get("charset") {
        return Some(charset.trim_matches(|c| c == '\'' || c == '"').to_string());
    }
    if media_type.contains("text") {
        return Some("ISO-8859-1".to_string());
    }
    if media_type.contains("application/json") {
        return Some("utf-8".to_string());
    }
    None
}

/// `requests.utils._parse_content_type_header`. Unlike Python, a parameter
/// without `=` is ignored instead of stored as `True` (Python then raises
/// `AttributeError` when it is `charset`; that crash is not ported).
fn parse_content_type_header(header: &str) -> (String, HashMap<String, String>) {
    let mut tokens = header.split(';');
    let media_type = tokens.next().unwrap_or("").trim().to_string();
    let mut params = HashMap::new();
    for token in tokens {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if let Some((key, value)) = token.split_once('=') {
            params.insert(
                key.trim_matches(|c| matches!(c, '\'' | '"' | ' '))
                    .to_lowercase(),
                value
                    .trim_matches(|c| matches!(c, '\'' | '"' | ' '))
                    .to_string(),
            );
        }
    }
    (media_type, params)
}

fn decode_with(encoding: &str, body: &[u8]) -> Option<String> {
    match encoding.to_ascii_lowercase().as_str() {
        "utf-8" | "utf8" | "u8" | "utf" => Some(String::from_utf8_lossy(body).into_owned()),
        "ascii" | "us-ascii" | "646" => Some(
            body.iter()
                .map(|byte| {
                    if byte.is_ascii() {
                        *byte as char
                    } else {
                        '\u{fffd}'
                    }
                })
                .collect(),
        ),
        "iso-8859-1" | "iso8859-1" | "latin-1" | "latin1" | "l1" | "cp819" | "iso-ir-100" => {
            Some(body.iter().map(|byte| *byte as char).collect())
        }
        "windows-1252" | "cp1252" => Some(
            body.iter()
                .map(|byte| match byte {
                    0x80 => '\u{20ac}',
                    0x82 => '\u{201a}',
                    0x83 => '\u{0192}',
                    0x84 => '\u{201e}',
                    0x85 => '\u{2026}',
                    0x86 => '\u{2020}',
                    0x87 => '\u{2021}',
                    0x88 => '\u{02c6}',
                    0x89 => '\u{2030}',
                    0x8a => '\u{0160}',
                    0x8b => '\u{2039}',
                    0x8c => '\u{0152}',
                    0x8e => '\u{017d}',
                    0x91 => '\u{2018}',
                    0x92 => '\u{2019}',
                    0x93 => '\u{201c}',
                    0x94 => '\u{201d}',
                    0x95 => '\u{2022}',
                    0x96 => '\u{2013}',
                    0x97 => '\u{2014}',
                    0x98 => '\u{02dc}',
                    0x99 => '\u{2122}',
                    0x9a => '\u{0161}',
                    0x9b => '\u{203a}',
                    0x9c => '\u{0153}',
                    0x9e => '\u{017e}',
                    0x9f => '\u{0178}',
                    other => *other as char,
                })
                .collect(),
        ),
        _ => None,
    }
}

/// The SSRF-guarded (or deliberately unguarded, for the presets that mirror
/// `requests` defaults) HTTP client.
pub struct HttpEngine {
    resolver: SharedResolver,
    tls: TlsSettings,
    /// Replaces the pinned-call-site trust for the whole engine. Only tests and
    /// deployments with a private CA should set it.
    pinned_trust: Option<TlsTrust>,
}

impl HttpEngine {
    /// Uses the host resolver (`getaddrinfo`).
    pub fn system() -> Self {
        Self::new(Arc::new(crate::dns::SystemResolver))
    }

    pub fn new(resolver: SharedResolver) -> Self {
        Self {
            resolver,
            tls: TlsSettings::new(),
            pinned_trust: None,
        }
    }

    /// Trust `trust` for SSRF-pinned call sites instead of the bundled roots.
    pub fn with_pinned_trust(mut self, trust: TlsTrust) -> Self {
        self.pinned_trust = Some(trust);
        self
    }

    pub(crate) fn resolver(&self) -> &SharedResolver {
        &self.resolver
    }

    pub(crate) fn tls(&self) -> &TlsSettings {
        &self.tls
    }

    pub(crate) fn trust_for(&self, ssrf: bool) -> TlsTrust {
        if ssrf {
            self.pinned_trust.clone().unwrap_or(TlsTrust::Certifi)
        } else {
            TlsTrust::Environment
        }
    }

    /// `plane.utils.url_security.pinned_fetch`: one hop, redirects returned to
    /// the caller.
    pub async fn fetch_once(
        &self,
        preset: &Preset,
        method: Method,
        policy: &SsrfPolicy,
        url: &str,
        request: &OutboundRequest,
    ) -> Result<PinnedResponse, FetchError> {
        self.request_hop(preset, method, policy, url, request).await
    }

    /// `pinned_fetch_following_redirects` (or `pinned_fetch`, when the preset
    /// does not follow): returns the final response and the URL it came from.
    pub async fn fetch(
        &self,
        preset: &Preset,
        method: Method,
        policy: &SsrfPolicy,
        url: &str,
        request: &OutboundRequest,
    ) -> Result<(PinnedResponse, String), FetchError> {
        let max_redirects = preset.max_redirects.unwrap_or(0);
        let mut current = url.to_string();
        let mut redirects: u32 = 0;
        loop {
            if !preset.follows_redirects() {
                let response = self
                    .request_hop(preset, method.clone(), policy, &current, request)
                    .await?;
                return Ok((response, current));
            }
            let response = self
                .request_hop(preset, method.clone(), policy, &current, request)
                .await?;
            if !response.is_redirect() {
                return Ok((response, current));
            }
            let Some(location) = response.location() else {
                return Ok((response, current));
            };
            if redirects >= max_redirects {
                return Err(FetchError::TooManyRedirects {
                    max_redirects,
                    url: url.to_string(),
                });
            }
            redirects += 1;
            current = pyurl::urljoin(&current, location)?;
        }
    }

    async fn request_hop(
        &self,
        preset: &Preset,
        method: Method,
        policy: &SsrfPolicy,
        url: &str,
        request: &OutboundRequest,
    ) -> Result<PinnedResponse, FetchError> {
        if !preset.ssrf {
            let client = self.build_client(preset, None)?;
            return send(&client, method, url, preset, request, url).await;
        }

        let hop = validate_hop(self.resolver.as_ref(), url, policy).await?;
        let mut last_transport: Option<FetchError> = None;
        for address in &hop.addresses {
            match self
                .send_pinned(preset, method.clone(), url, &hop, *address, request)
                .await
            {
                Ok(response) => return Ok(response),
                Err(error) if error.is_request_exception() => {
                    last_transport = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_transport.unwrap_or_else(|| {
            FetchError::value(format!(
                "No reachable address for host: {}",
                hop.target.hostname
            ))
        }))
    }

    async fn send_pinned(
        &self,
        preset: &Preset,
        method: Method,
        url: &str,
        hop: &ValidatedHop,
        address: std::net::IpAddr,
        request: &OutboundRequest,
    ) -> Result<PinnedResponse, FetchError> {
        let target: &Target = &hop.target;
        let request_url = Url::parse(&target.request_url()).map_err(|error| {
            FetchError::Preparation(format!("invalid URL after validation: {error}"))
        })?;
        let host_key = request_url
            .host_str()
            .ok_or_else(|| FetchError::Preparation("validated URL has no host".to_string()))?
            .to_ascii_lowercase();
        let pin = PinnedDns::new(host_key, SocketAddr::new(address, target.port));
        let client = self.build_client(preset, Some(pin))?;
        send(&client, method, url, preset, request, request_url.as_ref()).await
    }

    fn build_client(&self, preset: &Preset, pin: Option<PinnedDns>) -> Result<Client, FetchError> {
        let config = self
            .tls
            .config(&self.trust_for(preset.ssrf))
            .map_err(FetchError::Tls)?;
        let mut builder: ClientBuilder = ClientBuilder::new()
            .use_preconfigured_tls((*config).clone())
            .redirect(redirect::Policy::none());
        if let Some(pin) = pin {
            builder = builder.dns_resolver(pin);
        }
        if preset.proxy == ProxyMode::Disabled {
            builder = builder.no_proxy();
        }
        if let Some(timeout) = preset.timeout {
            // `requests`' `timeout=` bounds each socket operation, not the
            // whole request: no total `timeout()` here on purpose.
            builder = builder.connect_timeout(timeout).read_timeout(timeout);
        }
        builder.build().map_err(FetchError::Transport)
    }
}

/// A resolver that answers exactly one name and fails closed for all others.
#[derive(Debug, Clone)]
struct PinnedDns {
    host: String,
    addresses: Vec<SocketAddr>,
}

impl PinnedDns {
    fn new(host: String, address: SocketAddr) -> Self {
        Self {
            host,
            addresses: vec![address],
        }
    }
}

impl reqwest::dns::Resolve for PinnedDns {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let key = name.as_str().to_ascii_lowercase();
        let result: Result<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>> =
            if key == self.host {
                Ok(Box::new(self.addresses.clone().into_iter()))
            } else {
                // Never fall back to the system resolver: that would reopen the
                // rebinding window this client exists to close.
                Err(Box::new(std::io::Error::other(format!(
                    "plane-net: refusing to resolve unpinned name {key:?}"
                ))))
            };
        Box::pin(std::future::ready(result))
    }
}

async fn send(
    client: &Client,
    method: Method,
    requested_url: &str,
    preset: &Preset,
    request: &OutboundRequest,
    url: &str,
) -> Result<PinnedResponse, FetchError> {
    let mut builder = client.request(method, url);
    for (name, value) in default_headers(preset) {
        builder = builder.header(name, header_value(&value)?);
    }
    // `OutboundRequest::headers` override the preset's headers on a name
    // collision, as a per-request `headers=` argument does in `requests`
    // (`requests/sessions.py:merge_setting`). `RequestBuilder::header`
    // *appends*, which would put both values on the wire, so each override goes
    // through `headers()`, whose merge replaces the stored value
    // (`reqwest::util::replace_headers`).
    for (name, value) in &request.headers {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            FetchError::Header(format!("invalid header name {name:?}: {error}"))
        })?;
        builder = builder.headers(HeaderMap::from_iter([(name, header_value(value)?)]));
    }
    if let Ok(target) = pyurl::split_target(requested_url) {
        builder = builder.header("Host", header_value(&target.host_header())?);
        if let Some((username, password)) = &target.auth {
            let credentials = format!("{username}:{password}");
            let encoded = base64::engine::general_purpose::STANDARD.encode(latin1(&credentials)?);
            builder = builder.header("Authorization", header_value(&format!("Basic {encoded}"))?);
        }
    }
    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }
    let response = builder.send().await.map_err(FetchError::Transport)?;
    let status = response.status().as_u16();
    let headers = HeaderList::from_map(response.headers());
    let limit = request.max_body_bytes.or(preset.max_body_bytes);
    let body = read_body(response, limit).await?;
    Ok(PinnedResponse {
        status,
        headers,
        url: url.to_string(),
        body,
    })
}

/// The headers `requests` would send: `default_headers()` in insertion order,
/// then the call site's own.
fn default_headers(preset: &Preset) -> Vec<(&'static str, String)> {
    let mut headers: Vec<(&'static str, String)> = vec![
        ("User-Agent", preset.user_agent.to_string()),
        ("Accept-Encoding", "gzip, deflate".to_string()),
        ("Accept", "*/*".to_string()),
        ("Connection", "keep-alive".to_string()),
    ];
    for (name, value) in preset.headers {
        if let Some(existing) = headers
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(name))
        {
            existing.1 = (*value).to_string();
        } else {
            headers.push((name, (*value).to_string()));
        }
    }
    headers
}

fn header_value(value: &str) -> Result<HeaderValue, FetchError> {
    HeaderValue::from_bytes(&latin1(value)?)
        .map_err(|error| FetchError::Header(format!("invalid header value {value:?}: {error}")))
}

/// `http.client` encodes header values and basic-auth credentials as
/// ISO-8859-1; a character outside it raises `UnicodeEncodeError` in Python.
fn latin1(value: &str) -> Result<Vec<u8>, FetchError> {
    value
        .chars()
        .map(|c| {
            u8::try_from(c as u32).map_err(|_| {
                FetchError::Header(format!(
                    "header value {value:?} contains a character outside ISO-8859-1"
                ))
            })
        })
        .collect()
}

async fn read_body(
    mut response: reqwest::Response,
    limit: Option<usize>,
) -> Result<Vec<u8>, FetchError> {
    if let Some(limit) = limit
        && let Some(length) = response.content_length()
        && length > limit as u64
    {
        return Err(FetchError::BodyTooLarge { limit });
    }
    let mut body: Vec<u8> = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or(0)
            .min(limit.unwrap_or(usize::MAX) as u64) as usize,
    );
    while let Some(chunk) = response.chunk().await.map_err(FetchError::Transport)? {
        if let Some(limit) = limit
            && body.len() + chunk.len() > limit
        {
            return Err(FetchError::BodyTooLarge { limit });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_list(pairs: &[(&str, &str)]) -> HeaderList {
        let mut headers = HeaderList::default();
        for (name, value) in pairs {
            headers.append(name, value);
        }
        headers
    }

    #[test]
    fn text_uses_latin1_for_text_without_charset() {
        let headers = header_list(&[("Content-Type", "text/plain")]);
        let response = PinnedResponse {
            status: 200,
            headers,
            url: String::new(),
            body: vec![0xE9],
        };
        assert_eq!(response.text(), "é");
    }

    #[test]
    fn text_uses_utf8_for_json_without_charset() {
        let headers = header_list(&[("Content-Type", "application/json")]);
        let response = PinnedResponse {
            status: 200,
            headers,
            url: String::new(),
            body: "é".as_bytes().to_vec(),
        };
        assert_eq!(response.text(), "é");
    }

    #[test]
    fn text_honours_the_charset_parameter() {
        let headers = header_list(&[("Content-Type", "text/plain; charset=\"utf-8\"")]);
        let response = PinnedResponse {
            status: 200,
            headers,
            url: String::new(),
            body: "é".as_bytes().to_vec(),
        };
        assert_eq!(response.text(), "é");
    }

    #[test]
    fn text_falls_back_to_utf8_with_replacement() {
        // requests' apparent_encoding is "utf-8" with chardet absent.
        let headers = header_list(&[("Content-Type", "application/octet-stream")]);
        let response = PinnedResponse {
            status: 200,
            headers,
            url: String::new(),
            body: vec![0xFF],
        };
        assert_eq!(response.text(), "\u{fffd}");
    }

    #[test]
    fn content_type_matching_is_case_sensitive_like_requests() {
        // `"text" in content_type` is a case-sensitive substring test.
        let headers = header_list(&[("Content-Type", "TEXT/PLAIN")]);
        let response = PinnedResponse {
            status: 200,
            headers,
            url: String::new(),
            body: vec![0xE9],
        };
        assert_eq!(response.text(), "\u{fffd}");
    }

    #[test]
    fn default_headers_put_the_user_agent_first() {
        let headers = default_headers(&crate::preset::WEBHOOK);
        assert_eq!(headers[0].0, "User-Agent");
        assert_eq!(headers[0].1, "Autopilot");
        assert!(headers.iter().any(|(name, _)| *name == "Accept-Encoding"));
    }

    #[test]
    fn unpinned_names_fail_closed() {
        let pin = PinnedDns::new(
            "example.com".to_string(),
            "93.184.216.34:443".parse().unwrap(),
        );
        let name: reqwest::dns::Name = "evil.example".parse().expect("valid name");
        let resolving = reqwest::dns::Resolve::resolve(&pin, name);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        assert!(runtime.block_on(resolving).is_err());
    }

    /// `OutboundRequest::headers` replace a preset header of the same name
    /// instead of appending a second value, which is what `requests` does with
    /// a per-request `headers=` argument. `RequestBuilder::header` appends, so
    /// `send` merges through `headers()`; this pins that.
    #[test]
    fn per_request_headers_override_the_presets() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let port = listener.local_addr().expect("addr").port();
            let stub = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let read = socket.read(&mut chunk).await.expect("read");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .expect("write");
                String::from_utf8_lossy(&request).into_owned()
            });

            let url = format!("http://127.0.0.1:{port}/x");
            let request = OutboundRequest::new()
                .header("User-Agent", "override/1.0")
                .header("Content-Type", "text/plain");
            HttpEngine::system()
                .fetch_once(
                    &crate::preset::UNSPLASH,
                    Method::GET,
                    &SsrfPolicy::strict(),
                    &url,
                    &request,
                )
                .await
                .expect("fetch");

            let head = stub.await.expect("stub");
            let lower = head.to_lowercase();
            assert_eq!(lower.matches("\r\nuser-agent:").count(), 1, "{head}");
            assert!(lower.contains("\r\nuser-agent: override/1.0\r\n"), "{head}");
            assert_eq!(lower.matches("\r\ncontent-type:").count(), 1, "{head}");
            assert!(lower.contains("\r\ncontent-type: text/plain\r\n"), "{head}");
        });
    }
}
