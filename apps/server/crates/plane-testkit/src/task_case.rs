// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The per-case half of `difftest task`: what to compare besides rows.
//!
//! A case file (JSON, next to the kwargs and setup SQL in
//! `apps/server/tools/jobs/fixtures/`) names the effects a task has that a
//! single table query cannot see:
//!
//! ```json
//! {
//!   "task": "plane.bgtasks.work_item_link_task.crawl_work_item_link_title",
//!   "sql": [{"label": "issue_links", "query": "SELECT ... FROM issue_links ORDER BY id"}],
//!   "s3": {"seeds": [{"key": "old.zip", "text": "x", "content_type": "application/zip"}]},
//!   "http": {"responses": {"/page": {"status": 200, "headers": {"Content-Type": "text/html"}, "body": "<title>Hi</title>"}}},
//!   "normalize": [{"pattern": "\"LastModified\":\"[^\"]*\"", "replacement": "\"LastModified\":\"<TS>\""}]
//! }
//! ```
//!
//! * `sql` queries run against both clones after the task; every column is
//!   cast to text by the query itself (`::text`, `COALESCE`) so the comparison
//!   has no type-dependent formatting. Row order is the query's `ORDER BY`.
//! * `s3.seeds` are written to **both** side buckets before the task. After
//!   the run, every object in each bucket is read back with its bytes,
//!   `Content-Type` and ACL, and the two listings are compared. The buckets
//!   are the tool's (created and deleted by it), never the shared `uploads`
//!   bucket.
//! * `http` starts a local recorder (`127.0.0.1:0`), substitutes `{{http}}` in
//!   the kwargs with its base URL, answers from `responses` (default `404`),
//!   and compares the requests each side made: method, path, query,
//!   `User-Agent` and body. Nothing leaves the machine.
//! * `normalize` regexes apply to every SQL cell and every S3 key before
//!   comparison. They exist for generated values a case cannot pin (a uuid4
//!   key segment, an S3 `LastModified`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::s3::{ObservedObject, SeedObject};

/// One case file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCase {
    /// Asserted against `--task` so a case cannot silently run another task.
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub sql: Vec<CaseQuery>,
    #[serde(default)]
    pub s3: Option<S3Case>,
    #[serde(default)]
    pub http: Option<HttpCase>,
    #[serde(default)]
    pub normalize: Vec<Normalize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseQuery {
    pub label: String,
    pub query: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S3Case {
    #[serde(default)]
    pub seeds: Vec<SeedSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedSpec {
    pub key: String,
    /// Inline body; either this or `base64`.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub base64: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub acl: Option<String>,
}

impl SeedSpec {
    pub fn body(&self) -> Result<Vec<u8>> {
        match (&self.text, &self.base64) {
            (Some(text), None) => Ok(text.as_bytes().to_vec()),
            (None, Some(encoded)) => base64_decode(encoded),
            (None, None) => Ok(Vec::new()),
            (Some(_), Some(_)) => bail!("seed {} sets both text and base64", self.key),
        }
    }

    pub fn object(&self) -> Result<SeedObject> {
        Ok(SeedObject {
            key: self.key.clone(),
            body: self.body()?,
            content_type: self.content_type.clone(),
            acl: self.acl.clone(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpCase {
    #[serde(default)]
    pub responses: BTreeMap<String, FakeResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FakeResponse {
    #[serde(default = "default_status")]
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: String,
    /// Binary body (a favicon fixture); `body` and this are exclusive.
    #[serde(default)]
    pub body_base64: Option<String>,
}

impl FakeResponse {
    fn body_bytes(&self) -> Result<Vec<u8>> {
        match &self.body_base64 {
            Some(encoded) => base64_decode(encoded),
            None => Ok(self.body.as_bytes().to_vec()),
        }
    }
}

fn default_status() -> u16 {
    200
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Normalize {
    pub pattern: String,
    pub replacement: String,
}

impl TaskCase {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading case {}", path.display()))?;
        let case: Self = serde_json::from_str(&text)
            .with_context(|| format!("parsing case {}", path.display()))?;
        // Compile the normalisers now so a bad pattern fails before any run.
        case.compiled()?;
        Ok(case)
    }

    fn compiled(&self) -> Result<Vec<(regex::Regex, String)>> {
        self.normalize
            .iter()
            .map(|rule| {
                Ok((
                    regex::Regex::new(&rule.pattern)
                        .with_context(|| format!("normalize pattern {:?}", rule.pattern))?,
                    rule.replacement.clone(),
                ))
            })
            .collect()
    }

    /// Apply every normaliser to a cell or a key.
    pub fn normalize_text(&self, value: &str) -> String {
        let mut out = value.to_string();
        for rule in &self.normalize {
            // The regex was validated in `load`; a bad one here is a
            // programming error, not user input.
            let regex = regex::Regex::new(&rule.pattern).expect("validated normalizer");
            out = regex
                .replace_all(&out, rule.replacement.as_str())
                .into_owned();
        }
        out
    }
}

/// Compare one case query's rows.
pub fn compare_rows(
    case: &TaskCase,
    label: &str,
    a: &[Vec<String>],
    b: &[Vec<String>],
) -> Result<()> {
    let normalize = |rows: &[Vec<String>]| -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| row.iter().map(|cell| case.normalize_text(cell)).collect())
            .collect()
    };
    let a = normalize(a);
    let b = normalize(b);
    if a == b {
        println!("PASS: sql {label}: {} row(s) identical", a.len());
        return Ok(());
    }
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index);
        let right = b.get(index);
        if left != right {
            println!("sql {label} row {index}:\n  django: {left:?}\n  rust:   {right:?}");
            break;
        }
    }
    bail!("sql {label}: rows differ")
}

/// Compare the two buckets' listings.
pub fn compare_objects(case: &TaskCase, a: &[ObservedObject], b: &[ObservedObject]) -> Result<()> {
    let normalize = |objects: &[ObservedObject]| -> Vec<ObservedObject> {
        objects
            .iter()
            .map(|object| ObservedObject {
                key: case.normalize_text(&object.key),
                ..object.clone()
            })
            .collect()
    };
    let a = normalize(a);
    let b = normalize(b);
    if a == b {
        println!(
            "PASS: s3: {} object(s) identical (keys, bytes, content type, acl)",
            a.len()
        );
        return Ok(());
    }
    let keys_a: Vec<&str> = a.iter().map(|object| object.key.as_str()).collect();
    let keys_b: Vec<&str> = b.iter().map(|object| object.key.as_str()).collect();
    if keys_a != keys_b {
        bail!("s3 keys differ:\n  django: {keys_a:?}\n  rust:   {keys_b:?}");
    }
    for (left, right) in a.iter().zip(&b) {
        if left.content_type != right.content_type {
            bail!(
                "s3 {}: content type differs ({:?} vs {:?})",
                left.key,
                left.content_type,
                right.content_type
            );
        }
        if left.acl != right.acl {
            bail!(
                "s3 {}: acl differs ({:?} vs {:?})",
                left.key,
                left.acl,
                right.acl
            );
        }
        if left.body != right.body {
            bail!(
                "s3 {}: bytes differ (django {} bytes {:02x?}..., rust {} bytes {:02x?}...)",
                left.key,
                left.body.len(),
                &left.body[..left.body.len().min(16)],
                right.body.len(),
                &right.body[..right.body.len().min(16)],
            );
        }
    }
    bail!("s3: objects differ")
}

/// One request the recorder saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub query: String,
    pub user_agent: Option<String>,
    pub body: Vec<u8>,
}

impl RecordedRequest {
    pub fn describe(&self) -> String {
        format!(
            "{} {}{} ua={:?} body={}B",
            self.method,
            self.path,
            if self.query.is_empty() {
                String::new()
            } else {
                format!("?{}", self.query)
            },
            self.user_agent,
            self.body.len()
        )
    }
}

/// Compare the requests each side made, in order.
pub fn compare_requests(a: &[RecordedRequest], b: &[RecordedRequest]) -> Result<()> {
    if a == b {
        println!("PASS: http: {} request(s) identical", a.len());
        return Ok(());
    }
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index);
        let right = b.get(index);
        if left != right {
            bail!(
                "http request {index} differs:\n  django: {}\n  rust:   {}",
                left.map_or_else(|| "<none>".to_string(), RecordedRequest::describe),
                right.map_or_else(|| "<none>".to_string(), RecordedRequest::describe),
            );
        }
    }
    bail!("http: request sequences differ")
}

/// The local fake server both sides talk to.
pub struct Recorder {
    url: String,
    log: Arc<Mutex<Vec<RecordedRequest>>>,
    handle: JoinHandle<()>,
}

impl Recorder {
    pub async fn start(case: HttpCase) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("binding the http recorder")?;
        let address = listener.local_addr().context("recorder address")?;
        let log = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&log);
        let responses = Arc::new(case.responses);
        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let log = Arc::clone(&shared);
                let responses = Arc::clone(&responses);
                tokio::spawn(async move {
                    let _ = serve(stream, log, responses).await;
                });
            }
        });
        Ok(Self {
            url: format!("http://{address}"),
            log,
            handle,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn request_count(&self) -> usize {
        self.log.lock().expect("recorder log").len()
    }

    pub fn slice(&self, from: usize) -> Vec<RecordedRequest> {
        self.log
            .lock()
            .expect("recorder log")
            .get(from..)
            .map(<[RecordedRequest]>::to_vec)
            .unwrap_or_default()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn serve(
    mut stream: tokio::net::TcpStream,
    log: Arc<Mutex<Vec<RecordedRequest>>>,
    responses: Arc<BTreeMap<String, FakeResponse>>,
) -> Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_bytes(&buffer, b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > 64 * 1024 {
            bail!("recorder request headers too large");
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let mut user_agent = None;
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "user-agent" {
                user_agent = Some(value.to_string());
            } else if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
        }
    }
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = buffer
        .get(body_start..body_start + content_length)
        .unwrap_or_default()
        .to_vec();
    let (path, query) = target
        .split_once('?')
        .map_or((target.clone(), String::new()), |(path, query)| {
            (path.to_string(), query.to_string())
        });
    log.lock().expect("recorder log").push(RecordedRequest {
        method,
        path: path.clone(),
        query,
        user_agent,
        body,
    });

    let response = responses.get(&path).cloned().unwrap_or(FakeResponse {
        status: 404,
        headers: BTreeMap::new(),
        body: String::new(),
        body_base64: None,
    });
    let body = response.body_bytes()?;
    let reason = match response.status {
        200 => "OK",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        307 => "Temporary Redirect",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let mut head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.flush().await?;
    Ok(())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn base64_decode(value: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .context("decoding a base64 fixture body")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizers_apply_in_order() {
        let case = TaskCase {
            task: None,
            sql: Vec::new(),
            s3: None,
            http: None,
            normalize: vec![
                Normalize {
                    pattern: "[0-9a-f]{8}".to_string(),
                    replacement: "<X>".to_string(),
                },
                Normalize {
                    pattern: "<X><X>".to_string(),
                    replacement: "<Y>".to_string(),
                },
            ],
        };
        assert_eq!(case.normalize_text("deadbeefdeadbeef"), "<Y>");
    }

    #[test]
    fn a_case_rejects_unknown_keys() {
        let error = serde_json::from_str::<TaskCase>(r#"{"sq": []}"#).unwrap_err();
        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[test]
    fn seed_bodies_are_text_or_base64_never_both() {
        let text = SeedSpec {
            key: "a".to_string(),
            text: Some("hi".to_string()),
            base64: None,
            content_type: None,
            acl: None,
        };
        assert_eq!(text.body().unwrap(), b"hi");
        let both = SeedSpec {
            key: "a".to_string(),
            text: Some("hi".to_string()),
            base64: Some("aGk=".to_string()),
            content_type: None,
            acl: None,
        };
        assert!(both.body().is_err());
    }
}
