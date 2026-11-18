// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Static statements of the v1 work-item serializer.
//!
//! The `PrimaryKeyRelatedField` lookups are DRF's `queryset.get(pk=…)` for the
//! model manager each field binds: `UserManager` (the stock Django manager, no
//! soft-delete filter), `Label.objects`/`EstimatePoint.objects`/`IssueType
//! .objects` (soft delete) and `State.objects`/`Issue.objects` (soft delete,
//! with `StateManager`'s triage exclusion and `IssueManager`'s
//! triage/archived/draft/project-archived exclusions).

use plane_db::catalog::Query;

plane_db::plane_query! {
    /// `PrimaryKeyRelatedField(queryset=User.objects.values_list("id", flat=True))`.
    USERS = "v1_issue_pk_users",
    "SELECT users.id AS pk FROM users WHERE users.id = $1",
    [("pk", Uuid)]
}

plane_db::plane_query! {
    /// `PrimaryKeyRelatedField(queryset=Label.objects.values_list("id", flat=True))`.
    LABELS = "v1_issue_pk_labels",
    "SELECT labels.id AS pk FROM labels WHERE labels.id = $1 AND labels.deleted_at IS NULL",
    [("pk", Uuid)]
}

plane_db::plane_query! {
    /// `PrimaryKeyRelatedField(queryset=IssueType.objects.all())`.
    ISSUE_TYPES = "v1_issue_pk_issue_types",
    "SELECT issue_types.id AS pk FROM issue_types \
     WHERE issue_types.id = $1 AND issue_types.deleted_at IS NULL",
    [("pk", Uuid)]
}

plane_db::plane_query! {
    /// `PrimaryKeyRelatedField(queryset=State.objects.all())`: triage states
    /// are not offered by the default manager (`state.py:65-69`).
    STATES = "v1_issue_pk_states",
    "SELECT states.id AS pk FROM states \
     WHERE states.id = $1 AND states.deleted_at IS NULL AND states.\"group\" <> 'triage'",
    [("pk", Uuid)]
}

plane_db::plane_query! {
    /// `PrimaryKeyRelatedField(queryset=Issue.objects.all())` (soft delete).
    PARENTS = "v1_issue_pk_parents",
    "SELECT issues.id AS pk FROM issues \
     WHERE issues.id = $1 AND issues.deleted_at IS NULL",
    [("pk", Uuid)]
}

plane_db::plane_query! {
    /// `PrimaryKeyRelatedField(queryset=EstimatePoint.objects.all())`.
    ESTIMATE_POINTS = "v1_issue_pk_estimate_points",
    "SELECT estimate_points.id AS pk FROM estimate_points \
     WHERE estimate_points.id = $1 AND estimate_points.deleted_at IS NULL",
    [("pk", Uuid)]
}

plane_db::plane_query! {
    /// `Project.objects.get(pk=…)` in `IssueListCreateAPIEndpoint.post`
    /// (`issue.py:540`): id only, no slug and no archived filter.
    PROJECT = "v1_issue_project",
    "SELECT projects.id, projects.workspace_id, projects.default_assignee_id \
     FROM projects WHERE projects.id = $1",
    [("id", Uuid), ("workspace_id", Uuid), ("default_assignee_id", Uuid)]
}

/// Every statement this file declares, for `tests/schema_check.rs`.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT,
    &USERS,
    &LABELS,
    &ISSUE_TYPES,
    &STATES,
    &PARENTS,
    &ESTIMATE_POINTS,
];
