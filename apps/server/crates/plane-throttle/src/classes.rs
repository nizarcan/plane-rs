// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Every DRF throttle class Plane uses, and the cache key each one builds.
//!
//! Found by grepping `apps/api/plane` for `Throttle` subclasses,
//! `throttle_classes`, `get_throttles` and manual throttle calls:
//!
//! | Class | Scope | Key | Applied at |
//! | --- | --- | --- | --- |
//! | `AnonRateThrottle` | `anon` | `throttle_anon_<ident>` | `DEFAULT_THROTTLE_CLASSES` (`common.py:141`) → every `/api/`, `/api/public/`, `/api/instances/` view that does not override `get_throttles`/`throttle_classes` |
//! | `UserRateThrottle` | `user` | `throttle_user_<pk or ident>` | Not instantiated by Plane; it is `EmailVerificationThrottle`'s base class. Kept because a future view may name it, and because it must fail the same way if it does (no `user` entry in `DEFAULT_THROTTLE_RATES`). |
//! | `AuthenticationThrottle` | `authentication` | `throttle_authentication_<ident>` | `/auth/` APIViews (`check`, `magic`, `password_management`, `common.py:49`), `UserEndpoint.update_email`, `UserEndpoint.generate_email_verification_code`, and the `@throttle_auth_redirect` decorator on the plain `View` flows |
//! | `EmailVerificationThrottle` | `email_verification` | `throttle_email_verification_<pk or ident>` | `UserEndpoint.generate_email_verification_code` (`plane/app/views/user/base.py:72-80`) |
//! | `ApiKeyRateThrottle` | `api_key` | `api_key:<X-Api-Key>` | `/api/v1/` via `BaseAPIView.get_throttles`/`BaseViewSet.get_throttles` (`plane/api/views/base.py:69-70`) |
//! | `AssetRateThrottle` | `asset_id` | `throttle_asset_<asset_id>` | `DuplicateAssetEndpoint` (`plane/app/views/asset/v2.py:841`) |
//!
//! `ScopedRateThrottle` is deliberately absent: no Plane view sets
//! `throttle_scope`, and `ScopedRateThrottle.allow_request` returns `True`
//! before touching the cache when the attribute is missing
//! (`rest_framework/throttling.py:219-226`). [`ThrottleClass::from_dotted`]
//! returns `None` for it; a policy generator must treat an unknown dotted path
//! as an error, never as "no throttle".

use http::HeaderMap;

use crate::error::Error;
use crate::rate::{Rate, rate_string};
use crate::throttle::ThrottleRequest;
use plane_core::config::ThrottlingSettings;
use plane_core::context::throttle_ident;

/// A throttle class Plane applies somewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThrottleClass {
    /// `rest_framework.throttling.AnonRateThrottle`.
    Anon,
    /// `rest_framework.throttling.UserRateThrottle` (base class only today).
    User,
    /// `plane.authentication.rate_limit.AuthenticationThrottle`.
    Authentication,
    /// `plane.authentication.rate_limit.EmailVerificationThrottle`.
    EmailVerification,
    /// `plane.api.rate_limit.ApiKeyRateThrottle`.
    ApiKey,
    /// `plane.throttles.asset.AssetRateThrottle`.
    Asset,
}

impl ThrottleClass {
    /// Every class this crate implements, in `DEFAULT_THROTTLE_CLASSES` order
    /// followed by the Plane-specific ones.
    pub const ALL: [ThrottleClass; 6] = [
        ThrottleClass::Anon,
        ThrottleClass::User,
        ThrottleClass::Authentication,
        ThrottleClass::EmailVerification,
        ThrottleClass::ApiKey,
        ThrottleClass::Asset,
    ];

    /// Resolve the dotted paths Django's settings and views use.
    ///
    /// `None` means "this crate does not implement that class": either it is
    /// not used by Plane (`ScopedRateThrottle`), or the route policy is stale
    /// and the request must not silently skip throttling.
    pub fn from_dotted(path: &str) -> Option<Self> {
        // Compare the trailing component only, so both the module path Django
        // writes in settings and a short `App.Class` import resolve.
        match path.rsplit('.').next()? {
            "AnonRateThrottle" => Some(Self::Anon),
            "UserRateThrottle" => Some(Self::User),
            "AuthenticationThrottle" => Some(Self::Authentication),
            "EmailVerificationThrottle" => Some(Self::EmailVerification),
            "ApiKeyRateThrottle" => Some(Self::ApiKey),
            "AssetRateThrottle" => Some(Self::Asset),
            _ => None,
        }
    }

    /// The dotted path as it appears in `apps/api`.
    pub fn dotted(self) -> &'static str {
        match self {
            Self::Anon => "rest_framework.throttling.AnonRateThrottle",
            Self::User => "rest_framework.throttling.UserRateThrottle",
            Self::Authentication => "plane.authentication.rate_limit.AuthenticationThrottle",
            Self::EmailVerification => "plane.authentication.rate_limit.EmailVerificationThrottle",
            Self::ApiKey => "plane.api.rate_limit.ApiKeyRateThrottle",
            Self::Asset => "plane.throttles.asset.AssetRateThrottle",
        }
    }

    /// `SimpleRateThrottle.scope`, which is also the `cache_format` field.
    pub fn scope(self) -> &'static str {
        match self {
            Self::Anon => "anon",
            Self::User => "user",
            Self::Authentication => "authentication",
            Self::EmailVerification => "email_verification",
            Self::ApiKey => "api_key",
            Self::Asset => "asset_id",
        }
    }

    /// The class-level `rate` attribute, when the class sets one. `None` means
    /// DRF falls back to `DEFAULT_THROTTLE_RATES[scope]`.
    fn class_rate(self, settings: &ThrottlingSettings) -> Option<&str> {
        match self {
            // No class-level `rate`; `get_rate()` reads the settings.
            Self::Anon | Self::User | Self::Asset => None,
            // `rate = os.environ.get("AUTHENTICATION_RATE_LIMIT", "10/minute")`.
            Self::Authentication => Some(&settings.authentication_rate_limit),
            // `rate = "3/hour"`.
            Self::EmailVerification => Some(settings.email_verification_rate),
            // `rate = settings.API_KEY_RATE_LIMIT`.
            Self::ApiKey => Some(&settings.api_key_rate_limit),
        }
    }

    /// The rate, resolved and parsed the way `SimpleRateThrottle.__init__` does.
    ///
    /// A malformed or missing rate is an error, and Django turns it into a 500
    /// on the request that first constructs the throttle (not at startup).
    pub fn rate(self, settings: &ThrottlingSettings) -> Result<Rate, Error> {
        let rate = rate_string(self.scope(), self.class_rate(settings), settings)?;
        crate::rate::parse_rate(Some(rate))
            .map_err(|reason| Error::Rate {
                scope: self.scope(),
                rate: rate.to_string(),
                reason,
            })?
            .ok_or(Error::MissingRate(self.scope()))
    }

    /// `SimpleRateThrottle.get_cache_key` for this class, or `None` when the
    /// request is not throttled by it.
    ///
    /// The key is the *unprefixed* key; django-redis prepends `":1:"`
    /// ([`crate::cache::make_key`]).
    pub fn cache_key(self, request: &ThrottleRequest<'_>) -> Option<String> {
        match self {
            // `AnonRateThrottle.get_cache_key` (`throttling.py:167-180`).
            Self::Anon => {
                if request.user.is_authenticated() {
                    return None;
                }
                Some(format!("throttle_anon_{}", request.ident()))
            }
            // `UserRateThrottle.get_cache_key` (`throttling.py:193-202`).
            Self::User => Some(format!("throttle_user_{}", user_ident(request))),
            // `AuthenticationThrottle.get_cache_key` keys on the client IP for
            // every caller, signed in or not (`rate_limit.py:32-34`).
            Self::Authentication => Some(format!("throttle_authentication_{}", request.ident())),
            // Extends `UserRateThrottle`, so the user's pk (or the IP).
            Self::EmailVerification => Some(format!(
                "throttle_email_verification_{}",
                user_ident(request)
            )),
            // `f"{self.scope}:{api_key}"` — no `cache_format`, and the raw
            // token becomes part of the Redis key name (`rate_limit.py:16-23`).
            Self::ApiKey => request
                .api_key()
                .map(|api_key| format!("api_key:{api_key}")),
            // `f"throttle_asset_{asset_id}"` (`asset.py:10-15`).
            Self::Asset => request
                .asset_id
                .map(|asset_id| format!("throttle_asset_{asset_id}")),
        }
    }

    /// `ApiKeyRateThrottle.allow_request` is the only throttle that writes
    /// `X-RateLimit-*` into `request.META`, from where
    /// `BaseAPIView.finalize_response` copies them onto every response
    /// (`plane/api/views/base.py:123-136`).
    pub fn emits_rate_limit_headers(self) -> bool {
        matches!(self, Self::ApiKey)
    }
}

/// `request.user.pk if request.user.is_authenticated else get_ident(request)`.
fn user_ident(request: &ThrottleRequest<'_>) -> String {
    if request.user.is_authenticated()
        && let Some(pk) = request.user.pk()
    {
        return pk.to_string();
    }
    request.ident()
}

/// `X-Forwarded-For` from the header map, as Django's `request.headers` reads
/// it (the first value, undecoded).
pub fn x_forwarded_for(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
}

/// `BaseThrottle.get_ident` for a request, with Python's `None` formatting.
pub fn ident_for(
    headers: &HeaderMap,
    remote_addr: Option<&str>,
    num_proxies: Option<u16>,
) -> String {
    throttle_ident(x_forwarded_for(headers), remote_addr, num_proxies)
        // `cache_format % {"ident": None}` renders `"None"`.
        .unwrap_or_else(|| "None".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::throttle::StaticUser;
    use uuid::Uuid;

    fn settings() -> ThrottlingSettings {
        ThrottlingSettings {
            api_key_rate_limit: "60/minute".to_string(),
            authentication_rate_limit: "10/minute".to_string(),
            num_proxies: None,
            anon_rate: "30/minute",
            asset_rate: "5/minute",
            email_verification_rate: "3/hour",
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                http::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                http::header::HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[test]
    fn dotted_paths_round_trip() {
        for class in ThrottleClass::ALL {
            assert_eq!(ThrottleClass::from_dotted(class.dotted()), Some(class));
        }
        assert_eq!(
            ThrottleClass::from_dotted("plane.authentication.rate_limit.AuthenticationThrottle"),
            Some(ThrottleClass::Authentication)
        );
        assert_eq!(
            ThrottleClass::from_dotted("rest_framework.throttling.ScopedRateThrottle"),
            None
        );
        assert_eq!(ThrottleClass::from_dotted(""), None);
    }

    #[test]
    fn rates_follow_the_settings() {
        let settings = settings();
        assert_eq!(
            ThrottleClass::Anon.rate(&settings).unwrap().num_requests,
            30
        );
        assert_eq!(
            ThrottleClass::Authentication
                .rate(&settings)
                .unwrap()
                .num_requests,
            10
        );
        assert_eq!(
            ThrottleClass::EmailVerification
                .rate(&settings)
                .unwrap()
                .duration,
            3600
        );
        assert_eq!(
            ThrottleClass::ApiKey.rate(&settings).unwrap().num_requests,
            60
        );
        assert_eq!(
            ThrottleClass::Asset.rate(&settings).unwrap().num_requests,
            5
        );
        assert!(matches!(
            ThrottleClass::User.rate(&settings),
            Err(Error::MissingRate("user"))
        ));
    }

    #[test]
    fn keys_match_the_contract_table() {
        // HTTP_CONTRACT §12.3.
        let settings = settings();
        let anonymous = StaticUser::anonymous();
        let map = headers(&[("x-forwarded-for", "1.1.1.1, 2.2.2.2")]);
        let request = ThrottleRequest {
            settings: &settings,
            user: &anonymous,
            headers: &map,
            remote_addr: Some("10.0.0.1"),
            asset_id: Some("a1b2"),
            scope_override: None,
        };
        assert_eq!(
            ThrottleClass::Anon.cache_key(&request).as_deref(),
            Some("throttle_anon_1.1.1.1,2.2.2.2")
        );
        assert_eq!(
            ThrottleClass::Authentication.cache_key(&request).as_deref(),
            Some("throttle_authentication_1.1.1.1,2.2.2.2")
        );
        assert_eq!(
            ThrottleClass::EmailVerification
                .cache_key(&request)
                .as_deref(),
            Some("throttle_email_verification_1.1.1.1,2.2.2.2")
        );
        assert_eq!(
            ThrottleClass::Asset.cache_key(&request).as_deref(),
            Some("throttle_asset_a1b2")
        );
        assert_eq!(ThrottleClass::ApiKey.cache_key(&request), None);

        // An authenticated caller: Anon skips, User/EmailVerification use pk.
        let pk = Uuid::parse_str("8f1c0d3e-0000-0000-0000-000000000001").unwrap();
        let authenticated = StaticUser::authenticated(pk);
        let request = ThrottleRequest {
            settings: &settings,
            user: &authenticated,
            headers: &map,
            remote_addr: Some("10.0.0.1"),
            asset_id: None,
            scope_override: None,
        };
        assert_eq!(ThrottleClass::Anon.cache_key(&request), None);
        assert_eq!(
            ThrottleClass::EmailVerification
                .cache_key(&request)
                .as_deref(),
            Some("throttle_email_verification_8f1c0d3e-0000-0000-0000-000000000001")
        );
        assert_eq!(
            ThrottleClass::Authentication.cache_key(&request).as_deref(),
            Some("throttle_authentication_1.1.1.1,2.2.2.2")
        );
        // No asset id: not throttled at all (`asset.py:12-13`).
        assert_eq!(ThrottleClass::Asset.cache_key(&request), None);
    }

    #[test]
    fn api_key_becomes_part_of_the_key() {
        let settings = settings();
        let anonymous = StaticUser::anonymous();
        let map = headers(&[("x-api-key", "plane_api_abc123")]);
        let request = ThrottleRequest {
            settings: &settings,
            user: &anonymous,
            headers: &map,
            remote_addr: None,
            asset_id: None,
            scope_override: None,
        };
        assert_eq!(
            ThrottleClass::ApiKey.cache_key(&request).as_deref(),
            Some("api_key:plane_api_abc123")
        );
    }

    #[test]
    fn ident_without_any_address_is_python_none() {
        let map = HeaderMap::new();
        assert_eq!(ident_for(&map, None, None), "None");
        assert_eq!(ident_for(&map, Some("203.0.113.7"), None), "203.0.113.7");
    }
}
