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
//!   `Content-Type` and ACL, and the two listings are compared. A ZIP body is
//!   decompressed and compared entry by entry (recursively: the export archive
//!   contains an XLSX), which drops the DOS entry times and the compressor's
//!   own bytes; `docProps/core.xml`'s created/modified are the only timestamps
//!   normalised inside a tree. The buckets
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
use std::io::Read as _;
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
    /// Capture the task's outbound SMTP through `tools/mail/fake_smtp.py`.
    #[serde(default)]
    pub smtp: Option<SmtpCase>,
    /// Execute only the enqueued job, leaving the tasks the handler publishes
    /// queued. Django's `task.apply()` never runs them, so a chain whose child
    /// is Rust-owned needs this to compare like with like (`D25`).
    #[serde(default)]
    pub single_job: bool,
    /// Normalisers for the case's SQL rows and S3 keys, and for the values the
    /// whole-schema database diff prints.
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
pub struct SmtpCase {
    /// `EMAIL_HOST_PASSWORD`; empty means no AUTH, like the dev settings.
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpCase {
    #[serde(default)]
    pub responses: BTreeMap<String, FakeResponse>,
    /// Normalisers for the recorded request bodies only. They exist for a
    /// task that embeds a generated value in its payload (a copied asset's
    /// fresh UUID); the SQL comparison still checks the stored value.
    #[serde(default)]
    pub normalize: Vec<Normalize>,
}

impl HttpCase {
    fn normalize_text(&self, value: &str) -> String {
        let mut out = value.to_string();
        for rule in &self.normalize {
            if let Ok(regex) = regex::Regex::new(&rule.pattern) {
                out = regex
                    .replace_all(&out, rule.replacement.as_str())
                    .into_owned();
            }
        }
        out
    }
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

/// The run's S3 bucket names, one per side.
///
/// A case cannot name them: `--s3-bucket-a/-b` are the caller's, and every
/// value that embeds a bucket (a presigned export URL, a copied object key)
/// differs by name alone. The comparison replaces each side's own bucket with
/// `<BUCKET>` before applying the case's normalisers, so a fixture no longer
/// has to hard-code an agent's bucket prefix (D26).
#[derive(Clone, Copy, Debug, Default)]
pub struct SideBuckets<'a> {
    pub a: Option<&'a str>,
    pub b: Option<&'a str>,
}

impl<'a> SideBuckets<'a> {
    /// The replacement applied to a bucket name.
    pub const PLACEHOLDER: &'static str = "<BUCKET>";

    /// Normalise one value that belongs to side A.
    #[must_use]
    pub fn a(&self, case: &TaskCase, value: &str) -> String {
        normalise_side(case, value, self.a)
    }

    /// Normalise one value that belongs to side B.
    #[must_use]
    pub fn b(&self, case: &TaskCase, value: &str) -> String {
        normalise_side(case, value, self.b)
    }
}

fn normalise_side(case: &TaskCase, value: &str, bucket: Option<&str>) -> String {
    let replaced = match bucket.filter(|bucket| !bucket.is_empty()) {
        Some(bucket) if value.contains(bucket) => value.replace(bucket, SideBuckets::PLACEHOLDER),
        _ => value.to_string(),
    };
    case.normalize_text(&replaced)
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
    buckets: SideBuckets<'_>,
    a: &[Vec<String>],
    b: &[Vec<String>],
) -> Result<()> {
    let normalize =
        |rows: &[Vec<String>], normalise: &dyn Fn(&str) -> String| -> Vec<Vec<String>> {
            rows.iter()
                .map(|row| row.iter().map(|cell| normalise(cell)).collect())
                .collect()
        };
    let a = normalize(a, &|cell| buckets.a(case, cell));
    let b = normalize(b, &|cell| buckets.b(case, cell));
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
pub fn compare_objects(
    case: &TaskCase,
    buckets: SideBuckets<'_>,
    a: &[ObservedObject],
    b: &[ObservedObject],
) -> Result<()> {
    let normalize =
        |objects: &[ObservedObject], key: &dyn Fn(&str) -> String| -> Vec<ObservedObject> {
            objects
                .iter()
                .map(|object| ObservedObject {
                    key: key(&object.key),
                    ..object.clone()
                })
                .collect()
        };
    let mut a = normalize(a, &|key| buckets.a(case, key));
    let mut b = normalize(b, &|key| buckets.b(case, key));
    // A one-shot key (a task that copies an object to a fresh uuid) is
    // normalised to a placeholder whose sort position depends on the raw uuid;
    // compare the normalised listings in a stable order.
    a.sort_by(|left, right| left.key.cmp(&right.key));
    b.sort_by(|left, right| left.key.cmp(&right.key));
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
        // A ZIP's compressed bytes carry the writer's zlib version and the DOS
        // entry times, neither of which is an effect. Compare its entries
        // instead, recursively: the export archive contains an XLSX, itself a
        // ZIP. Only `docProps/core.xml`'s created/modified timestamps are
        // normalised inside the entry tree.
        if let (Some(left_zip), Some(right_zip)) =
            (expand_zip(&left.body, 0), expand_zip(&right.body, 0))
        {
            compare_zip_entries(case, &left.key, &left_zip, &right_zip)?;
            continue;
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
    println!(
        "PASS: s3: {} object(s) identical (zip archives compared entry by entry)",
        a.len()
    );
    Ok(())
}

/// One member of a ZIP, with its nested archive expanded when it is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntry {
    pub name: String,
    pub body: Vec<u8>,
    pub nested: Option<Vec<ZipEntry>>,
}

/// Decompress a ZIP body, or return `None` when it is not one.
///
/// openpyxl and Python's `zipfile` write standard local headers; the harness's
/// readers use them in order, so a central-directory parser is unnecessary.
#[must_use]
pub fn expand_zip(body: &[u8], depth: usize) -> Option<Vec<ZipEntry>> {
    if depth > 4 || !body.starts_with(b"PK\x03\x04") {
        return None;
    }
    let mut entries = Vec::new();
    let mut index = 0usize;
    while index + 30 <= body.len() {
        let signature = u32::from_le_bytes(body[index..index + 4].try_into().ok()?);
        if signature != 0x0403_4b50 {
            break;
        }
        let method = u16::from_le_bytes(body[index + 8..index + 10].try_into().ok()?);
        let compressed = u32::from_le_bytes(body[index + 18..index + 22].try_into().ok()?) as usize;
        let name_length =
            u16::from_le_bytes(body[index + 26..index + 28].try_into().ok()?) as usize;
        let extra_length =
            u16::from_le_bytes(body[index + 28..index + 30].try_into().ok()?) as usize;
        let name_start = index + 30;
        let name =
            String::from_utf8_lossy(body.get(name_start..name_start + name_length)?).to_string();
        let data_start = name_start + name_length + extra_length;
        let data = body.get(data_start..data_start + compressed)?;
        let content = if method == 8 {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(data)
                .read_to_end(&mut out)
                .ok()?;
            out
        } else {
            data.to_vec()
        };
        let nested = expand_zip(&content, depth + 1);
        entries.push(ZipEntry {
            name,
            body: content,
            nested,
        });
        index = data_start + compressed;
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// The `docProps/core.xml` timestamps are `datetime.now(timezone.utc)`.
fn normalize_core_xml(text: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(text) else {
        return text.to_vec();
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("xsi:type=\"dcterms:W3CDTF\">") {
        let (before, after) = rest.split_at(start);
        out.push_str(before);
        out.push_str("xsi:type=\"dcterms:W3CDTF\"><TS>");
        match after.find('<') {
            Some(end) => rest = &after[end..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.into_bytes()
}

fn compare_zip_entries(
    case: &TaskCase,
    object_key: &str,
    a: &[ZipEntry],
    b: &[ZipEntry],
) -> Result<()> {
    let names_a: Vec<&str> = a.iter().map(|entry| entry.name.as_str()).collect();
    let names_b: Vec<&str> = b.iter().map(|entry| entry.name.as_str()).collect();
    if names_a != names_b {
        bail!("s3 {object_key}: zip entries differ:\n  django: {names_a:?}\n  rust:   {names_b:?}");
    }
    for (left, right) in a.iter().zip(b) {
        let path = format!("{object_key}:{}", case.normalize_text(&left.name));
        match (&left.nested, &right.nested) {
            (Some(left_zip), Some(right_zip)) => {
                compare_zip_entries(case, &path, left_zip, right_zip)?;
            }
            (None, None) => {
                let left_body = if left.name == "docProps/core.xml" {
                    normalize_core_xml(&left.body)
                } else {
                    left.body.clone()
                };
                let right_body = if right.name == "docProps/core.xml" {
                    normalize_core_xml(&right.body)
                } else {
                    right.body.clone()
                };
                if left_body != right_body {
                    if let Ok(dir) = std::env::var("PLANE_TESTKIT_DUMP_DIR") {
                        let safe: String = path
                            .chars()
                            .map(|c| {
                                if c.is_ascii_alphanumeric() || c == '.' {
                                    c
                                } else {
                                    '_'
                                }
                            })
                            .collect();
                        let _ = std::fs::write(format!("{dir}/{safe}.django"), &left_body);
                        let _ = std::fs::write(format!("{dir}/{safe}.rust"), &right_body);
                    }
                    bail!(
                        "s3 {path}: entry differs (django {} bytes {:02x?}..., rust {} bytes {:02x?}...)",
                        left_body.len(),
                        &left_body[..left_body.len().min(24)],
                        right_body.len(),
                        &right_body[..right_body.len().min(24)],
                    );
                }
            }
            _ => bail!("s3 {path}: one side's entry is a nested archive, the other is not"),
        }
    }
    println!("PASS: s3 {object_key}: {} zip entr(ies) identical", a.len());
    Ok(())
}

/// Compare the two `fake_smtp.py` transcripts: envelope and message bytes,
/// with `Date:`, `Message-ID:` and the random MIME boundary masked. The
/// transcript itself is not compared here because the harness proves the SMTP
/// dialogue separately (`tools/mail/compare_smtp_logs.py`).
pub fn compare_smtp_logs(a: &std::path::Path, b: &std::path::Path) -> Result<()> {
    let read = |path: &std::path::Path| -> Result<Vec<serde_json::Value>> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).with_context(|| "parsing a fake_smtp record"))
            .collect()
    };
    let a = read(a)?;
    let b = read(b)?;
    if a.len() != b.len() {
        bail!(
            "smtp: django sent {} message(s), rust sent {}",
            a.len(),
            b.len()
        );
    }
    for (index, (left, right)) in a.iter().zip(&b).enumerate() {
        let envelope = |record: &serde_json::Value| -> serde_json::Value {
            serde_json::json!({
                "mail_from": record.get("mail_from"),
                "rcpt_to": record.get("rcpt_to"),
            })
        };
        if envelope(left) != envelope(right) {
            bail!(
                "smtp message {index}: envelope differs:\n  django: {}\n  rust:   {}",
                envelope(left),
                envelope(right)
            );
        }
        let data = |record: &serde_json::Value| -> String {
            normalize_smtp_data(
                record
                    .get("data")
                    .and_then(|data| data.as_str())
                    .unwrap_or_default(),
            )
        };
        let left_data = data(left);
        let right_data = data(right);
        if left_data != right_data {
            let line = left_data
                .lines()
                .zip(right_data.lines())
                .enumerate()
                .find(|(_, (left, right))| left != right);
            match line {
                Some((number, (left, right))) => bail!(
                    "smtp message {index}: bytes differ at line {}:\n  django: {left:?}\n  rust:   {right:?}",
                    number + 1
                ),
                None => bail!(
                    "smtp message {index}: bytes differ (django {} bytes, rust {} bytes)",
                    left_data.len(),
                    right_data.len()
                ),
            }
        }
    }
    println!("PASS: smtp: {} message(s) identical", a.len());
    Ok(())
}

fn normalize_smtp_data(data: &str) -> String {
    let mut out = data.to_string();
    if let Some(captures) = regex::Regex::new(r#"boundary="([^"]+)""#)
        .expect("boundary regex")
        .captures(&out)
        && let Some(boundary) = captures.get(1)
    {
        out = out.replace(boundary.as_str(), "<BOUNDARY>");
    }
    let unfolder = r"(?:\r?\n[ \t][^\r\n]*)*";
    for (header, placeholder) in [("Date", "<DATE>"), ("Message-ID", "<MESSAGE-ID>")] {
        let pattern = format!(r"(?m)^{header}:[^\r\n]*{unfolder}");
        out = regex::Regex::new(&pattern)
            .expect("header regex")
            .replace_all(&out, format!("{header}: {placeholder}"))
            .into_owned();
    }
    out
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
pub fn compare_requests(
    case: &TaskCase,
    buckets: SideBuckets<'_>,
    a: &[RecordedRequest],
    b: &[RecordedRequest],
) -> Result<()> {
    // The case's normalisers also apply to a request body: a task that copies
    // an object embeds the fresh asset id in its `convert-document` payload,
    // and that id is legitimately different on each side.
    let http = case.http.as_ref();
    let normalize = |request: &RecordedRequest, side: &dyn Fn(&str) -> String| RecordedRequest {
        method: request.method.clone(),
        path: request.path.clone(),
        query: request.query.clone(),
        user_agent: request.user_agent.clone(),
        body: match http {
            Some(http) => http
                .normalize_text(&String::from_utf8_lossy(&request.body))
                .into_bytes(),
            None => side(&String::from_utf8_lossy(&request.body)).into_bytes(),
        },
    };
    let a: Vec<RecordedRequest> = a
        .iter()
        .map(|r| normalize(r, &|v| buckets.a(case, v)))
        .collect();
    let b: Vec<RecordedRequest> = b
        .iter()
        .map(|r| normalize(r, &|v| buckets.b(case, v)))
        .collect();
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
            smtp: None,
            single_job: false,
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
    fn side_buckets_are_normalised_before_the_case_rules() {
        let case = TaskCase {
            task: None,
            sql: Vec::new(),
            s3: None,
            http: None,
            smtp: None,
            single_job: false,
            normalize: vec![Normalize {
                pattern: "token=[a-z]+".to_string(),
                replacement: "token=<TOK>".to_string(),
            }],
        };
        let buckets = SideBuckets {
            a: Some("t_p8b_alpha_a"),
            b: Some("t_p8b_alpha_b"),
        };
        let url = "https://s3/t_p8b_alpha_a/store/key?token=abc";
        assert_eq!(
            buckets.a(&case, url),
            "https://s3/<BUCKET>/store/key?token=<TOK>"
        );
        // A's name on B's side is not this side's bucket and stays literal.
        assert_eq!(
            buckets.b(&case, url),
            "https://s3/t_p8b_alpha_a/store/key?token=<TOK>"
        );
        // An empty bucket name never matches every string.
        let empty = SideBuckets {
            a: Some(""),
            b: None,
        };
        assert_eq!(empty.a(&case, "plain"), "plain");
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
