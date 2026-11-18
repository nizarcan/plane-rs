// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the v1 label routes (290/291).
//!
//! The four write statements are the app surface's
//! ([`plane_http_app::labels::queries`]): both views create, update, soft-delete
//! and parent-check the same model, and their serializers run the same
//! `PrimaryKeyRelatedField` lookup. What is new here is the v1 queryset and the
//! three `external_id` lookups the view does before and around the save.
//!
//! ## The queryset (`plane/api/views/issue.py:957-976`)
//!
//! ```python
//! Label.objects.filter(workspace__slug=slug)
//!     .filter(project_id=project_id)
//!     .filter(project__project_projectmember__member=user,
//!             project__project_projectmember__is_active=True)
//!     .filter(project__archived_at__isnull=True)
//!     .select_related("project", "workspace", "parent")
//!     .distinct()
//!     .order_by(self.kwargs.get("order_by", "-created_at"))
//! ```
//!
//! `order_by` reads `self.kwargs` — the URL kwargs — and no v1 URL binds
//! `order_by`, so the order is always `-created_at`. `select_related` cannot
//! change the row set; `distinct()` and the manager's `deleted_at IS NULL`
//! (`plane/db/mixins.py:57`) do.
//!
//! The app surface's `labels::queries::LABELS_BY_PROJECT` is *not* this query:
//! it orders `sort_order`, omits the archived-project predicate and omits
//! `pm.is_active`. Reusing it would silently reorder a project's labels.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// The list (`plane/api/views/issue.py:1103-1110`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id.
    LABELS_BY_PROJECT = "v1_labels_by_project",
    "SELECT DISTINCT l.\"created_at\", l.\"updated_at\", l.\"created_by_id\", \
     l.\"updated_by_id\", l.\"deleted_at\", l.\"id\", l.\"workspace_id\", \
     l.\"project_id\", l.\"parent_id\", l.\"name\", l.\"description\", \
     l.\"color\", l.\"sort_order\", l.\"external_source\", l.\"external_id\" \
     FROM labels l \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     INNER JOIN projects p ON (l.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 \
       AND pm.member_id = $3 AND pm.is_active AND p.archived_at IS NULL \
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

plane_query! {
    /// The same queryset plus `pk=` — `get_object()` for the detail GET and
    /// `get_queryset().get(pk=…)` for PATCH/DELETE
    /// (`plane/api/views/issue.py:1121,1139,1169`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id,
    /// `$4` label id.
    LABEL_IN_PROJECT = "v1_label_in_project",
    "SELECT DISTINCT l.\"created_at\", l.\"updated_at\", l.\"created_by_id\", \
     l.\"updated_by_id\", l.\"deleted_at\", l.\"id\", l.\"workspace_id\", \
     l.\"project_id\", l.\"parent_id\", l.\"name\", l.\"description\", \
     l.\"color\", l.\"sort_order\", l.\"external_source\", l.\"external_id\" \
     FROM labels l \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     INNER JOIN projects p ON (l.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 \
       AND pm.member_id = $3 AND pm.is_active AND p.archived_at IS NULL \
       AND l.id = $4",
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
    /// `Label.objects.get(pk=serializer.instance.id)` — the re-fetch the create
    /// and patch responses serialise (`plane/api/views/issue.py:1025,1164`).
    ///
    /// Bind order: `$1` label id.
    LABEL_BY_ID = "v1_label_by_id",
    "SELECT \"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \
     \"deleted_at\", \"id\", \"workspace_id\", \"project_id\", \"parent_id\", \
     \"name\", \"description\", \"color\", \"sort_order\", \"external_source\", \
     \"external_id\" \
     FROM labels WHERE deleted_at IS NULL AND id = $1 LIMIT 21",
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
    /// `Label.objects.filter(project_id, workspace__slug, external_source,
    /// external_id).exists()` — the create's 409 guard
    /// (`plane/api/views/issue.py:1003-1012`).
    ///
    /// Bind order: `$1` project id, `$2` workspace slug, `$3` external source,
    /// `$4` external id.
    LABEL_EXTERNAL_EXISTS = "v1_label_external_exists",
    "SELECT 1::int4 AS present FROM labels l \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     WHERE l.deleted_at IS NULL AND l.project_id = $1 AND w.slug = $2 \
       AND l.external_source = $3 AND l.external_id = $4 LIMIT 1",
    [("present", Int4)]
}

plane_query! {
    /// The row the create's 409 body reports
    /// (`plane/api/views/issue.py:1013-1022`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` external id,
    /// `$4` external source.
    LABEL_EXTERNAL_FIRST = "v1_label_external_first",
    "SELECT l.\"created_at\", l.\"updated_at\", l.\"created_by_id\", \
     l.\"updated_by_id\", l.\"deleted_at\", l.\"id\", l.\"workspace_id\", \
     l.\"project_id\", l.\"parent_id\", l.\"name\", l.\"description\", \
     l.\"color\", l.\"sort_order\", l.\"external_source\", l.\"external_id\" \
     FROM labels l \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 \
       AND l.external_id = $3 AND l.external_source = $4 \
     ORDER BY l.\"created_at\" DESC LIMIT 1",
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
    /// The patch's 409 guard: the same filter as [`LABEL_EXTERNAL_EXISTS`] with
    /// `.exclude(id=pk)` (`plane/api/views/issue.py:1141-1152`).
    ///
    /// Bind order: `$1` project id, `$2` workspace slug, `$3` external source,
    /// `$4` external id, `$5` the URL label id.
    LABEL_EXTERNAL_EXISTS_EXCLUDING = "v1_label_external_exists_excluding",
    "SELECT 1::int4 AS present FROM labels l \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     WHERE l.deleted_at IS NULL AND l.project_id = $1 AND w.slug = $2 \
       AND l.external_source = $3 AND l.external_id = $4 AND l.id <> $5 \
     LIMIT 1",
    [("present", Int4)]
}

plane_query! {
    /// `Label.objects.filter(workspace__slug, project_id, name=<raw>).first()`
    /// — the `except IntegrityError` lookup (`plane/api/views/issue.py:1029-1033`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` name.
    LABEL_BY_NAME_FIRST = "v1_label_by_name_first",
    "SELECT l.\"created_at\", l.\"updated_at\", l.\"created_by_id\", \
     l.\"updated_by_id\", l.\"deleted_at\", l.\"id\", l.\"workspace_id\", \
     l.\"project_id\", l.\"parent_id\", l.\"name\", l.\"description\", \
     l.\"color\", l.\"sort_order\", l.\"external_source\", l.\"external_id\" \
     FROM labels l \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 \
       AND l.name = $3 \
     ORDER BY l.\"created_at\" DESC LIMIT 1",
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
    /// `Label.objects.create(...)` (`plane/api/views/issue.py:1024`) — the
    /// `Label.save()`/H20 path, then `WorkspaceBaseModel.save`. Unlike the app
    /// surface's `labels::queries::LABEL_INSERT`, the v1 serializer accepts
    /// `external_source`/`external_id`, so both columns are bound.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, workspace, project, parent, name, description, color, sort_order,
    /// external_source, external_id.
    LABEL_INSERT = "v1_label_insert",
    "INSERT INTO labels \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, project_id, parent_id, name, description, color, sort_order, \
      external_source, external_id) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    []
}

plane_query! {
    /// `Label.objects.filter(project=…).aggregate(Max("sort_order"))` inside H20
    /// (`plane/db/models/label.py:38-44`). `project=None` is `IS NULL`; the
    /// manager's `deleted_at IS NULL` applies.
    ///
    /// Bind order: `$1` project id.
    LABEL_MAX_SORT_ORDER = "v1_label_max_sort_order",
    "SELECT MAX(sort_order) AS largest FROM labels \
     WHERE deleted_at IS NULL AND project_id IS NOT DISTINCT FROM $1",
    [("largest", Float8)]
}

plane_query! {
    /// The list's `SELECT COUNT(*)` (`plane/utils/paginator.py:110-132`): the
    /// ORM wraps the distinct queryset, so the count matches the page's rows.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id.
    LABELS_COUNT_BY_PROJECT = "v1_labels_count_by_project",
    "SELECT COUNT(*) AS count FROM (SELECT DISTINCT l.id \
     FROM labels l \
     INNER JOIN workspaces w ON (l.workspace_id = w.id) \
     INNER JOIN projects p ON (l.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 \
       AND pm.member_id = $3 AND pm.is_active AND p.archived_at IS NULL) counted",
    [("count", Int8)]
}

plane_query! {
    /// The v1 `ProjectLiteSerializer`'s columns for `?expand=project`
    /// (`plane/api/serializers/project.py:301-324`). The app surface's
    /// `projects::queries::PROJECTS_BY_IDS` omits `icon_prop`, `emoji` and
    /// `archived_at`, which this serializer renders.
    ///
    /// Bind order: `$1` project ids.
    PROJECT_LITE_BY_IDS = "v1_labels_project_lite_by_ids",
    "SELECT p.id, p.identifier, p.name, p.cover_image, p.cover_image_asset_id, \
       p.icon_prop, p.emoji, p.description, p.archived_at \
     FROM projects p \
     WHERE p.deleted_at IS NULL AND p.id = ANY($1)",
    [
        ("id", Uuid),
        ("identifier", Varchar),
        ("name", Varchar),
        ("cover_image", Text),
        ("cover_image_asset_id", Uuid),
        ("icon_prop", Jsonb),
        ("emoji", Varchar),
        ("description", Text),
        ("archived_at", Timestamptz),
    ]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &LABELS_BY_PROJECT,
    &LABEL_IN_PROJECT,
    &LABEL_BY_ID,
    &LABEL_EXTERNAL_EXISTS,
    &LABEL_EXTERNAL_FIRST,
    &LABEL_EXTERNAL_EXISTS_EXCLUDING,
    &LABEL_BY_NAME_FIRST,
    &LABEL_INSERT,
    &LABEL_MAX_SORT_ORDER,
    &LABELS_COUNT_BY_PROJECT,
    &PROJECT_LITE_BY_IDS,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("labels_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "labels_queries.rs declares {declared} statements but ALL_QUERIES has {}",
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
        assert!(LABEL_BY_ID.sql.starts_with("SELECT "));
    }
}
