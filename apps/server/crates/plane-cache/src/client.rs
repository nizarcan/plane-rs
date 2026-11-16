// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `django_redis.cache.RedisCache` wire format: keys, values and commands.
//!
//! Plane configures the cache with the defaults (`plane/settings/common.py:274-291`):
//! `django_redis.cache.RedisCache`, `CLIENT_CLASS=DefaultClient`, no
//! `KEY_PREFIX`, no `VERSION`, no `SERIALIZER`, no `COMPRESSOR`, no
//! `PICKLE_VERSION`. That pins every detail below.
//!
//! **Keys.** `DefaultClient.make_key` calls
//! `django.core.cache.backends.base.default_key_func` with `key_prefix=""` and
//! `version=1`, giving `":1:" + key` (`base.py:33-42`, `:99-110`;
//! `django_redis/client/default.py:876-891`). `None` is formatted with `%s`,
//! so the absent API-key case reads the literal key `:1:None` — see
//! [`make_key_opt`]. `keys()`/`delete_many()` round-trip through
//! `default_reverse_key`, which strips the first two colon-separated fields
//! (`django_redis/util.py`).
//!
//! **Values.** `DefaultClient.encode` stores integers as their ASCII digits
//! and everything else through `PickleSerializer.dumps`, i.e.
//! `pickle.dumps(value, 4)`; `IdentityCompressor` is the identity
//! (`default.py:583-603`, `serializers/pickle.py:10-40`). A `bool` is *not* an
//! integer here: `isinstance(value, (bool, Enum))` forces it through pickle.
//! `decode` tries `int(value)` first, so a raw digit string reads back as an
//! integer (`default.py:563-578`).
//!
//! **Writes.** `DefaultClient.set` with a timeout does
//! `SET <key> <value> PX <int(timeout * 1000)>` (`default.py:231-283`), so a
//! 60-second throttle window is `PX 60000`. A non-positive timeout makes
//! Django delete the key instead; `timeout=None` writes without an expiry.
//!
//! **Reads.** `DefaultClient.get(key, default)` issues a plain `GET` and
//! returns `default` when the key is missing.
//!
//! **`keys()` issues `KEYS`, not `SCAN`.** `plane/utils/cache.py`'s
//! `invalidate_cache(multiple=True)` calls `cache.keys("*<key>*")`, which is
//! `DefaultClient.keys` → `client.keys(pattern)` (`default.py:851-874`). `KEYS`
//! is O(keyspace) and blocks the server; the port issues the same command
//! because the cost is part of the behaviour being reproduced. Do not "fix"
//! this without changing Django first.

use crate::error::Error;
use crate::pickle;
use crate::pyint::PyInt;
use crate::redis::Redis;
use crate::value::PyValue;

/// `KEY_PREFIX` from Plane's `CACHES` (unset → `""`).
pub const KEY_PREFIX: &str = "";
/// `VERSION` from Plane's `CACHES` (unset → `1`).
pub const VERSION: i64 = 1;
/// `TIMEOUT` from Plane's `CACHES` (unset → Django's `300`).
pub const DEFAULT_TIMEOUT: i64 = 300;

/// `default_key_func(key, "", 1)`: the Redis key django-redis writes.
pub fn make_key(key: &str) -> String {
    // Python's `"%s:%s:%s" % (prefix, version, key)`.
    let mut out = String::with_capacity(KEY_PREFIX.len() + key.len() + 4);
    out.push_str(KEY_PREFIX);
    out.push(':');
    out.push_str(&VERSION.to_string());
    out.push(':');
    out.push_str(key);
    out
}

/// `make_key()` applied to an optional key.
///
/// `make_key(None)` is `":1:None"` because `%s` formats `None` as `"None"`.
/// `ApiKeyRateThrottle` really does `GET :1:None` on every keyless `/api/v1/`
/// request, so both backends must read the same key.
pub fn make_key_opt(key: Option<&str>) -> String {
    match key {
        Some(key) => make_key(key),
        None => format!("{KEY_PREFIX}:{VERSION}:None"),
    }
}

/// `make_pattern`: the same `key_func` applied to a glob pattern.
///
/// `KEY_PREFIX` and `VERSION` are glob-escaped (`DefaultClient.glob_escape`,
/// `default.py:74-76`), which with an empty prefix and version `1` changes
/// nothing.
pub fn make_pattern(pattern: &str) -> String {
    format!("{KEY_PREFIX}:{VERSION}:{pattern}")
}

/// `django_redis.util.default_reverse_key`: `key.split(":", 2)[2]`.
///
/// A key with fewer than three colon-separated fields makes Python raise
/// `IndexError`; this returns the whole key instead of panicking, because the
/// only way to reach it is a Redis keyspace some other tool wrote.
pub fn reverse_key(full_key: &str) -> String {
    let mut parts = full_key.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (_, _, Some(key)) => key.to_string(),
        _ => full_key.to_string(),
    }
}

/// What `DefaultClient.decode` can return.
#[derive(Debug, Clone, PartialEq)]
pub enum CachedValue {
    /// `DefaultClient.encode(allow_int=True)`: an integer stored as text.
    Int(PyInt),
    /// A pickled Python value.
    Pickled(PyValue),
}

impl CachedValue {
    /// The value as a [`PyValue`], for callers that do not care how it was
    /// stored.
    #[must_use]
    pub fn into_value(self) -> PyValue {
        match self {
            CachedValue::Int(value) => PyValue::Int(value),
            CachedValue::Pickled(value) => value,
        }
    }
}

/// `DefaultClient.encode`: an `int` becomes its ASCII digits, everything else
/// a protocol-4 pickle.
#[must_use]
pub fn encode(value: &PyValue) -> Vec<u8> {
    match value {
        PyValue::Int(value) => value.to_decimal_string().into_bytes(),
        other => pickle::dumps(other),
    }
}

/// `DefaultClient.decode`: try `int(value)`, otherwise unpickle.
pub fn decode(value: &[u8]) -> Result<CachedValue, Error> {
    // `int(bytes)` decodes ASCII first; non-ASCII bytes raise
    // `UnicodeDecodeError` (a `ValueError`), so they fall through.
    if let Ok(text) = std::str::from_utf8(value)
        && let Some(integer) = PyInt::parse(text)
    {
        return Ok(CachedValue::Int(integer));
    }
    pickle::loads(value)
        .map(CachedValue::Pickled)
        .map_err(|error| Error::Decode(error.to_string()))
}

/// A `DefaultClient` view over one Redis connection handle.
#[derive(Debug)]
pub struct Client<'a> {
    redis: &'a Redis,
}

impl<'a> Client<'a> {
    pub fn new(redis: &'a Redis) -> Self {
        Self { redis }
    }

    /// `DefaultClient.get(key, None)`: the decoded value, or `None`.
    ///
    /// A stored `None` is indistinguishable from a miss, exactly as in Django,
    /// where `cache.get` returns its `default` for both.
    pub async fn get(&self, key: &str) -> Result<Option<CachedValue>, Error> {
        let Some(bytes) = self.redis.get_bytes(&make_key(key)).await? else {
            return Ok(None);
        };
        decode(&bytes).map(Some)
    }

    /// `cache.set(key, value, timeout)`.
    ///
    /// `timeout = Some(seconds)` writes `SET … PX <seconds*1000>`;
    /// `None` writes without an expiry; a non-positive timeout deletes the key
    /// (`default.py:257-270`).
    pub async fn set(
        &self,
        key: &str,
        value: &PyValue,
        timeout: Option<i64>,
    ) -> Result<bool, Error> {
        let full = make_key(key);
        let bytes = encode(value);
        let Some(seconds) = timeout else {
            self.redis.set_bytes(&full, &bytes).await?;
            return Ok(true);
        };
        let milliseconds = seconds.saturating_mul(1000);
        if milliseconds <= 0 {
            return self.redis.del(&full).await;
        }
        self.redis.set_bytes_px(&full, &bytes, milliseconds).await?;
        Ok(true)
    }

    /// `cache.delete(key)`: `DEL <key>`, returning how many keys were removed.
    pub async fn delete(&self, key: &str) -> Result<usize, Error> {
        Ok(usize::from(self.redis.del(&make_key(key)).await?))
    }

    /// `cache.delete_many(keys)`.
    ///
    /// The keys are un-prefixed and get prefixed here, which is why
    /// [`Client::keys`] returns un-prefixed keys: Django's
    /// `invalidate_cache` feeds one into the other.
    pub async fn delete_many(&self, keys: &[String]) -> Result<usize, Error> {
        if keys.is_empty() {
            // `delete_many` returns 0 without touching Redis.
            return Ok(0);
        }
        let full: Vec<String> = keys.iter().map(|key| make_key(key)).collect();
        self.redis.del_many(&full).await
    }

    /// `cache.keys(pattern)`: `KEYS :1:<pattern>`, with the prefix stripped
    /// from every match.
    pub async fn keys(&self, pattern: &str) -> Result<Vec<String>, Error> {
        let matches = self.redis.keys(&make_pattern(pattern)).await?;
        Ok(matches.iter().map(|key| reverse_key(key)).collect())
    }

    /// `cache.get_many(keys)`: one `MGET`, then decode.
    pub async fn get_many(&self, keys: &[String]) -> Result<Vec<(String, CachedValue)>, Error> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let full: Vec<String> = keys.iter().map(|key| make_key(key)).collect();
        let values = self.redis.mget(&full).await?;
        let mut out = Vec::with_capacity(values.len());
        for (key, value) in keys.iter().zip(values) {
            if let Some(bytes) = value {
                out.push((key.clone(), decode(&bytes)?));
            }
        }
        Ok(out)
    }

    /// `cache.set_many(data, timeout)`: a pipeline of `SET`s. The observable
    /// state is the same sequence of writes.
    pub async fn set_many(
        &self,
        entries: &[(String, PyValue)],
        timeout: Option<i64>,
    ) -> Result<(), Error> {
        for (key, value) in entries {
            self.set(key, value, timeout).await?;
        }
        Ok(())
    }

    /// The raw value behind a full (already prefixed) Redis key, for tests and
    /// the differential harness.
    pub async fn get_raw(&self, full_key: &str) -> Result<Option<Vec<u8>>, Error> {
        self.redis.get_bytes(full_key).await
    }

    /// `PTTL` of a full (already prefixed) Redis key, in milliseconds.
    pub async fn pttl(&self, full_key: &str) -> Result<i64, Error> {
        self.redis.pttl_ms(full_key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::PyValue;

    #[test]
    fn keys_match_django() {
        assert_eq!(make_key("k"), ":1:k");
        assert_eq!(make_key("/api/instances/"), ":1:/api/instances/");
        assert_eq!(
            make_key("/api/workspaces/acme/labels/:8f1c0d3e"),
            ":1:/api/workspaces/acme/labels/:8f1c0d3e"
        );
        assert_eq!(make_key_opt(None), ":1:None");
        assert_eq!(make_key_opt(Some("x")), ":1:x");
        assert_eq!(make_pattern("*/api/instances/*"), ":1:*/api/instances/*");
        assert_eq!(reverse_key(":1:/api/instances/"), "/api/instances/");
        assert_eq!(reverse_key(":1:a:b:c"), "a:b:c");
        assert_eq!(pickle::DEFAULT_PROTOCOL, 4);
    }

    #[test]
    fn decodes_every_shape_django_writes() {
        assert_eq!(
            decode(b"42").ok(),
            Some(CachedValue::Int(PyInt::from_i64(42)))
        );
        assert_eq!(
            decode(b"-7").ok(),
            Some(CachedValue::Int(PyInt::from_i64(-7)))
        );
        // `int()` accepts surrounding whitespace and underscores.
        assert_eq!(
            decode(b" 1_0 ").ok(),
            Some(CachedValue::Int(PyInt::from_i64(10)))
        );
        assert_eq!(
            decode(b"\x80\x04N.").ok(),
            Some(CachedValue::Pickled(PyValue::None))
        );
        assert!(decode(b"\x80\x04\x5d\x94\x2e\x2e").is_err());
    }

    #[test]
    fn non_ascii_bytes_are_not_integers() {
        // `int(b"\xff")` raises UnicodeDecodeError (a ValueError), so Django
        // falls through to the serializer, which fails too.
        assert!(decode(b"\xff").is_err());
    }

    #[test]
    fn encodes_ints_raw_and_everything_else_pickled() {
        assert_eq!(encode(&PyValue::int(42)), b"42");
        // A `bool` is pickled, not stored as `0`/`1`.
        assert_eq!(encode(&PyValue::bool(true)), b"\x80\x04\x88.");
        assert_eq!(
            encode(&PyValue::string("x")),
            b"\x80\x04\x95\x05\x00\x00\x00\x00\x00\x00\x00\x8c\x01x\x94."
        );
    }
}
