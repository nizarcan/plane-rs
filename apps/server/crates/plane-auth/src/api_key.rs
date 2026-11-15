// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `APIKeyAuthentication` for `/api/v1/`:
//! `plane/api/middleware/api_authentication.py`.
//!
//! `plane/app/middleware/api_authentication.py` is a **byte-identical copy**
//! (diffed: same imports, same class, same lines) and is dead code — it is not
//! in `MIDDLEWARE`, not referenced by any view, and only
//! `plane/api/middleware/api_authentication.py` is imported
//! (`plane/api/views/base.py:25`). There is nothing to reconcile.
//!
//! The query is the exact filter of `validate_api_token`
//! (`plane/api/middleware/api_authentication.py:28-37`):
//!
//! ```python
//! APIToken.objects.get(
//!     Q(Q(expired_at__gt=timezone.now()) | Q(expired_at__isnull=True)),
//!     token=token,
//!     is_active=True,
//!     user__is_active=True,
//! )
//! ```
//!
//! `APIToken.objects` is a `SoftDeletionManager`, so `deleted_at IS NULL` is
//! part of the filter even though the Python source does not spell it out.
//!
//! On success `last_used` is written with
//! `save(update_fields=["last_used"])`, i.e. **only that column**:
//! `updated_at`, `created_by`/`updated_by` and `updated_by` stay untouched
//! (MODEL_HOOKS.md §1.6 and §4.3, `plane/api/middleware/api_authentication.py:41-42`).

use std::sync::Arc;

use chrono::Utc;
use tokio_postgres::Client;
use uuid::Uuid;

use crate::error::AuthError;
use crate::queries;
use crate::user::AuthUser;

/// The exact `AuthenticationFailed` detail
/// (`plane/api/middleware/api_authentication.py:36`).
pub const INVALID_API_TOKEN: &str = "Given API token is not valid";

/// The header `APIKeyAuthentication.auth_header_name` reads.
pub const API_KEY_HEADER: &str = "x-api-key";

/// A successfully authenticated `api_tokens` row.
#[derive(Debug, Clone)]
pub struct ApiToken {
    pub id: Uuid,
    pub token: String,
    /// `api_token.user`, already checked for `is_active`.
    pub user: Arc<AuthUser>,
}

/// Why API-key authentication failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyError {
    /// `AuthenticationFailed("Given API token is not valid")` — a 403 from DRF
    /// because `APIKeyAuthentication` has no `authenticate_header`.
    Invalid,
    /// A database failure, which Django would let escape as a 500.
    Database(AuthError),
}

impl std::fmt::Display for ApiKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid => f.write_str(INVALID_API_TOKEN),
            Self::Database(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ApiKeyError {}

impl From<AuthError> for ApiKeyError {
    fn from(error: AuthError) -> Self {
        Self::Database(error)
    }
}

impl From<tokio_postgres::Error> for ApiKeyError {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Database(AuthError::from(error))
    }
}

/// `APIKeyAuthentication.authenticate(request)`.
///
/// `None` when the header is absent or empty (`if not token: return None`), so
/// the next authenticator (or `NotAuthenticated`) decides.
pub async fn authenticate(
    client: &Client,
    header: Option<&str>,
) -> Result<Option<ApiToken>, ApiKeyError> {
    let Some(token) = header.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let row = client
        .query_opt(queries::LOAD_API_TOKEN.sql, &[&token])
        .await?
        .ok_or(ApiKeyError::Invalid)?;
    let token_id: Uuid = row.get(0);
    let token_value: String = row.get(1);
    // Columns 2.. are the user row, starting at its `id`.
    let user = AuthUser::from_row(&row, 2);

    // `api_token.last_used = timezone.now(); save(update_fields=["last_used"])`.
    client
        .execute(queries::TOUCH_API_TOKEN.sql, &[&token_id, &Utc::now()])
        .await?;

    Ok(Some(ApiToken {
        id: token_id,
        token: token_value,
        user: Arc::new(user),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_invalid_token_message_is_djangos() {
        assert_eq!(INVALID_API_TOKEN, "Given API token is not valid");
        assert_eq!(API_KEY_HEADER, "x-api-key");
        assert_eq!(ApiKeyError::Invalid.to_string(), INVALID_API_TOKEN);
    }
}
