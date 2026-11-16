// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Statements owned by the intake write routes (`plans/INTAKE.md` set A).
//!
//! `tests/schema_check.rs` prepares every entry against a migrated database.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Intake.objects.filter(workspace__slug=slug, project_id=project_id,
    /// pk=pk).first()` (`plane/app/views/intake/base.py:85`).
    ///
    /// `Intake.objects` is the default `SoftDeletionManager`
    /// (`plane/db/mixins.py:56-58`), so `deleted_at IS NULL` is part of the
    /// predicate — a soft-deleted intake is `None`, the handler's 500. The
    /// `ORDER BY name` is `Intake.Meta.ordering = ("name",)`
    /// (`plane/db/models/intake.py:39`) plus `.first()`'s `LIMIT 1`.
    ///
    /// Only `is_default` is read: the delete itself is H03's pk-keyed
    /// `UPDATE` (`plane_db::hooks::soft_delete`), and no other column of the
    /// row is observable.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` intake id.
    INTAKE_FOR_DESTROY = "intake_write_for_destroy",
    "SELECT i.\"is_default\" FROM \"intakes\" i \
     INNER JOIN \"workspaces\" w ON (i.\"workspace_id\" = w.\"id\") \
     WHERE i.\"deleted_at\" IS NULL AND i.\"project_id\" = $2 AND w.\"slug\" = $1 \
       AND i.\"id\" = $3 \
     ORDER BY i.\"name\" ASC LIMIT 1",
    [("is_default", Bool)]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&INTAKE_FOR_DESTROY];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_intake_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "intake/write_intake_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
