// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `states` group.
//!
//! Two of the three statements are the ORM's own SQL text, join tree and
//! ordering included, because the routes they serve return a *partially*
//! ordered result: `ORDER BY sequence` has ties (every project's "Backlog" is
//! 15000) and the tie order is the plan's, which is what Django returns today.
//! Reproducing the statement reproduces the plan and therefore the order; a
//! "nicer" query with a different join tree would not.
//!
//! The manager predicate is `StateManager` (`apps/api/plane/db/models/state.py:62-68`),
//! which excludes `group = 'triage'` on top of the soft-delete filter
//! (`plane/db/mixins.py:57`); both appear in the statements below.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `StateViewSet.get_queryset()` + `list`
    /// (`plane/app/views/state/base.py:29-40`): every non-triage state of one
    /// project the caller is an active project member of, ordered by
    /// `Meta.ordering = ("sequence",)`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id.
    STATES_BY_PROJECT = "states_by_project",
    "SELECT DISTINCT s.\"created_at\", s.\"updated_at\", s.\"created_by_id\", \
     s.\"updated_by_id\", s.\"deleted_at\", s.\"id\", s.\"project_id\", \
     s.\"workspace_id\", s.\"name\", s.\"description\", s.\"color\", s.\"slug\", \
     s.\"sequence\", s.\"group\", s.\"is_triage\", s.\"default\", \
     s.\"external_source\", s.\"external_id\" \
     FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     INNER JOIN projects p ON (s.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND w.slug = $1 AND s.project_id = $2 AND p.archived_at IS NULL \
       AND pm.is_active AND pm.member_id = $3 AND NOT s.is_triage \
     ORDER BY s.\"sequence\" ASC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("color", Varchar),
        ("slug", Varchar),
        ("sequence", Float8),
        ("group", Varchar),
        ("is_triage", Bool),
        ("default", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

plane_query! {
    /// The same queryset as [`STATES_BY_PROJECT`] plus `pk=`, which is how DRF's
    /// `GenericAPIView.get_object()` resolves `retrieve`
    /// (`plane/app/views/state/base.py:47-55`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id,
    /// `$4` state id.
    STATE_IN_PROJECT = "state_in_project",
    "SELECT DISTINCT s.\"created_at\", s.\"updated_at\", s.\"created_by_id\", \
     s.\"updated_by_id\", s.\"deleted_at\", s.\"id\", s.\"project_id\", \
     s.\"workspace_id\", s.\"name\", s.\"description\", s.\"color\", s.\"slug\", \
     s.\"sequence\", s.\"group\", s.\"is_triage\", s.\"default\", \
     s.\"external_source\", s.\"external_id\" \
     FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     INNER JOIN projects p ON (s.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND w.slug = $1 AND s.project_id = $2 AND p.archived_at IS NULL \
       AND pm.is_active AND pm.member_id = $3 AND NOT s.is_triage \
       AND s.id = $4",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("color", Varchar),
        ("slug", Varchar),
        ("sequence", Float8),
        ("group", Varchar),
        ("is_triage", Bool),
        ("default", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

plane_query! {
    /// `WorkspaceStatesEndpoint.get` (`plane/app/views/workspace/state.py:18-41`):
    /// every non-triage state of the workspace's unarchived projects the caller
    /// is an active member of.
    ///
    /// Two quirks are Django's and are ported as they are:
    ///
    /// * `projects.deleted_at` is **not** filtered, so a soft-deleted project's
    ///   states are still returned (the ledger's "members of a soft-deleted
    ///   workspace still pass" family);
    /// * the result has no total order: `ORDER BY sequence` ties across
    ///   projects, and the tie order is the plan's.
    ///
    /// Bind order: `$1` member id, `$2` workspace slug.
    WORKSPACE_STATES = "workspace_states",
    "SELECT s.\"created_at\", s.\"updated_at\", s.\"created_by_id\", \
     s.\"updated_by_id\", s.\"deleted_at\", s.\"id\", s.\"project_id\", \
     s.\"workspace_id\", s.\"name\", s.\"description\", s.\"color\", s.\"slug\", \
     s.\"sequence\", s.\"group\", s.\"is_triage\", s.\"default\", \
     s.\"external_source\", s.\"external_id\" \
     FROM states s \
     INNER JOIN projects p ON (s.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND p.archived_at IS NULL AND pm.is_active AND pm.member_id = $1 \
       AND w.slug = $2 AND NOT s.is_triage \
     ORDER BY s.\"sequence\" ASC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("color", Varchar),
        ("slug", Varchar),
        ("sequence", Float8),
        ("group", Varchar),
        ("is_triage", Bool),
        ("default", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &STATES_BY_PROJECT,
    &STATE_IN_PROJECT,
    &WORKSPACE_STATES,
    &STATE_SEQUENCE_MAX,
    &STATE_PROJECT_WORKSPACE,
    &STATE_INSERT,
    &STATE_UPDATE,
    &STATE_FOR_DELETE,
    &STATE_ISSUE_EXISTS,
    &STATE_MARK_DEFAULT,
];

// ---------------------------------------------------------------------------
// Writes (`plane/app/views/state/base.py:56-135`)
// ---------------------------------------------------------------------------

plane_query! {
    /// `State.objects.filter(project=self.project).aggregate(Max("sequence"))`
    /// inside H19 (`plane/db/models/state.py:120-123`).
    ///
    /// `State.objects` is `StateManager`: soft-deleted rows and
    /// `group = 'triage'` are excluded. `project=self.project` binds the FK.
    STATE_SEQUENCE_MAX = "states_sequence_max",
    "SELECT MAX(s.sequence) AS max_sequence FROM states s \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND s.project_id = $1",
    [("max_sequence", Float8)]
}

plane_query! {
    /// `ProjectBaseModel.save`'s `self.workspace = self.project.workspace`
    /// (`plane/db/models/project.py:237-239`) for
    /// `State.objects.create(project_id=…)`.
    ///
    /// `self.project` is a `Project.objects.get()` (soft-delete predicate,
    /// `plane/db/mixins.py:57`); the workspace FK access itself uses the base
    /// manager, so only the id is read.
    STATE_PROJECT_WORKSPACE = "states_project_workspace",
    "SELECT p.workspace_id FROM projects p \
     WHERE p.id = $1 AND p.deleted_at IS NULL \
     LIMIT 1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `State.objects.create(**validated_data, project_id=…)`
    /// (`plane/db/models/state.py:79-126`, `BaseModel.save`).
    STATE_INSERT = "states_insert",
    "INSERT INTO states (created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, id, project_id, workspace_id, name, description, color, slug, \
     sequence, \"group\", is_triage, \"default\", external_source, external_id) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
     $15, $16, $17, $18)",
    []
}

plane_query! {
    /// `State.save()` on an existing row: a full-row `UPDATE` (`Model.save()`),
    /// after H19's slug and H01's audit columns.
    STATE_UPDATE = "states_update",
    "UPDATE states SET created_at = $1, updated_at = $2, created_by_id = $3, \
     updated_by_id = $4, deleted_at = $5, id = $6, project_id = $7, \
     workspace_id = $8, name = $9, description = $10, color = $11, slug = $12, \
     sequence = $13, \"group\" = $14, is_triage = $15, \"default\" = $16, \
     external_source = $17, external_id = $18 \
     WHERE id = $6",
    []
}

plane_query! {
    /// `State.objects.get(is_triage=False, pk=pk, project_id=project_id,
    /// workspace__slug=slug)` in `destroy` (`plane/app/views/state/base.py:113`).
    ///
    /// The manager hides soft-deleted and `group = 'triage'` rows; the view
    /// adds `is_triage=False`. No project-membership or `archived_at` filter
    /// (the permission decorator is the only authorization).
    STATE_FOR_DELETE = "states_for_delete",
    "SELECT s.\"created_at\", s.\"updated_at\", s.\"created_by_id\", \
     s.\"updated_by_id\", s.\"deleted_at\", s.\"id\", s.\"project_id\", \
     s.\"workspace_id\", s.\"name\", s.\"description\", s.\"color\", s.\"slug\", \
     s.\"sequence\", s.\"group\", s.\"is_triage\", s.\"default\", \
     s.\"external_source\", s.\"external_id\" \
     FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND s.is_triage = FALSE AND s.id = $1 AND s.project_id = $2 \
       AND w.slug = $3",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("color", Varchar),
        ("slug", Varchar),
        ("sequence", Float8),
        ("group", Varchar),
        ("is_triage", Bool),
        ("default", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

plane_query! {
    /// `Issue.objects.filter(state=pk).exists()` in `destroy`
    /// (`plane/app/views/state/base.py:121`).
    ///
    /// `Issue.objects` is the inherited `SoftDeletionManager` (`Issue` declares
    /// `issue_objects`, not `objects`), so only `deleted_at` is filtered: draft,
    /// archived and triage issues still block the delete.
    STATE_ISSUE_EXISTS = "states_issue_exists",
    "SELECT 1 AS \"exists\" FROM issues \
     WHERE state_id = $1 AND deleted_at IS NULL \
     LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `State.objects.filter(project_id=project_id).update(default=Case(...))`
    /// in `mark_as_default` (`plane/app/views/state/base.py:101-103`).
    ///
    /// `State.objects` hides soft-deleted and triage-group rows. `is_triage`
    /// (the boolean) is *not* filtered here, and `updated_at` is not touched by
    /// a queryset `update()`.
    STATE_MARK_DEFAULT = "states_mark_default",
    "UPDATE states \
     SET \"default\" = CASE WHEN id = $1 THEN TRUE ELSE FALSE END \
     WHERE project_id = $2 AND deleted_at IS NULL \
       AND NOT (\"group\" = 'triage')",
    []
}
