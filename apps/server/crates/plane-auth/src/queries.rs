// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-auth`'s statement catalog.
//!
//! Every statement lives here with the exact result columns it expects;
//! `tests/schema_check.rs` prepares them all against a Django-migrated database
//! (DECISIONS.md D1). Adding a statement means adding a
//! [`plane_query!`](plane_db::plane_query) invocation (or a [`Query`] literal
//! for a write), listing it in [`ALL_QUERIES`], and bumping the count in
//! `catalog_is_complete`.
//!
//! Column names and types were read from a clone of `plane_tpl` (Django's
//! migrations, unmodified).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `SessionStore._get_session_from_db`
    /// (`django/contrib/sessions/backends/db.py:30-42`):
    /// `SELECT … WHERE session_key = %s AND expire_date > now()`.
    LOAD_SESSION_BY_KEY = "auth_load_session_by_key",
    "SELECT session_data, expire_date \
     FROM sessions \
     WHERE session_key = $1 AND expire_date > now()",
    [
        ("session_data", Text),
        ("expire_date", Timestamptz),
    ]
}

plane_query! {
    /// `SessionStore.exists` (`db.py:52-53`), used while generating a key.
    SESSION_EXISTS = "auth_session_exists",
    "SELECT 1::int4 AS present FROM sessions WHERE session_key = $1",
    [("present", Int4)]
}

plane_query! {
    /// `Model.save(force_insert=True)` for a `sessions` row: every concrete
    /// column (`SessionStore.create_model_instance`, `db.py:77-100`).
    INSERT_SESSION = "auth_insert_session",
    "INSERT INTO sessions (session_key, session_data, expire_date, user_id, device_info) \
     VALUES ($1, $2, $3, $4, $5)",
    []
}

plane_query! {
    /// `Model.save(force_update=True)`: every non-PK column, so `expire_date` is
    /// refreshed on each save exactly as Django's `auto_now`-less model does.
    UPDATE_SESSION = "auth_update_session",
    "UPDATE sessions \
     SET session_data = $2, expire_date = $3, user_id = $4, device_info = $5 \
     WHERE session_key = $1",
    []
}

plane_query! {
    /// `SessionStore.delete` (`db.py:142-151`).
    DELETE_SESSION = "auth_delete_session",
    "DELETE FROM sessions WHERE session_key = $1",
    []
}

plane_query! {
    /// `ModelBackend.get_user` (`django/contrib/auth/backends.py:229-235`) and
    /// `get_by_natural_key` for the email lookup. `users` does not use
    /// `BaseModel`, so there is no `deleted_at` column.
    LOAD_USER_BY_ID = "auth_load_user_by_id",
    "SELECT id, password, username, email, display_name, first_name, last_name, \
     user_timezone, is_active, is_staff, is_superuser, is_email_verified, is_bot, \
     date_joined \
     FROM users \
     WHERE id = $1",
    [
        ("id", Uuid),
        ("password", Varchar),
        ("username", Varchar),
        ("email", Varchar),
        ("display_name", Varchar),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("user_timezone", Varchar),
        ("is_active", Bool),
        ("is_staff", Bool),
        ("is_superuser", Bool),
        ("is_email_verified", Bool),
        ("is_bot", Bool),
        ("date_joined", Timestamptz),
    ]
}

plane_query! {
    /// `UserManager.get_by_natural_key` with `USERNAME_FIELD = "email"`:
    /// `User.objects.get(email=…)`, an exact match (`User.save` lower-cases the
    /// stored value; `SignInAuthEndpoint` lower-cases the input).
    LOAD_USER_BY_EMAIL = "auth_load_user_by_email",
    "SELECT id, password, username, email, display_name, first_name, last_name, \
     user_timezone, is_active, is_staff, is_superuser, is_email_verified, is_bot, \
     date_joined \
     FROM users \
     WHERE email = $1",
    [
        ("id", Uuid),
        ("password", Varchar),
        ("username", Varchar),
        ("email", Varchar),
        ("display_name", Varchar),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("user_timezone", Varchar),
        ("is_active", Bool),
        ("is_staff", Bool),
        ("is_superuser", Bool),
        ("is_email_verified", Bool),
        ("is_bot", Bool),
        ("date_joined", Timestamptz),
    ]
}

plane_query! {
    /// `APIKeyAuthentication.validate_api_token`
    /// (`plane/api/middleware/api_authentication.py:28-44`): the token must not
    /// be expired, must be active, its user must be active, and
    /// `APIToken.objects` (a `SoftDeletionManager`) hides soft-deleted rows.
    LOAD_API_TOKEN = "auth_load_api_token",
    "SELECT t.id, t.token, u.id AS user_id, u.password, u.username, u.email, u.display_name, \
     u.first_name, u.last_name, u.user_timezone, u.is_active, u.is_staff, \
     u.is_superuser, u.is_email_verified, u.is_bot, u.date_joined \
     FROM api_tokens t \
     JOIN users u ON u.id = t.user_id \
     WHERE (t.expired_at > now() OR t.expired_at IS NULL) \
       AND t.token = $1 \
       AND t.is_active \
       AND u.is_active \
       AND t.deleted_at IS NULL",
    [
        ("id", Uuid),
        ("token", Varchar),
        ("user_id", Uuid),
        ("password", Varchar),
        ("username", Varchar),
        ("email", Varchar),
        ("display_name", Varchar),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("user_timezone", Varchar),
        ("is_active", Bool),
        ("is_staff", Bool),
        ("is_superuser", Bool),
        ("is_email_verified", Bool),
        ("is_bot", Bool),
        ("date_joined", Timestamptz),
    ]
}

plane_query! {
    /// `api_token.save(update_fields=["last_used"])`
    /// (`plane/api/middleware/api_authentication.py:42`): **only** that column.
    /// `updated_at`, `updated_by` and the audit columns are not written
    /// (MODEL_HOOKS.md §1.6 "`update_fields` wins", §4.3).
    TOUCH_API_TOKEN = "auth_touch_api_token",
    "UPDATE api_tokens SET last_used = $2 WHERE id = $1",
    []
}

plane_query! {
    /// `AbstractBaseUser.check_password`'s `setter`:
    /// `save(update_fields=["password"])`
    /// (`django/contrib/auth/base_user.py:106-112`). Only that column:
    /// `User.save`'s token rotation and `updated_at` stay in memory when
    /// `update_fields` excludes them (MODEL_HOOKS.md §1.6).
    UPDATE_USER_PASSWORD = "auth_update_user_password",
    "UPDATE users SET password = $2 WHERE id = $1",
    []
}

/// Every catalogued statement, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &LOAD_SESSION_BY_KEY,
    &SESSION_EXISTS,
    &INSERT_SESSION,
    &UPDATE_SESSION,
    &DELETE_SESSION,
    &LOAD_USER_BY_ID,
    &LOAD_USER_BY_EMAIL,
    &LOAD_API_TOKEN,
    &TOUCH_API_TOKEN,
    &UPDATE_USER_PASSWORD,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_complete_and_unique() {
        let source = include_str!("queries.rs");
        // `concat!` keeps this match from counting its own pattern string.
        let declarations = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declarations,
            ALL_QUERIES.len(),
            "queries.rs declares {declarations} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );

        let mut names = BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(!query.name.is_empty());
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
            let mut columns = BTreeSet::new();
            for column in query.columns {
                assert!(
                    columns.insert(column.name),
                    "{} repeats column {:?}",
                    query.name,
                    column.name
                );
            }
        }
    }
}
