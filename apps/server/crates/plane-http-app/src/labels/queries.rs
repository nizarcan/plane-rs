// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `labels` group.
//!
//! The statements reproduce the ORM's joins and ordering because two of the
//! three routes return a partially ordered result (`ORDER BY sort_order` and
//! `ORDER BY created_at DESC` both have ties in real data) and because the
//! workspace route's duplicate behaviour is the ORM's: it does **not** call
//! `.distinct()`.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `LabelViewSet.get_queryset()` + `list`
    /// (`plane/app/views/issue/label.py:24-36`): the labels of one project the
    /// caller is a member of, `.distinct()`, ordered by `sort_order`.
    ///
    /// `select_related("parent")` is dropped: the relation is 1:1 and the
    /// serializer emits `parent_id`, so the join cannot change the row set the
    /// `DISTINCT` sees.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id.
    LABELS_BY_PROJECT = "labels_by_project",
    "SELECT DISTINCT l.\"created_at\", l.\"updated_at\", l.\"created_by_id\", \
     l.\"updated_by_id\", l.\"deleted_at\", l.\"id\", l.\"workspace_id\", \
     l.\"project_id\", l.\"parent_id\", l.\"name\", l.\"description\", \
     l.\"color\", l.\"sort_order\", l.\"external_source\", l.\"external_id\" \
     FROM labels l \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     INNER JOIN projects p ON (l.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 \
       AND pm.member_id = $3 \
     ORDER BY l.\"sort_order\" ASC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("parent_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("color", Varchar),
        ("sort_order", Float8),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

plane_query! {
    /// The same queryset plus `pk=`, the lookup DRF's `get_object()` makes for
    /// `retrieve` (`plane/app/views/issue/label.py:26-36`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id,
    /// `$4` label id.
    LABEL_IN_PROJECT = "label_in_project",
    "SELECT DISTINCT l.\"created_at\", l.\"updated_at\", l.\"created_by_id\", \
     l.\"updated_by_id\", l.\"deleted_at\", l.\"id\", l.\"workspace_id\", \
     l.\"project_id\", l.\"parent_id\", l.\"name\", l.\"description\", \
     l.\"color\", l.\"sort_order\", l.\"external_source\", l.\"external_id\" \
     FROM labels l \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     INNER JOIN projects p ON (l.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 \
       AND pm.member_id = $3 AND l.id = $4",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("parent_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("color", Varchar),
        ("sort_order", Float8),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

plane_query! {
    /// `WorkspaceLabelsEndpoint.get` (`plane/app/views/workspace/label.py:22-30`):
    /// the labels of the workspace's unarchived projects the caller is an
    /// active member of. No `DISTINCT` and no explicit order: the ORM applies
    /// `Label.Meta.ordering = ("-created_at",)`, and a caller with two
    /// `project_members` rows for one project sees the label twice — exactly
    /// what Django returns today.
    ///
    /// Bind order: `$1` member id, `$2` workspace slug.
    WORKSPACE_LABELS = "workspace_labels",
    "SELECT l.\"created_at\", l.\"updated_at\", l.\"created_by_id\", \
     l.\"updated_by_id\", l.\"deleted_at\", l.\"id\", l.\"workspace_id\", \
     l.\"project_id\", l.\"parent_id\", l.\"name\", l.\"description\", \
     l.\"color\", l.\"sort_order\", l.\"external_source\", l.\"external_id\" \
     FROM labels l \
     INNER JOIN projects p ON (l.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     WHERE l.deleted_at IS NULL AND p.archived_at IS NULL AND pm.is_active \
       AND pm.member_id = $1 AND w.slug = $2 \
     ORDER BY l.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("parent_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("color", Varchar),
        ("sort_order", Float8),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&LABELS_BY_PROJECT, &LABEL_IN_PROJECT, &WORKSPACE_LABELS];
