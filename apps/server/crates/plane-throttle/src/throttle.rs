// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `SimpleRateThrottle.allow_request` and `APIView.check_throttles`.
//!
//! The algorithm is DRF 3.18.3's, unchanged, including the fact that it is a
//! non-atomic read-modify-write (`rest_framework/throttling.py:109-141`):
//!
//! ```python
//! history = cache.get(key, [])
//! now = time.time()
//! while history and history[-1] <= now - duration: history.pop()
//! if len(history) >= num_requests: return False
//! history.insert(0, now)
//! cache.set(key, history, duration)
//! return True
//! ```
//!
//! Two Django workers racing on one bucket can therefore both read `n`
//! entries and both write `n + 1`, losing a request. That is the specification:
//! a Rust request interleaved with a Django request must behave as two Django
//! processes would, so the race is reproduced rather than fixed with a Lua
//! script (`RUST_REWRITE.md`, "Rate limits"; `PROGRESS.md` lists the
//! `NUM_PROXIES` foot-gun for the same reason).
//!
//! `check_throttles` evaluates **every** throttle in the list, even after one
//! has failed, and only then raises with the maximum of the non-`None` waits
//! (`rest_framework/views.py:357-376`). Each throttle that passes writes its
//! own key, so the order of the class list is observable.

use std::fmt;

use http::HeaderMap;
use uuid::Uuid;

use crate::cache::Cache;
use crate::classes::{ThrottleClass, ident_for};
use crate::error::Error;
use crate::rate::{Rate, wait};
use crate::redis::Redis;
use crate::render::ThrottleRejection;
use plane_core::config::ThrottlingSettings;

/// The parts of `request.user` that DRF throttles read: `is_authenticated` and
/// `pk`.
///
/// [`plane_auth::RequestUser`] is the production implementation (the `impl`
/// below); [`StaticUser`] exists for tests and for call sites that have not
/// resolved a session. The trait keeps the throttle independent of how the
/// request user was loaded, and makes the difference between "no user" and
/// "anonymous user" explicit at the type level.
pub trait ThrottleUser: fmt::Debug {
    /// `request.user.is_authenticated`.
    fn is_authenticated(&self) -> bool;
    /// `request.user.pk`; `None` for an anonymous user.
    fn pk(&self) -> Option<Uuid>;
}

/// `request.user` as `plane-auth` resolves it (`user.rs`).
impl ThrottleUser for plane_auth::RequestUser {
    fn is_authenticated(&self) -> bool {
        plane_auth::RequestUser::is_authenticated(self)
    }

    fn pk(&self) -> Option<Uuid> {
        self.id()
    }
}

/// A `ThrottleUser` for tests and for pipelines that have not resolved a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StaticUser {
    authenticated: bool,
    pk: Option<Uuid>,
}

impl StaticUser {
    /// Django's `AnonymousUser`.
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// An authenticated user with this primary key.
    pub fn authenticated(pk: Uuid) -> Self {
        Self {
            authenticated: true,
            pk: Some(pk),
        }
    }
}

impl ThrottleUser for StaticUser {
    fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    fn pk(&self) -> Option<Uuid> {
        self.pk
    }
}

/// Everything a throttle reads from the request, plus the configured rates.
///
/// `scope_override` is `view.throttle_scope`, which DRF only consults for
/// `ScopedRateThrottle`; no Plane view sets it, so it is carried for
/// completeness and never affects a key here.
#[derive(Debug, Clone, Copy)]
pub struct ThrottleRequest<'a> {
    pub settings: &'a ThrottlingSettings,
    pub user: &'a dyn ThrottleUser,
    pub headers: &'a HeaderMap,
    /// `request.META["REMOTE_ADDR"]`, without the port.
    pub remote_addr: Option<&'a str>,
    /// `view.kwargs["asset_id"]`, read by `AssetRateThrottle`.
    pub asset_id: Option<&'a str>,
    /// `view.throttle_scope` (unused by Plane).
    pub scope_override: Option<&'a str>,
}

impl ThrottleRequest<'_> {
    /// `BaseThrottle.get_ident` (HTTP_CONTRACT §12.2, `plane_core::context::throttle_ident`).
    pub fn ident(&self) -> String {
        ident_for(self.headers, self.remote_addr, self.settings.num_proxies)
    }

    /// `request.headers.get("X-Api-Key")`.
    pub fn api_key(&self) -> Option<&str> {
        self.headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
    }
}

/// `X-RateLimit-Remaining` / `X-RateLimit-Reset`, as `ApiKeyRateThrottle` puts
/// them into `request.META` and `BaseAPIView.finalize_response` copies them onto
/// the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitHeaders {
    /// `max(0, num_requests - len(history))` after the insert.
    pub remaining: u64,
    /// `int(now + duration)`: an absolute Unix timestamp, not a delta.
    pub reset: i64,
}

impl RateLimitHeaders {
    /// The two headers, in the order `finalize_response` sets them.
    pub fn as_headers(self) -> [(&'static str, String); 2] {
        [
            ("X-RateLimit-Remaining", self.remaining.to_string()),
            ("X-RateLimit-Reset", self.reset.to_string()),
        ]
    }
}

/// The result of running a view's throttle list.
#[derive(Debug, Clone, PartialEq)]
pub enum ThrottleOutcome {
    /// Every throttle passed (or did not apply). `rate_limit` is `Some` only
    /// when an `ApiKeyRateThrottle` ran and passed.
    Allowed {
        rate_limit: Option<RateLimitHeaders>,
    },
    /// At least one throttle refused. The request must be answered 429 with
    /// [`ThrottleRejection::render`] or Plane's error dict.
    Throttled(ThrottleRejection),
}

impl ThrottleOutcome {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed { .. })
    }
}

/// `APIView.check_throttles` (`rest_framework/views.py:357-376`) for one
/// request, at one instant.
///
/// `now` is `time.time()` at the call site; it is a parameter so tests can
/// freeze it and so one request uses one clock reading (Django reads the clock
/// once per throttle, which can only differ across a second boundary).
///
/// Call this where Django calls it: after authentication and permission
/// checks, before the view body (`views.py:404-421`). A request rejected with
/// 401/403 never reaches a throttle and therefore never consumes budget.
pub async fn check_throttles(
    redis: &Redis,
    classes: &[ThrottleClass],
    request: &ThrottleRequest<'_>,
    now: f64,
) -> Result<ThrottleOutcome, Error> {
    let cache = Cache::new(redis);
    let mut durations: Vec<f64> = Vec::new();
    let mut refused = false;
    let mut rate_limit: Option<RateLimitHeaders> = None;

    // `get_throttles()` constructs every throttle before `check_throttles`
    // calls the first one (`rest_framework/views.py:363`), so a malformed rate
    // anywhere in the list fails the request before any bucket is touched.
    let mut resolved: Vec<(ThrottleClass, Rate)> = Vec::with_capacity(classes.len());
    for class in classes {
        resolved.push((*class, class.rate(request.settings)?));
    }

    for (class, rate) in resolved {
        let key = class.cache_key(request);
        let mut allowed = true;

        if let Some(key) = &key {
            let mut history = cache.get_history(Some(key)).await?;
            pop_expired(&mut history, now, rate);
            if history.len() as i64 >= rate.num_requests {
                // `throttle_failure()`: no write, keep the history for `wait()`.
                allowed = false;
                refused = true;
                if let Some(wait) = wait(&history, now, rate) {
                    durations.push(wait);
                }
            } else {
                // `throttle_success()`: insert at the front, then
                // `cache.set(key, history, duration)`.
                history.insert(0, now);
                cache.set_history(key, &history, rate.duration).await?;
            }
        }

        if allowed && class.emits_rate_limit_headers() {
            // `ApiKeyRateThrottle.allow_request` re-reads the bucket it just
            // wrote (`plane/api/rate_limit.py:38-48`). With no X-Api-Key the
            // key is None and this is a GET of `:1:None` — see
            // `cache::make_key_opt`.
            let mut history = cache.get_history(key.as_deref()).await?;
            pop_expired(&mut history, now, rate);
            let available = rate.num_requests - history.len() as i64;
            rate_limit = Some(RateLimitHeaders {
                remaining: available.max(0) as u64,
                reset: (now + rate.duration as f64) as i64,
            });
        }
    }

    if !refused {
        return Ok(ThrottleOutcome::Allowed { rate_limit });
    }
    // `duration = max(durations, default=None)`, then `Throttled(duration)`.
    //
    // `durations` is empty when every failing throttle returned `None` from
    // `wait()`: `available_requests = num_requests - len(history) + 1` is
    // non-positive only when the bucket holds *more* entries than the current
    // limit, which happens when a deployment lowers a rate while a longer
    // history is still cached (DRF issue #1438, cited at
    // `rest_framework/views.py:368-369`). DRF still refuses the request, with
    // `Throttled(None)`: no "Expected available in" suffix, no `Retry-After`.
    let wait = durations.iter().copied().reduce(f64::max).map(f64::ceil);
    Ok(ThrottleOutcome::Throttled(ThrottleRejection::from_wait(
        wait,
    )))
}

/// `while history and history[-1] <= now - duration: history.pop()`.
fn pop_expired(history: &mut Vec<f64>, now: f64, rate: Rate) {
    let cutoff = now - rate.duration as f64;
    while history.last().is_some_and(|oldest| *oldest <= cutoff) {
        history.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_core::config::ThrottlingSettings;

    fn settings(num_proxies: Option<u16>) -> ThrottlingSettings {
        ThrottlingSettings {
            api_key_rate_limit: "60/minute".to_string(),
            authentication_rate_limit: "10/minute".to_string(),
            num_proxies,
            anon_rate: "30/minute",
            asset_rate: "5/minute",
            email_verification_rate: "3/hour",
        }
    }

    #[test]
    fn expiry_is_inclusive() {
        // `<=` matters: an entry exactly one window old is dropped.
        let rate = Rate {
            num_requests: 1,
            duration: 60,
        };
        let mut history = vec![1_060.0, 1_000.0];
        pop_expired(&mut history, 1_060.0, rate);
        assert_eq!(history, vec![1_060.0]);

        let mut history = vec![1_060.0, 1_000.000_001];
        pop_expired(&mut history, 1_060.0, rate);
        assert_eq!(history, vec![1_060.0, 1_000.000_001]);
    }

    #[test]
    fn ceiling_matches_math_ceil() {
        // `math.ceil` in `Throttled.__init__`, for the values `wait()` can
        // produce (always positive).
        assert_eq!(1.0_f64.ceil() as u64, 1);
        assert_eq!(1.000_001_f64.ceil() as u64, 2);
        assert_eq!(59.999_f64.ceil() as u64, 60);
    }

    /// `plane_auth::RequestUser` is what a handler passes; this pins the two
    /// keys that depend on the authenticated/anonymous split.
    #[test]
    fn plane_auth_request_user_drives_the_key() {
        let settings = settings(None);
        let headers = HeaderMap::new();
        let anonymous = plane_auth::RequestUser::Anonymous;
        let request = ThrottleRequest {
            settings: &settings,
            user: &anonymous,
            headers: &headers,
            remote_addr: Some("203.0.113.7"),
            asset_id: None,
            scope_override: None,
        };
        assert_eq!(
            ThrottleClass::Anon.cache_key(&request).as_deref(),
            Some("throttle_anon_203.0.113.7")
        );

        let pk = Uuid::parse_str("8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0").unwrap();
        let signed_in = plane_auth::RequestUser::from_user(plane_auth::AuthUser {
            id: pk,
            email: "user@example.com".to_string(),
            username: "user".to_string(),
            display_name: "User".to_string(),
            first_name: "User".to_string(),
            last_name: String::new(),
            user_timezone: "UTC".to_string(),
            is_active: true,
            is_superuser: false,
            is_staff: false,
            is_bot: false,
            is_email_verified: true,
            password: "!".to_string(),
        });
        let request = ThrottleRequest {
            settings: &settings,
            user: &signed_in,
            headers: &headers,
            remote_addr: Some("203.0.113.7"),
            asset_id: None,
            scope_override: None,
        };
        // Signed-in callers are not throttled by `AnonRateThrottle`.
        assert_eq!(ThrottleClass::Anon.cache_key(&request), None);
        // `EmailVerificationThrottle` keys on the pk, not the address.
        assert_eq!(
            ThrottleClass::EmailVerification
                .cache_key(&request)
                .as_deref(),
            Some("throttle_email_verification_8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0")
        );
        // `AuthenticationThrottle` still keys on the address.
        assert_eq!(
            ThrottleClass::Authentication.cache_key(&request).as_deref(),
            Some("throttle_authentication_203.0.113.7")
        );
    }
}
