// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The outbound OpenAI call of `get_llm_response`
//! (`plane/app/views/external/base.py:123-145`), byte for byte.
//!
//! The Python builds `openai.OpenAI(api_key=...)` and calls
//! `chat.completions.create(model=..., messages=[{"role": "user", "content":
//! task + "\n" + prompt}])`. Everything the SDK puts on the wire is part of the
//! contract, so it is pinned twice:
//!
//! * `tools/ai/openai_wire.py` runs the pinned SDK (`openai==1.63.2`,
//!   `httpx==0.28.1`, CPython 3.12.13) against a local capture server and writes
//!   `tests/golden/openai_wire.json`;
//! * `tests/openai_wire.rs` compares this module's [`prepare`] output with that
//!   file, including the body bytes, and drives one request through the real
//!   [`plane_net`] transport into a local stub.
//!
//! The request is, in the SDK's order:
//!
//! ```text
//! POST {OPENAI_BASE_URL or https://api.openai.com/v1}/chat/completions
//! Accept: application/json
//! Content-Type: application/json
//! User-Agent: OpenAI/Python 1.63.2
//! X-Stainless-Lang: python
//! X-Stainless-Package-Version: 1.63.2
//! X-Stainless-OS / -Arch / -Runtime / -Runtime-Version
//! Authorization: Bearer <configured key>
//! X-Stainless-Async: false
//! OpenAI-Organization / OpenAI-Project   (only when OPENAI_ORG_ID / _PROJECT_ID)
//! x-stainless-retry-count: <attempt>
//! x-stainless-read-timeout: 600
//!
//! {"messages":[{"role":"user","content":"<task>\n<prompt>"}],"model":"<model>"}
//! ```
//!
//! Details read from the pinned SDK source rather than assumed:
//!
//! * the body's key order is `messages`, then `model` — `create()` builds the
//!   dict in signature order (`resources/chat/completions/completions.py`), and
//!   httpx's `encode_json` writes it with `separators=(",", ":")`,
//!   `ensure_ascii=False`, `allow_nan=False` and `sort_keys=False`
//!   (`httpx/_content.py:176-182`). CPython's escaping is reproduced in
//!   [`write_json_string`];
//! * `base_url` is `OPENAI_BASE_URL` or the default, with a trailing slash
//!   enforced and `chat/completions` appended to the *path* only
//!   (`openai/_base_client.py:445-458`; the query string of a `base_url` is
//!   kept);
//! * `OpenAI-Organization` / `OpenAI-Project` come from `OPENAI_ORG_ID` /
//!   `OPENAI_PROJECT_ID` at client construction (`openai/_client.py:121-127`),
//!   which is per request here;
//! * `x-stainless-retry-count` and `x-stainless-read-timeout` are added per
//!   attempt (`openai/_base_client.py:420-435`);
//! * retries: `DEFAULT_MAX_RETRIES = 2` (`openai/_constants.py:11`), on 408, 409,
//!   429 and 5xx (`openai/_base_client.py:680-700`), plus transport errors;
//! * redirects are followed, up to httpx's 20 (`httpx/_client.py:198,960-1000`),
//!   with httpx's method rules: a 303 (or 302) turns the POST into a GET, a 301
//!   does only for a POST, and a method change to GET drops the body — while
//!   `Content-Type` stays, exactly as `_redirect_headers` leaves it.
//!
//! Two deliberate differences, both unobservable through the HTTP contract:
//!
//! * the retry backoff sleep is not reproduced (0.5 s, then 1 s, plus jitter in
//!   the SDK): retries here are immediate, because the crate has no timer
//!   dependency and no observer of the delay exists. The *number* of attempts
//!   and which responses cause one are ported;
//! * `X-Stainless-OS` / `-Arch` are derived from the Rust binary's own platform
//!   (Python derives them from `platform.system()` / `platform.machine()` of
//!   the interpreter it runs on, which is the same machine in a deployment).
//!   `X-Stainless-Runtime-Version` is pinned to the interpreter recorded in the
//!   golden: the header is informational, and a Rust binary cannot know the
//!   patch level of a Python it does not run.

use http::Method;
use plane_net::{FetchError, HttpEngine, OutboundRequest, SsrfPolicy, preset, pyurl};

use crate::ai::config::LlmConfig;

/// `openai.__version__` the golden was recorded with (`apps/api/pyproject.toml`
/// pins `openai==1.63.2`). Bump it together with the golden.
pub const SDK_VERSION: &str = "1.63.2";

/// `platform.python_version()` of the interpreter the golden recorded.
///
/// The SDK sends it as `X-Stainless-Runtime-Version`
/// (`openai/_base_client.py:2024`). It is cosmetic — OpenAI's API does not read
/// it — but it is part of the recorded request, so the golden test pins it.
pub const PYTHON_RUNTIME_VERSION: &str = "3.12.13";

/// `OpenAI.base_url`'s default (`openai/_client.py:125-128`).
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// `DEFAULT_MAX_RETRIES` (`openai/_constants.py:11`): two retries, three
/// attempts.
pub const MAX_ATTEMPTS: u32 = 3;

/// `httpx.DEFAULT_MAX_REDIRECTS` (`httpx/_config.py`), which the SDK keeps.
pub const MAX_REDIRECTS: usize = 20;

/// The request the SDK would send, as bytes this crate controls.
///
/// The transport still adds `Host`, `Content-Length`, `Accept-Encoding` and
/// `Connection`, as it does for every preset; `headers` is exactly the set the
/// SDK's `_build_headers` produces, in httpx's insertion order.
///
/// The names here keep the SDK's spelling (the golden compares it), but the
/// transport normalises them: `http::HeaderName` stores a name lower-cased, and
/// HTTP/1.1 field names are case-insensitive (RFC 9110 §5.1). The wire bytes
/// therefore differ in case while the header set does not; no client reads the
/// case of a request header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// `str(timeout.read)` for `DEFAULT_TIMEOUT` (`openai/_constants.py:10`).
const READ_TIMEOUT: &str = "600";

/// `platform.system()`-derived `X-Stainless-OS`
/// (`openai/_base_client.py:1974-2012`), for the platform this binary runs on.
///
/// Python maps `darwin`→`MacOS`, `windows`→`Windows`, and the BSDs by `distro.id`;
/// anything else is `OtherPlatform(<platform.platform().lower()>)`, which no
/// Rust build can know, so it becomes `Unknown`.
pub const fn platform_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "MacOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "ios") {
        "iOS"
    } else if cfg!(target_os = "android") {
        "Android"
    } else if cfg!(target_os = "freebsd") {
        "FreeBSD"
    } else if cfg!(target_os = "openbsd") {
        "OpenBSD"
    } else if cfg!(target_os = "netbsd") {
        "NetBSD"
    } else {
        "Unknown"
    }
}

/// `get_architecture()` (`openai/_base_client.py:2054-2078`) for the platform
/// this binary runs on: `arm64`/`aarch64` → `arm64`, `arm` → `arm`, `x86_64` →
/// `x64`, a 32-bit x86 → `x32`.
pub const fn platform_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "x86") {
        "x32"
    } else {
        "unknown"
    }
}

/// `httpx.URL(base_url)` + `_enforce_trailing_slash` + `_prepare_url`
/// (`openai/_base_client.py:445-458`).
///
/// httpx replaces only the raw *path*, so a `base_url` with a query keeps it:
/// `http://h/v1?x=1` becomes `http://h/v1/chat/completions?x=1`. A fragment is
/// kept the same way. Percent-encoding and dot-segment handling are httpx's
/// `URL` normalisation, which this does not reproduce: `OPENAI_BASE_URL` is a
/// deployment setting, not user input.
pub fn chat_completions_url(base_url: &str) -> String {
    let split = base_url.find(['?', '#']).unwrap_or(base_url.len());
    let (path, suffix) = base_url.split_at(split);
    let mut path = path.to_string();
    if !path.ends_with('/') {
        path.push('/');
    }
    path.push_str("chat/completions");
    path.push_str(suffix);
    path
}

/// CPython's `json.encoder.py_encode_basestring` with `ensure_ascii=False`.
///
/// `json.dumps` escapes `"` and `\`, the five named control characters, and
/// every other code point below `0x20` as `\u00XX`; everything else, including
/// `0x7f`, U+2028 and U+2029, is written unchanged (as UTF-8).
fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", other as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// `httpx._content.encode_json` of the SDK's `create(model=…, messages=[…])`.
pub fn encode_body(model: &str, content: &str) -> Vec<u8> {
    let mut body = String::with_capacity(model.len() + content.len() + 64);
    body.push_str("{\"messages\":[{\"role\":\"user\",\"content\":");
    write_json_string(&mut body, content);
    body.push_str("}],\"model\":");
    write_json_string(&mut body, model);
    body.push('}');
    body.into_bytes()
}

/// The whole request, as `_build_request` + `encode_json` produce it.
///
/// `create()` sets no `x-stainless-helper-method` (only the streaming helpers
/// do), so the recorded request has none either.
///
/// `retries_taken` is the attempt index: `x-stainless-retry-count` is `"0"` on
/// the first attempt and `"1"`, `"2"` on the retries.
pub fn prepare(
    base_url: &str,
    api_key: &str,
    model: &str,
    content: &str,
    organization: Option<&str>,
    project: Option<&str>,
    retries_taken: u32,
) -> WireRequest {
    let mut headers: Vec<(String, String)> = vec![
        ("Accept".into(), "application/json".into()),
        ("Content-Type".into(), "application/json".into()),
        ("User-Agent".into(), format!("OpenAI/Python {SDK_VERSION}")),
        ("X-Stainless-Lang".into(), "python".into()),
        (
            "X-Stainless-Package-Version".into(),
            SDK_VERSION.to_string(),
        ),
        ("X-Stainless-OS".into(), platform_os().to_string()),
        ("X-Stainless-Arch".into(), platform_arch().to_string()),
        ("X-Stainless-Runtime".into(), "CPython".into()),
        (
            "X-Stainless-Runtime-Version".into(),
            PYTHON_RUNTIME_VERSION.into(),
        ),
        ("Authorization".into(), format!("Bearer {api_key}")),
        ("X-Stainless-Async".into(), "false".into()),
    ];
    // `default_headers` inserts these two with `Omit()` when unset, i.e. the
    // header is absent (`openai/_client.py:175-180`).
    if let Some(organization) = organization {
        headers.push(("OpenAI-Organization".into(), organization.into()));
    }
    if let Some(project) = project {
        headers.push(("OpenAI-Project".into(), project.into()));
    }
    headers.push(("x-stainless-retry-count".into(), retries_taken.to_string()));
    headers.push(("x-stainless-read-timeout".into(), READ_TIMEOUT.into()));

    WireRequest {
        url: chat_completions_url(base_url),
        headers,
        body: encode_body(model, content),
    }
}

/// What `get_llm_response` returns: the SDK's `(text, None)` or its
/// `(None, error)` (`external/base.py:135-145`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmReply {
    /// `text = chat_completion.choices[0].message.content`. `None` is a JSON
    /// `null` content, which the caller then crashes on (`text.replace`).
    Text(Option<String>),
    /// The mapped error message, by exception class name.
    Failed(String),
}

impl LlmReply {
    /// `f"Error occurred while generating response from {provider}"`.
    fn generic(config: &LlmConfig) -> Self {
        Self::Failed(format!(
            "Error occurred while generating response from {}",
            config.provider
        ))
    }
}

/// `get_llm_response(task, prompt, api_key, model, provider)` minus the
/// `task + "\n" + prompt` join, which the caller does because a non-`str`
/// operand is a `TypeError` there (`external/base.py:125`, outside the `try`).
pub async fn get_llm_response(content: &str, config: &LlmConfig) -> LlmReply {
    // `OpenAI(api_key=...)` with no `base_url`/`organization`/`project`
    // arguments: the environment is read at client construction, per request
    // (`openai/_client.py:112-128`).
    let base_url =
        std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let organization = std::env::var("OPENAI_ORG_ID").ok();
    let project = std::env::var("OPENAI_PROJECT_ID").ok();
    respond(
        &base_url,
        config,
        content,
        organization.as_deref(),
        project.as_deref(),
    )
    .await
}

/// The retry loop around one logical call (`_request`,
/// `openai/_base_client.py:970-1060`), with the environment already resolved so
/// a unit test can point it at a local stub.
async fn respond(
    base_url: &str,
    config: &LlmConfig,
    content: &str,
    organization: Option<&str>,
    project: Option<&str>,
) -> LlmReply {
    // `if provider.lower() == "gemini": model = f"gemini/{model}"`
    // (`external/base.py:127-129`).
    let model = if config.provider.to_lowercase() == "gemini" {
        format!("gemini/{}", config.model)
    } else {
        config.model.clone()
    };

    for attempt in 0..MAX_ATTEMPTS {
        let wire = prepare(
            base_url,
            &config.api_key,
            &model,
            content,
            organization,
            project,
            attempt,
        );
        // A transport error is retried by the SDK too; its class name is none
        // of the two the Python special-cases, so it ends as the generic
        // message below.
        if let Ok(response) = send(&wire).await {
            if retryable(response.status) && attempt + 1 < MAX_ATTEMPTS {
                continue;
            }
            return classify(response.status, response.body.as_slice(), config);
        }
    }
    LlmReply::generic(config)
}

/// The statuses `_should_retry` accepts, plus the transport-error branch:
/// 408, 409, 429 and every 5xx (`openai/_base_client.py:680-700`).
fn retryable(status: u16) -> bool {
    matches!(status, 408 | 409 | 429) || status >= 500
}

/// `_process_response`'s outcomes, mapped by exception class name
/// (`external/base.py:139-145`).
fn classify(status: u16, body: &[u8], config: &LlmConfig) -> LlmReply {
    match status {
        401 => LlmReply::Failed(format!("Invalid API key for {}", config.provider)),
        429 => LlmReply::Failed(format!("Rate limit exceeded for {}", config.provider)),
        status if !(200..300).contains(&status) => LlmReply::generic(config),
        _ => match parse_reply(body) {
            // `choices[0].message.content`
            Ok(content) => LlmReply::Text(content),
            // A JSON decode error, a missing `choices`, an empty list or a
            // non-object `message` all raise inside the SDK, inside the
            // Python's `try`.
            Err(()) => LlmReply::generic(config),
        },
    }
}

/// `chat_completion.choices[0].message.content` from a 2xx body.
///
/// `httpx.Response.json()` is `json.loads(content)`, so the bytes are sniffed
/// the way CPython's `json.detect_encoding` does before decoding.
fn parse_reply(body: &[u8]) -> Result<Option<String>, ()> {
    let value = plane_serde::pyjson::loads(body, detect_encoding(body)).map_err(|_| ())?;
    let choices = value
        .get("choices")
        .and_then(|value| value.as_list())
        .ok_or(())?;
    let first = choices.first().ok_or(())?;
    let message = first
        .get("message")
        .filter(|value| value.as_dict().is_some())
        .ok_or(())?;
    // `content: Optional[str] = None` on `ChatCompletionMessage`: an absent key
    // and a JSON null are both `None`, and the view's `text.replace` then
    // raises `AttributeError` (a family 500).
    match message.get("content") {
        None => Ok(None),
        Some(value) if value.is_null() => Ok(None),
        Some(value) => match value.as_pystr() {
            Some(text) => Ok(Some(text.as_str().ok_or(())?.to_string())),
            // A non-string content is `AttributeError: '<type>' object has no
            // attribute 'replace'` in the view.
            None => Ok(None),
        },
    }
}

/// CPython 3.12's `json.detect_encoding` (`Lib/json/__init__.py`).
fn detect_encoding(body: &[u8]) -> &'static str {
    fn starts_with(body: &[u8], prefix: &[u8]) -> bool {
        body.starts_with(prefix)
    }
    if starts_with(body, &[0x00, 0x00, 0xfe, 0xff]) || starts_with(body, &[0xff, 0xfe, 0x00, 0x00])
    {
        return "utf-32";
    }
    if starts_with(body, &[0xfe, 0xff]) || starts_with(body, &[0xff, 0xfe]) {
        return "utf-16";
    }
    if starts_with(body, &[0xef, 0xbb, 0xbf]) {
        return "utf-8-sig";
    }
    if body.len() >= 4 {
        if body[0] == 0 {
            return if body[1] == 0 {
                "utf-32-be"
            } else {
                "utf-16-be"
            };
        }
        if body[1] == 0 {
            return if body[2] == 0 {
                "utf-32-le"
            } else {
                "utf-16-le"
            };
        }
    } else if body.len() == 2 {
        if body[0] == 0 {
            return "utf-16-be";
        }
        if body[1] == 0 {
            return "utf-16-le";
        }
    }
    "utf-8"
}

/// One finished response, as `_send_handling_redirects` leaves it.
struct Finished {
    status: u16,
    body: Vec<u8>,
}

/// One attempt: the request, then httpx's redirect chain.
async fn send(wire: &WireRequest) -> Result<Finished, FetchError> {
    let mut method = Method::POST;
    let mut url = wire.url.clone();
    let mut headers = wire.headers.clone();
    let mut body = Some(wire.body.clone());
    let mut redirects = 0usize;
    // The current hop's (scheme, host, port), for httpx's same-origin test.
    let mut origin = pyurl::split_target(&wire.url)
        .ok()
        .map(|target| (target.scheme, target.hostname, target.port));

    loop {
        let mut request = OutboundRequest::new();
        for (name, value) in &headers {
            request = request.header(name.clone(), value.clone());
        }
        if let Some(body) = &body {
            request = request.body(body.clone());
        }
        // The `LLM` preset (`plane-net/src/preset.rs:330-347`): 600 s read and
        // connect, environment proxies, no SSRF pinning, the whole body.
        let response = engine()
            .fetch_once(
                &preset::LLM,
                method.clone(),
                &SsrfPolicy::strict(),
                &url,
                &request,
            )
            .await?;

        let location = response.location().map(str::to_string);
        if !matches!(response.status, 301 | 302 | 303 | 307 | 308) || location.is_none() {
            return Ok(Finished {
                status: response.status,
                body: response.bytes().to_vec(),
            });
        }
        // `if len(history) > self.max_redirects: raise TooManyRedirects`
        // (`httpx/_client.py:970-972`).
        if redirects >= MAX_REDIRECTS {
            return Err(FetchError::TooManyRedirects {
                max_redirects: u32::try_from(MAX_REDIRECTS).unwrap_or(u32::MAX),
                url: wire.url.clone(),
            });
        }
        redirects += 1;

        let location = location.unwrap_or_default();
        let next = pyurl::urljoin(&url, &location)
            .map_err(|error| FetchError::value(error.to_string()))?;
        // `_redirect_method` (`httpx/_client.py:494-515`).
        if method != Method::HEAD
            && (response.status == 303
                || response.status == 302
                || (response.status == 301 && method == Method::POST))
        {
            method = Method::GET;
        }
        if method == Method::GET && body.is_some() {
            // `_redirect_headers` pops `Content-Length` and `Transfer-Encoding`
            // but leaves `Content-Type`; `_redirect_stream` drops the body.
            body = None;
            headers.retain(|(name, _)| {
                !name.eq_ignore_ascii_case("content-length")
                    && !name.eq_ignore_ascii_case("transfer-encoding")
            });
        }
        // `_redirect_headers` (`httpx/_client.py:546-570`): away from the
        // current origin, `Authorization` is dropped — unless it is a plain
        // http → https upgrade of the same host and port. The comparison is
        // hop-to-hop, as httpx compares the redirecting request with the new
        // URL.
        if let (Some((scheme, hostname, port)), Ok(next_target)) =
            (origin.as_ref(), pyurl::split_target(&next))
        {
            let same_host =
                hostname.eq_ignore_ascii_case(&next_target.hostname) && *port == next_target.port;
            let same_origin = same_host && scheme.eq_ignore_ascii_case(&next_target.scheme);
            let https_upgrade = same_host && scheme == "http" && next_target.scheme == "https";
            if !same_origin && !https_upgrade {
                headers.retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
            }
            origin = Some((next_target.scheme, next_target.hostname, next_target.port));
        }
        url = next;
    }
}

/// The process-wide outbound client, shared with the other unguarded call sites
/// (`DECISIONS.md` D9).
fn engine() -> &'static HttpEngine {
    static ENGINE: std::sync::OnceLock<HttpEngine> = std::sync::OnceLock::new();
    ENGINE.get_or_init(HttpEngine::system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_url_join_matches_httpx() {
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("http://127.0.0.1:1234"),
            "http://127.0.0.1:1234/chat/completions"
        );
        assert_eq!(
            chat_completions_url("http://127.0.0.1:1234/proxy/openai/"),
            "http://127.0.0.1:1234/proxy/openai/chat/completions"
        );
        // httpx replaces only the path, so a query survives.
        assert_eq!(
            chat_completions_url("http://127.0.0.1:1234/v1?x=1"),
            "http://127.0.0.1:1234/v1/chat/completions?x=1"
        );
    }

    /// CPython's escaping, for every interesting class of character.
    #[test]
    fn the_body_is_cpython_json_dumps() {
        let content = "a\"b\\c\u{8}\u{c}\n\r\t\u{0}\u{1f}\u{7f}\u{2028}\u{2029}é🚀";
        let body = encode_body("gpt-4o-mini", content);
        assert_eq!(
            String::from_utf8(body).expect("utf-8"),
            "{\"messages\":[{\"role\":\"user\",\"content\":\"a\\\"b\\\\c\\b\\f\\n\\r\\t\\u0000\\u001f\u{7f}\u{2028}\u{2029}é🚀\"}],\"model\":\"gpt-4o-mini\"}".to_string()
        );
    }

    #[test]
    fn the_header_set_is_the_sdk_set() {
        let wire = prepare(
            "https://api.openai.com/v1",
            "sk-test",
            "gpt-4o-mini",
            "x",
            Some("org"),
            None,
            2,
        );
        let names: Vec<&str> = wire.headers.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Accept",
                "Content-Type",
                "User-Agent",
                "X-Stainless-Lang",
                "X-Stainless-Package-Version",
                "X-Stainless-OS",
                "X-Stainless-Arch",
                "X-Stainless-Runtime",
                "X-Stainless-Runtime-Version",
                "Authorization",
                "X-Stainless-Async",
                "OpenAI-Organization",
                "x-stainless-retry-count",
                "x-stainless-read-timeout",
            ]
        );
        let value = |name: &str| {
            wire.headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(value("Authorization"), Some("Bearer sk-test"));
        assert_eq!(value("OpenAI-Organization"), Some("org"));
        assert_eq!(value("OpenAI-Project"), None);
        assert_eq!(value("x-stainless-retry-count"), Some("2"));
        assert_eq!(value("x-stainless-read-timeout"), Some("600"));
        assert_eq!(value("X-Stainless-Async"), Some("false"));
    }

    #[test]
    fn the_reply_sniffs_utf16_like_json_loads() {
        // `"\u{1f600}"` in UTF-16-LE with a BOM: `json.loads(bytes)` decodes it.
        let mut body = vec![0xff, 0xfe];
        for unit in "\"ok\"".encode_utf16() {
            body.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(detect_encoding(&body), "utf-16");
        let reply = parse_reply(b"{\"choices\":[{\"message\":{\"content\":\"ok\"}}]}");
        assert_eq!(reply, Ok(Some("ok".to_string())));
    }

    #[test]
    fn a_missing_or_null_content_is_none() {
        assert_eq!(
            parse_reply(b"{\"choices\":[{\"message\":{}}]}"),
            Ok(None),
            "an absent content key is the model's default None"
        );
        assert_eq!(
            parse_reply(b"{\"choices\":[{\"message\":{\"content\":null}}]}"),
            Ok(None)
        );
        assert!(parse_reply(b"{\"choices\":[]}").is_err());
        assert!(parse_reply(b"{\"choices\":[{}]}").is_err());
        assert!(parse_reply(b"not json").is_err());
        assert!(parse_reply(b"{}").is_err());
    }

    #[test]
    fn the_retry_statuses_match_the_sdk() {
        for status in [408, 409, 429, 500, 502, 599] {
            assert!(retryable(status), "{status}");
        }
        for status in [200, 201, 301, 400, 401, 403, 404, 422, 499] {
            assert!(!retryable(status), "{status}");
        }
    }

    // -----------------------------------------------------------------
    // The live path, against a stub bound to 127.0.0.1. No request can
    // reach api.openai.com: `respond` takes the base URL, and every test
    // gives it the stub's origin.
    // -----------------------------------------------------------------

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A reply with a fixed body and status, closed after one request.
    fn http_response(status: u16, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    const CHAT_COMPLETION: &str = r#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"message":{"role":"assistant","content":"the answer"},"finish_reason":"stop"}]}"#;

    /// Serve `replies` on one connection each, in order, and return the raw
    /// requests.
    async fn serve(replies: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::with_capacity(replies.len());
            for reply in replies {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let read = socket.read(&mut chunk).await.expect("read");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    if let Some(head_end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&request[..head_end]).to_lowercase();
                        let length = head
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= head_end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                socket.write_all(reply.as_bytes()).await.expect("write");
                socket.flush().await.expect("flush");
            }
            requests
        });
        (format!("http://127.0.0.1:{port}"), handle)
    }

    fn config(provider: &str, model: &str) -> LlmConfig {
        LlmConfig {
            api_key: "wire-golden-key".to_string(),
            model: model.to_string(),
            provider: provider.to_string(),
        }
    }

    /// The transport adds `Host`, `Content-Length`, `Accept-Encoding` and
    /// `Connection`; everything else must arrive exactly as [`prepare`] built
    /// it, and the body must be the encoded JSON.
    #[tokio::test]
    async fn the_transport_sends_the_prepared_request() {
        let (base, handle) = serve(vec![http_response(200, CHAT_COMPLETION)]).await;
        let reply = respond(
            &base,
            &config("openai", "gpt-4o-mini"),
            "Summarise\nit",
            None,
            Some("proj-wire"),
        )
        .await;
        assert_eq!(reply, LlmReply::Text(Some("the answer".to_string())));

        let requests = handle.await.expect("stub");
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        let (head, body) = request.split_once("\r\n\r\n").expect("head");
        let mut lines = head.lines();
        assert_eq!(lines.next(), Some("POST /chat/completions HTTP/1.1"));
        let lower = head.to_lowercase();
        for (name, value) in &[
            ("accept", "application/json"),
            ("content-type", "application/json"),
            ("user-agent", "openai/python 1.63.2"),
            ("x-stainless-lang", "python"),
            ("x-stainless-package-version", "1.63.2"),
            ("x-stainless-runtime", "cpython"),
            ("authorization", "bearer wire-golden-key"),
            ("x-stainless-async", "false"),
            ("openai-project", "proj-wire"),
            ("x-stainless-retry-count", "0"),
            ("x-stainless-read-timeout", "600"),
        ] {
            assert!(
                lower.contains(&format!("\r\n{name}: {value}\r\n")),
                "missing {name}: {value} in\n{head}"
            );
        }
        assert!(
            !lower.contains("openai-organization"),
            "no organization was configured"
        );
        assert!(
            !lower.contains("python-requests"),
            "the SDK user agent must replace the preset's"
        );
        assert_eq!(
            body,
            String::from_utf8(encode_body("gpt-4o-mini", "Summarise\nit")).expect("utf-8")
        );
    }

    /// Two retries on a 5xx, and `x-stainless-retry-count` counts them.
    #[tokio::test]
    async fn a_server_error_is_retried_twice() {
        let (base, handle) = serve(vec![
            http_response(500, "{}"),
            http_response(500, "{}"),
            http_response(503, "{}"),
        ])
        .await;
        let reply = respond(&base, &config("openai", "gpt-4o-mini"), "t\np", None, None).await;
        assert_eq!(
            reply,
            LlmReply::Failed("Error occurred while generating response from openai".to_string())
        );
        let requests = handle.await.expect("stub");
        assert_eq!(requests.len(), 3, "three attempts in total");
        for (attempt, request) in requests.iter().enumerate() {
            assert!(
                request
                    .to_lowercase()
                    .contains(&format!("x-stainless-retry-count: {attempt}\r\n")),
                "attempt {attempt} carries its index:\n{request}"
            );
        }
    }

    /// `AuthenticationError` and `RateLimitError` are the two the Python
    /// special-cases by class name (`external/base.py:139-145`).
    #[tokio::test]
    async fn a_401_and_a_429_have_their_own_messages() {
        let (base, handle) = serve(vec![http_response(
            401,
            r#"{"error":{"message":"bad key"}}"#,
        )])
        .await;
        assert_eq!(
            respond(&base, &config("openai", "gpt-4o-mini"), "t\np", None, None).await,
            LlmReply::Failed("Invalid API key for openai".to_string())
        );
        handle.await.expect("stub");

        let (base, handle) = serve(vec![
            http_response(429, "{}"),
            http_response(429, "{}"),
            http_response(429, "{}"),
        ])
        .await;
        assert_eq!(
            respond(&base, &config("anthropic", "claude-2"), "t\np", None, None).await,
            LlmReply::Failed("Rate limit exceeded for anthropic".to_string())
        );
        assert_eq!(handle.await.expect("stub").len(), 3, "a 429 is retried");
    }

    /// `provider.lower() == "gemini"` prefixes the model; the raw provider
    /// string is what the error messages use.
    #[tokio::test]
    async fn gemini_prefixes_the_model() {
        let (base, handle) = serve(vec![http_response(200, CHAT_COMPLETION)]).await;
        respond(&base, &config("Gemini", "gemini-pro"), "t\np", None, None).await;
        let requests = handle.await.expect("stub");
        assert!(
            requests[0].contains(r#""model":"gemini/gemini-pro""#),
            "{}",
            requests[0]
        );
    }

    /// httpx follows redirects; a 307 keeps the method and the body.
    #[tokio::test]
    async fn a_redirect_is_followed_with_the_body_intact() {
        let (base, handle) = serve(vec![
            // A relative Location: httpx joins it against the request URL.
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: /moved\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
            http_response(200, CHAT_COMPLETION),
        ])
        .await;
        let reply = respond(&base, &config("openai", "gpt-4o-mini"), "t\np", None, None).await;
        assert_eq!(reply, LlmReply::Text(Some("the answer".to_string())));
        let requests = handle.await.expect("stub");
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("POST /chat/completions "));
        assert!(requests[1].starts_with("POST /moved "), "{}", requests[1]);
        assert!(
            requests[1].contains(r#""content":"t\np""#),
            "the body survives a 307: {}",
            requests[1]
        );
    }
}
