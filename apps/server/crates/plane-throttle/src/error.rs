// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Errors from the throttle path.
//!
//! Every variant here is something Django raises as an unhandled exception
//! (or Django's cache raises), which the request pipeline must render as the
//! 500 page (`HTTP_CONTRACT.md` §11.2). None of them is a 429: a throttled
//! request is an [`crate::ThrottleOutcome`], not an error.

/// Why a throttle could not be evaluated.
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

    /// `DRF`'s `parse_rate` failed: `ValueError` on the split or the integer,
    /// `IndexError` on an empty period, `KeyError` on an unknown period.
    #[error("invalid throttle rate {rate:?} for scope {scope:?}: {reason}")]
    Rate {
        scope: &'static str,
        rate: String,
        reason: RateError,
    },

    /// `SimpleRateThrottle.get_rate`: `ImproperlyConfigured`, reached when the
    /// class has no usable `rate` and `DEFAULT_THROTTLE_RATES` has no entry.
    /// Plane wires `API_KEY_RATE_LIMIT=""` and `AUTHENTICATION_RATE_LIMIT=""`
    /// into exactly this (`ENV.md` §1.10).
    #[error("No default throttle rate set for '{0}' scope")]
    MissingRate(&'static str),

    /// The cached value is an integer (django-redis stores integers raw) or
    /// something else that is not a throttle history. Django's `len()` and
    /// indexing on it raise `TypeError`, i.e. a 500.
    #[error("cache value for {key:?} is not a list of floats: {reason}")]
    MalformedHistory { key: String, reason: String },

    /// The rendered 429 body failed to serialise (cannot happen for a
    /// two-field object of literals; kept so the pipeline has no `unwrap`).
    #[error("render: {0}")]
    Render(#[from] plane_serde::Error),
}

/// Why `SimpleRateThrottle.parse_rate` rejected a rate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RateError {
    /// `num, period = rate.split("/")` with the wrong number of fields.
    #[error("not enough values to unpack (expected 2, got {0})")]
    TooFewParts(usize),
    /// The same for three or more fields (`ValueError: too many values`).
    #[error("too many values to unpack (expected 2)")]
    TooManyParts,
    /// `int(num)`.
    #[error("invalid literal for int() with base 10: {0:?}")]
    NotAnInteger(String),
    /// `period[0]` on an empty period: `IndexError: string index out of range`.
    #[error("string index out of range")]
    EmptyPeriod,
    /// `{"s": 1, "m": 60, "h": 3600, "d": 86400}[period[0]]`.
    #[error("unknown rate period {0:?} (expected one of s, m, h, d)")]
    UnknownPeriod(char),
}
