// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The e-mail-change actions of `UserEndpoint` (`plane/app/views/user/base.py`):
//!
//! | Route | Python | Handler |
//! | --- | --- | --- |
//! | `POST /api/users/me/email/generate-code/` | `generate_email_verification_code` (`:144-189`) | [`generate_code`] |
//! | `PATCH /api/users/me/email/` | `update_email` (`:191-276`) | [`update_email`] |
//!
//! Both read the body with `request.data.get(...).strip()...`, i.e. **not**
//! through a serializer: a non-mapping body or a non-string value is Python's
//! `AttributeError` (family 500), and the error literals below are the message
//! strings the view builds by hand.
//!
//! ## Redis keys
//!
//! * `email_update_issued:<address>` — `cache.add(key, 0, timeout=3600)` then
//!   `cache.incr(key)`; the counter is per **address** and the window starts at
//!   the first `add`, because `incr` never resets the TTL (`base.py:158-162`,
//!   `plans/USERS_ME.md` §2c). The cap is five per hour; the refused request
//!   still spends an increment and answers `self.throttled(request, None)`'s
//!   429 with **no** `Retry-After`, after Plane's global exception handler has
//!   replaced the body with `{"error_code": 5900, "error_message":
//!   "RATE_LIMIT_EXCEEDED"}` (`plane/authentication/adapter/exception.py:26-32`).
//! * `magic_email_update_<user id>_<address>` — `json.dumps({"token": code})`
//!   stored as a Python `str` for 600 s (`base.py:168-173`). `json.dumps` uses
//!   its default `", "`/`": "` separators, hence the literal formatting below.
//! * `<key>:verify_attempts` — an unpickled integer, `add(0, 600)` + `incr`
//!   **before** the stored code is read, and deleted together with the code once
//!   `attempts >= 5` (`base.py:216-242`, `MagicCodeProvider.MAX_VERIFY_ATTEMPTS`
//!   = 5 at `plane/authentication/provider/credentials/magic_code.py:28`).
//!
//! ## Order, no transactions
//!
//! `plans/USERS_ME.md` §4 lists the exact sequence. Nothing here opens a
//! transaction (`ATOMIC_REQUESTS` is unset), so a late failure keeps the earlier
//! writes: 166 saves the row before the two confirmation publishes, and a
//! broker failure after `logout()` is a 500 with the new address already
//! stored.

use deadpool_postgres::Client;
use plane_cache::{CachedValue, PyValue};
use plane_db::hooks;
use plane_db::models::user::User;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::{Message, task_name, tasks};
use plane_jobs::queue::Job;
use plane_serde::pyjson::{PyStr, Value as Json};

use super::user_base;
use super::write_queries;
use crate::common::{client, jobs};
use crate::views::write_support::{Body, body_get, python_type_name};
use crate::write::{self, email, repr::python_repr};

/// `EMAIL_UPDATE_CODES_PER_TARGET` (`plane/app/views/user/base.py:60`).
const CODES_PER_TARGET: i64 = 5;
/// `EMAIL_UPDATE_CODES_WINDOW` (`plane/app/views/user/base.py:61`).
const CODES_WINDOW: i64 = 3600;
/// `MagicCodeProvider.MAX_VERIFY_ATTEMPTS`
/// (`plane/authentication/provider/credentials/magic_code.py:28`).
const MAX_VERIFY_ATTEMPTS: i64 = 5;

// ---------------------------------------------------------------------------
// POST /api/users/me/email/generate-code/ (165)
// ---------------------------------------------------------------------------

/// `UserEndpoint.generate_email_verification_code` (`base.py:144-189`).
pub fn generate_code<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user = user_base::user_row(ctx).await?;
        let data = ctx.data()?;

        // `new_email = request.data.get("email", "").strip().lower()`
        // (`:151`), evaluated before any cache or database access.
        let new_email = email_field(data)?;

        // `_validate_new_email` (`:102-142`).
        if let Some(response) = validate_new_email(&user, &new_email) {
            return Ok(response);
        }
        let client = client(ctx)?;
        if email_taken(client, &new_email, user.id).await? {
            return Ok(taken_response());
        }

        // `cache.add(issued_key, 0, timeout=…)` / `cache.incr(issued_key)`
        // (`:158-161`). Both are outside the `try`, so a cache failure is the
        // family 500 rather than the 400 below.
        let cache = request_cache(ctx)?;
        let issued_key = format!("email_update_issued:{new_email}");
        cache
            .add(&issued_key, &PyValue::int(0), Some(CODES_WINDOW))
            .await?;
        let issued = cache.incr(&issued_key, 1).await?;
        if issued > CODES_PER_TARGET {
            // `self.throttled(request, None)` (`:162`): DRF's `Throttled(None)`
            // goes through Plane's global exception handler, which replaces
            // the body with `AuthenticationException(RATE_LIMIT_EXCEEDED)`
            // (`plane/authentication/adapter/exception.py:26-32`) and adds no
            // `Retry-After` because `wait` is `None`.
            return Ok(ViewResponse::json(
                429,
                serde_json::json!({
                    "error_code": 5900,
                    "error_message": "RATE_LIMIT_EXCEEDED",
                }),
            ));
        }

        // The `try:` body (`:164-183`). Every failure inside it — cache,
        // random source or broker — is caught and answered as the 400 below
        // (`:184-189`), which is why this is a nested result instead of `?`.
        let stored = async {
            let cache_key = cache_key(user.id, &new_email);
            let token = random_code()?;
            // `json.dumps({"token": token})`: `", "`/`": "` separators.
            let payload = format!("{{\"token\": \"{token}\"}}");
            cache
                .set(&cache_key, &PyValue::string(payload), Some(600))
                .await?;
            // Each newly issued code starts with a fresh verify-attempt budget.
            cache.delete(&attempts_key(&cache_key)).await?;

            let task = task_name::SEND_EMAIL_UPDATE_MAGIC_CODE;
            let message = tasks::send_email_update_magic_code(new_email.clone(), token);
            jobs::dispatch_task(ctx, task, &message, Job::new(task)).await?;
            Ok::<(), ViewError>(())
        }
        .await;

        match stored {
            Ok(()) => Ok(ViewResponse::json(
                200,
                serde_json::json!({"message": "Verification code sent to email"}),
            )),
            Err(_) => Ok(crate::write::error_body(
                400,
                "Failed to generate verification code. Please try again.",
            )),
        }
    })
}

/// The outcome of the `try:` block at `base.py:213-248`.
enum Verified {
    /// The stored code matched and the attempt budget was not exhausted.
    Ok,
    /// `if not cached_data` (`:224-229`).
    Expired,
    /// `attempts > 5 or not compare_digest(...)` (`:234-242`).
    Invalid,
}

// ---------------------------------------------------------------------------
// PATCH /api/users/me/email/ (166)
// ---------------------------------------------------------------------------

/// `UserEndpoint.update_email` (`base.py:191-276`).
pub fn update_email<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let mut user = user_base::user_row(ctx).await?.into_owned();
        let data = ctx.data()?;

        // `:198-199`: both reads happen before validation.
        let new_email = email_field(data)?;
        let code = code_field(data)?;

        if let Some(response) = validate_new_email(&user, &new_email) {
            return Ok(response);
        }
        let client = client(ctx)?;
        // The last branch of `_validate_new_email` (`base.py:135-140`), which
        // runs before the `code` check.
        if email_taken(client, &new_email, user.id).await? {
            return Ok(taken_response());
        }
        if code.text.is_empty() {
            return Ok(crate::write::error_body(
                400,
                "Verification code is required",
            ));
        }

        // The `try:` at `:213-248`. `Verified` is returned from the cache-check
        // branches, so the three literal 400s stay distinguishable; any error
        // becomes `Failed to verify code. Please try again.`
        let cache = request_cache(ctx)?;
        let cache_key = cache_key(user.id, &new_email);
        let attempts_key = attempts_key(&cache_key);
        let verified: Result<Verified, ViewError> = async {
            cache
                .add(&attempts_key, &PyValue::int(0), Some(600))
                .await?;
            let attempts = cache.incr(&attempts_key, 1).await?;
            let cached = cache.get(&cache_key).await?;

            // `if not cached_data` (`:224`): a missing key, but also a stored
            // falsy value, answers "expired or invalid".
            let Some(cached) = cached.filter(python_truthy) else {
                return Ok(Verified::Expired);
            };
            let stored = json_token(cached)?;

            // `attempts > 5 or not secrets.compare_digest(...)` (`:234-236`).
            let matches = stored
                .as_ref()
                .zip(code.as_bytes())
                .is_some_and(|(stored, code)| {
                    plane_auth::random::constant_time_compare(stored, code)
                });
            if attempts > MAX_VERIFY_ATTEMPTS || !matches {
                // `attempts >= 5` deletes the code and the counter (`:237-238`).
                if attempts >= MAX_VERIFY_ATTEMPTS {
                    cache
                        .delete_many(&[cache_key.clone(), attempts_key.clone()])
                        .await?;
                }
                return Ok(Verified::Invalid);
            }
            Ok(Verified::Ok)
        }
        .await;

        match verified {
            Ok(Verified::Ok) => {}
            Ok(Verified::Expired) => {
                return Ok(crate::write::error_body(
                    400,
                    "Verification code has expired or is invalid",
                ));
            }
            Ok(Verified::Invalid) => {
                return Ok(crate::write::error_body(400, "Invalid verification code"));
            }
            Err(_) => {
                return Ok(crate::write::error_body(
                    400,
                    "Failed to verify code. Please try again.",
                ));
            }
        }

        // Final availability check (`:250-255`), after the code was accepted.
        if email_taken(client, &new_email, user.id).await? {
            return Ok(taken_response());
        }

        let old_email = user.email.clone();
        user.email = Some(new_email.clone());
        // Reset email verification status when email is changed.
        user.is_email_verified = false;
        let user = save_user(client, user).await?;

        // `cache.delete(cache_key)` — the verify-attempts counter is left
        // behind (`:263-264`).
        cache.delete(&cache_key).await?;

        // `logout(request)` (`:266-267`): flush + the middleware's cookie
        // deletion and `Vary: Cookie`.
        ctx.session().flush(client).await?;

        // `send_email_update_confirmation.delay(new_email)` then
        // `(old_email)` (`:269-272`), in that order. `old_email` may be
        // `NULL`; the task argument is then Python `None`.
        for recipient in [Some(new_email.clone()), old_email] {
            let task = task_name::SEND_EMAIL_UPDATE_CONFIRMATION;
            let message = Message::new(
                task,
                vec![recipient.map_or(TaskArg::Null, TaskArg::str)],
                Vec::new(),
            );
            jobs::dispatch_task(ctx, task, &message, Job::new(task)).await?;
        }

        // `UserMeSerializer(user).data` (`:274-276`), from the in-memory row,
        // with the two asset properties the serializer reads.
        user_base::render_user_me(ctx, std::borrow::Cow::Owned(user)).await
    })
}

// ---------------------------------------------------------------------------
// Shared with the deactivation path
// ---------------------------------------------------------------------------

/// `user.save()` for a full-row write: `User.save`'s assignments (H30) and
/// then Django's `auto_now` timestamp.
///
/// Two independent `timezone.now()` calls happen in Python — one inside H30
/// for `token_updated_at`, one in `Model.save`'s `pre_save` for `updated_at`
/// (`plane/db/models/user.py:169-187`, `django/db/models/fields/__init__.py:874`)
/// — so they are two [`write::now`] calls here. The token rotates whenever
/// `token_updated_at` is non-NULL, which the fixture makes true for every user.
pub(super) async fn save_user(client: &Client, user: User) -> Result<User, ViewError> {
    let hook_now = write::now();
    let prepared = hooks::user::before_save(
        user.email.as_deref(),
        &user.display_name,
        user.is_superuser,
        user.is_staff,
        &user.token,
        user.token_updated_at,
        hook_now,
    )
    // `self.email.lower()` on NULL: the `AttributeError` is a family 500.
    .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let updated_at = write::now();

    let mut user = user;
    user.email = Some(prepared.email);
    user.token = prepared.token;
    user.token_updated_at = prepared.token_updated_at;
    user.display_name = prepared.display_name;
    user.is_staff = prepared.is_staff;
    user.updated_at = updated_at;

    let statement = write_queries::USER_UPDATE.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &user.password,
                &user.last_login,
                &user.username,
                &user.mobile_number,
                &user.email,
                &user.display_name,
                &user.first_name,
                &user.last_name,
                &user.avatar,
                &user.avatar_asset_id,
                &user.cover_image,
                &user.cover_image_asset_id,
                &user.date_joined,
                &user.created_at,
                &user.updated_at,
                &user.last_location,
                &user.created_location,
                &user.is_superuser,
                &user.is_managed,
                &user.is_password_expired,
                &user.is_active,
                &user.is_staff,
                &user.is_email_verified,
                &user.is_password_autoset,
                &user.is_password_reset_required,
                &user.token,
                &user.last_active,
                &user.last_login_time,
                &user.last_logout_time,
                &user.last_login_ip,
                &user.last_logout_ip,
                &user.last_login_medium,
                &user.last_login_uagent,
                &user.token_updated_at,
                &user.is_bot,
                &user.bot_type,
                &user.user_timezone,
                &user.is_email_valid,
                &user.masked_at,
                &user.id,
            ],
        )
        .await
        // `users.email` is unique: a concurrent write that took the address
        // after 166's existence check is Django's `IntegrityError` → family 400
        // (`plane/app/views/base.py:210-214`), not a 500.
        .map_err(|error| match error.code() {
            Some(code) if code.code().starts_with("23") => ViewError::Integrity,
            _ => ViewError::from(error),
        })?;
    Ok(user)
}

/// `User.objects.filter(email=…).exclude(id=user.id).exists()`
/// (`plane/app/views/user/base.py:136,251`).
pub(super) async fn email_taken(
    client: &Client,
    email: &str,
    user_id: uuid::Uuid,
) -> Result<bool, ViewError> {
    let statement = write_queries::USER_EMAIL_EXISTS.prepare(client).await?;
    Ok(client
        .query_opt(&statement, &[&email, &user_id])
        .await?
        .is_some())
}

// ---------------------------------------------------------------------------
// Body, cache keys and Python helpers
// ---------------------------------------------------------------------------

/// `_validate_new_email` (`plane/app/views/user/base.py:102-142`), minus its
/// asynchronous existence check.
fn validate_new_email(user: &User, new_email: &str) -> Option<ViewResponse> {
    if new_email.is_empty() {
        return Some(crate::write::error_body(400, "Email is required"));
    }
    // `django.core.validators.validate_email` (`:120-126`); its exception text
    // is not observable — the view answers this literal.
    if !email::validate_email(new_email) {
        return Some(crate::write::error_body(400, "Invalid email format"));
    }
    if user.email.as_deref() == Some(new_email) {
        return Some(crate::write::error_body(
            400,
            "New email must be different from current email",
        ));
    }
    None
}

fn taken_response() -> ViewResponse {
    crate::write::error_body(400, "An account with this email already exists")
}

/// `request.data.get("email", "").strip().lower()` (`base.py:151,198`).
fn email_field(data: &Data) -> Result<String, ViewError> {
    let raw = body_pystr(data, "email", "strip")?;
    let text = raw.to_string_lossy();
    Ok(python_strip(&text).to_lowercase())
}

/// A `code` value after `.strip()` (`base.py:199`).
///
/// `surrogate` records that the Python `str` contains a lone surrogate, which
/// `str.encode()` rejects; the view's outer `except` turns that into its 400
/// "Failed to verify code" (`:244-248`). Kept as bytes rather than a Rust
/// `String` so the check survives the strip.
struct Code {
    text: String,
    surrogate: bool,
}

impl Code {
    /// `str(code).encode()` as `secrets.compare_digest` receives it, or `None`
    /// for a `UnicodeEncodeError`.
    fn as_bytes(&self) -> Option<&[u8]> {
        (!self.surrogate).then_some(self.text.as_bytes())
    }
}

fn code_field(data: &Data) -> Result<Code, ViewError> {
    let raw = body_pystr(data, "code", "strip")?;
    let surrogate = raw.has_surrogates();
    let text = python_strip(&raw.to_string_lossy()).to_owned();
    Ok(Code { text, surrogate })
}

/// `request.data.get(key, "")` as the Python `str` it must be, or the
/// `AttributeError` the view would raise for a non-string value.
fn body_pystr(data: &Data, key: &str, method: &str) -> Result<PyStr, ViewError> {
    match body_get(data, key)? {
        Body::Missing => Ok(PyStr::default()),
        Body::Form(text) => Ok(PyStr::new(text)),
        Body::Json(Json::Str(text)) => Ok(text.clone()),
        Body::Json(other) => Err(ViewError::Unknown(format!(
            "AttributeError: '{}' object has no attribute '{method}'",
            python_type_name(other)
        ))),
    }
}

/// CPython's `str.strip()`: `char::is_whitespace` plus U+001C..U+001F
/// (`Objects/unicodeobject.c: _PyUnicode_IsWhitespace`), the same set
/// `plane_db::hooks::user` uses for H30.
fn python_strip(value: &str) -> &str {
    value.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '\u{1c}'..='\u{1f}')
    })
}

/// `f"magic_email_update_{user.id}_{new_email}"` (`base.py:168,214`).
fn cache_key(user_id: uuid::Uuid, email: &str) -> String {
    format!("magic_email_update_{user_id}_{email}")
}

/// `f"{cache_key}:verify_attempts"` (`base.py:175,219`).
fn attempts_key(cache_key: &str) -> String {
    format!("{cache_key}:verify_attempts")
}

/// Python truthiness for `if not cached_data` (`base.py:224`).
fn python_truthy(value: &CachedValue) -> bool {
    match value {
        CachedValue::Int(value) => !value.is_zero(),
        CachedValue::Pickled(value) => match value {
            PyValue::None => false,
            PyValue::Bool(value) => *value,
            PyValue::Int(value) => !value.is_zero(),
            PyValue::Float(value) => *value != 0.0,
            PyValue::Str(value) => !value.as_wtf8().is_empty(),
            PyValue::Bytes(value) => !value.is_empty(),
            PyValue::List(value)
            | PyValue::Tuple(value)
            | PyValue::Set(value)
            | PyValue::FrozenSet(value) => !value.is_empty(),
            PyValue::Dict(value) => !value.is_empty(),
            _ => true,
        },
    }
}

/// `json.loads(cached_data)` then `str(data.get("token")).encode()`
/// (`base.py:231-236`).
///
/// `None` means the value cannot be encoded (a lone surrogate), which the
/// view's outer `except` maps to its 400. A value that is not a JSON object is
/// Python's `AttributeError` on `.get` — also inside the `try`, so it is the
/// same 400.
fn json_token(cached: CachedValue) -> Result<Option<Vec<u8>>, ViewError> {
    let bytes = match cached.into_value() {
        // `json.loads` accepts `str`, `bytes` and `bytearray`; anything else is
        // a `TypeError` caught by the same outer handler.
        PyValue::Str(text) => text.as_wtf8().to_vec(),
        PyValue::Bytes(bytes) => (*bytes).clone(),
        _ => return Err(ViewError::Unknown("TypeError: json.loads".to_string())),
    };
    let parsed = plane_serde::pyjson::loads(&bytes, "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let members = match &parsed {
        Json::Dict(members) => members,
        _ => {
            return Err(ViewError::Unknown(
                "AttributeError: value has no attribute 'get'".to_string(),
            ));
        }
    };
    let Some(token) = members.get("token") else {
        // `str(None)` is `"None"`.
        return Ok(Some(b"None".to_vec()));
    };
    match token {
        Json::Str(text) => Ok((!text.has_surrogates()).then(|| text.as_wtf8().to_vec())),
        other => Ok(Some(python_repr(other).into_bytes())),
    }
}

/// `secrets.randbelow(900000) + 100000` (`base.py:170`): Python's
/// `_randbelow` rejection sampling, so the distribution is the same.
fn random_code() -> Result<String, ViewError> {
    const RANGE: u32 = 900_000;
    let limit = u32::MAX - (u32::MAX % RANGE);
    let mut bytes = [0u8; 4];
    loop {
        getrandom::fill(&mut bytes)
            .map_err(|error| ViewError::Unknown(format!("secrets.randbelow: {error}")))?;
        let value = u32::from_le_bytes(bytes);
        if value < limit {
            return Ok((value % RANGE + 100_000).to_string());
        }
    }
}

/// `django.core.cache.cache`. A process built without Redis would fail the
/// request in Django, so a missing handle is a family 500 rather than a miss.
fn request_cache<'a>(ctx: &'a ViewCtx<'a>) -> Result<plane_cache::Client<'a>, ViewError> {
    ctx.cache().ok_or_else(|| {
        ViewError::Unknown("django.core.cache is not configured for this process".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_strip_matches_cpythons_whitespace_classes() {
        assert_eq!(python_strip("  a@b  "), "a@b");
        assert_eq!(python_strip("\u{1c}a@b\u{1f}"), "a@b");
        assert_eq!(python_strip("\u{a0}a@b\u{3000}"), "a@b");
        // A zero-width space is not whitespace.
        assert_eq!(python_strip("\u{200b}a@b"), "\u{200b}a@b");
    }

    #[test]
    fn codes_are_six_digits_in_the_python_range() {
        for _ in 0..64 {
            let code = random_code().expect("the OS random source");
            assert_eq!(code.len(), 6, "{code}");
            let value: u32 = code.parse().expect("six digits");
            assert!((100_000..1_000_000).contains(&value), "{value}");
        }
    }

    #[test]
    fn cache_keys_are_the_python_f_strings() {
        let id = uuid::Uuid::from_u128(7);
        let key = cache_key(id, "new@example.com");
        assert_eq!(
            key,
            "magic_email_update_00000000-0000-0000-0000-000000000007_new@example.com"
        );
        assert_eq!(format!("{key}:verify_attempts"), attempts_key(&key));
    }
}
