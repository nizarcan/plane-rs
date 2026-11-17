// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django 5.2.17's `PasswordResetTokenGenerator` and the `urlsafe_base64`
//! helpers the reset routes pair it with
//! (`plane/authentication/views/{app,space}/password_management.py:38-42,
//! 104-105`).
//!
//! ```text
//! ts       = int((datetime.now() - datetime(2001, 1, 1)).total_seconds())
//! ts_b36   = int_to_base36(ts)
//! hash     = salted_hmac("django.contrib.auth.tokens.PasswordResetTokenGenerator",
//!                        f"{pk}{password}{login:%Y-%m-%d %H:%M:%S}{ts}{email}",
//!                        SECRET_KEY).hexdigest()[::2]
//! token    = f"{ts_b36}-{hash}"
//! valid    = now - ts <= PASSWORD_RESET_TIMEOUT (259200 s)
//! ```
//!
//! `datetime.now()` is **naive local time** (`tokens.py:120-122`); the process
//! timezone is UTC (`TIME_ZONE = "UTC"`), so [`num_seconds`] takes a UTC
//! instant. `last_login` is an aware UTC datetime in the database and
//! `_make_hash_value` replaces its tzinfo before formatting, i.e. it formats
//! the UTC wall clock.
//!
//! `tests/golden/reset_tokens.json` pins 2 268 tokens and 108 `check_token`
//! answers, minted by the real generator with `_now` monkey-patched and the
//! real `urlsafe_base64_decode` for its success/`ValueError` split.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};

use crate::signing::salted_hmac_hexdigest;

/// `PasswordResetTokenGenerator.key_salt` (`tokens.py:11`).
pub const KEY_SALT: &str = "django.contrib.auth.tokens.PasswordResetTokenGenerator";
/// `settings.PASSWORD_RESET_TIMEOUT` (`django/conf/global_settings.py:539`):
/// three days.
pub const PASSWORD_RESET_TIMEOUT: i64 = 60 * 60 * 24 * 3;
/// `datetime(2001, 1, 1)`, the token epoch (`tokens.py:116`).
pub const TOKEN_EPOCH_YEAR: i32 = 2001;

/// A `users` row as `_make_hash_value` reads it.
///
/// `pk` is `str(user.pk)`; `email` is `getattr(user, "email", "") or ""`, so
/// `None` and `""` are the same empty string.
#[derive(Debug, Clone, Copy)]
pub struct TokenUser<'a> {
    pub pk: &'a str,
    pub password: &'a str,
    pub email: Option<&'a str>,
    pub last_login: Option<DateTime<Utc>>,
}

impl TokenUser<'_> {
    /// `_make_hash_value(user, timestamp)` (`tokens.py:100-118`).
    fn hash_value(&self, timestamp: i64) -> String {
        let login = match self.last_login {
            None => String::new(),
            Some(value) => value.format("%Y-%m-%d %H:%M:%S").to_string(),
        };
        format!(
            "{}{}{}{}{}",
            self.pk,
            self.password,
            login,
            timestamp,
            self.email.unwrap_or("")
        )
    }
}

/// `_num_seconds(dt)` (`tokens.py:116-117`): whole seconds since 2001-01-01.
///
/// `int(total_seconds())` truncates toward zero; all instants this is called
/// with are after the epoch, so `TimeDelta::num_seconds` (also truncating)
/// agrees.
#[must_use]
pub fn num_seconds(now: DateTime<Utc>) -> i64 {
    // 2001-01-01T00:00:00Z as a Unix timestamp; `from_timestamp` cannot fail
    // for a constant in range, and the fallback keeps the function total.
    let epoch = DateTime::from_timestamp(978_307_200, 0).unwrap_or_default();
    (now - epoch).num_seconds()
}

/// `int_to_base36(i)` (`django/utils/http.py:165-176`).
///
/// Negative input is a `ValueError`; `0` is `"0"`.
///
/// # Errors
///
/// [`Base36Error::Negative`].
pub fn int_to_base36(mut value: i64) -> Result<String, Base36Error> {
    const CHARSET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value < 0 {
        return Err(Base36Error::Negative);
    }
    if value < 36 {
        return Ok(char::from(CHARSET[value as usize]).to_string());
    }
    let mut digits = Vec::new();
    while value != 0 {
        digits.push(CHARSET[(value % 36) as usize]);
        value /= 36;
    }
    digits.reverse();
    Ok(String::from_utf8(digits).unwrap_or_default())
}

/// `base36_to_int(s)` (`django/utils/http.py:152-163`): `int(s, 36)` after a
/// 13-character cap.
///
/// Python's `int()` accepts surrounding whitespace, one `+`/`-`, and
/// underscores between digits; tokens are ASCII, but rejecting exactly what
/// Python rejects keeps [`check_token`]'s answer identical.
///
/// # Errors
///
/// [`Base36Error::TooLong`] or [`Base36Error::Invalid`] (Python's `ValueError`).
pub fn base36_to_int(text: &str) -> Result<i64, Base36Error> {
    if text.chars().count() > 13 {
        return Err(Base36Error::TooLong);
    }
    let trimmed = text.trim();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    if digits.is_empty() {
        return Err(Base36Error::Invalid);
    }
    let mut value: i64 = 0;
    let mut previous_underscore = true; // a leading underscore is invalid
    for character in digits.chars() {
        if character == '_' {
            if previous_underscore {
                return Err(Base36Error::Invalid);
            }
            previous_underscore = true;
            continue;
        }
        let digit = character.to_digit(36).ok_or(Base36Error::Invalid)?;
        value = value
            .checked_mul(36)
            .and_then(|value| value.checked_add(i64::from(digit)))
            .ok_or(Base36Error::Invalid)?;
        previous_underscore = false;
    }
    if previous_underscore {
        return Err(Base36Error::Invalid);
    }
    Ok(if negative { -value } else { value })
}

/// Why a base-36 value was rejected (`ValueError` in Python).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Base36Error {
    /// `int_to_base36(-1)`.
    #[error("Negative base36 conversion input.")]
    Negative,
    /// More than 13 digits (`http.py:160-161`).
    #[error("Base36 input too large")]
    TooLong,
    /// Not a base-36 integer.
    #[error("invalid literal for int() with base 36")]
    Invalid,
}

/// `_make_token_with_timestamp(user, timestamp, secret)` (`tokens.py:78-97`).
#[must_use]
pub fn make_token_with_timestamp(user: &TokenUser<'_>, timestamp: i64, secret: &str) -> String {
    let ts_b36 = int_to_base36(timestamp).unwrap_or_default();
    let digest = salted_hmac_hexdigest(KEY_SALT, user.hash_value(timestamp).as_bytes(), secret);
    // `hexdigest()[::2]`: every second character, starting at the first.
    let shortened: String = digest.chars().step_by(2).collect();
    format!("{ts_b36}-{shortened}")
}

/// `make_token(user)` (`tokens.py:37-45`) with an explicit clock.
#[must_use]
pub fn make_token(user: &TokenUser<'_>, now: DateTime<Utc>, secret: &str) -> String {
    make_token_with_timestamp(user, num_seconds(now), secret)
}

/// `check_token(user, token)` (`tokens.py:47-76`).
///
/// `SECRET_KEY_FALLBACKS` is empty in Plane, so only `secret` is tried; the
/// comparison is constant-time. Expiry is `> PASSWORD_RESET_TIMEOUT`, not
/// `>=`: a token exactly 259 200 seconds old is still valid.
#[must_use]
pub fn check_token(user: &TokenUser<'_>, token: &str, now: DateTime<Utc>, secret: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let parts: Vec<&str> = token.split('-').collect();
    if parts.len() != 2 {
        return false;
    }
    let Ok(timestamp) = base36_to_int(parts[0]) else {
        return false;
    };
    let expected = make_token_with_timestamp(user, timestamp, secret);
    if !constant_time_eq(expected.as_bytes(), token.as_bytes()) {
        return false;
    }
    num_seconds(now) - timestamp <= PASSWORD_RESET_TIMEOUT
}

/// `constant_time_compare` (`django/utils/crypto.py:61-73`).
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    use aws_lc_rs::constant_time::verify_slices_are_equal;
    verify_slices_are_equal(left, right).is_ok()
}

/// `urlsafe_base64_encode(s)` (`django/utils/http.py:179-184`):
/// URL-safe base64 with `=`, `\r` and `\n` stripped.
#[must_use]
pub fn urlsafe_base64_encode(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

/// `urlsafe_base64_decode(s)` (`django/utils/http.py:187-196`).
///
/// Python uses `binascii.a2b_base64`, which **ignores** characters outside the
/// alphabet (`"!!"` decodes to empty) and pads to a multiple of four with
/// `ljust(len + len % 4, "=")`. A 1-mod-4 data length is a `ValueError`.
///
/// # Errors
///
/// [`Base64Error::Invalid`] where Python raises `ValueError`; the reset views
/// map it to `INVALID_PASSWORD_TOKEN`.
pub fn urlsafe_base64_decode(text: &str) -> Result<Vec<u8>, Base64Error> {
    // `a2b_base64` keeps only the alphabet and the padding character.
    let filtered: Vec<u8> = text
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'='))
        .collect();
    let mut data_end = filtered.len();
    while data_end > 0 && filtered[data_end - 1] == b'=' {
        data_end -= 1;
    }
    // `=` may only be padding: `ab=c` is "Incorrect padding" in
    // `binascii.a2b_base64`.
    if filtered[..data_end].contains(&b'=') {
        return Err(Base64Error::Invalid);
    }
    if data_end % 4 == 1 {
        return Err(Base64Error::Invalid);
    }
    // CPython's `a2b_base64` decodes a 2- or 3-character tail without the
    // padding it accepted from `ljust`, and does not require the discarded
    // trailing bits to be zero (`"ab"` is `b"i"`); the stock engine rejects
    // both, so this one is lenient.
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::URL_SAFE,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
    );
    engine
        .decode(&filtered[..data_end])
        .map_err(|_| Base64Error::Invalid)
}

/// Why `urlsafe_base64_decode` rejected a value (`ValueError` in Python).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Base64Error {
    /// Invalid padding or an invalid data length.
    #[error("Invalid base64-encoded string")]
    Invalid,
}

/// `datetime.now()` in the process timezone. Plane's `TIME_ZONE` is `UTC`
/// (`plane/settings/common.py`), which is the only reason a UTC clock is
/// correct here.
#[must_use]
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeDelta, TimeZone};

    fn user<'a>(pk: &'a str, password: &'a str, email: &'a str) -> TokenUser<'a> {
        TokenUser {
            pk,
            password,
            email: Some(email),
            last_login: None,
        }
    }

    #[test]
    fn base36_round_trips_and_caps() {
        for value in [0, 1, 35, 36, 1295, 1777777777, 2147483647] {
            let encoded = int_to_base36(value).expect("positive");
            assert_eq!(base36_to_int(&encoded), Ok(value));
        }
        assert_eq!(int_to_base36(0), Ok("0".to_string()));
        assert_eq!(int_to_base36(-1), Err(Base36Error::Negative));
        assert_eq!(base36_to_int("00000000000000"), Err(Base36Error::TooLong));
        assert_eq!(base36_to_int(""), Err(Base36Error::Invalid));
        assert_eq!(base36_to_int("_1"), Err(Base36Error::Invalid));
        assert_eq!(base36_to_int("1_"), Err(Base36Error::Invalid));
        assert_eq!(base36_to_int("1_0"), Ok(36));
        // `int(s, 36)` accepts a sign and whitespace.
        assert_eq!(base36_to_int(" 1 "), Ok(1));
        assert_eq!(base36_to_int("+1"), Ok(1));
        assert_eq!(base36_to_int("-1"), Ok(-1));
        assert_eq!(base36_to_int("Z"), Ok(35));
    }

    #[test]
    fn a_token_is_the_django_layout() {
        // Recorded from `PasswordResetTokenGenerator` with
        // SECRET_KEY="secret", pk=42, password="pbkdf2_sha256$1$salt$hash",
        // email="a@b.com", last_login=None, timestamp=1777777777.
        let token = make_token_with_timestamp(
            &user("42", "pbkdf2_sha256$1$salt$hash", "a@b.com"),
            1_777_777_777,
            "secret",
        );
        assert_eq!(token, "tefy41-efa8d8a049566b646f8fa04041c79505");
    }

    #[test]
    fn num_seconds_counts_from_2001() {
        assert_eq!(
            num_seconds(Utc.with_ymd_and_hms(2001, 1, 1, 0, 0, 0).unwrap()),
            0
        );
        assert_eq!(
            num_seconds(Utc.with_ymd_and_hms(2001, 1, 1, 0, 0, 1).unwrap()),
            1
        );
        assert_eq!(
            num_seconds(Utc.with_ymd_and_hms(2026, 11, 17, 12, 0, 0).unwrap()),
            (Utc.with_ymd_and_hms(2026, 11, 17, 12, 0, 0).unwrap()
                - Utc.with_ymd_and_hms(2001, 1, 1, 0, 0, 0).unwrap())
            .num_seconds()
        );
        // Sub-second parts truncate, they do not round.
        assert_eq!(
            num_seconds(
                Utc.with_ymd_and_hms(2001, 1, 1, 0, 0, 1).unwrap() + TimeDelta::milliseconds(999)
            ),
            1
        );
    }

    #[test]
    fn expiry_boundary_is_inclusive() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 12, 0, 0).unwrap();
        let user = TokenUser {
            last_login: Some(Utc.with_ymd_and_hms(2023, 1, 2, 3, 4, 5).unwrap()),
            ..user("42", "pbkdf2_sha256$1$salt$hash", "a@b.com")
        };
        let ts = num_seconds(now);
        let fresh = make_token_with_timestamp(&user, ts, "secret");
        assert!(check_token(&user, &fresh, now, "secret"));
        let at_limit = make_token_with_timestamp(&user, ts - PASSWORD_RESET_TIMEOUT, "secret");
        assert!(check_token(&user, &at_limit, now, "secret"));
        let expired = make_token_with_timestamp(&user, ts - PASSWORD_RESET_TIMEOUT - 1, "secret");
        assert!(!check_token(&user, &expired, now, "secret"));
        // A tampered payload, a wrong secret and a malformed token.
        assert!(!check_token(&user, &fresh, now, "other"));
        assert!(!check_token(&user, "x-y", now, "secret"));
        assert!(!check_token(&user, "", now, "secret"));
        assert!(!check_token(
            &user,
            &format!("{fresh}-extra"),
            now,
            "secret"
        ));
    }

    #[test]
    fn base64_matches_django() {
        assert_eq!(urlsafe_base64_encode(b""), "");
        assert_eq!(urlsafe_base64_encode(b"1"), "MQ");
        assert_eq!(urlsafe_base64_encode(b"42"), "NDI");
        assert_eq!(
            urlsafe_base64_encode(b"a1b2c3d4-0000-0000-0000-000000000000"),
            "YTFiMmMzZDQtMDAwMC0wMDAwLTAwMDAtMDAwMDAwMDAwMDAw"
        );
        assert_eq!(urlsafe_base64_decode(""), Ok(Vec::new()));
        assert_eq!(urlsafe_base64_decode("MQ"), Ok(b"1".to_vec()));
        assert_eq!(urlsafe_base64_decode("MQ=="), Ok(b"1".to_vec()));
        // Non-alphabet characters are ignored, as `a2b_base64` does.
        assert_eq!(urlsafe_base64_decode("!!"), Ok(Vec::new()));
        assert_eq!(urlsafe_base64_decode("a b"), Ok(b"i".to_vec()));
        // A 1-mod-4 data length and a mid-string `=` are ValueError.
        assert_eq!(urlsafe_base64_decode("a"), Err(Base64Error::Invalid));
        assert_eq!(urlsafe_base64_decode("abcde"), Err(Base64Error::Invalid));
        assert_eq!(urlsafe_base64_decode("ab=c"), Err(Base64Error::Invalid));
    }
}
