// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/provider/credentials/magic_code.py`: the magic-code
//! state in Redis and the [`AuthProvider`] it feeds.
//!
//! # The Redis keys
//!
//! The keys are **unprefixed**: `redis_instance()`
//! (`plane/settings/redis.py:8-16`) is a raw `redis-py` client, not
//! django-redis, so nothing adds the `:1:` namespace. Everything lives in the
//! database `REDIS_URL` selects.
//!
//! | Key | Value | TTL |
//! | --- | --- | --- |
//! | `magic_<email>` | `{"current_attempt": 0, "email": "<email>", "token": "<code>"}` | `SET … EX 600` |
//! | `magic_<email>:issued` | the issued-code counter | `EXPIRE 600` on the first `INCR` only |
//! | `magic_<email>:verify_attempts` | the wrong-code counter | clamped to the token's remaining TTL |
//!
//! The two counters go through one Lua script
//! ([`plane_cache::lua::INCR_EXPIRE_ON_FIRST`]) so concurrent requests cannot
//! exceed a cap between a read and a write (`magic_code.py:36-46`).
//!
//! # Behaviour that looks wrong and is ported as-is
//!
//! * The `SMTP_NOT_CONFIGURED` / `MAGIC_LINK_LOGIN_DISABLED` errors carry
//!   `{"email": self.key}` — which is `magic_<email>` on the verify path,
//!   because `views/app/magic.py:126` constructs the provider with
//!   `key=f"magic_{email}"` (`magic_code.py:71,78`).
//! * `set_user_data` counts the attempt **before** comparing, and reads the
//!   token **after** counting, so a token deleted in between is reported as
//!   expired rather than compared (`magic_code.py:138-155`).
//! * The fifth wrong code is `…_ATTEMPT_EXHAUSTED`, not `INVALID_MAGIC_CODE`:
//!   `exhausted` is recomputed after a failed compare (`:173`). It also
//!   deletes the token and the verify counter but **not** the issued counter,
//!   so regeneration stays capped (`:175-179`).
//! * The verify counter's TTL is clamped to `>= 1`: `EXPIRE key 0` deletes the
//!   key immediately, which would reset the count (`:146-149`).
//! * A successful verify deletes all three keys in one `DEL`
//!   (`:171`), so a later generate starts from a fresh issue window.

use deadpool_postgres::Client;
use plane_cache::Redis;
use plane_serde::Dialect;
use serde::Serialize;
use serde_json::Value;

use crate::auth_flow::adapter::{AdapterError, AuthProvider, python_str};
use crate::auth_flow::error::{AuthErrorCode, AuthFlowError};

/// `MAX_VERIFY_ATTEMPTS` (`magic_code.py:28`): wrong codes compared against
/// one token before it is invalidated.
pub const MAX_VERIFY_ATTEMPTS: i64 = 5;
/// `MAX_CODES_PER_WINDOW` (`magic_code.py:33`): the first code plus three
/// regenerations.
pub const MAX_CODES_PER_WINDOW: i64 = 4;
/// `ISSUE_WINDOW_SECONDS` (`magic_code.py:34`): the window starts at the first
/// code and is not extended by later requests.
pub const ISSUE_WINDOW_SECONDS: i64 = 600;
/// The token record's TTL (`magic_code.py:117`).
pub const TOKEN_TTL_SECONDS: i64 = 600;

/// The two instance-configuration values the provider reads in `__init__`
/// (`magic_code.py:57-79`), resolved by the caller (`AUTH_FLOWS.md` §2i).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MagicCodeFlags {
    /// `EMAIL_HOST` non-empty. An empty value is `SMTP_NOT_CONFIGURED`.
    pub smtp_configured: bool,
    /// `ENABLE_MAGIC_LINK_LOGIN != "0"`.
    pub magic_link_login_enabled: bool,
}

impl MagicCodeFlags {
    /// The defaults the Python reads from the environment when no
    /// `instance_configurations` row exists: `EMAIL_HOST` unset (falsy) and
    /// `ENABLE_MAGIC_LINK_LOGIN` defaulting to `"1"` (`magic_code.py:59-63`).
    #[must_use]
    pub fn from_environment() -> Self {
        Self {
            smtp_configured: std::env::var("EMAIL_HOST")
                .map(|value| !value.is_empty())
                .unwrap_or(false),
            magic_link_login_enabled: std::env::var("ENABLE_MAGIC_LINK_LOGIN")
                .map(|value| value != "0")
                .unwrap_or(true),
        }
    }
}

/// `key = "magic_" + str(email)` (`magic_code.py:91`).
#[must_use]
pub fn magic_key(email: &str) -> String {
    format!("magic_{email}")
}

/// `MagicCodeProvider._issued_key` (`magic_code.py:52-54`).
#[must_use]
pub fn issued_key(token_key: &str) -> String {
    format!("{token_key}:issued")
}

/// `MagicCodeProvider._verify_attempts_key` (`magic_code.py:48-50`).
#[must_use]
pub fn verify_attempts_key(token_key: &str) -> String {
    format!("{token_key}:verify_attempts")
}

/// `initiate()`'s return value: `(key, token)`.
///
/// The `Debug` implementation redacts the code, because it is the login
/// credential for the next ten minutes.
#[derive(Clone, PartialEq, Eq)]
pub struct IssuedCode {
    /// `magic_<email>`.
    pub key: String,
    /// The six-digit code, as decimal text (`str(int)`).
    pub token: String,
}

impl std::fmt::Debug for IssuedCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IssuedCode")
            .field("key", &self.key)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// `secrets.randbelow(n)`: uniform in `0..n`.
///
/// `secrets` delegates to `SystemRandom`, i.e. `os.urandom`; the rejection
/// sampling here has the same uniform distribution (the exact bits differ,
/// which no caller can observe).
fn randbelow(bound: u64) -> u64 {
    assert!(bound > 0, "randbelow needs a positive bound");
    let modulus = u64::from(u32::MAX) + 1;
    let limit = modulus - (modulus % bound);
    loop {
        let mut bytes = [0u8; 4];
        if let Err(error) = getrandom::fill(&mut bytes) {
            panic!("the OS random number generator failed: {error}");
        }
        let value = u64::from(u32::from_ne_bytes(bytes));
        if value < limit {
            return value % bound;
        }
    }
}

/// `str(secrets.randbelow(900000) + 100000)` (`magic_code.py:87`): six digits,
/// no leading-zero bias because the range is `100000..=999999`.
#[must_use]
pub fn generate_code() -> String {
    (randbelow(900_000) + 100_000).to_string()
}

/// The `json.dumps` record written to `magic_<email>`
/// (`magic_code.py:115-117`).
#[derive(Debug, Serialize)]
struct MagicRecord<'a> {
    current_attempt: u8,
    email: &'a str,
    token: &'a str,
}

/// `MagicCodeProvider`. Construct it with the key the caller holds — the email
/// for `initiate`, `magic_<email>` for verification — and the resolved flags.
#[derive(Debug)]
pub struct MagicCodeProvider {
    /// `self.key`.
    key: String,
    /// `self.code`: the code the user typed, on the verify path.
    code: Option<String>,
    /// `self.user_data`; `None` until `set_user_data` runs, exactly like
    /// Python's `None`.
    user_data: Option<Value>,
}

impl MagicCodeProvider {
    /// `MagicCodeProvider.__init__` (`magic_code.py:56-83`), including both
    /// configuration rejections.
    ///
    /// # Errors
    ///
    /// [`AuthErrorCode::SmtpNotConfigured`] or
    /// [`AuthErrorCode::MagicLinkLoginDisabled`], both with the raw `key` in
    /// their payload.
    pub fn new(
        key: impl Into<String>,
        code: Option<String>,
        flags: MagicCodeFlags,
    ) -> Result<Self, AdapterError> {
        let key = key.into();
        if !flags.smtp_configured {
            return Err(AdapterError::Auth(AuthFlowError::with_email(
                AuthErrorCode::SmtpNotConfigured,
                key,
            )));
        }
        if !flags.magic_link_login_enabled {
            return Err(AdapterError::Auth(AuthFlowError::with_email(
                AuthErrorCode::MagicLinkLoginDisabled,
                key,
            )));
        }
        Ok(Self {
            key,
            code,
            user_data: None,
        })
    }

    /// `self.key`.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// `initiate()` (`magic_code.py:85-121`).
    ///
    /// The issued counter is incremented **before** the existence lookup, so a
    /// rejected request still consumes one of the four codes and a database
    /// failure leaves the counter incremented.
    ///
    /// # Errors
    ///
    /// The exhaustion error (`…_SIGN_IN` / `…_SIGN_UP`, from the `users`
    /// lookup), or a Redis/database failure.
    pub async fn initiate(
        &self,
        client: &Client,
        redis: &Redis,
    ) -> Result<IssuedCode, AdapterError> {
        let token = generate_code();
        let key = magic_key(&self.key);

        let issued = redis
            .incr_expire_on_first(&issued_key(&key), ISSUE_WINDOW_SECONDS)
            .await?;
        if issued > MAX_CODES_PER_WINDOW {
            // `email = str(self.key)` — the raw key, *not* the
            // `replace("magic_", …)` form `set_user_data` uses (`:102`).
            return Err(self
                .sign_variant_error(client, &self.key, "EMAIL_CODE_ATTEMPT_EXHAUSTED")
                .await?
                .into());
        }

        let record = MagicRecord {
            current_attempt: 0,
            email: &self.key,
            token: &token,
        };
        // `json.dumps` with its defaults: `", "` / `": "` separators and
        // `ensure_ascii=True`.
        let value =
            plane_serde::to_vec(Dialect::Django, &record).map_err(AdapterError::internal)?;
        redis.set_bytes_ex(&key, &value, TOKEN_TTL_SECONDS).await?;
        // "Reset the verify-attempt counter so each newly issued token starts
        // with a fresh budget" (`magic_code.py:118-120`).
        redis.del(&verify_attempts_key(&key)).await?;
        Ok(IssuedCode { key, token })
    }

    /// `set_user_data()` (`magic_code.py:134-184`): the verify path.
    ///
    /// # Errors
    ///
    /// `EXPIRED_MAGIC_CODE`, `INVALID_MAGIC_CODE` or
    /// `EMAIL_CODE_ATTEMPT_EXHAUSTED`, each in its `_SIGN_IN`/`_SIGN_UP`
    /// variant; a Redis failure or a malformed stored record is a 500.
    pub async fn set_user_data(
        &mut self,
        client: &Client,
        redis: &Redis,
    ) -> Result<(), AdapterError> {
        // `email = str(self.key).replace("magic_", "", 1)` (`:136`).
        let email = self.email_for_variant();
        let attempts_key = verify_attempts_key(&self.key);
        if !redis.exists(&self.key).await? {
            return Err(self
                .sign_variant_error(client, &email, "EXPIRED_MAGIC_CODE")
                .await?
                .into());
        }

        // "Count the attempt before comparing, so at most MAX_VERIFY_ATTEMPTS
        // codes are compared against a token, including concurrent ones."
        let remaining_ttl = redis.ttl_seconds(&self.key).await?;
        let remaining_ttl = if remaining_ttl <= 0 { 1 } else { remaining_ttl };
        let verify_attempts = redis
            .incr_expire_on_first(&attempts_key, remaining_ttl)
            .await?;

        let raw = redis.get_bytes(&self.key).await?;
        let mut exhausted = verify_attempts > MAX_VERIFY_ATTEMPTS;
        if let Some(raw) = &raw
            && !exhausted
        {
            let data: Value = serde_json::from_slice(raw).map_err(|error| {
                AdapterError::Internal(format!("json.decoder.JSONDecodeError: {error}"))
            })?;
            // `str(data["token"]) == str(self.code)`: a missing key is a
            // `KeyError` (a 500), and `str(None)` is `"None"`.
            let stored = data
                .get("token")
                .ok_or_else(|| AdapterError::Internal("KeyError: 'token'".to_string()))?;
            let matches = python_str(stored) == python_str(&code_value(self.code.as_deref()));
            if matches {
                let record_email = data.get("email").cloned().unwrap_or(Value::Null);
                self.user_data = Some(magic_user_data(record_email));
                // "Delete the token and its counters from redis on success."
                redis
                    .del_many(&[self.key.clone(), attempts_key, issued_key(&self.key)])
                    .await?;
                return Ok(());
            }
            exhausted = verify_attempts == MAX_VERIFY_ATTEMPTS;
        }

        if exhausted {
            // "Invalidate the token and its verify counter in one DEL … the
            // separate issued-code counter … is left in place."
            redis.del_many(&[self.key.clone(), attempts_key]).await?;
            return Err(self
                .sign_variant_error(client, &email, "EMAIL_CODE_ATTEMPT_EXHAUSTED")
                .await?
                .into());
        }
        if raw.is_some() {
            return Err(self
                .sign_variant_error(client, &email, "INVALID_MAGIC_CODE")
                .await?
                .into());
        }
        Err(self
            .sign_variant_error(client, &email, "EXPIRED_MAGIC_CODE")
            .await?
            .into())
    }

    /// `_raise_for_email` (`magic_code.py:123-132`): append `_SIGN_IN` when
    /// the address has a `users` row, `_SIGN_UP` otherwise.
    async fn sign_variant_error(
        &self,
        client: &Client,
        email: &str,
        error: &str,
    ) -> Result<AuthFlowError, AdapterError> {
        // The lookup happens before the error is built, so a database failure
        // is the 500 and never a guessed variant.
        let statement = crate::queries::USER_EXISTS_BY_EMAIL.prepare(client).await?;
        let row = client.query_one(&statement, &[&email]).await?;
        let exists: bool = row.get(0);
        let message = format!("{error}_{}", if exists { "SIGN_IN" } else { "SIGN_UP" });
        let code = AuthErrorCode::from_message(&message)
            .ok_or_else(|| AdapterError::Internal(format!("unknown auth error code {message}")))?;
        Ok(AuthFlowError::with_email(code, email))
    }

    /// `str(self.key).replace("magic_", "", 1)` (`magic_code.py:136`): the
    /// **first** occurrence anywhere, not a prefix strip, so an address whose
    /// local part contains `magic_` loses that text too.
    fn email_for_variant(&self) -> String {
        self.key.replacen("magic_", "", 1)
    }
}

/// `str(self.code)` where `self.code` is `None` on the generate path.
fn code_value(code: Option<&str>) -> Value {
    match code {
        Some(code) => Value::String(code.to_string()),
        None => Value::Null,
    }
}

/// `super().set_user_data({...})`'s payload (`magic_code.py:158-168`).
fn magic_user_data(email: Value) -> Value {
    serde_json::json!({
        "email": email,
        "user": {
            "avatar": "",
            "first_name": "",
            "last_name": "",
            "provider_id": "",
            "is_password_autoset": true,
        },
    })
}

impl AuthProvider for MagicCodeProvider {
    fn provider(&self) -> &'static str {
        "magic-code"
    }

    fn user_data(&self) -> &Value {
        // Python's `self.user_data` starts as `None`; `authenticate()` always
        // calls `set_user_data` first, and a caller that did not would be
        // Python's `AttributeError`, which the 500 path reports.
        self.user_data.as_ref().unwrap_or(&Value::Null)
    }

    fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_the_python_f_strings() {
        assert_eq!(magic_key("a@b.com"), "magic_a@b.com");
        assert_eq!(issued_key("magic_a@b.com"), "magic_a@b.com:issued");
        assert_eq!(
            verify_attempts_key("magic_a@b.com"),
            "magic_a@b.com:verify_attempts"
        );
    }

    #[test]
    fn the_record_is_json_dumps_output() {
        let record = MagicRecord {
            current_attempt: 0,
            email: "a@b.com",
            token: "123456",
        };
        let bytes = plane_serde::to_vec(Dialect::Django, &record).expect("json");
        assert_eq!(
            String::from_utf8(bytes).expect("utf-8"),
            r#"{"current_attempt": 0, "email": "a@b.com", "token": "123456"}"#
        );
        // `ensure_ascii=True`: a non-ASCII address is escaped.
        let record = MagicRecord {
            current_attempt: 0,
            email: "café@exämple.com",
            token: "100000",
        };
        let bytes = plane_serde::to_vec(Dialect::Django, &record).expect("json");
        assert_eq!(
            String::from_utf8(bytes).expect("utf-8"),
            r#"{"current_attempt": 0, "email": "caf\u00e9@ex\u00e4mple.com", "token": "100000"}"#
        );
    }

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..200 {
            let code = generate_code();
            assert_eq!(code.len(), 6, "{code}");
            let value: u32 = code.parse().expect("digits");
            assert!((100_000..=999_999).contains(&value), "{code}");
        }
    }

    #[test]
    fn randbelow_is_uniform_enough() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            seen.insert(randbelow(10));
        }
        assert_eq!(seen.len(), 10);
        assert_eq!(randbelow(1), 0);
    }

    #[test]
    fn the_constructor_reports_the_raw_key() {
        let error = MagicCodeProvider::new(
            "magic_a@b.com",
            None,
            MagicCodeFlags {
                smtp_configured: false,
                magic_link_login_enabled: true,
            },
        )
        .expect_err("no smtp");
        let AdapterError::Auth(error) = error else {
            panic!("not an auth error");
        };
        assert_eq!(error.code, AuthErrorCode::SmtpNotConfigured);
        assert_eq!(
            error.payload,
            vec![(
                "email".to_string(),
                Value::String("magic_a@b.com".to_string())
            )]
        );

        let error = MagicCodeProvider::new(
            "a@b.com",
            None,
            MagicCodeFlags {
                smtp_configured: true,
                magic_link_login_enabled: false,
            },
        )
        .expect_err("disabled");
        let AdapterError::Auth(error) = error else {
            panic!("not an auth error");
        };
        assert_eq!(error.code, AuthErrorCode::MagicLinkLoginDisabled);
    }

    #[test]
    fn the_sign_variant_names_exist() {
        for name in [
            "EXPIRED_MAGIC_CODE_SIGN_IN",
            "EXPIRED_MAGIC_CODE_SIGN_UP",
            "INVALID_MAGIC_CODE_SIGN_IN",
            "INVALID_MAGIC_CODE_SIGN_UP",
            "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN",
            "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP",
        ] {
            assert!(AuthErrorCode::from_message(name).is_some(), "{name}");
        }
    }
}
