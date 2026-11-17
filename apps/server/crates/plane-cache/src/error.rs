// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Errors from the cache path.
//!
//! Every variant here is something Django raises as an unhandled exception
//! (a `redis` error wrapped in `ConnectionInterrupted`, an unpickling error, a
//! `TypeError` from `pickle.loads`), which the request pipeline must render as
//! the 500 page (`HTTP_CONTRACT.md` §11.2).

/// Why the cache could not be used.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `REDIS_URL` is unset for a role that needs Redis (`ENV.md` §0).
    #[error("REDIS_URL is not set")]
    RedisUrlMissing,

    /// `redis-py` rejects anything but `redis://`, `rediss://`, `unix://`
    /// (`redis/connection.py:882-891`); Django raises `ValueError` at import.
    #[error("Redis URL must specify one of the following schemes (redis://, rediss://, unix://)")]
    RedisUrlScheme,

    /// A malformed `REDIS_URL` part (port, db, escaped userinfo).
    #[error("invalid REDIS_URL: {0}")]
    RedisUrl(&'static str),

    /// `REDIS_SSL_CERT_REQS` outside `none`/`optional`/`required`, which
    /// `redis.connection.SSLConnection` rejects (`connection.py:726-729`).
    #[error("Invalid SSL Certificate Requirements Flag: {0}")]
    SslCertReqs(String),

    /// `REDIS_SSL_CA_CERTS` could not be read.
    #[error("cannot read REDIS_SSL_CA_CERTS: {0}")]
    SslCaCerts(String),

    /// The Redis connection or command failed.
    #[error("redis: {0}")]
    Redis(#[from] redis::RedisError),

    /// The stored value is not something `DefaultClient.decode` can read.
    #[error("cache value is not decodable: {0}")]
    Decode(String),

    /// The stored value decoded, but not to the shape the caller needs (for
    /// example `{"data": …}` with no `data` key).
    #[error("cache value has an unexpected shape: {0}")]
    Shape(String),

    /// `cache.incr(key)` on a key that does not exist:
    /// django-redis's `ValueError("Key '<repr(full key)>' not found")`
    /// (`django_redis/client/default.py:697-699`). Note the *double* quoting:
    /// Python's `repr` already adds quotes, and the message wraps it in one
    /// more pair, so a normal key reads `Key '':1:k'' not found`.
    #[error("Key '{key}' not found")]
    MissingKey {
        /// `repr()` of the full (`:1:`-prefixed) Redis key.
        key: String,
    },

    /// The cached value could not be rendered as JSON.
    #[error("render: {0}")]
    Render(#[from] plane_serde::Error),
}

/// Why a pickle could not be encoded or decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PickleError {
    /// The stream ended in the middle of an opcode or argument.
    #[error("truncated pickle: {0}")]
    Truncated(&'static str),
    /// An opcode this codec does not model.
    #[error("unsupported pickle opcode 0x{0:02x}")]
    UnsupportedOpcode(u8),
    /// `PROTO` above protocol 5.
    #[error("unsupported pickle protocol {0}")]
    UnsupportedProtocol(u8),
    /// `STACK_GLOBAL`/`GLOBAL` named a class this codec cannot rebuild.
    #[error("unsupported global {module}.{name}")]
    UnsupportedGlobal {
        /// The module name.
        module: String,
        /// The class name.
        name: String,
    },
    /// Bytes after `STOP`, which `pickle.loads` ignores but a cache value must
    /// not contain.
    #[error("trailing bytes after STOP")]
    TrailingBytes,
    /// A structurally invalid stream (an opcode out of place).
    #[error("malformed pickle: {0}")]
    Malformed(&'static str),
    /// A `GET`/`BINGET` to an index the memo does not have.
    #[error("memo index {0} out of range")]
    InvalidMemoIndex(usize),
    /// A reference to a container that is still being filled: a recursive
    /// object, which a tree cannot represent.
    #[error("recursive container (memo index {0})")]
    RecursiveBuilding(usize),
    /// A `BINUNICODE` that is not valid WTF-8.
    #[error("string is not valid WTF-8")]
    InvalidString,
    /// A `LONG`/`INT` argument that is not a decimal integer.
    #[error("invalid integer text {0:?}")]
    InvalidInteger(String),
    /// A `FLOAT` argument that is not a float.
    #[error("invalid FLOAT text {0:?}")]
    InvalidFloat(String),
    /// `decimal.Decimal` text that Python would reject.
    #[error("invalid decimal text {0:?}")]
    InvalidDecimal(String),
    /// `BUILD` on a type this codec does not rebuild.
    #[error("unsupported BUILD state")]
    UnsupportedBuild,
}
