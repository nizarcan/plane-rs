// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `django_redis.cache.RedisCache` wire format, for throttle values only.
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
//! so the absent API key case reads the literal key `:1:None` — see
//! [`make_key_opt`].
//!
//! **Values.** `DefaultClient.encode` stores integers as their ASCII digits
//! and everything else through `PickleSerializer.dumps`, i.e.
//! `pickle.dumps(value, 4)`; `IdentityCompressor` is the identity
//! (`default.py:583-603`, `serializers/pickle.py:10-40`). Throttle histories are
//! always lists, so every write is a protocol-4 pickle ([`crate::pickle`]).
//!
//! **Writes.** `DefaultClient.set` with a timeout does
//! `SET <key> <value> PX <int(timeout * 1000)>` (`default.py:231-283`), so a
//! 60-second throttle window is `PX 60000`. A non-positive timeout makes
//! Django delete the key instead.
//!
//! **Reads.** `DefaultClient.get(key, default)` issues a plain `GET` and
//! returns `default` when the key is missing; `decode` first tries
//! `int(value)` and only then unpickles (`default.py:342-367`, `563-578`).

use crate::error::Error;
use crate::pickle;
use crate::redis::Redis;
use plane_core::py;

/// `KEY_PREFIX` from Plane's `CACHES` (unset → `""`).
pub const KEY_PREFIX: &str = "";
/// `VERSION` from Plane's `CACHES` (unset → `1`).
pub const VERSION: i64 = 1;

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

/// `make_key()` applied to an optional throttle key.
///
/// `SimpleRateThrottle.allow_request` returns early when `get_cache_key`
/// returns `None`, but `ApiKeyRateThrottle.allow_request` then calls
/// `self.cache.get(self.key, [])` unconditionally (`plane/api/rate_limit.py:38`),
/// and `make_key(None)` is `":1:None"` because `%s` formats `None` as
/// `"None"`. Django really does `GET :1:None` on every keyless `/api/v1/`
/// request; repeated here so both backends read the same key.
pub fn make_key_opt(key: Option<&str>) -> String {
    match key {
        Some(key) => make_key(key),
        None => format!("{KEY_PREFIX}:{VERSION}:None"),
    }
}

/// `PickleSerializer.dumps` for a throttle history.
pub fn encode_floats(history: &[f64]) -> Vec<u8> {
    pickle::dumps_floats(history)
}

/// What `DefaultClient.decode` can return.
#[derive(Debug, Clone, PartialEq)]
pub enum CachedValue {
    /// `DefaultClient.encode(allow_int=True)`: an integer stored as text.
    Int(i64),
    /// A pickled Python `list`.
    List(Vec<f64>),
}

/// `DefaultClient.decode` (`django_redis/client/default.py:563-578`): try
/// `int(value)`, otherwise decompress (identity) and unpickle.
pub fn decode(value: &[u8]) -> Result<CachedValue, Error> {
    if let Ok(text) = std::str::from_utf8(value)
        && let Ok(integer) = py::int(text)
    {
        return Ok(CachedValue::Int(integer));
    }
    let floats = pickle::loads_floats(value).map_err(|error| {
        // Django raises out of `pickle.loads`, which surfaces as a 500 with
        // no key in the message; keep both for operators.
        Error::MalformedHistory {
            key: String::new(),
            reason: error.to_string(),
        }
    })?;
    Ok(CachedValue::List(floats))
}

/// A `DefaultClient` view over one Redis connection handle.
#[derive(Debug)]
pub struct Cache<'a> {
    redis: &'a Redis,
}

impl<'a> Cache<'a> {
    pub fn new(redis: &'a Redis) -> Self {
        Self { redis }
    }

    /// `DefaultClient.get(key, None)`: the decoded value, or `None`.
    pub async fn get(&self, key: &str) -> Result<Option<CachedValue>, Error> {
        let Some(bytes) = self.redis.get_bytes(&make_key(key)).await? else {
            return Ok(None);
        };
        decode(&bytes).map(Some).map_err(|error| match error {
            Error::MalformedHistory { reason, .. } => Error::MalformedHistory {
                key: make_key(key),
                reason,
            },
            other => other,
        })
    }

    /// `cache.get(key, [])` followed by the `SimpleRateThrottle` type
    /// assumption: a list of floats. A cached integer is an error, exactly as
    /// Django's `len(history)` on an `int` is a `TypeError`.
    pub async fn get_history(&self, key: Option<&str>) -> Result<Vec<f64>, Error> {
        let full = make_key_opt(key);
        let Some(bytes) = self.redis.get_bytes(&full).await? else {
            return Ok(Vec::new());
        };
        match decode(&bytes).map_err(|error| match error {
            Error::MalformedHistory { reason, .. } => Error::MalformedHistory {
                key: full.clone(),
                reason,
            },
            other => other,
        })? {
            CachedValue::List(history) => Ok(history),
            CachedValue::Int(integer) => Err(Error::MalformedHistory {
                key: full,
                reason: format!("cached integer {integer}"),
            }),
        }
    }

    /// `cache.set(key, history, timeout)`: `SET key <pickle> PX <timeout*1000>`.
    pub async fn set_history(
        &self,
        key: &str,
        history: &[f64],
        timeout_seconds: i64,
    ) -> Result<(), Error> {
        let full = make_key(key);
        // `timeout = int(timeout * 1000)` then, when `timeout <= 0`,
        // `cache.delete(key)` (`default.py:257-270`).
        let milliseconds = timeout_seconds.saturating_mul(1000);
        if milliseconds <= 0 {
            self.redis.del(&full).await?;
            return Ok(());
        }
        self.redis
            .set_bytes_px(&full, &encode_floats(history), milliseconds)
            .await
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

    #[test]
    fn keys_match_django() {
        // HTTP_CONTRACT §12.3 / probe A.3.
        assert_eq!(make_key("k"), ":1:k");
        assert_eq!(
            make_key("throttle_anon_203.0.113.7"),
            ":1:throttle_anon_203.0.113.7"
        );
        assert_eq!(
            make_key("api_key:plane_api_abc123"),
            ":1:api_key:plane_api_abc123"
        );
        assert_eq!(make_key_opt(None), ":1:None");
        assert_eq!(make_key_opt(Some("x")), ":1:x");
        assert_eq!(crate::pickle::DEFAULT_PROTOCOL, 4);
    }

    #[test]
    fn decodes_every_shape_django_writes() {
        assert_eq!(decode(b"42").ok(), Some(CachedValue::Int(42)));
        assert_eq!(decode(b"-7").ok(), Some(CachedValue::Int(-7)));
        // `int()` accepts surrounding whitespace and underscores.
        assert_eq!(decode(b" 1_0 ").ok(), Some(CachedValue::Int(10)));
        assert_eq!(
            decode(b"\x80\x04\x5d\x94\x2e").ok(),
            Some(CachedValue::List(Vec::new()))
        );
        assert!(decode(b"\x80\x04\x5d\x94\x2e\x2e").is_err());
    }

    #[test]
    fn non_ascii_bytes_are_not_integers() {
        // `int(b"\xff")` raises UnicodeDecodeError (a ValueError), so Django
        // falls through to the serializer, which fails too.
        assert!(matches!(
            decode(b"\xff"),
            Err(Error::MalformedHistory { .. })
        ));
    }
}
