// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The seeded query catalog.
//!
//! These are the first statements later phases need (instance, session, user
//! and instance-configuration reads). Adding a query means adding a
//! [`plane_query!`](crate::plane_query) invocation **and** listing it in
//! [`ALL_QURIES`]; `catalog_is_complete` (below) fails the test suite otherwise.
//!
//! Column names and types were read from a clone of the `plane_tpl` template
//! database (Django's migrations, unmodified) and are re-checked by the
//! `schema_check` integration test.

use crate::catalog::Query;
use crate::plane_query;

plane_query! {
    /// The instance row Plane registers on first boot
    /// (`plane/license/management/commands/register_instance.py`).
    LOAD_INSTANCE = "load_instance",
    "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
     instance_name, whitelist_emails, instance_id, current_version, latest_version, \
     edition, domain, last_checked_at, namespace, is_telemetry_enabled, \
     is_support_required, is_setup_done, is_signup_screen_visited, is_verified, \
     is_test, is_current_version_deprecated \
     FROM instances \
     ORDER BY created_at DESC \
     LIMIT 1",
    [
        ("id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("instance_name", Varchar),
        ("whitelist_emails", Text),
        ("instance_id", Varchar),
        ("current_version", Varchar),
        ("latest_version", Varchar),
        ("edition", Varchar),
        ("domain", Text),
        ("last_checked_at", Timestamptz),
        ("namespace", Varchar),
        ("is_telemetry_enabled", Bool),
        ("is_support_required", Bool),
        ("is_setup_done", Bool),
        ("is_signup_screen_visited", Bool),
        ("is_verified", Bool),
        ("is_test", Bool),
        ("is_current_version_deprecated", Bool),
    ]
}

plane_query! {
    /// Django's `SessionStore.load`: `SELECT … WHERE session_key = %s AND
    /// expire_date > now()` (`django/contrib/sessions/backends/db.py`).
    LOAD_SESSION_BY_KEY = "load_session_by_key",
    "SELECT session_key, session_data, expire_date, user_id, device_info \
     FROM sessions \
     WHERE session_key = $1 AND expire_date > now()",
    [
        ("session_key", Varchar),
        ("session_data", Text),
        ("expire_date", Timestamptz),
        ("user_id", Varchar),
        ("device_info", Jsonb),
    ]
}

plane_query! {
    /// `AuthenticationMiddleware`'s `get_user` reads the user by id
    /// (`django/contrib/auth/__init__.py`). The model does not use `BaseModel`,
    /// so there is no `deleted_at` column (verified against the template DB).
    LOAD_USER_BY_ID = "load_user_by_id",
    "SELECT id, created_at, updated_at, last_login, username, email, display_name, \
     first_name, last_name, avatar, avatar_asset_id, is_active, is_staff, is_superuser, \
     is_email_verified, is_bot, user_timezone, last_login_time, date_joined \
     FROM users \
     WHERE id = $1",
    [
        ("id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("last_login", Timestamptz),
        ("username", Varchar),
        ("email", Varchar),
        ("display_name", Varchar),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("is_active", Bool),
        ("is_staff", Bool),
        ("is_superuser", Bool),
        ("is_email_verified", Bool),
        ("is_bot", Bool),
        ("user_timezone", Varchar),
        ("last_login_time", Timestamptz),
        ("date_joined", Timestamptz),
    ]
}

plane_query! {
    /// `get_configuration_value` reads the whole table once
    /// (`plane/license/utils/instance_value.py:17`); soft-deleted rows are
    /// invisible.
    LOAD_INSTANCE_CONFIGURATIONS = "load_instance_configurations",
    "SELECT id, key, value, category, is_encrypted \
     FROM instance_configurations \
     WHERE deleted_at IS NULL \
     ORDER BY key",
    [
        ("id", Uuid),
        ("key", Varchar),
        ("value", Text),
        ("category", Text),
        ("is_encrypted", Bool),
    ]
}

/// Every catalogued statement, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &LOAD_INSTANCE,
    &LOAD_SESSION_BY_KEY,
    &LOAD_USER_BY_ID,
    &LOAD_INSTANCE_CONFIGURATIONS,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `plane_query!` in this file must appear in `ALL_QUERIES`, and names
    /// must be unique. This is the guard the catalog aggregation relies on (see
    /// `crate::catalog`).
    #[test]
    fn catalog_is_complete() {
        let source = include_str!("queries.rs");
        // `concat!` keeps this match from counting its own pattern string.
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );

        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(!query.name.is_empty());
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
            assert!(!query.columns.is_empty(), "{} has no columns", query.name);
            let mut columns = std::collections::BTreeSet::new();
            for column in query.columns {
                assert!(
                    columns.insert(column.name),
                    "{} repeats column {:?}",
                    query.name,
                    column.name
                );
            }
            assert!(
                query.sql.trim_start().starts_with("SELECT"),
                "{} should be a SELECT",
                query.name
            );
        }
    }
}
