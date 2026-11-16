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
pub static ALL_QUERIES: &[&Query] = &[&STATES_BY_PROJECT, &STATE_IN_PROJECT, &WORKSPACE_STATES];
