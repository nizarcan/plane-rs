// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the intake-issue create (`plans/INTAKE.md` set B,
//! orders 55 and 59, `POST …/intake-issues/` and `POST …/inbox-issues/`).
//!
//! The statements are the ORM SQL `IntakeIssueViewSet.create`
//! (`plane/app/views/intake/base.py:229-331`) emits, in the view's own order:
//!
//! | View statement | Here |
//! | --- | --- |
//! | `Project.objects.get(pk=project_id)` (`:244`) | [`PROJECT_FOR_INTAKE_CREATE`] |
//! | `State.triage_objects.filter(project_id, workspace__slug).first()` (`:247-249`) | [`TRIAGE_STATE_FOR_CREATE`] |
//! | `State.objects.create(...)` (`:250-257`) | `states::queries::{STATE_SEQUENCE_MAX, STATE_INSERT}` |
//! | `IntakeIssue.objects.create(...)` (`:272-279`) | [`INTAKE_ISSUE_INSERT`] |
//! | the create re-fetch (`:300-329`) | [`INTAKE_ISSUE_FOR_CREATE`] |
//!
//! `Intake.objects.filter(...).first()` reuses the group's shared
//! [`super::queries::INTAKE_ONLY`] (`plans/INTAKE.md` §2). The issue, its
//! sequence row and its assignee/label rows reuse `issues::write_issue` and
//! `plane_db::hooks`, and the issue serializer's own project checks stay in
//! `issues::write_issue_queries`.
//!
//! `tests/schema_check.rs` prepares every entry against a migrated database.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Project.objects.get(pk=project_id)` (`plane/app/views/intake/base.py:244`):
    /// the default soft-deletion manager and **no** workspace or archived
    /// filter, so a project of another workspace is accepted (upstream shape).
    /// Unlike the issue-create view, this one also reads the project's
    /// `workspace_id` for the triage fallback and its `default_assignee_id` for
    /// `IssueCreateSerializer.create`'s assignee fallback.
    ///
    /// Bind order: `$1` project id.
    PROJECT_FOR_INTAKE_CREATE = "intake_write_issue_project_for_create",
    "SELECT p.\"workspace_id\", p.\"default_assignee_id\" \
     FROM projects p \
     WHERE p.\"deleted_at\" IS NULL AND p.id = $1 \
     LIMIT 1",
    [
        ("workspace_id", Uuid),
        ("default_assignee_id", Uuid),
    ]
}

plane_query! {
    /// `State.triage_objects.filter(project_id=project_id,
    /// workspace__slug=slug).first()` (`plane/app/views/intake/base.py:247-249`).
    ///
    /// `State.triage_objects` is `TriageStateManager` (`plane/db/models/state.py:62-68`):
    /// `deleted_at IS NULL AND group = 'triage'`. `Meta.ordering = ("sequence",)`
    /// makes `.first()` `ORDER BY sequence ASC LIMIT 1`.
    ///
    /// Bind order: `$1` project id, `$2` workspace slug.
    TRIAGE_STATE_FOR_CREATE = "intake_write_issue_triage_state",
    "SELECT s.\"id\" FROM states s \
     INNER JOIN workspaces w ON (s.\"workspace_id\" = w.\"id\") \
     WHERE s.\"deleted_at\" IS NULL AND s.\"group\" = 'triage' \
       AND s.\"project_id\" = $1 AND w.\"slug\" = $2 \
     ORDER BY s.\"sequence\" ASC LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `IntakeIssue.objects.create(intake_id=…, project_id=…, issue_id=…,
    /// source=SourceType.IN_APP)` (`plane/app/views/intake/base.py:272-279`).
    ///
    /// `ProjectBaseModel.save` fills `workspace_id` from the project
    /// (`plane/db/models/project.py:236-239`), `BaseModel.save` (H01) sets
    /// `created_by_id` to the request user and leaves `updated_by_id` NULL, and
    /// the model defaults give `status = -2`, `snoozed_till = NULL`,
    /// `duplicate_to_id = NULL`, the other source columns NULL and
    /// `extra = {}`. H16 (`plane/db/models/intake.py:100-109`) only acts on a
    /// non-NULL `snoozed_till`, and this view never supplies one — the body's
    /// `snoozed_till` (top level or inside `issue`) is not a create argument.
    INTAKE_ISSUE_INSERT = "intake_write_issue_insert",
    "INSERT INTO intake_issues (created_at, updated_at, created_by_id, \
     updated_by_id, deleted_at, id, project_id, workspace_id, intake_id, \
     issue_id, status, snoozed_till, duplicate_to_id, source, source_email, \
     external_source, external_id, extra) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
     $15, $16, $17, $18)",
    []
}

plane_query! {
    /// The create view's response re-fetch (`plane/app/views/intake/base.py:300-329`):
    ///
    /// ```python
    /// IntakeIssue.objects.select_related("issue")
    ///     .prefetch_related("issue__labels", "issue__assignees")
    ///     .annotate(label_ids=…, assignee_ids=…)
    ///     .get(intake_id=intake_id.id, issue_id=serializer.data["id"],
    ///          project_id=project_id)
    /// ```
    ///
    /// It is **not** `retrieve`'s statement: the assignee aggregate filters
    /// `issue__assignees__member_project__is_active` instead of
    /// `issue__issue_assignee__deleted_at` (`:314-322` vs `:523-533`), so a
    /// soft-deleted through row is still listed as long as the user has an
    /// active `ProjectMember` row (in **any** project: the join on
    /// `project_members` is by `member_id` only). The two prefetch queries read
    /// relations no serializer field uses (both use the annotations), so they
    /// are not run.
    ///
    /// `.get()` clears `IntakeIssue.Meta.ordering` and caps the row count at 21
    /// (`django/db/models/query.py` `QuerySet.get`), which is kept `LIMIT 21`
    /// so a duplicated link stays Django's `MultipleObjectsReturned`.
    ///
    /// The select list matches [`super::queries::INTAKE_ISSUE_DETAIL`]'s: 18
    /// `IntakeIssue` columns, `label_ids`, `assignee_ids`, then the 28 `issues`
    /// columns in `plane_db::models::issue::Issue` order.
    ///
    /// Bind order: `$1` intake id, `$2` issue id, `$3` project id.
    INTAKE_ISSUE_FOR_CREATE = "intake_write_issue_for_create",
    "SELECT ii.\"created_at\", ii.\"updated_at\", ii.\"created_by_id\", \
     ii.\"updated_by_id\", ii.\"deleted_at\", ii.\"id\", ii.\"project_id\", \
     ii.\"workspace_id\", ii.\"intake_id\", ii.\"issue_id\", ii.\"status\", \
     ii.\"snoozed_till\", ii.\"duplicate_to_id\", ii.\"source\", \
     ii.\"source_email\", ii.\"external_source\", ii.\"external_id\", ii.\"extra\", \
     COALESCE(ARRAY_AGG(DISTINCT \"issue_labels\".\"label_id\") \
       FILTER (WHERE (NOT (\"issue_labels\".\"label_id\" IS NULL) \
         AND \"issue_labels\".\"deleted_at\" IS NULL)), '{}'::uuid[]) AS \"label_ids\", \
     COALESCE(ARRAY_AGG(DISTINCT \"issue_assignees\".\"assignee_id\") \
       FILTER (WHERE (NOT (\"issue_assignees\".\"assignee_id\" IS NULL) \
         AND \"project_members\".\"is_active\")), '{}'::uuid[]) AS \"assignee_ids\", \
     issues.\"created_at\" AS \"i_created_at\", issues.\"updated_at\" AS \"i_updated_at\", \
     issues.\"created_by_id\" AS \"i_created_by_id\", \
     issues.\"updated_by_id\" AS \"i_updated_by_id\", \
     issues.\"deleted_at\" AS \"i_deleted_at\", issues.\"id\" AS \"i_id\", \
     issues.\"project_id\" AS \"i_project_id\", \
     issues.\"workspace_id\" AS \"i_workspace_id\", \
     issues.\"parent_id\" AS \"i_parent_id\", issues.\"state_id\" AS \"i_state_id\", \
     issues.\"point\" AS \"i_point\", \
     issues.\"estimate_point_id\" AS \"i_estimate_point_id\", \
     issues.\"name\" AS \"i_name\", \
     issues.\"description_json\" AS \"i_description_json\", \
     issues.\"description_html\" AS \"i_description_html\", \
     issues.\"description_stripped\" AS \"i_description_stripped\", \
     issues.\"description_binary\" AS \"i_description_binary\", \
     issues.\"priority\" AS \"i_priority\", \
     issues.\"start_date\" AS \"i_start_date\", \
     issues.\"target_date\" AS \"i_target_date\", \
     issues.\"sequence_id\" AS \"i_sequence_id\", \
     issues.\"sort_order\" AS \"i_sort_order\", \
     issues.\"completed_at\" AS \"i_completed_at\", \
     issues.\"archived_at\" AS \"i_archived_at\", \
     issues.\"is_draft\" AS \"i_is_draft\", \
     issues.\"external_source\" AS \"i_external_source\", \
     issues.\"external_id\" AS \"i_external_id\", \
     issues.\"type_id\" AS \"i_type_id\" \
     FROM \"intake_issues\" ii \
     INNER JOIN \"issues\" ON (ii.\"issue_id\" = issues.\"id\") \
     LEFT OUTER JOIN \"issue_labels\" ON (issues.\"id\" = \"issue_labels\".\"issue_id\") \
     LEFT OUTER JOIN \"issue_assignees\" ON (issues.\"id\" = \"issue_assignees\".\"issue_id\") \
     LEFT OUTER JOIN \"users\" ON (\"issue_assignees\".\"assignee_id\" = \"users\".\"id\") \
     LEFT OUTER JOIN \"project_members\" ON (\"users\".\"id\" = \"project_members\".\"member_id\") \
     WHERE ii.\"deleted_at\" IS NULL AND ii.\"intake_id\" = $1 \
       AND ii.\"issue_id\" = $2 AND ii.\"project_id\" = $3 \
     GROUP BY ii.\"id\", issues.\"id\" LIMIT 21",
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
        ("label_ids", ArrayOf(Uuid)),
        ("assignee_ids", ArrayOf(Uuid)),
        ("i_created_at", Timestamptz),
        ("i_updated_at", Timestamptz),
        ("i_created_by_id", Uuid),
        ("i_updated_by_id", Uuid),
        ("i_deleted_at", Timestamptz),
        ("i_id", Uuid),
        ("i_project_id", Uuid),
        ("i_workspace_id", Uuid),
        ("i_parent_id", Uuid),
        ("i_state_id", Uuid),
        ("i_point", Int4),
        ("i_estimate_point_id", Uuid),
        ("i_name", Varchar),
        ("i_description_json", Jsonb),
        ("i_description_html", Text),
        ("i_description_stripped", Text),
        ("i_description_binary", Bytea),
        ("i_priority", Varchar),
        ("i_start_date", Date),
        ("i_target_date", Date),
        ("i_sequence_id", Int4),
        ("i_sort_order", Float8),
        ("i_completed_at", Timestamptz),
        ("i_archived_at", Date),
        ("i_is_draft", Bool),
        ("i_external_source", Varchar),
        ("i_external_id", Varchar),
        ("i_type_id", Uuid),
    ]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_FOR_INTAKE_CREATE,
    &TRIAGE_STATE_FOR_CREATE,
    &INTAKE_ISSUE_INSERT,
    &INTAKE_ISSUE_FOR_CREATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_intake_issue_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "intake/write_intake_issue_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
