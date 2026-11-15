// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `SimpleRateThrottle.parse_rate` and `wait()`, ported line by line from
//! `rest_framework/throttling.py` 3.18.3.

use crate::error::RateError;
use plane_core::config::ThrottlingSettings;
use plane_core::py;

/// A parsed `"<n>/<period>"` rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    /// `num_requests`.
    pub num_requests: i64,
    /// `duration`, in seconds.
    pub duration: i64,
}

/// `SimpleRateThrottle.parse_rate` (`throttling.py:76-86`):
///
/// ```python
/// if rate is None:
///     return (None, None)
/// num, period = rate.split('/')
/// num_requests = int(num)
/// duration = {'s': 1, 'm': 60, 'h': 3600, 'd': 86400}[period[0]]
/// ```
///
/// Only the first character of the period is significant, so `"10/minute"`,
/// `"10/m"` and `"10/monkeys"` are the same rate. `rate=None` means "no
/// throttling" and is reported as `Ok(None)`.
pub fn parse_rate(rate: Option<&str>) -> Result<Option<Rate>, RateError> {
    let Some(rate) = rate else {
        return Ok(None);
    };
    let parts: Vec<&str> = rate.split('/').collect();
    let (num, period) = match parts.as_slice() {
        [num, period] => (*num, *period),
        [_] => return Err(RateError::TooFewParts(1)),
        _ => return Err(RateError::TooManyParts),
    };
    // `int(num)`; Python accepts surrounding whitespace and Unicode digits.
    let num_requests = py::int(num).map_err(|_| RateError::NotAnInteger(num.to_string()))?;
    let Some(first) = period.chars().next() else {
        return Err(RateError::EmptyPeriod);
    };
    let duration = match first {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        other => return Err(RateError::UnknownPeriod(other)),
    };
    Ok(Some(Rate {
        num_requests,
        duration,
    }))
}

/// `SimpleRateThrottle.wait` (`throttling.py:149-162`):
///
/// ```python
/// if self.history:
///     remaining_duration = self.duration - (self.now - self.history[-1])
/// else:
///     remaining_duration = self.duration
/// available_requests = self.num_requests - len(self.history) + 1
/// if available_requests <= 0:
///     return None
/// return remaining_duration / float(available_requests)
/// ```
///
/// `history` is newest first, so `history[-1]` is the oldest retained request.
/// `None` means "no estimate", which `check_throttles` filters out before
/// taking the maximum.
pub fn wait(history: &[f64], now: f64, rate: Rate) -> Option<f64> {
    let remaining_duration = match history.last() {
        Some(oldest) => rate.duration as f64 - (now - *oldest),
        None => rate.duration as f64,
    };
    let available_requests = rate.num_requests - history.len() as i64 + 1;
    if available_requests <= 0 {
        return None;
    }
    Some(remaining_duration / available_requests as f64)
}

/// `SimpleRateThrottle.timer`: `time.time`, as `f64` seconds since the epoch.
///
/// The throttle code takes `now` as a parameter so tests can freeze it; this is
/// what a live request passes.
pub fn now() -> f64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_secs_f64(),
        // Before 1970 is not a real deployment; `time.time()` would be negative
        // too, and a negative `now` only makes every entry look expired.
        Err(error) => -error.duration().as_secs_f64(),
    }
}

/// The per-scope rate strings, as Django resolves them.
///
/// `SimpleRateThrottle.__init__` keeps a class-level `rate` when it is truthy
/// and otherwise reads `DEFAULT_THROTTLE_RATES[scope]`
/// (`throttling.py:56-75`); a missing entry is `ImproperlyConfigured`. Plane
/// sets three of the five rates in code and two from the environment
/// (`plane/settings/common.py:141-145,162`;
/// `plane/authentication/rate_limit.py:29,101`).
pub fn rate_string<'a>(
    scope: &'static str,
    class_rate: Option<&'a str>,
    settings: &'a ThrottlingSettings,
) -> Result<&'a str, crate::Error> {
    if let Some(rate) = class_rate
        && !rate.is_empty()
    {
        return Ok(rate);
    }
    let configured = match scope {
        // `DEFAULT_THROTTLE_RATES` (`common.py:142-145`).
        "anon" => Some(settings.anon_rate),
        "asset_id" => Some(settings.asset_rate),
        // Hard-coded on the class (`rate_limit.py:101`).
        "email_verification" => Some(settings.email_verification_rate),
        // `get_rate()` would look these up in `DEFAULT_THROTTLE_RATES`, which
        // Plane does not define for them: `ImproperlyConfigured`.
        _ => None,
    };
    match configured {
        Some(rate) => Ok(rate),
        None => Err(crate::Error::MissingRate(scope)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(rate: &str) -> Rate {
        parse_rate(Some(rate)).unwrap().unwrap()
    }

    #[test]
    fn parses_the_rates_plane_uses() {
        // Probe A.3.
        assert_eq!(
            rate("30/minute"),
            Rate {
                num_requests: 30,
                duration: 60
            }
        );
        assert_eq!(
            rate("5/minute"),
            Rate {
                num_requests: 5,
                duration: 60
            }
        );
        assert_eq!(
            rate("10/minute"),
            Rate {
                num_requests: 10,
                duration: 60
            }
        );
        assert_eq!(
            rate("3/hour"),
            Rate {
                num_requests: 3,
                duration: 3600
            }
        );
        assert_eq!(
            rate("60/minute"),
            Rate {
                num_requests: 60,
                duration: 60
            }
        );
    }

    #[test]
    fn only_the_first_period_letter_matters() {
        // `{'s': 1, 'm': 60, 'h': 3600, 'd': 86400}[period[0]]`.
        assert_eq!(
            rate("7/s"),
            Rate {
                num_requests: 7,
                duration: 1
            }
        );
        assert_eq!(
            rate("7/sec"),
            Rate {
                num_requests: 7,
                duration: 1
            }
        );
        assert_eq!(
            rate("7/monkeys"),
            Rate {
                num_requests: 7,
                duration: 60
            }
        );
        assert_eq!(
            rate("7/day"),
            Rate {
                num_requests: 7,
                duration: 86400
            }
        );
        // `int()` tolerates whitespace and signs, like Python.
        assert_eq!(
            rate(" 12 /minutes"),
            Rate {
                num_requests: 12,
                duration: 60
            }
        );
    }

    #[test]
    fn rejects_malformed_rates_like_drf() {
        assert_eq!(parse_rate(Some("")), Err(RateError::TooFewParts(1)));
        assert_eq!(parse_rate(Some("10")), Err(RateError::TooFewParts(1)));
        assert_eq!(
            parse_rate(Some("10/minute/x")),
            Err(RateError::TooManyParts)
        );
        assert_eq!(
            parse_rate(Some("ten/minute")),
            Err(RateError::NotAnInteger("ten".to_string()))
        );
        assert_eq!(parse_rate(Some("10/")), Err(RateError::EmptyPeriod));
        assert_eq!(
            parse_rate(Some("10/year")),
            Err(RateError::UnknownPeriod('y'))
        );
        // Case matters: `"10/Minute"` is a KeyError in Python.
        assert_eq!(
            parse_rate(Some("10/Minute")),
            Err(RateError::UnknownPeriod('M'))
        );
        assert_eq!(parse_rate(None), Ok(None));
    }

    #[test]
    fn wait_matches_the_drf_formula() {
        let anon = Rate {
            num_requests: 30,
            duration: 60,
        };
        // 30 requests in the window: one has to expire before there is room.
        let full: Vec<f64> = (0..30).map(|index| 1_000.0 - index as f64).collect();
        // remaining = 60 - (1000 - 971) = 31; available = 30 - 30 + 1 = 1.
        assert_eq!(wait(&full, 1_000.0, anon), Some(31.0));

        // Empty history: the whole duration, over `num_requests + 1`.
        assert_eq!(wait(&[], 1_000.0, anon), Some(60.0 / 31.0));

        // Fewer than the limit: `available_requests` grows, so the estimate
        // shrinks (this is the term that is easy to drop).
        let two = vec![1_000.0, 999.0];
        assert_eq!(wait(&two, 1_000.0, anon), Some((60.0 - 1.0) / 29.0));

        // `available_requests <= 0` cannot happen through `allow_request`
        // (a failure needs `len(history) >= num_requests`, and then
        // `available_requests == 1`), but `wait()` returns None for it.
        let over: Vec<f64> = (0..40).map(|_| 1_000.0).collect();
        assert_eq!(wait(&over, 1_000.0, anon), None);

        // A history older than the window is not special-cased here: the
        // caller pops those entries first. Negative values are possible and
        // `math.ceil` then rounds toward zero.
        assert_eq!(wait(&[1_000.0], 1_060.0, anon), Some(0.0));
        assert!(wait(&[1_000.0], 1_061.0, anon).unwrap() < 0.0);
    }

    #[test]
    fn rate_strings_resolve_like_django() {
        let settings = ThrottlingSettings {
            api_key_rate_limit: "60/minute".to_string(),
            authentication_rate_limit: "10/minute".to_string(),
            num_proxies: None,
            anon_rate: "30/minute",
            asset_rate: "5/minute",
            email_verification_rate: "3/hour",
        };
        assert_eq!(rate_string("anon", None, &settings).ok(), Some("30/minute"));
        assert_eq!(
            rate_string("authentication", Some("10/minute"), &settings).ok(),
            Some("10/minute")
        );
        // An empty class rate is falsy, so DRF calls `get_rate()`; Plane has no
        // `api_key` entry, so `ImproperlyConfigured` (ENV.md §1.10).
        assert!(matches!(
            rate_string("api_key", Some(""), &settings),
            Err(crate::Error::MissingRate("api_key"))
        ));
        // `UserRateThrottle` is only used as `EmailVerificationThrottle`'s base
        // class, and `DEFAULT_THROTTLE_RATES` has no `user` entry.
        assert!(matches!(
            rate_string("user", None, &settings),
            Err(crate::Error::MissingRate("user"))
        ));
    }

    #[test]
    fn now_is_seconds_since_the_epoch() {
        // 2026-11-15T00:00:00Z, to show the unit (not milliseconds).
        assert!(now() > 1_763_000_000.0);
        assert!(now() < 2_000_000_000.0);
    }
}
