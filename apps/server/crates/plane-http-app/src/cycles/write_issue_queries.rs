// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of cycles set B: cycle issues (routes 33/34) and the
//! transfer snapshot (route 38).
//!
//! The distribution statements here are the **count-based** twins of
//! `cycles::queries::CYCLE_ANALYTICS_{ASSIGNEES,LABELS}_ISSUES`: the transfer
//! counts `Count("id")`, so the unassigned/unlabelled group still counts its
//! issues, whereas the analytics statements count the join column. The
//! estimate variants (`Sum`) are identical and are reused from the read
//! catalog.

use plane_db::plane_query;

// ---------------------------------------------------------------------------
// Cycle issues (routes 33/34)
// ---------------------------------------------------------------------------

plane_query! {
    /// `Cycle.objects.get(workspace__slug=slug, project_id=project_id,
    /// pk=cycle_id)` (`plane/app/views/cycle/issue.py:229`), plus
    /// `workspace_id` for the inserted rows' `WorkspaceBaseModel.save`.
    CYCLE_FOR_ISSUES = "cycles_write_cycle_for_issues",
    "SELECT c.id, c.end_date, c.workspace_id FROM cycles c \
     INNER JOIN workspaces w ON w.id = c.workspace_id \
     WHERE c.deleted_at IS NULL AND w.slug = $1 AND c.project_id = $2 AND c.id = $3",
    [
        ("id", Uuid),
        ("end_date", Timestamptz),
        ("workspace_id", Uuid),
    ]
}

plane_query! {
    /// Rows of the requested issues that already belong to **another** cycle
    /// (`issue.py:243-249`): `CycleIssue.objects.filter(~Q(cycle_id=this),
    /// issue_id__in=issues, workspace__slug=slug, project_id=…)`, default
    /// ordering `-created_at`. They are moved to the URL cycle by
    /// `bulk_update`.
    EXISTING_CYCLE_ISSUES = "cycles_write_existing_cycle_issues",
    "SELECT ci.id, ci.cycle_id, ci.issue_id FROM cycle_issues ci \
     INNER JOIN workspaces w ON w.id = ci.workspace_id \
     WHERE ci.deleted_at IS NULL AND ci.cycle_id <> $1 AND ci.issue_id = ANY($2) \
       AND ci.project_id = $3 AND w.slug = $4 \
     ORDER BY ci.created_at DESC",
    [
        ("id", Uuid),
        ("cycle_id", Uuid),
        ("issue_id", Uuid),
    ]
}

plane_query! {
    /// The new issues to insert (`issue.py:251-261`):
    /// `Issue.issue_objects.filter(workspace__slug, project_id, pk__in)`
    /// `.values_list("id", flat=True)`, ordered by `-created_at` — this order
    /// is the `created_cycle_issues` order.
    NEW_ISSUE_IDS = "cycles_write_new_issue_ids",
    "SELECT i.id FROM issues i \
     LEFT OUTER JOIN states s ON s.id = i.state_id \
     INNER JOIN projects p ON p.id = i.project_id \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND (s.\"group\" IS NULL OR s.\"group\" <> 'triage') \
       AND i.archived_at IS NULL AND p.archived_at IS NULL AND NOT i.is_draft \
       AND i.project_id = $1 AND w.slug = $2 AND i.id = ANY($3) \
     ORDER BY i.created_at DESC",
    [("id", Uuid)]
}

plane_query! {
    /// `CycleIssue.objects.bulk_update(rows, ["cycle_id"], batch_size=100)`
    /// (`issue.py:303`): the FK only — no `updated_at`, no audit columns.
    MOVE_CYCLE_ISSUES = "cycles_write_move_cycle_issues",
    "UPDATE cycle_issues SET cycle_id = $1 WHERE deleted_at IS NULL AND id = ANY($2)",
    []
}

plane_query! {
    /// `CycleIssue.objects.filter(...).delete()` (`issue.py:351-356`): H04, a
    /// bare `deleted_at = now` with no `updated_at` and no cascade task.
    SOFT_DELETE_CYCLE_ISSUE = "cycles_write_soft_delete_cycle_issue",
    "UPDATE cycle_issues SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND issue_id = $2 AND project_id = $3 AND cycle_id = $4 \
       AND EXISTS (SELECT 1 FROM workspaces w \
                   WHERE w.id = cycle_issues.workspace_id AND w.slug = $5)",
    []
}

// ---------------------------------------------------------------------------
// Transfer (route 38)
// ---------------------------------------------------------------------------

plane_query! {
    /// `Cycle.objects.filter(workspace__slug, project_id, pk=new_cycle_id)
    /// .first()` (`cycle_transfer_issues.py:58`). The Python does **not**
    /// check for `None` before `new_cycle.end_date`, so a miss is a 500.
    NEW_CYCLE = "cycles_write_transfer_new_cycle",
    "SELECT c.id, c.end_date FROM cycles c \
     INNER JOIN workspaces w ON w.id = c.workspace_id \
     WHERE c.deleted_at IS NULL AND w.slug = $1 AND c.project_id = $2 AND c.id = $3",
    [("id", Uuid), ("end_date", Timestamptz)]
}

plane_query! {
    /// `old_cycle` with its six annotated counts
    /// (`cycle_transfer_issues.py:68-172`), plus the dates `burndown_plot`
    /// needs. The filters are the `issue_cycle` join's, so a soft-deleted
    /// `CycleIssue` row or a soft-deleted/archived/draft issue does not count.
    OLD_CYCLE = "cycles_write_transfer_old_cycle",
    "SELECT c.start_date, c.end_date, \
     COUNT(ci.id) FILTER (WHERE i.archived_at IS NULL AND NOT i.is_draft \
       AND ci.deleted_at IS NULL AND i.deleted_at IS NULL) AS total_issues, \
     COUNT(ci.issue_id) FILTER (WHERE s.\"group\" = 'completed' AND i.archived_at IS NULL \
       AND NOT i.is_draft AND ci.deleted_at IS NULL AND i.deleted_at IS NULL) AS completed_issues, \
     COUNT(ci.issue_id) FILTER (WHERE s.\"group\" = 'cancelled' AND i.archived_at IS NULL \
       AND NOT i.is_draft AND ci.deleted_at IS NULL AND i.deleted_at IS NULL) AS cancelled_issues, \
     COUNT(ci.issue_id) FILTER (WHERE s.\"group\" = 'started' AND i.archived_at IS NULL \
       AND NOT i.is_draft AND ci.deleted_at IS NULL AND i.deleted_at IS NULL) AS started_issues, \
     COUNT(ci.issue_id) FILTER (WHERE s.\"group\" = 'unstarted' AND i.archived_at IS NULL \
       AND NOT i.is_draft AND ci.deleted_at IS NULL AND i.deleted_at IS NULL) AS unstarted_issues, \
     COUNT(ci.issue_id) FILTER (WHERE s.\"group\" = 'backlog' AND i.archived_at IS NULL \
       AND NOT i.is_draft AND ci.deleted_at IS NULL AND i.deleted_at IS NULL) AS backlog_issues \
     FROM cycles c \
     INNER JOIN workspaces w ON w.id = c.workspace_id \
     LEFT OUTER JOIN cycle_issues ci ON ci.cycle_id = c.id \
     LEFT OUTER JOIN issues i ON i.id = ci.issue_id \
     LEFT OUTER JOIN states s ON s.id = i.state_id \
     WHERE c.deleted_at IS NULL AND w.slug = $1 AND c.project_id = $2 AND c.id = $3 \
     GROUP BY c.id",
    [
        ("start_date", Timestamptz),
        ("end_date", Timestamptz),
        ("total_issues", Int8),
        ("completed_issues", Int8),
        ("cancelled_issues", Int8),
        ("started_issues", Int8),
        ("unstarted_issues", Int8),
        ("backlog_issues", Int8),
    ]
}

plane_query! {
    /// The snapshot's assignee distribution for issues
    /// (`cycle_transfer_issues.py:295-352`): `Count("id")` (not the join
    /// column), so the unassigned group counts its issues.
    TRANSFER_ASSIGNEES_ISSUES = "cycles_write_transfer_assignees_issues",
    "SELECT users.display_name AS display_name, issue_assignees.assignee_id AS assignee_id, \
     CASE WHEN users.avatar_asset_id IS NOT NULL \
          THEN '/api/assets/v2/static/' || users.avatar_asset_id::varchar || '/' \
          WHEN users.avatar_asset_id IS NULL THEN users.avatar ELSE NULL END AS avatar_url, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_issues \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     LEFT OUTER JOIN users ON issue_assignees.assignee_id = users.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY users.display_name, issue_assignees.assignee_id, users.avatar_asset_id, users.avatar \
     ORDER BY users.display_name ASC",
    [
        ("display_name", Varchar),
        ("assignee_id", Uuid),
        ("avatar_url", Text),
        ("total_issues", Int8),
        ("completed_issues", Int8),
        ("pending_issues", Int8),
    ]
}

plane_query! {
    /// The snapshot's label distribution for issues
    /// (`cycle_transfer_issues.py:354-410`), the label twin of
    /// [`TRANSFER_ASSIGNEES_ISSUES`].
    TRANSFER_LABELS_ISSUES = "cycles_write_transfer_labels_issues",
    "SELECT labels.name AS label_name, labels.color AS color, \
     issue_labels.label_id AS label_id, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_issues \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_labels ON issues.id = issue_labels.issue_id \
     LEFT OUTER JOIN labels ON issue_labels.label_id = labels.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY labels.name, labels.color, issue_labels.label_id \
     ORDER BY labels.name ASC",
    [
        ("label_name", Varchar),
        ("color", Varchar),
        ("label_id", Uuid),
        ("total_issues", Int8),
        ("completed_issues", Int8),
        ("pending_issues", Int8),
    ]
}

plane_query! {
    /// The issues the transfer moves (`cycle_transfer_issues.py:437-444`):
    /// unfinished, non-archived, non-draft issues of the source cycle, in
    /// `CycleIssue`'s `-created_at` order (which is the
    /// `updated_cycle_issues` order).
    TRANSFER_ISSUES = "cycles_write_transfer_issues",
    "SELECT ci.id, ci.issue_id FROM cycle_issues ci \
     INNER JOIN issues i ON i.id = ci.issue_id \
     INNER JOIN workspaces w ON w.id = ci.workspace_id \
     LEFT OUTER JOIN states s ON s.id = i.state_id \
     WHERE ci.deleted_at IS NULL AND ci.cycle_id = $1 AND ci.project_id = $2 \
       AND w.slug = $3 AND i.archived_at IS NULL AND NOT i.is_draft \
       AND s.\"group\" IN ('backlog', 'unstarted', 'started') \
     ORDER BY ci.created_at DESC",
    [("id", Uuid), ("issue_id", Uuid)]
}

plane_query! {
    /// `current_cycle.save(update_fields=["progress_snapshot"])`
    /// (`cycle_transfer_issues.py:432-434`): one column, so `updated_at` and
    /// `updated_by` keep their values and no save hook runs.
    SAVE_PROGRESS_SNAPSHOT = "cycles_write_transfer_save_snapshot",
    "UPDATE cycles SET progress_snapshot = $1 WHERE id = $2",
    []
}

/// `base_host(request=request, is_app=True)` (`plane/utils/host.py:17-57`),
/// shared by the two set B handlers.
///
/// # Errors
///
/// A missing `WEB_URL`/`APP_BASE_URL`.
pub(crate) fn base_host(
    ctx: &plane_drf::view::ViewCtx<'_>,
) -> Result<String, plane_drf::error::ViewError> {
    plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| plane_drf::error::ViewError::Unknown(error.to_string()))
}

/// The catalog entry `tests/schema_check.rs` verifies against a migrated
/// database.
pub static ALL_QUERIES: &[&plane_db::Query] = &[
    &CYCLE_FOR_ISSUES,
    &EXISTING_CYCLE_ISSUES,
    &NEW_ISSUE_IDS,
    &MOVE_CYCLE_ISSUES,
    &SOFT_DELETE_CYCLE_ISSUE,
    &NEW_CYCLE,
    &OLD_CYCLE,
    &TRANSFER_ASSIGNEES_ISSUES,
    &TRANSFER_LABELS_ISSUES,
    &TRANSFER_ISSUES,
    &SAVE_PROGRESS_SNAPSHOT,
];
