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

// ---------------------------------------------------------------------------
// Writes (routes 68, 69, 70)
// ---------------------------------------------------------------------------

plane_query! {
    /// `Project.objects.get(pk=project_id)` — route 70
    /// (`plane/app/views/issue/label.py:81`) and the `self.project` accessor
    /// `Label.save()` makes on create (`WorkspaceBaseModel.save`,
    /// `plane/db/models/workspace.py:192-195`). Both use the default manager.
    ///
    /// Bind order: `$1` project id.
    PROJECT_FOR_LABEL = "labels_project_for_label",
    "SELECT p.id, p.workspace_id FROM projects p \
     WHERE p.deleted_at IS NULL AND p.id = $1 LIMIT 1",
    [("id", Uuid), ("workspace_id", Uuid)]
}

plane_query! {
    /// `PrimaryKeyRelatedField`'s existence check for `parent`
    /// (`rest_framework/relations.py:251-261`): `Label.objects.get(pk=<value>)`
    /// over the default manager, so a soft-deleted parent is *not* a parent.
    ///
    /// Bind order: `$1` label id.
    LABEL_PARENT_EXISTS = "labels_parent_exists",
    "SELECT 1 AS one FROM labels WHERE deleted_at IS NULL AND id = $1 LIMIT 1",
    [("one", Int4)]
}

plane_query! {
    /// `LabelSerializer.validate_name` on create (`plane/app/serializers/
    /// issue.py:385-396`): `project_id` from the context, `name__iexact`.
    ///
    /// Bind order: `$1` project id, `$2` name.
    LABEL_NAME_TAKEN_CREATE = "labels_name_taken_create",
    "SELECT 1 AS one FROM labels \
     WHERE deleted_at IS NULL AND project_id = $1 \
       AND UPPER(name) = UPPER($2) LIMIT 1",
    [("one", Int4)]
}

plane_query! {
    /// [`LABEL_NAME_TAKEN_CREATE`] for PATCH, which excludes the instance
    /// (`exclude(id=self.instance.pk)`).
    ///
    /// Bind order: `$1` project id, `$2` name, `$3` instance id.
    LABEL_NAME_TAKEN_PATCH = "labels_name_taken_patch",
    "SELECT 1 AS one FROM labels \
     WHERE deleted_at IS NULL AND project_id = $1 \
       AND UPPER(name) = UPPER($2) AND id <> $3 LIMIT 1",
    [("one", Int4)]
}

plane_query! {
    /// `validate_name` as `UpdateModelMixin.update` runs it: the mixin's
    /// `get_serializer_context()` carries no `project_id`, so the serializer
    /// filters `project_id=None` — the workspace-level labels.
    ///
    /// Bind order: `$1` name, `$2` instance id.
    LABEL_NAME_TAKEN_UPDATE = "labels_name_taken_update",
    "SELECT 1 AS one FROM labels \
     WHERE deleted_at IS NULL AND project_id IS NULL \
       AND UPPER(name) = UPPER($1) AND id <> $2 LIMIT 1",
    [("one", Int4)]
}

plane_query! {
    /// `partial_update`'s pre-serializer check (`label.py:48-58`): an exact,
    /// case-sensitive `name` filter excluding the instance.
    ///
    /// Bind order: `$1` project id, `$2` name, `$3` instance id.
    LABEL_NAME_EXACT_PATCH = "labels_name_exact_patch",
    "SELECT 1 AS one FROM labels \
     WHERE deleted_at IS NULL AND project_id = $1 \
       AND name = $2 AND id <> $3 LIMIT 1",
    [("one", Int4)]
}

plane_query! {
    /// `Label.objects.create(...)` → `Label.save()` (H20) →
    /// `WorkspaceBaseModel.save` → `BaseModel.save` (H01). `description`
    /// defaults to `""`, `external_*` to NULL, `deleted_at` NULL.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, id,
    /// workspace, project, parent, name, description, color, sort_order,
    /// external_source, external_id.
    LABEL_INSERT = "labels_insert",
    "INSERT INTO labels \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, project_id, parent_id, name, description, color, sort_order, \
      external_source, external_id) \
     VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11, $12, NULL, NULL)",
    []
}

plane_query! {
    /// `serializer.save()` on an existing row: a full-row UPDATE (`Model.save()`
    /// with no `update_fields`), after H01 set `updated_by`/`updated_at`.
    ///
    /// Bind order: every column but the id, then the id.
    LABEL_UPDATE = "labels_update",
    "UPDATE labels SET created_at = $1, updated_at = $2, created_by_id = $3, \
       updated_by_id = $4, deleted_at = $5, id = $6, workspace_id = $7, \
       project_id = $8, parent_id = $9, name = $10, description = $11, \
       color = $12, sort_order = $13, external_source = $14, external_id = $15 \
     WHERE id = $16",
    []
}

plane_query! {
    /// `Label.objects.bulk_create([...], batch_size=50, ignore_conflicts=True)`
    /// (route 70). `bulk_create` bypasses `Label.save()`, so every row keeps
    /// the model default `sort_order` 65535; `ON CONFLICT DO NOTHING` drops a
    /// row that collides with a live label of the same project.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, id,
    /// workspace, project, name, description, color, sort_order.
    LABEL_BULK_INSERT = "labels_bulk_insert",
    "INSERT INTO labels \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, project_id, parent_id, name, description, color, sort_order, \
      external_source, external_id) \
     VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, NULL, $8, $9, $10, $11, NULL, NULL) \
     ON CONFLICT DO NOTHING",
    []
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &LABELS_BY_PROJECT,
    &LABEL_IN_PROJECT,
    &WORKSPACE_LABELS,
    &PROJECT_FOR_LABEL,
    &LABEL_PARENT_EXISTS,
    &LABEL_NAME_TAKEN_CREATE,
    &LABEL_NAME_TAKEN_PATCH,
    &LABEL_NAME_TAKEN_UPDATE,
    &LABEL_NAME_EXACT_PATCH,
    &LABEL_INSERT,
    &LABEL_UPDATE,
    &LABEL_BULK_INSERT,
];
