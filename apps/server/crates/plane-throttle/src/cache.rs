// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `django_redis.cache.RedisCache` wire format, for throttle values.
//!
//! The keys, values and commands are [`plane_cache`]'s: this module only adds
//! the `SimpleRateThrottle` shape on top — a `list[float]` history read with
//! `cache.get(key, [])` and written with `cache.set(key, history, duration)`.
//!
//! Plane configures the cache with the defaults (`plane/settings/common.py:274-291`):
//! `django_redis.cache.RedisCache`, `CLIENT_CLASS=DefaultClient`, no
//! `KEY_PREFIX`, no `VERSION`, no `SERIALIZER`, no `COMPRESSOR`, no
//! `PICKLE_VERSION`, so a key is `":1:" + key` and a value is
//! `pickle.dumps(value, 4)`.
//!
//! `ApiKeyRateThrottle` reads a bucket for a request with no `X-Api-Key`:
//! `make_key(None)` is `":1:None"` because `%s` formats `None` as `"None"`, and
//! `GET :1:None` really is issued on every keyless `/api/v1/` request — see
//! [`make_key_opt`].

use plane_cache::{CachedValue, Client, PyValue};

use crate::error::Error;

pub use plane_cache::client::{
    DEFAULT_TIMEOUT, KEY_PREFIX, VERSION, decode, make_key, make_key_opt,
};
pub use plane_cache::pickle::DEFAULT_PROTOCOL;

/// `PickleSerializer.dumps` for a throttle history.
pub fn encode_floats(history: &[f64]) -> Vec<u8> {
    plane_cache::pickle::dumps(&floats_to_value(history))
}

fn floats_to_value(history: &[f64]) -> PyValue {
    PyValue::list(history.iter().map(|value| PyValue::float(*value)).collect())
}

/// A `DefaultClient` view over one Redis connection handle.
#[derive(Debug)]
pub struct Cache<'a> {
    client: Client<'a>,
}

impl<'a> Cache<'a> {
    pub fn new(redis: &'a plane_cache::Redis) -> Self {
        Self {
            client: Client::new(redis),
        }
    }

    /// `cache.get(key, None)`: the decoded value, or `None`.
    pub async fn get(&self, key: &str) -> Result<Option<CachedValue>, Error> {
        Ok(self.client.get(key).await?)
    }

    /// `cache.get(key, [])` followed by the `SimpleRateThrottle` type
    /// assumption: a list of floats.
    ///
    /// A cached integer is an error, exactly as Django's `len(history)` on an
    /// `int` is a `TypeError`. `None` means the *key* `":1:None"`: Django
    /// formats the missing API key with `%s`, so both backends read and write
    /// that literal key for keyless requests.
    pub async fn get_history(&self, key: Option<&str>) -> Result<Vec<f64>, Error> {
        let key = key.unwrap_or("None");
        let Some(value) = self.client.get(key).await? else {
            return Ok(Vec::new());
        };
        let full = make_key(key);
        match value {
            CachedValue::Int(integer) => Err(Error::MalformedHistory {
                key: full,
                reason: format!("cached integer {integer}"),
            }),
            CachedValue::Pickled(PyValue::List(items) | PyValue::Tuple(items)) => items
                .iter()
                .map(|item| match item {
                    PyValue::Float(value) => Ok(*value),
                    other => Err(Error::MalformedHistory {
                        key: full.clone(),
                        reason: format!("history item is a {}", other.type_name()),
                    }),
                })
                .collect(),
            CachedValue::Pickled(other) => Err(Error::MalformedHistory {
                key: full,
                reason: format!("cached value is a {}", other.type_name()),
            }),
        }
    }

    /// `cache.set(key, history, timeout)`: `SET key <pickle> PX <timeout*1000>`,
    /// or `DEL` when the timeout is not positive.
    pub async fn set_history(
        &self,
        key: &str,
        history: &[f64],
        timeout_seconds: i64,
    ) -> Result<(), Error> {
        self.client
            .set(key, &floats_to_value(history), Some(timeout_seconds))
            .await?;
        Ok(())
    }

    /// The raw value behind a full (already prefixed) Redis key, for tests and
    /// the differential harness.
    pub async fn get_raw(&self, full_key: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.client.get_raw(full_key).await?)
    }

    /// `PTTL` of a full (already prefixed) Redis key, in milliseconds.
    pub async fn pttl(&self, full_key: &str) -> Result<i64, Error> {
        Ok(self.client.pttl(full_key).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_cache::PyInt;

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
        assert_eq!(DEFAULT_PROTOCOL, 4);
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
            decode(b"\x80\x04\x5d\x94\x2e").ok(),
            Some(CachedValue::Pickled(PyValue::list(Vec::new())))
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
    fn history_pickles_round_trip() {
        let history = [1_789_000_000.123_456, 1_788_999_940.0];
        let encoded = encode_floats(&history);
        assert_eq!(
            plane_cache::hex(&encoded),
            "80049517000000000000005d94284741daa87e5007e6b44741daa87e41000000652e"
        );
    }
}
