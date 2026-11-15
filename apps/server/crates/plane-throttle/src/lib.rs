// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DRF rate throttles over the same Redis keys django-redis uses.
//!
//! During the strangler migration both backends answer requests against one
//! Redis. A request counted by Django must be visible to Rust and the other way
//! round, so this crate reproduces, byte for byte, what
//! `django_redis.cache.RedisCache` writes and what
//! `rest_framework.throttling.SimpleRateThrottle` does with it:
//!
//! * [`cache`] — django-redis key making (`:1:<key>`), protocol-4 pickled
//!   histories, `SET ... PX <ms>` and `GET`;
//! * [`pickle`] — the encoder/decoder for `list[float]`, verified against
//!   CPython's bytes;
//! * [`rate`] — `parse_rate`, `wait()` and `time.time`;
//! * [`classes`] — every throttle class Plane uses, with its scope, rate and
//!   cache key;
//! * [`throttle`] — `allow_request` and `check_throttles`, including the
//!   non-atomic read-modify-write Django has;
//! * [`render`] — the 429 body/headers and the auth-flow redirect;
//! * [`redis`] — one lazy `ConnectionManager` per process, built from
//!   `Settings` with redis-py's URL rules.
//!
//! The typical call site, in the order Django runs it (after authentication and
//! permission checks, before the view body):
//!
//! ```no_run
//! # use plane_core::config::ThrottlingSettings;
//! # use plane_throttle::{check_throttles, Redis, StaticUser, ThrottleClass, ThrottleOutcome, ThrottleRequest};
//! # async fn run(redis: &Redis, settings: &ThrottlingSettings, headers: &http::HeaderMap) -> Result<(), Box<dyn std::error::Error>> {
//! let user = StaticUser::anonymous();
//! let request = ThrottleRequest {
//!     settings,
//!     user: &user,
//!     headers,
//!     remote_addr: Some("203.0.113.7"),
//!     asset_id: None,
//!     scope_override: None,
//! };
//! let outcome = check_throttles(redis, &[ThrottleClass::Anon], &request, plane_throttle::now()).await?;
//! match outcome {
//!     ThrottleOutcome::Allowed { .. } => {}
//!     ThrottleOutcome::Throttled(rejection) => {
//!         let response = rejection.render()?; // 429, Retry-After, RATE_LIMIT_EXCEEDED
//!         assert_eq!(response.status, 429);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Coverage of the pieces that are easy to get wrong is in the module docs;
//! `HTTP_CONTRACT.md` §12 and its probe A.3 are the reference, and
//! `tools/throttle/interop.py` runs the real Python classes against the same
//! Redis database as the Rust tests.

pub mod cache;
pub mod classes;
pub mod error;
pub mod pickle;
pub mod rate;
pub mod redis;
pub mod render;
pub mod throttle;

pub use cache::Cache;
pub use classes::ThrottleClass;
pub use error::{Error, RateError};
pub use rate::{Rate, now, parse_rate, wait};
pub use redis::{ParsedUrl, Redis};
pub use render::{
    RATE_LIMIT_EXCEEDED_CODE, RATE_LIMIT_EXCEEDED_MESSAGE, ThrottleRejection,
    rate_limit_redirect_url,
};
pub use throttle::{
    RateLimitHeaders, StaticUser, ThrottleOutcome, ThrottleRequest, ThrottleUser, check_throttles,
};
