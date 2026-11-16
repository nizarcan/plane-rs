// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `views` writes (routes 178, 179, 180, 181).
//! The read statements live in [`super::queries`].
//!
//! Every statement reproduces the ORM SQL of `plane/app/views/view/base.py`, so
//! the differential harness sees the rows Django writes. Three shapes are worth
//! naming:
//!
//! * the PATCH row is `IssueView.objects.select_for_update().get(...)` — a
//!   `FOR UPDATE` join to `workspaces`, default manager (`deleted_at IS NULL`);
//! * the soft delete is H03's `self.save()`: one full-row update, so it writes
//!   the recomputed `query` **and** the workspace the row's project implies
//!   (`WorkspaceBaseModel.save`, `plane/db/models/workspace.py:192-195`);
//! * the favourite deletes are H04 — a bare `deleted_at` update with no
//!   `updated_at` — while the recent-visit delete is `delete(soft=False)` and
//!   therefore keeps the manager's `deleted_at IS NULL` predicate
//!   (`plane/db/mixins.py:49-53`).

use plane_db::{Query, plane_query};

plane_query! {
    /// `Workspace.objects.get(slug=slug)` for `WorkspaceViewViewSet.
    /// perform_create` (`base.py:63-65`). The default manager's
    /// `deleted_at IS NULL` applies; a miss is `DoesNotExist` (404).
    ///
    /// Bind order: `$1` slug.
    VIEW_WORKSPACE_BY_SLUG = "views_write_workspace_by_slug",
    "SELECT w.id FROM workspaces w WHERE w.deleted_at IS NULL AND w.slug = $1 LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// The forward-descriptor read `WorkspaceBaseModel.save` performs
    /// (`plane/db/models/workspace.py:192-195`): `self.project` is fetched
    /// through `Project._base_manager`, which applies **no** `deleted_at`
    /// filter, and only `workspace_id` is read.
    ///
    /// Bind order: `$1` project id.
    VIEW_PROJECT_WORKSPACE = "views_write_project_workspace",
    "SELECT p.workspace_id FROM projects p WHERE p.id = $1 LIMIT 1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `IssueView.objects.create(**validated_data)`'s INSERT, in the model's
    /// column order. `access`/`is_locked` keep their model defaults (the
    /// serializer is read-only for both) and the JSON columns carry the
    /// serializer's value or the model default.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, workspace, project, name, description, query, filters,
    /// display_filters, display_properties, rich_filters, access, sort_order,
    /// logo_props, owned_by, is_locked, archived_at.
    VIEW_INSERT = "views_write_insert",
    "INSERT INTO issue_views \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, project_id, name, description, query, filters, \
      display_filters, display_properties, rich_filters, access, sort_order, \
      logo_props, owned_by_id, is_locked, archived_at) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
       $15, $16, $17, $18, $19, $20, $21)",
    []
}

plane_query! {
    /// `IssueView.objects.select_for_update().get(pk=pk, workspace__slug=slug)`
    /// (`base.py:96`, `:388`). The default manager's `deleted_at IS NULL`
    /// applies, the join to `workspaces` carries the slug (and is **not**
    /// scoped by `project__isnull` — a project view is patchable through the
    /// workspace route), and `FOR UPDATE` locks the joined rows as Django's
    /// query does.
    ///
    /// Bind order: `$1` view id, `$2` slug.
    VIEW_PATCH_ROW = "views_write_patch_row",
    "SELECT iv.id, iv.created_at, iv.updated_at, iv.deleted_at, iv.name, \
      iv.description, iv.query, iv.filters, iv.display_filters, \
      iv.display_properties, iv.rich_filters, iv.access, iv.sort_order, \
      iv.logo_props, iv.is_locked, iv.archived_at, iv.created_by_id, \
      iv.updated_by_id, iv.workspace_id, iv.project_id, iv.owned_by_id \
     FROM issue_views iv \
     INNER JOIN workspaces w ON iv.workspace_id = w.id \
     WHERE iv.deleted_at IS NULL AND iv.id = $1 AND w.slug = $2 \
     LIMIT 1 FOR UPDATE",
    [
        ("id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("name", Varchar),
        ("description", Text),
        ("query", Jsonb),
        ("filters", Jsonb),
        ("display_filters", Jsonb),
        ("display_properties", Jsonb),
        ("rich_filters", Jsonb),
        ("access", Int2),
        ("sort_order", Float8),
        ("logo_props", Jsonb),
        ("is_locked", Bool),
        ("archived_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("owned_by_id", Uuid),
    ]
}

plane_query! {
    /// `IssueView.objects.select_for_update().get(pk=pk, workspace__slug=slug,
    /// project_id=project_id)` for 179 (`base.py:388`): the project routes add
    /// the project predicate, which makes them unable to reach a workspace
    /// view.
    ///
    /// Bind order: `$1` view id, `$2` project id, `$3` slug.
    VIEW_PATCH_PROJECT_ROW = "views_write_patch_project_row",
    "SELECT iv.id, iv.created_at, iv.updated_at, iv.deleted_at, iv.name, \
      iv.description, iv.query, iv.filters, iv.display_filters, \
      iv.display_properties, iv.rich_filters, iv.access, iv.sort_order, \
      iv.logo_props, iv.is_locked, iv.archived_at, iv.created_by_id, \
      iv.updated_by_id, iv.workspace_id, iv.project_id, iv.owned_by_id \
     FROM issue_views iv \
     INNER JOIN workspaces w ON iv.workspace_id = w.id \
     WHERE iv.deleted_at IS NULL AND iv.id = $1 AND iv.project_id = $2 \
       AND w.slug = $3 \
     LIMIT 1 FOR UPDATE",
    [
        ("id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("name", Varchar),
        ("description", Text),
        ("query", Jsonb),
        ("filters", Jsonb),
        ("display_filters", Jsonb),
        ("display_properties", Jsonb),
        ("rich_filters", Jsonb),
        ("access", Int2),
        ("sort_order", Float8),
        ("logo_props", Jsonb),
        ("is_locked", Bool),
        ("archived_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("owned_by_id", Uuid),
    ]
}

plane_query! {
    /// `instance.save()` on the PATCH row: a full-row update with no
    /// `update_fields`, the pk excluded (Django's `non_pks`). H24's `query` and
    /// H01's `updated_by`/`updated_at` are already in the row the handler
    /// binds.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// workspace, project, name, description, query, filters, display_filters,
    /// display_properties, rich_filters, access, sort_order, logo_props,
    /// owned_by, is_locked, archived_at, id.
    VIEW_UPDATE = "views_write_update",
    "UPDATE issue_views SET created_at = $1, updated_at = $2, \
      created_by_id = $3, updated_by_id = $4, deleted_at = $5, \
      workspace_id = $6, project_id = $7, name = $8, description = $9, \
      query = $10, filters = $11, display_filters = $12, \
      display_properties = $13, rich_filters = $14, access = $15, \
      sort_order = $16, logo_props = $17, owned_by_id = $18, is_locked = $19, \
      archived_at = $20 \
     WHERE id = $21",
    []
}

plane_query! {
    /// `IssueView.objects.get(pk=pk, project_id=project_id,
    /// workspace__slug=slug)` for `IssueViewViewSet.destroy` (`base.py:409`).
    ///
    /// Bind order: `$1` view id, `$2` project id, `$3` slug.
    VIEW_DESTROY_PROJECT_LOOKUP = "views_write_destroy_project_lookup",
    "SELECT iv.owned_by_id, iv.filters FROM issue_views iv \
     INNER JOIN workspaces w ON iv.workspace_id = w.id \
     WHERE iv.deleted_at IS NULL AND iv.id = $1 AND iv.project_id = $2 \
       AND w.slug = $3 LIMIT 1",
    [("owned_by_id", Uuid), ("filters", Jsonb)]
}

plane_query! {
    /// `IssueView.objects.get(pk=pk, workspace__slug=slug)` for
    /// `WorkspaceViewViewSet.destroy` (`base.py:151`): no `project__isnull`
    /// predicate, so a project view is destroyable through `/views/{pk}/`.
    ///
    /// Bind order: `$1` view id, `$2` slug.
    VIEW_DESTROY_WORKSPACE_LOOKUP = "views_write_destroy_workspace_lookup",
    "SELECT iv.owned_by_id, iv.filters FROM issue_views iv \
     INNER JOIN workspaces w ON iv.workspace_id = w.id \
     WHERE iv.deleted_at IS NULL AND iv.id = $1 AND w.slug = $2 LIMIT 1",
    [("owned_by_id", Uuid), ("filters", Jsonb)]
}

plane_query! {
    /// `IssueView.delete()` — H03's `self.deleted_at = timezone.now();
    /// self.save()` (`plane/db/mixins.py:65-77`). The save is full: H24
    /// rewrites `query` from the stored `filters`, H01 sets `updated_by`, and
    /// `WorkspaceBaseModel.save` re-derives `workspace` from the row's project.
    /// Every other column keeps its stored value, so the update is equivalent
    /// to the full-row save.
    ///
    /// Bind order: `$1` query, `$2` deleted_at, `$3` updated_at, `$4`
    /// updated_by, `$5` view id.
    VIEW_SOFT_DELETE = "views_write_soft_delete",
    "UPDATE issue_views SET query = $1, deleted_at = $2, updated_at = $3, \
      updated_by_id = $4, \
      workspace_id = COALESCE((SELECT p.workspace_id FROM projects p \
        WHERE p.id = issue_views.project_id), issue_views.workspace_id) \
     WHERE id = $5",
    []
}

plane_query! {
    /// `ProjectMember.objects.filter(workspace__slug=slug,
    /// project_id=project_id, member=user, role=20, is_active=True).exists()`
    /// (`base.py:410-417`).
    ///
    /// Bind order: `$1` slug, `$2` project id, `$3` user id.
    VIEW_PROJECT_ADMIN_EXISTS = "views_write_project_admin_exists",
    "SELECT 1 AS \"exists\" FROM project_members pm \
     INNER JOIN workspaces w ON pm.workspace_id = w.id \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $3 \
       AND pm.project_id = $2 AND pm.role = 20 AND w.slug = $1 \
     LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace__slug=slug, member=user,
    /// role=20, is_active=True)` (`base.py:153-156`).
    ///
    /// Bind order: `$1` slug, `$2` user id.
    VIEW_WORKSPACE_ADMIN_EXISTS = "views_write_workspace_admin_exists",
    "SELECT 1 AS \"exists\" FROM workspace_members wm \
     INNER JOIN workspaces w ON wm.workspace_id = w.id \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND wm.member_id = $2 \
       AND wm.role = 20 AND w.slug = $1 \
     LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `UserFavorite.objects.filter(project_id=project_id,
    /// workspace__slug=slug, entity_identifier=pk, entity_type="view").delete()`
    /// (`base.py:422-427`) — H04 for **every** user: a bare `deleted_at` update,
    /// no `updated_at`, live rows only.
    ///
    /// Bind order: `$1` deleted_at, `$2` view id, `$3` project id, `$4` slug.
    VIEW_FAVORITE_SOFT_DELETE = "views_write_favorite_soft_delete",
    "UPDATE user_favorites SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND entity_type = 'view' \
       AND entity_identifier = $2 AND project_id = $3 \
       AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $4)",
    []
}

plane_query! {
    /// `UserFavorite.objects.filter(workspace__slug=slug,
    /// entity_identifier=pk, project__isnull=True, entity_type="view")
    /// .delete()` (`base.py:159-164`) — H04 for every user.
    ///
    /// Bind order: `$1` deleted_at, `$2` view id, `$3` slug.
    VIEW_WORKSPACE_FAVORITE_SOFT_DELETE = "views_write_workspace_favorite_soft_delete",
    "UPDATE user_favorites SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND entity_type = 'view' \
       AND entity_identifier = $2 AND project_id IS NULL \
       AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $3)",
    []
}

plane_query! {
    /// `UserRecentVisit.objects.filter(project_id=project_id,
    /// workspace__slug=slug, entity_identifier=pk, entity_name="view")
    /// .delete(soft=False)` (`base.py:429-434`): a hard DELETE that still runs
    /// on the default manager's queryset, so `deleted_at IS NULL` applies
    /// (`plane/db/mixins.py:49-53`).
    ///
    /// Bind order: `$1` project id, `$2` view id, `$3` slug.
    VIEW_RECENT_VISIT_HARD_DELETE = "views_write_recent_visit_hard_delete",
    "DELETE FROM user_recent_visits \
     WHERE deleted_at IS NULL AND project_id = $1 AND entity_identifier = $2 \
       AND entity_name = 'view' \
       AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $3)",
    []
}

/// Every catalogued statement of this file, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &VIEW_WORKSPACE_BY_SLUG,
    &VIEW_PROJECT_WORKSPACE,
    &VIEW_INSERT,
    &VIEW_PATCH_ROW,
    &VIEW_PATCH_PROJECT_ROW,
    &VIEW_UPDATE,
    &VIEW_DESTROY_PROJECT_LOOKUP,
    &VIEW_DESTROY_WORKSPACE_LOOKUP,
    &VIEW_SOFT_DELETE,
    &VIEW_PROJECT_ADMIN_EXISTS,
    &VIEW_WORKSPACE_ADMIN_EXISTS,
    &VIEW_FAVORITE_SOFT_DELETE,
    &VIEW_WORKSPACE_FAVORITE_SOFT_DELETE,
    &VIEW_RECENT_VISIT_HARD_DELETE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_view_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
