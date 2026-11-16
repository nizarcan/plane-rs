// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A django-redis-compatible cache, and Plane's DRF response caching on top of
//! it.
//!
//! During the strangler migration both backends answer requests against one
//! Redis and both read and write the same cache keys. This crate reproduces,
//! byte for byte, what `django_redis.cache.RedisCache` writes and what
//! `plane/utils/cache.py` does with it:
//!
//! * [`client`] — `DefaultClient`'s `get`/`set`/`delete`/`delete_many`/`keys`/
//!   `get_many`/`set_many`, the `:1:<key>` key format, the `KEYS` command, and
//!   the raw-int-versus-pickle value rule;
//! * [`pickle`] — CPython protocol-4 encoding and decoding for every value a
//!   cached payload can hold, including DRF's `ReturnList`/`ReturnDict`
//!   wrappers and `uuid.UUID` objects;
//! * [`value`] — the [`PyValue`] tree, which renders through
//!   `plane_serde::Dialect::Drf` exactly as `JSONRenderer` renders the
//!   unpickled objects;
//! * [`drf`] — `cache_response`, `invalidate_cache` and
//!   `invalidate_cache_directly` as functions a handler calls;
//! * [`page`] — Django's `cache_page`/`CacheMiddleware` semantics in a private
//!   key namespace and value format (DECISIONS.md D5a);
//! * [`redis`] — one lazy `ConnectionManager` per process, built from
//!   `Settings` with redis-py's URL rules;
//! * [`pyint`] — a Python `int` (arbitrary precision) in the binary form the
//!   pickler needs.
//!
//! # A typical handler
//!
//! ```no_run
//! # use plane_cache::drf::{RequestFacts, lookup, response_cache_key, store};
//! # use plane_cache::{PyValue, Redis};
//! # async fn handler(redis: &Redis, facts: &RequestFacts<'_>) -> Result<(), Box<dyn std::error::Error>> {
//! // `@cache_response(60 * 60 * 2)` on a `many=True` view.
//! let key = response_cache_key(None, true, facts);
//! if let Some(cached) = lookup(redis, &key).await? {
//!     // `Response(cached.data, status = cached.status)`: render the tree.
//!     let body = plane_serde::to_vec(plane_serde::Dialect::Drf, &cached.data)?;
//!     let _ = (body, cached.status);
//!     return Ok(());
//! }
//! // ... miss: build the same tree the serializer's `.data` would have.
//! let data = PyValue::return_list(Vec::new());
//! store(redis, &key, &data, 200, 60 * 60 * 2, false).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Known limits, all deliberate
//!
//! * The pickler's memo is keyed on object identity. [`value::PyInterner`]
//!   reproduces DRF's sharing: one `str` object per serializer field name
//!   (shared across rows), one per `ChoiceField` constant, and fresh objects
//!   for database strings (except the empty string and single Latin-1
//!   characters, which CPython caches). Use
//!   `PyValue::return_list`/`return_dict`/`return_mapping` for a top-level
//!   `.data`.
//! * A module/class-name string that CPython's `save_global` creates ('datetime',
//!   'uuid', 'Decimal', …) is interned by the encoder. A payload that happens
//!   to contain the *same string object* as one of those would memoise
//!   differently; no Plane payload does, and a value tree cannot express it.
//! * A recursive pickle (a container memoised and referenced before it is
//!   complete) is rejected: [`PyValue`] is a tree.

pub mod client;
pub mod drf;
pub mod error;
pub mod page;
pub mod pickle;
pub mod pyint;
pub mod redis;
pub mod value;

pub use client::{CachedValue, Client, DEFAULT_TIMEOUT, make_key, make_key_opt};
pub use drf::{CachedResponse, Invalidation, RequestFacts};
pub use error::{Error, PickleError};
pub use page::{CachedPage, PageResponse};
pub use pyint::PyInt;
pub use redis::{ParsedUrl, Redis};
pub use value::{PyDateTime, PyInterner, PyMapping, PyReduce, PyTz, PyValue, ReduceFunc};

/// Lower-case hex, as `binascii.hexlify(...).decode()` renders raw Redis
/// values in the interop tool and the differential harness.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}
