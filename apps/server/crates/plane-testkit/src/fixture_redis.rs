// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A scenario's `fixture_redis`: deterministic django-redis entries seeded
//! into both targets' own Redis databases before the baseline snapshot.
//!
//! A differential step can only compare a value both targets generated the
//! same way. Some values are random on one side and only reachable through a
//! side effect the harness does not capture — the e-mail-change code is the
//! first: `UserEndpoint.generate_email_verification_code` stores
//! `json.dumps({"token": str(secrets.randbelow(900000) + 100000)})` under
//! `magic_email_update_<user id>_<email>` (`plane/app/views/user/base.py:170-175`)
//! and passes the code to a Celery task. `capture` has no Redis source
//! (`config.rs`'s `CaptureSource`), so the scenario seeds the entry instead:
//! both targets start with the same code and the update step is deterministic.
//!
//! ## File format
//!
//! ```json
//! {
//!   "entries": [
//!     {
//!       "key": "magic_email_update_<uuid>_new@example.com",
//!       "value": "{\"token\": \"123456\"}",
//!       "timeout": 600
//!     },
//!     { "key": "email_update_issued:new@example.com", "value": 0, "timeout": 3600 }
//!   ]
//! }
//! ```
//!
//! * `key` is the **logical** cache key: the harness prepends django-redis's
//!   `:1:` (`KEY_PREFIX` is empty, `VERSION` is 1;
//!   `plane/settings/common.py:274-291`, `django_redis/client/default.py:876-891`).
//! * `value` follows `DefaultClient.encode`: a JSON integer is stored as its
//!   ASCII digits, everything else as a protocol-4 pickle (the same bytes
//!   `plane-cache/src/pickle.rs` writes). A JSON string therefore means a
//!   Python `str` — the code entry's value is a JSON *string containing JSON*,
//!   exactly what the view stores.
//! * `timeout` is seconds (`SET … PX timeout*1000`); absent or `null` means no
//!   expiry. The harness compares TTLs within 2 s, so a fixture TTL only has
//!   to be the same value on both sides.
//!
//! ## `encoding: "raw"`
//!
//! A few keys do **not** live in the django-redis keyspace. The magic-link
//! provider uses `redis_instance()` (`plane/settings/redis.py:8-16`), a raw
//! `redis-py` client: the key has no `:1:` prefix and the value is written with
//! a plain `SET` — the JSON record `{"current_attempt": 0, "email": …,
//! "token": …}` and the `:issued`/`:verify_attempts` counters (ASCII digits).
//! An entry with `"encoding": "raw"` is written exactly as its JSON value says:
//!
//! * the key is **not** prefixed;
//! * a string is written as its UTF-8 bytes (no pickle), an integer as its
//!   decimal text; every other JSON type is refused.
//!
//! ```json
//! { "key": "magic_a@example.com",
//!   "value": "{\"current_attempt\": 0, \"email\": \"a@example.com\", \"token\": \"123456\"}",
//!   "timeout": 600, "encoding": "raw" }
//! ```
//!
//! The encoder is deliberately small: it handles the shapes a fixture needs
//! (int, str, bool, null) and refuses anything else, so a fixture cannot
//! silently write a value `cache.get` would decode differently.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

/// `django_redis`'s full-key prefix for Plane's `CACHES` (`KEY_PREFIX=""`,
/// `VERSION=1`).
pub const KEY_PREFIX: &str = ":1:";

/// One seeded entry, value already encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The full Redis key, prefix included.
    pub key: String,
    pub value: Vec<u8>,
    /// `PX` in milliseconds; `None` leaves the key without an expiry.
    pub ttl_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureFile {
    entries: Vec<FixtureEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureEntry {
    key: String,
    value: Value,
    #[serde(default)]
    timeout: Option<i64>,
    #[serde(default)]
    encoding: Encoding,
}

/// How an entry's value is written.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Encoding {
    /// A django-redis entry: `:1:`-prefixed key, `DefaultClient.encode` value.
    #[default]
    Cache,
    /// A `redis_instance()` entry: the literal key, the literal value.
    Raw,
}

/// Read and encode a fixture file.
pub fn load(path: &Path) -> Result<Vec<Entry>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file: FixtureFile =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let mut entries = Vec::with_capacity(file.entries.len());
    for entry in file.entries {
        if entry.key.is_empty() {
            bail!("{}: an entry has an empty key", path.display());
        }
        let (key, value) = match entry.encoding {
            Encoding::Cache => (
                format!("{KEY_PREFIX}{}", entry.key),
                encode(&entry.value)
                    .with_context(|| format!("{}: entry {:?}", path.display(), entry.key))?,
            ),
            Encoding::Raw => (
                entry.key.clone(),
                encode_raw(&entry.value)
                    .with_context(|| format!("{}: raw entry {:?}", path.display(), entry.key))?,
            ),
        };
        let ttl_ms = match entry.timeout {
            None => None,
            Some(seconds) if seconds > 0 => Some(seconds.saturating_mul(1000)),
            Some(seconds) => bail!(
                "{}: entry {:?} has timeout {seconds}; django-redis deletes a key \
                 for a non-positive timeout, which a fixture cannot express",
                path.display(),
                entry.key
            ),
        };
        entries.push(Entry { key, value, ttl_ms });
    }
    Ok(entries)
}

/// A raw entry's bytes: a Python `str` as UTF-8, an `int` as its decimal text.
///
/// `redis-py` sends whatever the view passes to `SET`. The two shapes the magic
/// provider writes are `json.dumps(...)` text and `INCR`'s counter, which is
/// stored as decimal ASCII. A bool or null would be Python's `str(bool)`
/// (`"True"`), which no call site writes; refusing it keeps the fixture honest.
pub fn encode_raw(value: &Value) -> Result<Vec<u8>> {
    match value {
        Value::String(text) => Ok(text.clone().into_bytes()),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => Ok(integer.to_string().into_bytes()),
            None => bail!("a raw value must be a string or an integer"),
        },
        Value::Bool(_) | Value::Null | Value::Array(_) | Value::Object(_) => {
            bail!("a raw value must be a JSON string or integer")
        }
    }
}

/// `django_redis.serializers.pickle.PickleSerializer.dumps(value)`: a
/// protocol-4 pickle, except that `DefaultClient.encode` stores an `int` as
/// its ASCII digits first.
pub fn encode(value: &Value) -> Result<Vec<u8>> {
    match value {
        Value::Number(number) => {
            let Some(integer) = number.as_i64() else {
                bail!("only integer numbers can be seeded (a float is pickled by django-redis)");
            };
            Ok(integer.to_string().into_bytes())
        }
        Value::String(text) => Ok(pickle_str(text)),
        Value::Bool(value) => Ok(if *value {
            vec![0x80, 0x04, 0x88, b'.']
        } else {
            vec![0x80, 0x04, 0x89, b'.']
        }),
        Value::Null => Ok(vec![0x80, 0x04, b'N', b'.']),
        Value::Array(_) | Value::Object(_) => {
            bail!("a fixture value must be a JSON int, string, bool or null")
        }
    }
}

/// `pickle.dumps(text, 4)`: `PROTO 4`, then one frame containing the string
/// opcode, `MEMOIZE` and `STOP` (CPython's `_Framer` writes a `FRAME` header
/// when the buffered opcodes are at least `_FRAME_SIZE_MIN` = 4 bytes, which a
/// string always is).
fn pickle_str(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut body = Vec::with_capacity(bytes.len() + 8);
    if bytes.len() < 256 {
        body.push(0x8c); // SHORT_BINUNICODE
        body.push(bytes.len() as u8);
    } else {
        body.push(b'X'); // BINUNICODE
        body.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    }
    body.extend_from_slice(bytes);
    body.push(0x94); // MEMOIZE
    body.push(b'.'); // STOP

    let mut out = Vec::with_capacity(body.len() + 11);
    out.extend_from_slice(&[0x80, 0x04]); // PROTO 4
    out.push(0x95); // FRAME
    out.extend_from_slice(&(body.len() as u64).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Expected bytes recorded from CPython 3.12.13's
    /// `pickle.dumps(value, 4)` (the venv's `DEFAULT_PROTOCOL`).
    #[test]
    fn pickles_match_cpython_protocol_4() {
        assert_eq!(
            hex(&encode(&json!("{\"token\": \"123456\"}")).expect("string")),
            "80049517000000000000008c137b22746f6b656e223a2022313233343536227d942e"
        );
        assert_eq!(
            hex(&encode(&json!("")).expect("string")),
            "80049504000000000000008c00942e"
        );
        assert_eq!(
            hex(&encode(&json!("x")).expect("string")),
            "80049505000000000000008c0178942e"
        );
        assert_eq!(hex(&encode(&json!(true)).expect("bool")), "8004882e");
        assert_eq!(hex(&encode(&json!(false)).expect("bool")), "8004892e");
        assert_eq!(hex(&encode(&json!(null)).expect("none")), "80044e2e");
    }

    /// A bignum is not an int for `django-redis`'s `isinstance` (a Python
    /// `int` is unbounded, so it *is* stored as digits in Python; fixture
    /// files use the JSON range here).
    #[test]
    fn integers_are_ascii_digits() {
        assert_eq!(encode(&json!(0)).expect("int"), b"0");
        assert_eq!(encode(&json!(42)).expect("int"), b"42");
        assert_eq!(encode(&json!(-7)).expect("int"), b"-7");
        assert!(encode(&json!(1.5)).is_err());
        assert!(encode(&json!([1])).is_err());
    }

    #[test]
    fn keys_are_prefixed_and_ttls_converted() {
        let directory = std::env::temp_dir();
        let path = directory.join(format!("plane-fixture-redis-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"entries":[{"key":"k","value":"v","timeout":600},{"key":"n","value":0}]}"#,
        )
        .expect("write fixture");
        let entries = load(&path).expect("load fixture");
        let _ = std::fs::remove_file(&path);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, ":1:k");
        assert_eq!(entries[0].ttl_ms, Some(600_000));
        assert_eq!(entries[1].key, ":1:n");
        assert_eq!(entries[1].value, b"0");
        assert_eq!(entries[1].ttl_ms, None);
    }

    /// `redis_instance()`'s keyspace: no `:1:` prefix and no pickle, because
    /// `redis-py`'s `SET` writes the value the view passed.
    #[test]
    fn raw_entries_are_neither_prefixed_nor_pickled() {
        let directory = std::env::temp_dir();
        let path = directory.join(format!(
            "plane-fixture-redis-raw-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"{"entries":[
                {"key":"magic_a@example.com","value":"{\"current_attempt\": 0}","timeout":600,"encoding":"raw"},
                {"key":"magic_a@example.com:issued","value":1,"timeout":600,"encoding":"raw"}
            ]}"#,
        )
        .expect("write fixture");
        let entries = load(&path).expect("load fixture");
        let _ = std::fs::remove_file(&path);
        assert_eq!(entries[0].key, "magic_a@example.com");
        assert_eq!(entries[0].value, b"{\"current_attempt\": 0}");
        assert_eq!(entries[0].ttl_ms, Some(600_000));
        assert_eq!(entries[1].key, "magic_a@example.com:issued");
        assert_eq!(entries[1].value, b"1");
        // The cache encoder stays the default.
        assert_eq!(encode(&json!("x")).expect("pickle"), pickle_str("x"));
        assert!(encode_raw(&json!(true)).is_err());
        assert!(encode_raw(&json!([1])).is_err());
    }
}
