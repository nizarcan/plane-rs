// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Errors from `plane-auth`.
//!
//! Nothing here decides an HTTP status: the caller (`plane-api` today, the DRF
//! runtime later) maps each variant onto Django's behaviour. The mapping is the
//! same everywhere:
//!
//! * [`AuthError::Database`] → Django's 500 page (an unhandled `DatabaseError`);
//! * [`AuthError::MissingPasswordLibrary`] → 500, because Django raises
//!   `ValueError` out of `check_password` when the hasher's optional library is
//!   not installed (`django/contrib/auth/hashers.py:263-275`);
//! * [`AuthError::InvalidSessionUser`] → 500: `UUIDField.to_python` raises
//!   `ValidationError` and `get_user` only catches `KeyError`
//!   (`django/contrib/auth/__init__.py:296-318`).

/// Anything that can go wrong inside `plane-auth`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("database error: {0}")]
    Database(String),
    /// The hash names a hasher whose optional Python library is not installed
    /// in this deployment. Django raises rather than returning `False`.
    #[error("couldn't load '{0}' algorithm library")]
    MissingPasswordLibrary(&'static str),
    /// `_auth_user_id` is not a valid UUID (`UUIDField.to_python` raised).
    #[error("session user id is not a valid UUID")]
    InvalidSessionUser,
    /// `base_host()` with neither `APP_BASE_URL` nor `WEB_URL` set.
    #[error("APP_BASE_URL or WEB_URL is not set")]
    ImproperlyConfigured(&'static str),
    /// A password hash is syntactically invalid for the hasher it names. Django
    /// raises `ValueError`/`AssertionError` from `decode`; callers treat it as
    /// "not this password" only where Django does.
    #[error("invalid password hash: {0}")]
    InvalidPasswordHash(String),
}

impl From<tokio_postgres::Error> for AuthError {
    fn from(error: tokio_postgres::Error) -> Self {
        // `as_db_error` keeps the server message, which is what an operator
        // needs; the rest of `Display` is driver noise.
        match error.as_db_error() {
            Some(db_error) => Self::Database(db_error.message().to_string()),
            None => Self::Database(error.to_string()),
        }
    }
}
