// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL of `/api/instances/configurations/`
//! (`plane/license/api/views/configuration.py:30-59`).
//!
//! | Statement | Python |
//! | --- | --- |
//! | `ALL_CONFIGURATIONS` | `InstanceConfiguration.objects.all()` (`:36`) |
//! | `CONFIGURATIONS_BY_KEY` | `.filter(key__in=request.data.keys())` (`:46`) |
//! | `UPDATE_CONFIGURATION_VALUES` | `.bulk_update(values, ["value"], batch_size=100)` (`:56`) |
//!
//! `InstanceConfiguration.objects` is `SoftDeletionManager`
//! (`plane/db/mixins.py:57`), so the two reads carry `deleted_at IS NULL`;
//! `bulk_update` filters only by primary key, so the write does **not**.
//! `Meta.ordering = ("-created_at",)` (`plane/license/models/instance.py:83`)
//! is the order of both reads, and therefore of the response body.
//!
//! The write is one statement for the whole set rather than Django's batched
//! `CASE WHEN`, because only the resulting rows are observable: `bulk_update`
//! writes `value` and nothing else — no `updated_at`, no audit column, no
//! `save()` hook, and no insert for a key that does not exist yet
//! (`PUBLIC_INSTANCES.md` §2d).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `InstanceConfiguration.objects.all()` (`plane/license/api/views/configuration.py:36`).
    ///
    /// No binds.
    ALL_CONFIGURATIONS = "instances_all_configurations",
    "SELECT c.\"created_at\", c.\"updated_at\", c.\"created_by_id\", \
     c.\"updated_by_id\", c.\"deleted_at\", c.\"id\", c.\"key\", c.\"value\", \
     c.\"category\", c.\"is_encrypted\" \
     FROM instance_configurations c \
     WHERE c.\"deleted_at\" IS NULL \
     ORDER BY c.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("key", Varchar),
        ("value", Text),
        ("category", Text),
        ("is_encrypted", Bool),
    ]
}

plane_query! {
    /// `InstanceConfiguration.objects.filter(key__in=request.data.keys())`
    /// (`plane/license/api/views/configuration.py:46`).
    ///
    /// Django renders `key IN (%s, …)`; `key = ANY($1)` is the same predicate
    /// with one bind. An empty body is `EmptyResultSet` in Django — the caller
    /// must skip the statement rather than send an empty array
    /// (`InstanceConfigurationEndpoint.patch` does the same through
    /// `bulk_update([])` returning 0).
    ///
    /// Bind order: `$1` the keys, `text[]`.
    CONFIGURATIONS_BY_KEY = "instances_configurations_by_key",
    "SELECT c.\"created_at\", c.\"updated_at\", c.\"created_by_id\", \
     c.\"updated_by_id\", c.\"deleted_at\", c.\"id\", c.\"key\", c.\"value\", \
     c.\"category\", c.\"is_encrypted\" \
     FROM instance_configurations c \
     WHERE c.\"deleted_at\" IS NULL AND c.\"key\" = ANY($1::text[]) \
     ORDER BY c.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("key", Varchar),
        ("value", Text),
        ("category", Text),
        ("is_encrypted", Bool),
    ]
}

plane_query! {
    /// `InstanceConfiguration.objects.bulk_update(values, ["value"], batch_size=100)`
    /// (`plane/license/api/views/configuration.py:56`).
    ///
    /// Django builds
    /// `UPDATE … SET value = CASE WHEN id = %s THEN %s::text … ELSE value END WHERE id IN (…)`;
    /// setting the same column from an `unnest`ed pair list is observably the
    /// same write. Neither form touches `updated_at`, `updated_by_id` or any
    /// other column — `bulk_update` bypasses `save()` — and neither inserts a
    /// row for an unknown key.
    ///
    /// The two arrays must be the same length and in the same order. Bind
    /// order: `$1` ids (`uuid[]`), `$2` values (`text[]`, `NULL` allowed).
    UPDATE_CONFIGURATION_VALUES = "instances_update_configuration_values",
    "UPDATE instance_configurations c SET \"value\" = v.\"value\" \
     FROM (SELECT unnest($1::uuid[]) AS \"id\", unnest($2::text[]) AS \"value\") v \
     WHERE c.\"id\" = v.\"id\"",
    []
}

/// Every statement in this catalog, for `tests/schema_check.rs`.
pub const ALL_QUERIES: &[&Query] = &[
    &ALL_CONFIGURATIONS,
    &CONFIGURATIONS_BY_KEY,
    &UPDATE_CONFIGURATION_VALUES,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_names_are_unique() {
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
        }
    }

    /// Both reads select `InstanceConfiguration::COLUMNS` under the alias `c`,
    /// so `InstanceConfiguration::from_row(row, 0)` decodes them.
    #[test]
    fn the_reads_select_the_model_columns() {
        let expected: Vec<String> =
            plane_db::models::license::instance::InstanceConfiguration::COLUMNS
                .split(", ")
                .map(|column| {
                    let column = column.trim();
                    let start = column.find('"').expect("quoted column") + 1;
                    let end = column.rfind('"').expect("quoted column");
                    format!("c.\"{}\"", &column[start..end])
                })
                .collect();
        let expected = format!("SELECT {} FROM", expected.join(", "));
        for query in [&ALL_CONFIGURATIONS, &CONFIGURATIONS_BY_KEY] {
            assert!(
                query.sql.starts_with(&expected),
                "{} does not select InstanceConfiguration::COLUMNS under `c`:\n{}",
                query.name,
                query.sql
            );
        }
    }

    /// §2d: `bulk_update(values, ["value"])` writes one column and does not
    /// filter on `deleted_at` (it updates by primary key only).
    #[test]
    fn the_write_sets_only_value() {
        assert!(
            UPDATE_CONFIGURATION_VALUES
                .sql
                .starts_with("UPDATE instance_configurations c SET \"value\" = v.\"value\" FROM")
        );
        assert!(!UPDATE_CONFIGURATION_VALUES.sql.contains("updated_at"));
        assert!(!UPDATE_CONFIGURATION_VALUES.sql.contains("deleted_at"));
        assert!(!UPDATE_CONFIGURATION_VALUES.sql.contains("INSERT"));
    }
}
