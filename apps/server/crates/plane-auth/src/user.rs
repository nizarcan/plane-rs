// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `request.user` and `django.contrib.auth.get_user`.
//!
//! `AuthenticationMiddleware` puts a `SimpleLazyObject` on the request
//! (`django/contrib/auth/middleware.py:30-41`); nothing is read or written
//! until something touches `request.user`, which runs
//! `django.contrib.auth.get_user` (`django/contrib/auth/__init__.py:294-330`):
//!
//! 1. `_auth_user_id` and `_auth_user_backend` must both be in the session
//!    (`SESSION_KEY`, `BACKEND_SESSION_KEY`); a missing key is anonymous and
//!    does **not** touch the database.
//! 2. The backend path must be in `AUTHENTICATION_BACKENDS` (Plane's only entry
//!    is `django.contrib.auth.backends.ModelBackend`,
//!    `plane/settings/common.py:166`).
//! 3. `ModelBackend.get_user` loads the row and returns `None` when the user is
//!    inactive (`backends.py:229-235`).
//! 4. `_auth_user_hash` must equal `user.get_session_auth_hash()` in constant
//!    time; a missing or mismatching hash **flushes the session** and the
//!    request is anonymous.
//!
//! The hash is
//! `salted_hmac("django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash",
//! password, algorithm="sha256").hexdigest()`
//! (`django/contrib/auth/base_user.py:146-157`). `SECRET_KEY_FALLBACKS` is
//! empty in Plane, so only the current key can verify; the fallback path above
//! is still implemented for parity.

use std::fmt;
use std::sync::Arc;

use plane_core::config::Settings;
use tokio_postgres::Client;
use uuid::Uuid;

use crate::error::AuthError;
use crate::password::make_password_async;
use crate::queries;
use crate::random::{RANDOM_STRING_CHARS, constant_time_compare, get_random_string};
use crate::session::{BACKEND_SESSION_KEY, HASH_SESSION_KEY, SESSION_KEY, SessionStore};
use crate::signing::salted_hmac_hexdigest;

/// The only entry in `AUTHENTICATION_BACKENDS`
/// (`plane/settings/common.py:166`).
pub const MODEL_BACKEND: &str = "django.contrib.auth.backends.ModelBackend";
/// `settings.AUTHENTICATION_BACKENDS`.
pub const AUTHENTICATION_BACKENDS: &[&str] = &[MODEL_BACKEND];
/// The `key_salt` `AbstractBaseUser.get_session_auth_hash` uses.
pub const SESSION_AUTH_HASH_SALT: &str =
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash";
/// `SECRET_KEY_FALLBACKS`: empty in Plane (`django/conf/global_settings.py`,
/// not overridden by `plane/settings`).
pub const SECRET_KEY_FALLBACKS: &[&str] = &[];

/// A `users` row, in the shape the auth layer needs.
///
/// Field names are Django's (`plane/db/models/user.py:56-121`). `email` is
/// nullable in the schema but every Plane code path sets it, and Django's
/// `User.save` would fail on `None`; `NULL` is read as `""` here and noted in
/// the ledger.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    pub display_name: String,
    pub first_name: String,
    pub last_name: String,
    pub user_timezone: String,
    pub is_active: bool,
    pub is_superuser: bool,
    pub is_staff: bool,
    pub is_bot: bool,
    pub is_email_verified: bool,
    /// The encoded hash, `algorithm$iterations$salt$hash` or `!…`.
    pub password: String,
}

impl AuthUser {
    /// `AbstractBaseUser.get_session_auth_hash()`.
    pub fn get_session_auth_hash(&self, secret_key: &str) -> String {
        salted_hmac_hexdigest(SESSION_AUTH_HASH_SALT, self.password.as_bytes(), secret_key)
    }

    /// `get_session_auth_fallback_hash()` for one fallback secret.
    pub fn get_session_auth_fallback_hash(&self, fallback_secret: &str) -> String {
        salted_hmac_hexdigest(
            SESSION_AUTH_HASH_SALT,
            self.password.as_bytes(),
            fallback_secret,
        )
    }

    /// `PermissionsMixin.is_authenticated` is always `True` on a real user.
    pub fn is_authenticated(&self) -> bool {
        true
    }

    /// `User.full_name` (`user.py:165-167`): `f"{first_name} {last_name}".strip()`.
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }

    /// A `users` row from a statement that selects the columns in
    /// [`crate::queries::LOAD_USER_BY_ID`] order, starting at `offset`.
    pub(crate) fn from_row(row: &tokio_postgres::Row, offset: usize) -> Self {
        let email: Option<String> = row.get(offset + 3);
        Self {
            id: row.get(offset),
            password: row.get(offset + 1),
            username: row.get(offset + 2),
            email: email.unwrap_or_default(),
            display_name: row.get(offset + 4),
            first_name: row.get(offset + 5),
            last_name: row.get(offset + 6),
            user_timezone: row.get(offset + 7),
            is_active: row.get(offset + 8),
            is_staff: row.get(offset + 9),
            is_superuser: row.get(offset + 10),
            is_email_verified: row.get(offset + 11),
            is_bot: row.get(offset + 12),
        }
    }
}

impl fmt::Debug for AuthUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the hash; it is password-equivalent for offline attacks.
        f.debug_struct("AuthUser")
            .field("id", &self.id)
            .field("email", &self.email)
            .field("username", &self.username)
            .field("display_name", &self.display_name)
            .field("is_active", &self.is_active)
            .field("is_superuser", &self.is_superuser)
            .field("is_staff", &self.is_staff)
            .field("is_bot", &self.is_bot)
            .field("is_email_verified", &self.is_email_verified)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Django's `request.user`: `AnonymousUser` or a user.
///
/// `Arc` because a request may hand the same user to several layers (DRF's
/// `Request.user` setter writes it back onto the Django request, and crum reads
/// it again).
#[derive(Debug, Clone, Default)]
pub enum RequestUser {
    #[default]
    Anonymous,
    User(Arc<AuthUser>),
}

impl RequestUser {
    pub fn from_user(user: AuthUser) -> Self {
        Self::User(Arc::new(user))
    }

    pub fn is_authenticated(&self) -> bool {
        matches!(self, Self::User(_))
    }

    pub fn user(&self) -> Option<&AuthUser> {
        match self {
            Self::User(user) => Some(user),
            Self::Anonymous => None,
        }
    }

    pub fn id(&self) -> Option<Uuid> {
        self.user().map(|user| user.id)
    }

    pub fn username(&self) -> Option<&str> {
        self.user().map(|user| user.username.as_str())
    }
}

/// `ModelBackend.get_user(user_id)` (`django/contrib/auth/backends.py:229-235`).
pub async fn load_user_by_id(
    client: &Client,
    user_id: Uuid,
) -> Result<Option<AuthUser>, AuthError> {
    let row = client
        .query_opt(queries::LOAD_USER_BY_ID.sql, &[&user_id])
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let user = AuthUser::from_row(&row, 0);
    // `user_can_authenticate`: reject users with `is_active=False`.
    Ok(user.is_active.then_some(user))
}

/// `UserManager.get_by_natural_key` with `USERNAME_FIELD = "email"`.
pub async fn load_user_by_email(
    client: &Client,
    email: &str,
) -> Result<Option<AuthUser>, AuthError> {
    let row = client
        .query_opt(queries::LOAD_USER_BY_EMAIL.sql, &[&email])
        .await?;
    Ok(row.as_ref().map(|row| AuthUser::from_row(row, 0)))
}

/// `django.contrib.auth.get_user(request)`.
///
/// The session is only read (and therefore only loaded) here; this is the
/// function the lazy `request.user` evaluates to.
pub async fn get_user(
    client: &Client,
    settings: &Settings,
    session: &mut SessionStore,
) -> Result<RequestUser, AuthError> {
    let secret_key = settings.secret_key.value();
    let Some(user_id) = session.get(client, secret_key, SESSION_KEY).await? else {
        return Ok(RequestUser::Anonymous);
    };
    let Some(backend) = session.get(client, secret_key, BACKEND_SESSION_KEY).await? else {
        return Ok(RequestUser::Anonymous);
    };
    let backend = backend.as_str().unwrap_or_default();
    if !AUTHENTICATION_BACKENDS.contains(&backend) {
        return Ok(RequestUser::Anonymous);
    }
    let user_id = parse_user_id(user_id.as_str().unwrap_or_default())?;
    let Some(user) = load_user_by_id(client, user_id).await? else {
        return Ok(RequestUser::Anonymous);
    };

    // Verify the session against the password hash.
    let session_hash = session.get(client, secret_key, HASH_SESSION_KEY).await?;
    let session_hash = session_hash.as_ref().and_then(|value| value.as_str());
    let session_auth_hash = user.get_session_auth_hash(secret_key);
    let verified = match session_hash {
        None => false,
        Some(value) => constant_time_compare(value.as_bytes(), session_auth_hash.as_bytes()),
    };
    if verified {
        return Ok(RequestUser::from_user(user));
    }
    // `SECRET_KEY_FALLBACKS` is empty in Plane; the branch exists for parity.
    // `SECRET_KEY_FALLBACKS` is `[]` (`django/conf/global_settings.py`, not
    // overridden in `plane/settings`), so this loop never runs. It exists so a
    // future deployment that sets fallbacks only has to fill the slice.
    let fallback_verifies = match session_hash {
        Some(value) => SECRET_KEY_FALLBACKS.iter().any(|secret| {
            constant_time_compare(
                value.as_bytes(),
                user.get_session_auth_fallback_hash(secret).as_bytes(),
            )
        }),
        None => false,
    };
    if fallback_verifies {
        session
            .cycle_key(client, settings, secret_key)
            .await
            .map_err(|error| match error {
                crate::session::SaveError::Database(error) => error,
                other => AuthError::Database(other.to_string()),
            })?;
        session
            .set(
                client,
                secret_key,
                HASH_SESSION_KEY,
                crate::json::Json::Str(session_auth_hash),
            )
            .await?;
        return Ok(RequestUser::from_user(user));
    }
    session.flush(client).await?;
    Ok(RequestUser::Anonymous)
}

/// `UUIDField.to_python`: `uuid.UUID(hex=value)`; a failure is a
/// `ValidationError`, which `get_user` does not catch (`auth/__init__.py:296`),
/// so it becomes a 500.
fn parse_user_id(value: &str) -> Result<Uuid, AuthError> {
    Uuid::parse_str(value).map_err(|_| AuthError::InvalidSessionUser)
}

/// `ModelBackend.authenticate` for the email/password pair.
///
/// Returns `None` for a missing user, a wrong password, or an inactive user.
/// A missing user still burns one default hash (`backends.py:60-64`), so the
/// response time does not reveal whether the account exists.
pub async fn authenticate_password(
    client: &Client,
    email: &str,
    password: &str,
) -> Result<Option<AuthUser>, AuthError> {
    let Some(user) = load_user_by_email(client, email).await? else {
        // `UserModel().set_password(password)`: a full default hash, discarded.
        make_password_async(Some(get_random_string(40, RANDOM_STRING_CHARS))).await?;
        return Ok(None);
    };
    let check =
        crate::password::check_password_with_rehash(password.to_string(), user.password.clone())
            .await?;
    if !check.0 {
        return Ok(None);
    }
    if let Some(rehash) = check.1 {
        // `check_password(setter=...)` → `save(update_fields=["password"])`.
        client
            .execute(queries::UPDATE_USER_PASSWORD.sql, &[&user.id, &rehash])
            .await?;
    }
    Ok(user.is_active.then_some(user))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> AuthUser {
        AuthUser {
            id: Uuid::nil(),
            email: "a@example.com".into(),
            username: "abc".into(),
            display_name: "A".into(),
            first_name: "Ada".into(),
            last_name: "Lovelace".into(),
            user_timezone: "UTC".into(),
            is_active: true,
            is_superuser: false,
            is_staff: false,
            is_bot: false,
            is_email_verified: true,
            password: "pbkdf2_sha256$1$salt$hash".into(),
        }
    }

    #[test]
    fn session_auth_hash_matches_django() {
        // `AbstractBaseUser._get_session_auth_hash` with
        // SECRET_KEY="secret" and password="pbkdf2_sha256$1$salt$hash".
        assert_eq!(
            user().get_session_auth_hash("secret"),
            "67b8a2db9a11f20a0a7107b74c79f0f7cf7c6da789d5741f6b65b93c65a418f6"
        );
    }

    #[test]
    fn request_user_helpers() {
        let anonymous = RequestUser::Anonymous;
        assert!(!anonymous.is_authenticated());
        assert!(anonymous.user().is_none());
        assert!(anonymous.id().is_none());

        let authenticated = RequestUser::from_user(user());
        assert!(authenticated.is_authenticated());
        assert_eq!(authenticated.id(), Some(Uuid::nil()));
        assert_eq!(authenticated.username(), Some("abc"));
        assert_eq!(
            authenticated.user().map(AuthUser::full_name),
            Some("Ada Lovelace".into())
        );
    }

    #[test]
    fn debug_never_prints_the_hash() {
        let debug = format!("{:?}", user());
        assert!(debug.contains("a@example.com"));
        assert!(!debug.contains("pbkdf2_sha256"));
    }

    #[test]
    fn user_id_parsing() {
        let id = Uuid::new_v4();
        assert_eq!(parse_user_id(&id.to_string()), Ok(id));
        assert_eq!(
            parse_user_id("not-a-uuid"),
            Err(AuthError::InvalidSessionUser)
        );
    }
}
