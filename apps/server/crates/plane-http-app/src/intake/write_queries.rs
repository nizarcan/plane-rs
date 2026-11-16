// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog shared by the intake-issue write routes
//! (`plans/INTAKE.md` set P0).
//!
//! Only statements two sets need live here; each write set keeps its own
//! response/re-fetch statements in its own `*_queries.rs` (`plans/INTAKE.md`
//! §2e). `tests/schema_check.rs` prepares every entry against a migrated
//! database.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// The link row both `IntakeIssueViewSet.partial_update` and
    /// `IntakeIssueViewSet.destroy` fetch by the work item's id
    /// (`plane/app/views/intake/base.py:341-346`, `:557-562`):
    ///
    /// ```python
    /// IntakeIssue.objects.get(
    ///     issue_id=pk,
    ///     workspace__slug=slug,
    ///     project_id=project_id,
    ///     intake_id=intake_id,
    /// )
    /// ```
    ///
    /// `IntakeIssue.objects` is the default soft-deletion manager
    /// (`plane/db/mixins.py:57`), so a soft-deleted link is a
    /// `DoesNotExist`/404 — unlike `intake_issue.issue`/`.project`, which
    /// dereference through `_base_manager`. The workspace join is the slug
    /// lookup; `intake_id` is the `Intake.objects.filter(...).first()` result
    /// the view passes in, which is why a project without an intake reaches the
    /// query with `intake_id = NULL` and gets no row (Django raises
    /// `DoesNotExist` from the cast, the family handler's 500 in C/D's port).
    ///
    /// The select list is [`plane_db::models::IntakeIssue::COLUMNS`] in
    /// `from_row` order, so `IntakeIssue::from_row(row, 0)` decodes it. Django
    /// runs `get()`, which evaluates **every** matching row to raise
    /// `MultipleObjectsReturned`; `LIMIT 2` keeps that distinction with one
    /// row of overhead (`intake_issues` has no unique constraint on
    /// `(intake_id, issue_id)`).
    ///
    /// Bind order: `$1` intake id, `$2` issue id, `$3` project id,
    /// `$4` workspace slug.
    INTAKE_ISSUE_FOR_ISSUE = "intake_write_issue_for_issue",
    "SELECT ii.\"created_at\", ii.\"updated_at\", ii.\"created_by_id\", \
     ii.\"updated_by_id\", ii.\"deleted_at\", ii.\"id\", ii.\"project_id\", \
     ii.\"workspace_id\", ii.\"intake_id\", ii.\"issue_id\", ii.\"status\", \
     ii.\"snoozed_till\", ii.\"duplicate_to_id\", ii.\"source\", \
     ii.\"source_email\", ii.\"external_source\", ii.\"external_id\", ii.\"extra\" \
     FROM intake_issues ii \
     INNER JOIN workspaces ON ii.\"workspace_id\" = workspaces.\"id\" \
     WHERE ii.\"deleted_at\" IS NULL \
       AND ii.\"intake_id\" = $1 AND ii.\"issue_id\" = $2 \
       AND ii.\"project_id\" = $3 AND workspaces.\"slug\" = $4 \
     LIMIT 2",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("intake_id", Uuid),
        ("issue_id", Uuid),
        ("status", Int4),
        ("snoozed_till", Timestamptz),
        ("duplicate_to_id", Uuid),
        ("source", Varchar),
        ("source_email", Text),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("extra", Jsonb),
    ]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&INTAKE_ISSUE_FOR_ISSUE];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "intake/write_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
