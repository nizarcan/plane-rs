// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the v1 intake routes (288/289).
//!
//! Reused from the app surface:
//!
//! | Python | Statement |
//! | --- | --- |
//! | `Intake.objects.filter(...).first()` (`api/views/intake.py:158,324,475`) | `plane_http_app::intake::queries::INTAKE_ONLY` |
//! | `IntakeIssue.objects.get(issue_id=…, …, intake_id=…)` (`:336,487`) | `plane_http_app::intake::write_queries::INTAKE_ISSUE_FOR_ISSUE` |
//! | `State.objects.create(...)`'s H19 aggregate (`:183-191`) | `plane_http_app::states::queries::STATE_SEQUENCE_MAX` |
//! | `State.objects.create(...)` (`:183-191`) | `plane_http_app::states::queries::STATE_INSERT` |
//! | `ProjectMember.objects.filter(role=20 …).exists()` (`:501-507`) | `plane_http_app::projects::queries::PROJECT_MEMBER_ADMIN_EXISTS` |
//! | `WorkspaceMember.objects.filter(role=20 …).exists()` (`:508-510`) | `plane_http_app::projects::queries::WORKSPACE_ADMIN_EXISTS` |
//!
//! What is new here:
//!
//! * `Project.objects.get(workspace__slug, pk)` — the project lookup the three
//!   verbs share (`:160,250,326,477`);
//! * the list/detail queryset with its snooze window and the restricted-guest
//!   scope (`:66-90`, `:244-268`);
//! * `State.triage_objects.filter(...).first()` (`:180`);
//! * the two `intake_issues` INSERT/UPDATE statements;
//! * the issue's state/label/assignee loads for the nested `issue_detail`;
//! * `ProjectMember.objects.get(...)`'s role (`:344-349`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Project.objects.get(workspace__slug=slug, pk=project_id)`
    /// (`api/views/intake.py:72,160,250,326,477`). The default manager filters
    /// `deleted_at IS NULL`, so a soft-deleted project is a 404.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    PROJECT_FOR_INTAKE = "v1_intake_project",
    "SELECT p.id, p.workspace_id, p.intake_view \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 AND p.id = $2 LIMIT 21",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("intake_view", Bool),
    ]
}

plane_query! {
    /// `State.triage_objects.filter(project_id=…, workspace__slug=…).first()`
    /// (`api/views/intake.py:180`).
    ///
    /// `TriageStateManager` adds `group = 'triage'` on top of the soft-deletion
    /// predicate (`plane/db/models/state.py:70-75`); `.first()` applies
    /// `Meta.ordering = ("sequence",)`.
    ///
    /// Bind order: `$1` project id, `$2` workspace slug.
    TRIAGE_STATE_FOR_PROJECT = "v1_intake_triage_state",
    "SELECT s.id \
     FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     WHERE s.deleted_at IS NULL AND s.\"group\" = 'triage' \
       AND s.project_id = $1 AND w.slug = $2 \
     ORDER BY s.\"sequence\" ASC LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `ProjectMember.objects.get(workspace__slug, project_id, member=user,
    /// is_active=True)` (`api/views/intake.py:344-349`).
    ///
    /// A plain `.get()`, so several live rows are `MultipleObjectsReturned`
    /// (the family 500). `ProjectMember.objects` filters `deleted_at`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id.
    PROJECT_MEMBER_FOR_INTAKE = "v1_intake_project_member",
    "SELECT pm.role \
     FROM project_members pm \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND w.slug = $1 AND pm.project_id = $2 \
       AND pm.member_id = $3 AND pm.is_active \
     LIMIT 21",
    [("role", Int2)]
}

plane_query! {
    /// The list/retrieve queryset (`api/views/intake.py:77-90`, `:255-268`):
    ///
    /// ```python
    /// IntakeIssue.objects.filter(
    ///     Q(snoozed_till__gte=now) | Q(snoozed_till__isnull=True),
    ///     workspace__slug=slug, project_id=project_id, intake_id=intake.id,
    /// ).order_by("-created_at")
    /// ```
    ///
    /// `created_by_id` is the restricted-guest scope (`:89`, `:267`).
    ///
    /// Bind order: `$1` now, `$2` workspace slug, `$3` project id, `$4`
    /// intake id.
    INTAKE_ISSUES_PAGE = "v1_intake_issues_page",
    "SELECT ii.\"created_at\", ii.\"updated_at\", ii.\"created_by_id\", \
     ii.\"updated_by_id\", ii.\"deleted_at\", ii.\"id\", ii.\"project_id\", \
     ii.\"workspace_id\", ii.\"intake_id\", ii.\"issue_id\", ii.\"status\", \
     ii.\"snoozed_till\", ii.\"duplicate_to_id\", ii.\"source\", \
     ii.\"source_email\", ii.\"external_source\", ii.\"external_id\", ii.\"extra\" \
     FROM intake_issues ii \
     INNER JOIN workspaces w ON (ii.\"workspace_id\" = w.\"id\") \
     WHERE ii.\"deleted_at\" IS NULL \
       AND (ii.\"snoozed_till\" >= $1 OR ii.\"snoozed_till\" IS NULL) \
       AND w.\"slug\" = $2 AND ii.\"project_id\" = $3 AND ii.\"intake_id\" = $4 \
     ORDER BY ii.\"created_at\" DESC",
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

plane_query! {
    /// [`INTAKE_ISSUES_PAGE`] with the restricted-guest scope
    /// (`api/views/intake.py:89`, `:267`).
    ///
    /// Bind order: `$1` now, `$2` workspace slug, `$3` project id, `$4`
    /// intake id, `$5` the caller's user id.
    INTAKE_ISSUES_PAGE_FOR_CREATOR = "v1_intake_issues_page_for_creator",
    "SELECT ii.\"created_at\", ii.\"updated_at\", ii.\"created_by_id\", \
     ii.\"updated_by_id\", ii.\"deleted_at\", ii.\"id\", ii.\"project_id\", \
     ii.\"workspace_id\", ii.\"intake_id\", ii.\"issue_id\", ii.\"status\", \
     ii.\"snoozed_till\", ii.\"duplicate_to_id\", ii.\"source\", \
     ii.\"source_email\", ii.\"external_source\", ii.\"external_id\", ii.\"extra\" \
     FROM intake_issues ii \
     INNER JOIN workspaces w ON (ii.\"workspace_id\" = w.\"id\") \
     WHERE ii.\"deleted_at\" IS NULL \
       AND (ii.\"snoozed_till\" >= $1 OR ii.\"snoozed_till\" IS NULL) \
       AND w.\"slug\" = $2 AND ii.\"project_id\" = $3 AND ii.\"intake_id\" = $4 \
       AND ii.\"created_by_id\" = $5 \
     ORDER BY ii.\"created_at\" DESC",
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

plane_query! {
    /// The list's `COUNT(*)` (`plane/utils/paginator.py:157`): the same filters
    /// as [`INTAKE_ISSUES_PAGE`], no join projection beyond `workspaces`.
    ///
    /// Bind order: `$1` now, `$2` workspace slug, `$3` project id, `$4`
    /// intake id.
    INTAKE_ISSUES_COUNT = "v1_intake_issues_count",
    "SELECT COUNT(*) AS count \
     FROM intake_issues ii \
     INNER JOIN workspaces w ON (ii.\"workspace_id\" = w.\"id\") \
     WHERE ii.\"deleted_at\" IS NULL \
       AND (ii.\"snoozed_till\" >= $1 OR ii.\"snoozed_till\" IS NULL) \
       AND w.\"slug\" = $2 AND ii.\"project_id\" = $3 AND ii.\"intake_id\" = $4",
    [("count", Int8)]
}

plane_query! {
    /// [`INTAKE_ISSUES_COUNT`] with the restricted-guest scope.
    ///
    /// Bind order: `$1` now, `$2` workspace slug, `$3` project id, `$4`
    /// intake id, `$5` the caller's user id.
    INTAKE_ISSUES_COUNT_FOR_CREATOR = "v1_intake_issues_count_for_creator",
    "SELECT COUNT(*) AS count \
     FROM intake_issues ii \
     INNER JOIN workspaces w ON (ii.\"workspace_id\" = w.\"id\") \
     WHERE ii.\"deleted_at\" IS NULL \
       AND (ii.\"snoozed_till\" >= $1 OR ii.\"snoozed_till\" IS NULL) \
       AND w.\"slug\" = $2 AND ii.\"project_id\" = $3 AND ii.\"intake_id\" = $4 \
       AND ii.\"created_by_id\" = $5",
    [("count", Int8)]
}

plane_query! {
    /// `self.get_queryset().get(issue_id=issue_id)` — the detail GET
    /// (`api/views/intake.py:292`): the list queryset filtered by the issue.
    ///
    /// Bind order: `$1` now, `$2` workspace slug, `$3` project id, `$4`
    /// intake id, `$5` issue id.
    INTAKE_ISSUE_BY_ISSUE = "v1_intake_issue_by_issue",
    "SELECT ii.\"created_at\", ii.\"updated_at\", ii.\"created_by_id\", \
     ii.\"updated_by_id\", ii.\"deleted_at\", ii.\"id\", ii.\"project_id\", \
     ii.\"workspace_id\", ii.\"intake_id\", ii.\"issue_id\", ii.\"status\", \
     ii.\"snoozed_till\", ii.\"duplicate_to_id\", ii.\"source\", \
     ii.\"source_email\", ii.\"external_source\", ii.\"external_id\", ii.\"extra\" \
     FROM intake_issues ii \
     INNER JOIN workspaces w ON (ii.\"workspace_id\" = w.\"id\") \
     WHERE ii.\"deleted_at\" IS NULL \
       AND (ii.\"snoozed_till\" >= $1 OR ii.\"snoozed_till\" IS NULL) \
       AND w.\"slug\" = $2 AND ii.\"project_id\" = $3 AND ii.\"intake_id\" = $4 \
       AND ii.\"issue_id\" = $5 \
     LIMIT 21",
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

plane_query! {
    /// `Issue.objects.get(pk=issue_id)` through the forward FK
    /// (`intake_issue.issue`, `api/views/intake.py:366-386` and
    /// `serializers/intake.py:66`): `Issue`'s *base* manager, so a soft-deleted
    /// issue still resolves.
    ///
    /// Bind order: `$1` issue id.
    ISSUE_BY_ID_ANY_STATE = "v1_intake_issue_by_id",
    "SELECT i.\"created_at\", i.\"updated_at\", i.\"created_by_id\", \
     i.\"updated_by_id\", i.\"deleted_at\", i.\"id\", i.\"project_id\", \
     i.\"workspace_id\", i.\"parent_id\", i.\"state_id\", i.\"point\", \
     i.\"estimate_point_id\", i.\"name\", i.\"description_json\", \
     i.\"description_html\", i.\"description_stripped\", \
     i.\"description_binary\", i.\"priority\", i.\"start_date\", \
     i.\"target_date\", i.\"sequence_id\", i.\"sort_order\", \
     i.\"completed_at\", i.\"archived_at\", i.\"is_draft\", \
     i.\"external_source\", i.\"external_id\", i.\"type_id\" \
     FROM issues i WHERE i.id = $1 LIMIT 21",
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

plane_query! {
    /// `Issue.objects.filter(workspace__slug, project_id, pk=issue_id).first()`
    /// — the delete's authorisation row (`api/views/intake.py:498`). The
    /// default manager filters `deleted_at IS NULL`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` issue id.
    ISSUE_FOR_INTAKE_DELETE = "v1_intake_issue_for_delete",
    "SELECT i.\"created_at\", i.\"updated_at\", i.\"created_by_id\", \
     i.\"updated_by_id\", i.\"deleted_at\", i.\"id\", i.\"project_id\", \
     i.\"workspace_id\", i.\"parent_id\", i.\"state_id\", i.\"point\", \
     i.\"estimate_point_id\", i.\"name\", i.\"description_json\", \
     i.\"description_html\", i.\"description_stripped\", \
     i.\"description_binary\", i.\"priority\", i.\"start_date\", \
     i.\"target_date\", i.\"sequence_id\", i.\"sort_order\", \
     i.\"completed_at\", i.\"archived_at\", i.\"is_draft\", \
     i.\"external_source\", i.\"external_id\", i.\"type_id\" \
     FROM issues i \
     INNER JOIN workspaces w ON (i.\"workspace_id\" = w.\"id\") \
     WHERE i.\"deleted_at\" IS NULL AND w.\"slug\" = $1 AND i.\"project_id\" = $2 \
       AND i.\"id\" = $3 \
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

plane_query! {
    /// The state of each issue for `issue_detail.state`, read through the
    /// forward FK's base manager (`api/views/intake.py:66`,
    /// `serializers/intake.py:66`).
    ///
    /// Bind order: `$1` state ids.
    STATES_LITE_BY_IDS = "v1_intake_states_lite",
    "SELECT s.id, s.name, s.color, s.\"group\" FROM states s \
     WHERE s.id = ANY($1)",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("color", Varchar),
        ("group", Varchar),
    ]
}

plane_query! {
    /// `obj.label_issue.all()` for `issue_detail.labels` (`issue.py:860-865`):
    /// `IssueLabel`'s soft-deletion manager and `Meta.ordering =
    /// ("-created_at",)`.
    ///
    /// Bind order: `$1` issue ids.
    ISSUE_LABEL_IDS = "v1_intake_issue_label_ids",
    "SELECT il.issue_id, il.label_id FROM issue_labels il \
     WHERE il.deleted_at IS NULL AND il.issue_id = ANY($1) \
     ORDER BY il.issue_id ASC, il.created_at DESC",
    [("issue_id", Uuid), ("label_id", Uuid)]
}

plane_query! {
    /// `obj.issue_assignee.all()` for `issue_detail.assignees`
    /// (`issue.py:867-871`): the same manager and ordering.
    ///
    /// Bind order: `$1` issue ids.
    ISSUE_ASSIGNEE_IDS = "v1_intake_issue_assignee_ids",
    "SELECT ia.issue_id, ia.assignee_id FROM issue_assignees ia \
     WHERE ia.deleted_at IS NULL AND ia.issue_id = ANY($1) \
     ORDER BY ia.issue_id ASC, ia.created_at DESC",
    [("issue_id", Uuid), ("assignee_id", Uuid)]
}

plane_query! {
    /// `Issue.objects.create(...)` (`api/views/intake.py:201-208`) — the row
    /// without the model hooks, which `plane_db::hooks::issue::create_issue_row`
    /// runs. The column order is `Issue::from_row`'s.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, project, workspace, parent, state, point, estimate_point, name,
    /// description_json, description_html, description_stripped,
    /// description_binary, priority, start_date, target_date, sequence_id,
    /// sort_order, completed_at, archived_at, is_draft, external_source,
    /// external_id, type_id.
    ISSUE_INSERT = "v1_intake_issue_insert",
    "INSERT INTO issues (created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, id, project_id, workspace_id, parent_id, state_id, point, \
     estimate_point_id, name, description_json, description_html, \
     description_stripped, description_binary, priority, start_date, \
     target_date, sequence_id, sort_order, completed_at, archived_at, is_draft, \
     external_source, external_id, type_id) VALUES \
     ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
      $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)",
    []
}

plane_query! {
    /// `IntakeIssue.objects.create(intake_id=…, project_id=…, issue=…,
    /// source="IN_APP")` (`api/views/intake.py:211-216`). The model defaults:
    /// `status=-2`, `snoozed_till` NULL, `duplicate_to` NULL, `source_email`
    /// NULL, `external_*` NULL, `extra={}`.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, project, workspace, intake, issue, status, snoozed_till,
    /// duplicate_to, source, source_email, external_source, external_id, extra.
    INTAKE_ISSUE_INSERT = "v1_intake_link_insert",
    "INSERT INTO intake_issues (created_at, updated_at, created_by_id, \
     updated_by_id, deleted_at, id, project_id, workspace_id, intake_id, \
     issue_id, status, snoozed_till, duplicate_to_id, source, source_email, \
     external_source, external_id, extra) VALUES \
     ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
      $16, $17, $18)",
    []
}

plane_query! {
    /// `intake_issue.save()` after the PATCH's `IntakeIssueUpdateSerializer`
    /// (`plane/app/serializers/intake.py:151`, the shared `update`): a full-row
    /// `UPDATE` after H16 and H01.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, project, workspace, intake, issue, status, snoozed_till,
    /// duplicate_to, source, source_email, external_source, external_id, extra,
    /// then the id.
    INTAKE_ISSUE_UPDATE = "v1_intake_issue_update",
    "UPDATE intake_issues SET created_at = $1, updated_at = $2, \
     created_by_id = $3, updated_by_id = $4, deleted_at = $5, id = $6, \
     project_id = $7, workspace_id = $8, intake_id = $9, issue_id = $10, \
     status = $11, snoozed_till = $12, duplicate_to_id = $13, source = $14, \
     source_email = $15, external_source = $16, external_id = $17, extra = $18 \
     WHERE id = $19",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_FOR_INTAKE,
    &TRIAGE_STATE_FOR_PROJECT,
    &PROJECT_MEMBER_FOR_INTAKE,
    &INTAKE_ISSUES_PAGE,
    &INTAKE_ISSUES_PAGE_FOR_CREATOR,
    &INTAKE_ISSUES_COUNT,
    &INTAKE_ISSUES_COUNT_FOR_CREATOR,
    &INTAKE_ISSUE_BY_ISSUE,
    &ISSUE_BY_ID_ANY_STATE,
    &ISSUE_FOR_INTAKE_DELETE,
    &STATES_LITE_BY_IDS,
    &ISSUE_LABEL_IDS,
    &ISSUE_ASSIGNEE_IDS,
    &ISSUE_INSERT,
    &INTAKE_ISSUE_INSERT,
    &INTAKE_ISSUE_UPDATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("intake_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "intake_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
        }
    }

    /// Every lookup is a `SELECT` over the model's columns; `schema_check`
    /// prepares each statement against the migrated database and checks the
    /// column names and types, which is the authoritative guard.
    #[test]
    fn the_lookups_are_selects() {
        assert!(ISSUE_BY_ID_ANY_STATE.sql.starts_with("SELECT "));
        assert!(ISSUE_FOR_INTAKE_DELETE.sql.starts_with("SELECT "));
    }
}
