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
///
/// Statements that belong to one library crate (`sessions` and `users` in
/// `plane-auth`, for example) are declared in that crate's own catalog and
/// checked by its own `tests/schema_check.rs`; they are not repeated here.
pub static ALL_QUERIES: &[&Query] = &[&LOAD_INSTANCE, &LOAD_INSTANCE_CONFIGURATIONS];

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
