// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Statements owned by the intake-issue delete route (`plans/INTAKE.md` set
//! D). The link lookup and the intake lookup are shared and live in
//! [`super::write_queries`] and [`super::queries`].
//!
//! `tests/schema_check.rs` prepares every entry against a migrated database.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Issue.objects.filter(workspace__slug=slug, project_id=project_id,
    /// pk=pk).first()` in `IntakeIssueViewSet.destroy`
    /// (`plane/app/views/intake/base.py:567`).
    ///
    /// `Issue.objects` is the default `SoftDeletionManager`
    /// (`plane/db/mixins.py:56-58`) — **not** `issue_objects` — so a
    /// soft-deleted issue is `None`, which the view then calls `.delete()` on:
    /// the `AttributeError`/500. The select list is
    /// [`plane_db::models::Issue::COLUMNS`], so `Issue::from_row(row, 0)`
    /// decodes it, and it matches `issues/write_issue_queries.rs`'s
    /// `ISSUE_FOR_DELETE` because both feed the same full `Issue.save()`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` issue id.
    ISSUE_FOR_INTAKE_DELETE = "intake_write_issue_for_delete",
    "SELECT issues.created_at, issues.updated_at, issues.created_by_id, \
     issues.updated_by_id, issues.deleted_at, issues.id, issues.project_id, \
     issues.workspace_id, issues.parent_id, issues.state_id, issues.point, \
     issues.estimate_point_id, issues.name, issues.description_json, \
     issues.description_html, issues.description_stripped, \
     issues.description_binary, issues.priority, issues.start_date, \
     issues.target_date, issues.sequence_id, issues.sort_order, \
     issues.completed_at, issues.archived_at, issues.is_draft, \
     issues.external_source, issues.external_id, issues.type_id \
     FROM issues \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
       AND workspaces.slug = $1 AND issues.project_id = $2 AND issues.id = $3 \
     LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("parent_id", Uuid),
        ("state_id", Uuid),
        ("point", Int4),
        ("estimate_point_id", Uuid),
        ("name", Varchar),
        ("description_json", Jsonb),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_binary", Bytea),
        ("priority", Varchar),
        ("start_date", Date),
        ("target_date", Date),
        ("sequence_id", Int4),
        ("sort_order", Float8),
        ("completed_at", Timestamptz),
        ("archived_at", Date),
        ("is_draft", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("type_id", Uuid),
    ]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&ISSUE_FOR_INTAKE_DELETE];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_intake_issue_delete_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "intake/write_intake_issue_delete_queries.rs declares {declared} statements but \
             ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
