// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Redis: logical-database setup and per-step snapshots.
//!
//! The harness speaks a small subset of RESP itself (the server only ever
//! needs `SELECT`/`FLUSHDB` plus the read commands below), so it can keep
//! binary-safe bulk values and stay dependency-free.
//!
//! A snapshot is `SCAN` + `TYPE` + `PTTL` for every key, the value for string
//! keys up to 4 KiB and the elements for list keys up to 100, and the length
//! for the collection types. That is what the differential comparison needs:
//! throttle histories are small lists, response caches are small strings, and
//! a key that grows past the caps is still compared by type and size.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::pickle::{RedisValue, decode_string};

/// The largest string value that is compared by content.
pub const MAX_STRING_VALUE: usize = 4096;
/// The largest number of list elements that are compared by content.
pub const MAX_LIST_ELEMENTS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespValue {
    Simple(String),
    Integer(i64),
    /// `None` for a nil bulk (`$-1`).
    Bulk(Option<Vec<u8>>),
    Array(Vec<RespValue>),
}

impl RespValue {
    pub fn as_status(&self) -> Option<&str> {
        match self {
            RespValue::Simple(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            RespValue::Bulk(Some(bytes)) => std::str::from_utf8(bytes).ok(),
            RespValue::Simple(text) => Some(text.as_str()),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            RespValue::Integer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[RespValue]> {
        match self {
            RespValue::Array(items) => Some(items),
            _ => None,
        }
    }
}

pub struct RedisAdmin {
    host: String,
    port: u16,
    password: Option<String>,
}

impl RedisAdmin {
    pub fn from_url(url: &str) -> Result<Self> {
        let rest = url
            .split_once("://")
            .map(|(_, rest)| rest)
            .ok_or_else(|| anyhow!("Redis URL {url:?} has no scheme"))?;
        let rest = rest.split('/').next().unwrap_or(rest);
        let (auth, host_port) = match rest.rsplit_once('@') {
            Some((auth, host_port)) => (Some(auth), host_port),
            None => (None, rest),
        };
        let (host, port) = match host_port.rsplit_once(':') {
            Some((host, port)) => (
                host.to_string(),
                port.parse::<u16>()
                    .with_context(|| format!("parsing Redis port {port:?}"))?,
            ),
            None => (host_port.to_string(), 6379),
        };
        if host.is_empty() {
            bail!("Redis URL {url:?} has no host");
        }
        let password = auth.and_then(|auth| {
            auth.split_once(':')
                .map(|(_, password)| password.to_string())
        });
        Ok(Self {
            host,
            port,
            password,
        })
    }

    async fn connect(&self, index: i64) -> Result<RedisConnection> {
        if index < 0 {
            bail!("Redis database index must be >= 0, got {index}");
        }
        let stream = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .with_context(|| format!("connecting to Redis at {}:{}", self.host, self.port))?;
        let mut connection = RedisConnection { stream };
        if let Some(password) = &self.password {
            let reply = connection.command(&[password.as_bytes(), b"AUTH"]).await?;
            if reply.as_status() != Some("OK") {
                bail!("Redis AUTH returned {reply:?}");
            }
        }
        let reply = connection
            .command(&[b"SELECT", index.to_string().as_bytes()])
            .await?;
        if reply.as_status() != Some("OK") {
            bail!("Redis SELECT {index} returned {reply:?}");
        }
        Ok(connection)
    }

    pub async fn ping(&self) -> Result<()> {
        let mut connection = self.connect(0).await?;
        let reply = connection.command(&[b"PING"]).await?;
        if reply.as_status() != Some("PONG") {
            bail!("Redis PING returned {reply:?}");
        }
        Ok(())
    }

    /// `SELECT index; FLUSHDB` on a fresh connection.
    pub async fn flush_db(&self, index: i64) -> Result<()> {
        let mut connection = self.connect(index).await?;
        let flush = connection.command(&[b"FLUSHDB"]).await?;
        if flush.as_status() != Some("OK") {
            bail!("Redis FLUSHDB on db {index} returned {flush:?}");
        }
        Ok(())
    }

    /// Open a dedicated connection used for repeated snapshots of one
    /// logical database.
    pub async fn snapshotter(&self, index: i64) -> Result<RedisSnapshotter> {
        Ok(RedisSnapshotter {
            index,
            connection: self.connect(index).await?,
        })
    }
}

pub struct RedisConnection {
    stream: TcpStream,
}

impl RedisConnection {
    pub async fn command(&mut self, args: &[&[u8]]) -> Result<RespValue> {
        let mut request = format!("*{}\r\n", args.len()).into_bytes();
        for arg in args {
            request.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
            request.extend_from_slice(arg);
            request.extend_from_slice(b"\r\n");
        }
        self.stream
            .write_all(&request)
            .await
            .context("writing a Redis command")?;
        read_reply(&mut self.stream).await
    }
}

async fn read_reply(stream: &mut TcpStream) -> Result<RespValue> {
    let mut buffer = Vec::with_capacity(64);
    loop {
        let mut chunk = [0_u8; 1024];
        let read = stream
            .read(&mut chunk)
            .await
            .context("reading a Redis reply")?;
        if read == 0 {
            bail!("Redis closed the connection before a full reply");
        }
        buffer.extend_from_slice(&chunk[..read]);
        let mut cursor = 0;
        if let Ok(Some(value)) = parse_resp(&buffer, &mut cursor) {
            return Ok(value);
        }
    }
}

/// Parse one RESP value from `input` starting at `cursor`.
///
/// Returns `Ok(None)` when the buffer ends before the value is complete.
pub fn parse_resp(input: &[u8], cursor: &mut usize) -> Result<Option<RespValue>> {
    let Some(&marker) = input.get(*cursor) else {
        return Ok(None);
    };
    let line_end = match find_crlf(input, *cursor) {
        Some(end) => end,
        None => return Ok(None),
    };
    let line = &input[*cursor + 1..line_end];
    match marker {
        b'+' => {
            let text = std::str::from_utf8(line).context("RESP status is not UTF-8")?;
            *cursor = line_end + 2;
            Ok(Some(RespValue::Simple(text.to_string())))
        }
        b'-' => {
            let text = String::from_utf8_lossy(line);
            *cursor = line_end + 2;
            bail!("Redis error: {text}");
        }
        b':' => {
            let text = std::str::from_utf8(line).context("RESP integer is not UTF-8")?;
            *cursor = line_end + 2;
            Ok(Some(RespValue::Integer(text.parse().with_context(
                || format!("parsing RESP integer {text:?}"),
            )?)))
        }
        b'$' => {
            let text = std::str::from_utf8(line).context("RESP bulk length is not UTF-8")?;
            let length: i64 = text
                .parse()
                .with_context(|| format!("parsing RESP bulk length {text:?}"))?;
            if length < 0 {
                *cursor = line_end + 2;
                return Ok(Some(RespValue::Bulk(None)));
            }
            let start = line_end + 2;
            let end = start + length as usize;
            if input.len() < end + 2 {
                return Ok(None);
            }
            *cursor = end + 2;
            Ok(Some(RespValue::Bulk(Some(input[start..end].to_vec()))))
        }
        b'*' => {
            let text = std::str::from_utf8(line).context("RESP array length is not UTF-8")?;
            let count: i64 = text
                .parse()
                .with_context(|| format!("parsing RESP array length {text:?}"))?;
            *cursor = line_end + 2;
            if count < 0 {
                return Ok(Some(RespValue::Array(Vec::new())));
            }
            let mut items = Vec::with_capacity(count as usize);
            for _ in 0..count {
                match parse_resp(input, cursor)? {
                    Some(value) => items.push(value),
                    None => return Ok(None),
                }
            }
            Ok(Some(RespValue::Array(items)))
        }
        other => bail!("unknown RESP marker {:?}", other as char),
    }
}

fn find_crlf(input: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while index + 1 < input.len() {
        if input[index] == b'\r' && input[index + 1] == b'\n' {
            return Some(index);
        }
        index += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

/// One key's state after a step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RedisEntry {
    /// `string`, `list`, `hash`, `set`, `zset` or `stream`.
    pub kind: String,
    /// Byte length for strings, element count for the collection types.
    pub size: usize,
    /// `PTTL` in milliseconds; `-1` means the key has no expiry.
    pub ttl_ms: i64,
    /// Decoded content: string values, or list elements. `None` when the value
    /// was larger than the snapshot caps or the type is compared by size only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Default)]
pub struct RedisSnapshot {
    pub keys: BTreeMap<Vec<u8>, RedisEntry>,
}

impl RedisSnapshot {
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RedisDiff {
    /// Keys that exist only after the step, with their after-state.
    pub added: BTreeMap<Vec<u8>, RedisEntry>,
    /// Keys that existed before the step and are gone.
    pub removed: BTreeSet<Vec<u8>>,
    /// Keys whose kind, size, TTL or value changed, with their after-state.
    pub changed: BTreeMap<Vec<u8>, RedisEntry>,
}

impl RedisDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    pub fn total_changes(&self) -> usize {
        self.added.len() + self.removed.len() + self.changed.len()
    }

    /// A report-friendly rendering. Keys are already normalised when this is
    /// called by the runner.
    pub fn to_json(&self) -> Value {
        let stringify = |keys: BTreeMap<Vec<u8>, RedisEntry>| {
            keys.into_iter()
                .map(|(key, entry)| {
                    (
                        String::from_utf8_lossy(&key).into_owned(),
                        serde_json::to_value(entry).unwrap_or(Value::Null),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
        };
        let added = Value::Object(stringify(self.added.clone()));
        let changed = Value::Object(stringify(self.changed.clone()));
        let removed = Value::Array(
            self.removed
                .iter()
                .map(|key| Value::String(String::from_utf8_lossy(key).into_owned()))
                .collect(),
        );
        let mut map = serde_json::Map::new();
        if added.as_object().is_some_and(|object| !object.is_empty()) {
            map.insert("added".to_string(), added);
        }
        if !removed.as_array().is_some_and(Vec::is_empty) {
            map.insert("removed".to_string(), removed);
        }
        if changed.as_object().is_some_and(|object| !object.is_empty()) {
            map.insert("changed".to_string(), changed);
        }
        Value::Object(map)
    }
}

pub struct RedisSnapshotter {
    pub index: i64,
    connection: RedisConnection,
}

#[derive(Debug, Clone)]
pub struct SnapshotResult {
    pub snapshot: RedisSnapshot,
    pub duration: Duration,
}

impl RedisSnapshotter {
    pub async fn snapshot(&mut self) -> Result<SnapshotResult> {
        let started = Instant::now();
        let snapshot = self.read_snapshot().await?;
        Ok(SnapshotResult {
            snapshot,
            duration: started.elapsed(),
        })
    }

    async fn read_snapshot(&mut self) -> Result<RedisSnapshot> {
        let mut keys: Vec<Vec<u8>> = Vec::new();
        let mut cursor: Vec<u8> = b"0".to_vec();
        loop {
            let args: [&[u8]; 4] = [b"SCAN", cursor.as_slice(), b"COUNT", b"500"];
            let reply = self.connection.command(&args).await?;
            let items = reply
                .as_array()
                .ok_or_else(|| anyhow!("redis SCAN returned {reply:?}"))?;
            if items.len() != 2 {
                bail!("redis SCAN returned {} elements", items.len());
            }
            let next = items[0]
                .as_text()
                .ok_or_else(|| anyhow!("redis SCAN cursor is not text"))?
                .to_string();
            let batch = items[1]
                .as_array()
                .ok_or_else(|| anyhow!("redis SCAN key list is not an array"))?;
            for key in batch {
                if let RespValue::Bulk(Some(key)) = key {
                    keys.push(key.clone());
                }
            }
            cursor = next.into_bytes();
            if cursor == b"0" {
                break;
            }
            if keys.len() > 100_000 {
                bail!("redis SCAN returned more than 100000 keys; is the database shared?");
            }
        }
        // SCAN order is not defined; reading in key order makes the per-key
        // error messages stable. Comparison sorts by the normalised key.
        keys.sort();
        let mut snapshot = RedisSnapshot::default();
        for key in keys {
            if let Some(entry) = self.sample(&key).await? {
                snapshot.keys.insert(key, entry);
            }
        }
        Ok(snapshot)
    }

    async fn integer(&mut self, args: &[&[u8]]) -> Result<i64> {
        let reply = self.connection.command(args).await?;
        reply.as_integer().ok_or_else(|| {
            anyhow!(
                "redis {:?} returned {reply:?}",
                String::from_utf8_lossy(args[0])
            )
        })
    }

    /// `None` when the key expired between `SCAN` and `TYPE`.
    async fn sample(&mut self, key: &[u8]) -> Result<Option<RedisEntry>> {
        let kind = {
            let reply = self.connection.command(&[b"TYPE", key]).await?;
            reply
                .as_text()
                .ok_or_else(|| anyhow!("redis TYPE returned {reply:?}"))?
                .to_string()
        };
        if kind == "none" {
            return Ok(None);
        }
        let ttl_ms = self.integer(&[b"PTTL", key]).await?;
        let mut value = None;
        let size = match kind.as_str() {
            "string" => {
                let length = self.integer(&[b"STRLEN", key]).await?;
                if length >= 0 && (length as usize) <= MAX_STRING_VALUE {
                    let reply = self.connection.command(&[b"GET", key]).await?;
                    if let RespValue::Bulk(Some(bytes)) = reply {
                        value = Some(redis_value_to_json(decode_string(&bytes)));
                    }
                }
                length.max(0) as usize
            }
            "list" => {
                let length = self.integer(&[b"LLEN", key]).await?;
                if length >= 0 && (length as usize) <= MAX_LIST_ELEMENTS {
                    let reply = self
                        .connection
                        .command(&[b"LRANGE", key, b"0", b"-1"])
                        .await?;
                    if let Some(items) = reply.as_array() {
                        value = Some(Value::Array(
                            items
                                .iter()
                                .map(|item| match item {
                                    RespValue::Bulk(Some(bytes)) => {
                                        redis_value_to_json(decode_string(bytes))
                                    }
                                    other => Value::String(format!("<resp:{other:?}>")),
                                })
                                .collect(),
                        ));
                    }
                }
                length.max(0) as usize
            }
            "hash" => self.integer(&[b"HLEN", key]).await?.max(0) as usize,
            "set" => self.integer(&[b"SCARD", key]).await?.max(0) as usize,
            "zset" => self.integer(&[b"ZCARD", key]).await?.max(0) as usize,
            "stream" => self.integer(&[b"XLEN", key]).await?.max(0) as usize,
            other => bail!("redis key {key:?} has unsupported type {other}"),
        };
        Ok(Some(RedisEntry {
            kind,
            size,
            ttl_ms,
            value,
        }))
    }
}

fn redis_value_to_json(value: RedisValue) -> Value {
    match value {
        RedisValue::Integer(value) => Value::Number(value.into()),
        RedisValue::Pickled(value) => value,
        RedisValue::Raw(value) => value,
        // A frame serde-pickle cannot model: compare presence and byte count.
        RedisValue::Unreadable => Value::String("<pickle-unreadable>".to_string()),
    }
}

pub fn diff_snapshots(before: &RedisSnapshot, after: &RedisSnapshot) -> RedisDiff {
    let mut diff = RedisDiff::default();
    for (key, entry) in &after.keys {
        match before.keys.get(key) {
            None => {
                diff.added.insert(key.clone(), entry.clone());
            }
            Some(previous) if previous != entry => {
                diff.changed.insert(key.clone(), entry.clone());
            }
            Some(_) => {}
        }
    }
    for key in before.keys.keys() {
        if !after.keys.contains_key(key) {
            diff.removed.insert(key.clone());
        }
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, size: usize) -> RedisEntry {
        RedisEntry {
            kind: kind.to_string(),
            size,
            ttl_ms: 60_000,
            value: None,
        }
    }

    #[test]
    fn parses_status_integer_and_bulk() {
        let mut cursor = 0;
        assert_eq!(
            parse_resp(b"+OK\r\n", &mut cursor).expect("parse"),
            Some(RespValue::Simple("OK".to_string()))
        );
        let mut cursor = 0;
        assert_eq!(
            parse_resp(b":42\r\n", &mut cursor).expect("parse"),
            Some(RespValue::Integer(42))
        );
        let mut cursor = 0;
        assert_eq!(
            parse_resp(b"$5\r\nhello\r\n", &mut cursor).expect("parse"),
            Some(RespValue::Bulk(Some(b"hello".to_vec())))
        );
        let mut cursor = 0;
        assert_eq!(
            parse_resp(b"$-1\r\n", &mut cursor).expect("parse"),
            Some(RespValue::Bulk(None))
        );
    }

    #[test]
    fn bulk_values_are_binary_safe() {
        let mut cursor = 0;
        let parsed = parse_resp(b"$3\r\n\x80\x04\x95\r\n", &mut cursor)
            .expect("parse")
            .expect("complete");
        assert_eq!(parsed, RespValue::Bulk(Some(b"\x80\x04\x95".to_vec())));
    }

    #[test]
    fn parses_nested_arrays() {
        let mut cursor = 0;
        let value = parse_resp(b"*2\r\n$1\r\na\r\n:7\r\n", &mut cursor).expect("parse");
        assert_eq!(
            value,
            Some(RespValue::Array(vec![
                RespValue::Bulk(Some(b"a".to_vec())),
                RespValue::Integer(7)
            ]))
        );
    }

    #[test]
    fn incomplete_input_returns_none() {
        let mut cursor = 0;
        assert_eq!(parse_resp(b"+OK", &mut cursor).expect("parse"), None);
        let mut cursor = 0;
        assert_eq!(parse_resp(b"$5\r\nhel", &mut cursor).expect("parse"), None);
    }

    #[test]
    fn errors_are_reported() {
        let mut cursor = 0;
        let error = parse_resp(b"-ERR bad\r\n", &mut cursor).expect_err("must fail");
        assert!(error.to_string().contains("ERR bad"));
    }

    #[test]
    fn redis_url_parsing() {
        let admin = RedisAdmin::from_url("redis://127.0.0.1:56379").expect("url");
        assert_eq!(admin.host, "127.0.0.1");
        assert_eq!(admin.port, 56379);
        assert!(admin.password.is_none());

        let admin = RedisAdmin::from_url("redis://user:secret@cache:6380/0").expect("url");
        assert_eq!(admin.host, "cache");
        assert_eq!(admin.port, 6380);
        assert_eq!(admin.password.as_deref(), Some("secret"));
    }

    #[test]
    fn diffs_report_added_removed_and_changed() {
        let mut before = RedisSnapshot::default();
        before.keys.insert(b"gone".to_vec(), entry("string", 1));
        before.keys.insert(b"same".to_vec(), entry("string", 1));
        before.keys.insert(b"grew".to_vec(), entry("list", 1));

        let mut after = RedisSnapshot::default();
        after.keys.insert(b"same".to_vec(), entry("string", 1));
        after.keys.insert(b"new".to_vec(), entry("string", 1));
        after.keys.insert(b"grew".to_vec(), entry("list", 2));

        let diff = diff_snapshots(&before, &after);
        assert_eq!(diff.added.len(), 1);
        assert!(diff.added.contains_key(b"new".as_slice()));
        assert_eq!(diff.removed.len(), 1);
        assert!(diff.removed.contains(b"gone".as_slice()));
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[b"grew".as_slice()].size, 2);
        assert_eq!(diff.total_changes(), 3);
    }

    #[test]
    fn redis_values_decode_through_the_pickle_layer() {
        // Guards the layering: redis.rs hands bytes to pickle.rs unchanged.
        assert_eq!(
            decode_string(b"7"),
            RedisValue::Integer(7),
            "integers are not pickled"
        );
    }
}
