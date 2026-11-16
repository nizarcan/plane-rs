// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the issue-core write routes
//! (`plans/WORK_ITEMS.md` set A, orders 64, 67, 71, 72, 92 and 96).
//!
//! Every statement has the shape Django's ORM emits for the same view; the
//! manager predicates are the generated constants' equivalents:
//!
//! * `Issue.issue_objects` (`plane/db/models/issue.py:92-101`) — soft delete,
//!   non-triage state, unarchived issue and project, not a draft;
//! * `Issue.objects` (`plane/db/mixins.py:57`) — soft delete only;
//! * `Project.objects` / `ProjectMember.objects` / `Label.objects` — the
//!   default soft-deletion manager.
//!
//! `tests/schema_check.rs` prepares every entry against a migrated database.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Project.objects.get(pk=project_id)` (`plane/app/views/issue/base.py:407`):
    /// the create view does **not** filter by workspace slug or `archived_at`,
    /// so a project of another workspace is accepted (upstream shape).
    ///
    /// Bind order: `$1` project id.
    PROJECT_FOR_ISSUE_CREATE = "issues_write_project_for_create",
    "SELECT p.\"workspace_id\", p.\"default_assignee_id\" \
     FROM projects p \
     WHERE p.id = $1 AND p.deleted_at IS NULL \
     LIMIT 1",
    [
        ("workspace_id", Uuid),
        ("default_assignee_id", Uuid),
    ]
}

plane_query! {
    /// `IssueViewSet.get_queryset().annotate(...).filter(pk=pk).first()`
    /// (`plane/app/views/issue/base.py:640-678`) — the full row a PATCH writes
    /// through `Issue.save()`. The annotations only feed `current_instance`,
    /// which comes from the shared `issues::list_retrieve::patch_detail`.
    ///
    /// Bind order: `$1` project id, `$2` workspace slug, `$3` issue id.
    ISSUE_FOR_PATCH = "issues_write_for_patch",
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
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
       AND (states.\"group\" IS NULL OR states.\"group\" <> 'triage') \
       AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
       AND NOT issues.is_draft \
       AND issues.project_id = $1 AND workspaces.slug = $2 AND issues.id = $3 \
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
    /// `Issue.objects.get(workspace__slug=slug, project_id=project_id, pk=pk)`
    /// (`plane/app/views/issue/base.py:718`) — the DELETE lookup, which is the
    /// soft-deletion manager only: archived, triage and draft rows are all
    /// deletable.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` issue id.
    ISSUE_FOR_DELETE = "issues_write_for_delete",
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

plane_query! {
    /// `Issue.issue_objects.get(workspace__slug=slug, project_id=project_id,
    /// pk=pk)` (`plane/app/views/issue/archive.py:257`), with
    /// `select_related("state")` folded into the join for the
    /// `state.group` check and the full `Issue.save()` on archive.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` issue id.
    ISSUE_FOR_ARCHIVE = "issues_write_for_archive",
    "SELECT issues.created_at, issues.updated_at, issues.created_by_id, \
     issues.updated_by_id, issues.deleted_at, issues.id, issues.project_id, \
     issues.workspace_id, issues.parent_id, issues.state_id, issues.point, \
     issues.estimate_point_id, issues.name, issues.description_json, \
     issues.description_html, issues.description_stripped, \
     issues.description_binary, issues.priority, issues.start_date, \
     issues.target_date, issues.sequence_id, issues.sort_order, \
     issues.completed_at, issues.archived_at, issues.is_draft, \
     issues.external_source, issues.external_id, issues.type_id, \
     states.\"group\" AS state_group \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
       AND (states.\"group\" IS NULL OR states.\"group\" <> 'triage') \
       AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
       AND NOT issues.is_draft \
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
        ("state_group", Varchar),
    ]
}

plane_query! {
    /// `Issue.objects.get(workspace__slug=slug, project_id=project_id,
    /// archived_at__isnull=False, pk=pk)` (`plane/app/views/issue/archive.py:285`)
    /// — the unarchive lookup uses the soft-deletion manager, not
    /// `issue_objects`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` issue id.
    ISSUE_FOR_UNARCHIVE = "issues_write_for_unarchive",
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
     WHERE issues.deleted_at IS NULL AND issues.archived_at IS NOT NULL \
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

plane_query! {
    /// `Issue.objects.filter(workspace__slug=slug, project_id=project_id,
    /// pk__in=issue_ids).select_related("state")`
    /// (`plane/app/views/issue/archive.py:308-310`), ordered by
    /// `Meta.ordering = ("-created_at",)`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` issue ids.
    ISSUES_FOR_BULK_ARCHIVE = "issues_write_for_bulk_archive",
    "SELECT issues.id, issues.name, issues.state_id, issues.sort_order, \
     issues.completed_at, issues.estimate_point_id, issues.priority, \
     issues.start_date, issues.target_date, issues.sequence_id, \
     issues.project_id, issues.parent_id, issues.created_at, \
     issues.updated_at, issues.created_by_id, issues.updated_by_id, \
     issues.is_draft, issues.archived_at, states.\"group\" AS state_group \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
       AND workspaces.slug = $1 AND issues.project_id = $2 \
       AND issues.id = ANY($3::uuid[]) \
     ORDER BY issues.created_at DESC",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("state_id", Uuid),
        ("sort_order", Float8),
        ("completed_at", Timestamptz),
        ("estimate_point_id", Uuid),
        ("priority", Varchar),
        ("start_date", Date),
        ("target_date", Date),
        ("sequence_id", Int4),
        ("project_id", Uuid),
        ("parent_id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("is_draft", Bool),
        ("archived_at", Date),
        ("state_group", Varchar),
    ]
}

plane_query! {
    /// `Issue.issue_objects.filter(workspace__slug=slug, project_id=project_id,
    /// pk__in=issue_ids)` (`plane/app/views/issue/base.py:777`), evaluated by
    /// `len(issues)` and reused as the subquery of the three deletes.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` issue ids.
    ISSUES_FOR_BULK_DELETE = "issues_write_for_bulk_delete",
    "SELECT issues.id \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
       AND (states.\"group\" IS NULL OR states.\"group\" <> 'triage') \
       AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
       AND NOT issues.is_draft \
       AND workspaces.slug = $1 AND issues.project_id = $2 \
       AND issues.id = ANY($3::uuid[])",
    [
        ("id", Uuid),
    ]
}

plane_query! {
    /// `CycleIssue.objects.filter(issue__in=issues).delete()`
    /// (`plane/app/views/issue/base.py:779`): the soft-deletion queryset's
    /// `update(deleted_at=now)`, with no audit columns.
    ///
    /// Bind order: `$1` deleted_at, `$2` issue ids.
    BULK_DELETE_CYCLE_ISSUES = "issues_write_bulk_delete_cycle_issues",
    "UPDATE cycle_issues SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND issue_id = ANY($2::uuid[])",
    []
}

plane_query! {
    /// `ModuleIssue.objects.filter(issue__in=issues).delete()`
    /// (`plane/app/views/issue/base.py:782`).
    ///
    /// Bind order: `$1` deleted_at, `$2` issue ids.
    BULK_DELETE_MODULE_ISSUES = "issues_write_bulk_delete_module_issues",
    "UPDATE module_issues SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND issue_id = ANY($2::uuid[])",
    []
}

plane_query! {
    /// `issues.delete()` (`plane/app/views/issue/base.py:785`): the
    /// soft-deletion queryset's `update(deleted_at=now)`, no audit columns.
    ///
    /// Bind order: `$1` deleted_at, `$2` issue ids.
    BULK_DELETE_ISSUES = "issues_write_bulk_delete_issues",
    "UPDATE issues SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND id = ANY($2::uuid[])",
    []
}

plane_query! {
    /// `UserRecentVisit.objects.filter(...).delete(soft=False)`
    /// (`plane/app/views/issue/base.py:723-730`): a hard `DELETE`, with the
    /// default manager's `deleted_at IS NULL` predicate.
    ///
    /// Bind order: `$1` project id, `$2` workspace slug, `$3` issue id.
    DELETE_RECENT_VISITS = "issues_write_delete_recent_visits",
    "DELETE FROM user_recent_visits \
     USING workspaces \
     WHERE user_recent_visits.workspace_id = workspaces.id \
       AND user_recent_visits.deleted_at IS NULL \
       AND user_recent_visits.project_id = $1 \
       AND workspaces.slug = $2 \
       AND user_recent_visits.entity_identifier = $3 \
       AND user_recent_visits.entity_name = 'issue'",
    []
}

plane_query! {
    /// `Issue.objects.filter(id__in=issue_ids, workspace__slug=slug,
    /// project_id=project_id)` (`plane/app/views/issue/base.py:1128`), ordered
    /// by `Meta.ordering = ("-created_at",)`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` issue ids.
    ISSUES_FOR_ISSUE_DATES = "issues_write_for_issue_dates",
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
       AND workspaces.slug = $1 AND issues.project_id = $2 \
       AND issues.id = ANY($3::uuid[]) \
     ORDER BY issues.created_at DESC",
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
    /// `State.objects.filter(project_id=…, pk=…)` in
    /// `IssueCreateSerializer.validate` (`plane/app/serializers/issue.py:175-180`).
    ///
    /// Bind order: `$1` state id, `$2` project id.
    VALID_STATE_IN_PROJECT = "issues_write_valid_state_in_project",
    "SELECT 1::int4 AS exists FROM states \
     WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL \
       AND NOT (\"group\" = 'triage') \
     LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `State.triage_objects.filter(project_id=…, pk=…)`, the manager
    /// `IssueCreateSerializer.validate` picks when the context carries
    /// `allow_triage_state=True` (`plane/app/serializers/issue.py:134-136`,
    /// `:179-185`; the intake-issue create is the only caller,
    /// `plane/app/views/intake/base.py:267`).
    ///
    /// `TriageStateManager` is `SoftDeletionManager` plus
    /// `filter(group='triage')` (`plane/db/models/state.py:62-68`), the exact
    /// complement of [`VALID_STATE_IN_PROJECT`].
    ///
    /// Bind order: `$1` state id, `$2` project id.
    VALID_TRIAGE_STATE_IN_PROJECT = "issues_write_valid_triage_state_in_project",
    "SELECT 1::int4 AS exists FROM states \
     WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL \
       AND \"group\" = 'triage' \
     LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `Issue.objects.filter(project_id=…, pk=…)` in the serializer's `validate`
    /// (`plane/app/serializers/issue.py:183-189`).
    ///
    /// Bind order: `$1` parent id, `$2` project id.
    VALID_PARENT_IN_PROJECT = "issues_write_valid_parent_in_project",
    "SELECT 1::int4 AS exists FROM issues \
     WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL \
     LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `EstimatePoint.objects.filter(project_id=…, pk=…)` in the serializer's
    /// `validate` (`plane/app/serializers/issue.py:197-203`).
    ///
    /// Bind order: `$1` estimate point id, `$2` project id.
    VALID_ESTIMATE_POINT_IN_PROJECT = "issues_write_valid_estimate_point_in_project",
    "SELECT 1::int4 AS exists FROM estimate_points \
     WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL \
     LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `ProjectMember.objects.filter(project_id=…, role__gte=15,
    /// is_active=True, member_id__in=…).values_list("member_id", flat=True)`
    /// (`plane/app/serializers/issue.py:161-166`).
    ///
    /// Bind order: `$1` project id, `$2` requested member ids.
    FILTER_ASSIGNEES = "issues_write_filter_assignees",
    "SELECT pm.member_id \
     FROM project_members pm \
     WHERE pm.project_id = $1 AND pm.role >= 15 AND pm.is_active \
       AND pm.deleted_at IS NULL \
       AND pm.member_id = ANY($2::uuid[])",
    [
        ("member_id", Uuid),
    ]
}

plane_query! {
    /// `Label.objects.filter(project_id=…, id__in=…).values_list("id",
    /// flat=True)` (`plane/app/serializers/issue.py:169-176`).
    ///
    /// Bind order: `$1` project id, `$2` requested label ids.
    FILTER_LABELS = "issues_write_filter_labels",
    "SELECT l.id \
     FROM labels l \
     WHERE l.project_id = $1 AND l.deleted_at IS NULL \
       AND l.id = ANY($2::uuid[])",
    [
        ("id", Uuid),
    ]
}

plane_query! {
    /// The soft-deleted rows the issue serializer's `parent`/`parent_id`
    /// fields reject after the generated table-only pk check
    /// (`Issue.objects` is the default soft-deletion manager).
    ///
    /// Bind order: `$1` issue ids.
    SOFT_DELETED_ISSUES = "issues_write_soft_deleted_issues",
    "SELECT id FROM issues \
     WHERE deleted_at IS NOT NULL AND id = ANY($1::uuid[])",
    [
        ("id", Uuid),
    ]
}

plane_query! {
    /// The soft-deleted labels the generated `label_ids` child check misses
    /// (`Label.objects` is the default soft-deletion manager).
    ///
    /// Bind order: `$1` label ids.
    SOFT_DELETED_LABELS = "issues_write_soft_deleted_labels",
    "SELECT id FROM labels \
     WHERE deleted_at IS NOT NULL AND id = ANY($1::uuid[])",
    [
        ("id", Uuid),
    ]
}

plane_query! {
    /// The states the issue serializer's **model** `state` field rejects after
    /// the generated table-only pk check.
    ///
    /// `IssueCreateSerializer` declares `state_id` with
    /// `queryset=State.all_state_objects.all()` (`plane/app/serializers/issue.py:84-86`),
    /// but `Meta.fields = "__all__"` also builds the model field `state`, whose
    /// `PrimaryKeyRelatedField` uses the **default** manager `State.objects` —
    /// `StateManager`, which hides soft-deleted rows and every
    /// `group = 'triage'` row (`plane/db/models/state.py:49-68`). DRF visits
    /// `self.fields` in declaration order, so `state` comes after `state_id` and
    /// wins in `validated_data` (`fk_value`). A body that sends the injected
    /// triage state under `state` therefore gets a field error, while the same
    /// id under `state_id` is accepted — an upstream oddity kept as it is.
    ///
    /// Bind order: `$1` state ids.
    INVALID_STATE_MANAGER = "issues_write_invalid_state_manager",
    "SELECT id FROM states \
     WHERE id = ANY($1::uuid[]) \
       AND (deleted_at IS NOT NULL OR \"group\" = 'triage')",
    [
        ("id", Uuid),
    ]
}

plane_query! {
    /// The soft-deleted estimate points the generated pk check misses.
    ///
    /// Bind order: `$1` estimate point ids.
    SOFT_DELETED_ESTIMATE_POINTS = "issues_write_soft_deleted_estimate_points",
    "SELECT id FROM estimate_points \
     WHERE deleted_at IS NOT NULL AND id = ANY($1::uuid[])",
    [
        ("id", Uuid),
    ]
}

plane_query! {
    /// The soft-deleted issue types the generated pk check misses.
    ///
    /// Bind order: `$1` issue type ids.
    SOFT_DELETED_ISSUE_TYPES = "issues_write_soft_deleted_issue_types",
    "SELECT id FROM issue_types \
     WHERE deleted_at IS NOT NULL AND id = ANY($1::uuid[])",
    [
        ("id", Uuid),
    ]
}

plane_query! {
    /// `Issue.objects.bulk_update(issues, ["archived_at"])`
    /// (`plane/app/views/issue/archive.py:337`): one UPDATE, no audit columns
    /// and no `updated_at`.
    ///
    /// Bind order: `$1` archived_at, `$2` issue ids.
    BULK_ARCHIVE_UPDATE = "issues_write_bulk_archive_update",
    "UPDATE issues SET archived_at = $1 WHERE id = ANY($2::uuid[])",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_FOR_ISSUE_CREATE,
    &ISSUE_FOR_PATCH,
    &ISSUE_FOR_DELETE,
    &ISSUE_FOR_ARCHIVE,
    &ISSUE_FOR_UNARCHIVE,
    &ISSUES_FOR_BULK_ARCHIVE,
    &ISSUES_FOR_BULK_DELETE,
    &BULK_DELETE_CYCLE_ISSUES,
    &BULK_DELETE_MODULE_ISSUES,
    &BULK_DELETE_ISSUES,
    &BULK_ARCHIVE_UPDATE,
    &DELETE_RECENT_VISITS,
    &ISSUES_FOR_ISSUE_DATES,
    &VALID_STATE_IN_PROJECT,
    &VALID_TRIAGE_STATE_IN_PROJECT,
    &VALID_PARENT_IN_PROJECT,
    &VALID_ESTIMATE_POINT_IN_PROJECT,
    &FILTER_ASSIGNEES,
    &FILTER_LABELS,
    &SOFT_DELETED_ISSUES,
    &SOFT_DELETED_LABELS,
    &INVALID_STATE_MANAGER,
    &SOFT_DELETED_ESTIMATE_POINTS,
    &SOFT_DELETED_ISSUE_TYPES,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_issue_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_issue_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
