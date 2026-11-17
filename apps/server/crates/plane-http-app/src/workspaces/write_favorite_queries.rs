// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL of `WorkspaceFavoriteEndpoint.post` / `.patch` / `.delete`
//! (`plane/app/views/workspace/favorite.py:38-93`).
//!
//! Six of these statements are the `.get()` half of `get_favorite_entities`
//! (`plane/app/serializers/favorite.py:70-99`): the create branch's visibility
//! gate, one per entity type. The read path renders the *same* predicate through
//! `plane_query` (`workspaces/favorite.rs`), because `entity_data` takes the
//! favourite's stored ids while the create takes the request's — same SQL
//! shape, different bindings. `EXISTS` is enough here: the view throws the
//! result away (`favorite.py:65-67`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `UserFavorite.objects.filter(workspace=workspace, user_id=…,
    /// entity_type=request.data.get("entity_type"),
    /// entity_identifier=request.data.get("entity_identifier")).first()`
    /// (`plane/app/views/workspace/favorite.py:45-50`).
    ///
    /// `entity_type`/`entity_identifier` are the **raw** body values, coerced by
    /// `CharField`/`UUIDField.get_prep_value`; the `parent_id` column is not
    /// filtered, so a child row can be returned. `.first()` is
    /// `Meta.ordering` (`-created_at`) + `LIMIT 1`.
    ///
    /// Bind order: `$1` workspace id, `$2` user id, `$3` entity type,
    /// `$4` entity identifier.
    FAVORITE_EARLY_RETURN = "workspaces_favorite_early_return",
    "SELECT f.created_at, f.updated_at, f.created_by_id, f.updated_by_id, \
     f.deleted_at, f.id, f.workspace_id, f.project_id, f.user_id, f.entity_type, \
     f.entity_identifier, f.name, f.is_folder, f.sequence, f.parent_id \
     FROM user_favorites f \
     WHERE f.deleted_at IS NULL AND f.workspace_id = $1 AND f.user_id = $2 \
       AND f.entity_type = $3 AND f.entity_identifier = $4 \
     ORDER BY f.created_at DESC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("user_id", Uuid),
        ("entity_type", Varchar),
        ("entity_identifier", Uuid),
        ("name", Varchar),
        ("is_folder", Bool),
        ("sequence", Float8),
        ("parent_id", Uuid),
    ]
}

plane_query! {
    /// `UserFavorite.objects.get(user=request.user, workspace__slug=slug,
    /// pk=favorite_id)` (`plane/app/views/workspace/favorite.py:80,91`) — the
    /// PATCH and DELETE lookup.
    ///
    /// The manager's `deleted_at IS NULL` applies to the favourite; the
    /// `workspaces` join has **no** `deleted_at` predicate (a related-table
    /// join never runs the related manager), so a member of a soft-deleted
    /// workspace still passes here — while `Workspace.objects.get` in POST
    /// 404s.
    ///
    /// Bind order: `$1` user id, `$2` slug, `$3` favourite id.
    FAVORITE_LOOKUP = "workspaces_favorite_lookup",
    "SELECT f.created_at, f.updated_at, f.created_by_id, f.updated_by_id, \
     f.deleted_at, f.id, f.workspace_id, f.project_id, f.user_id, f.entity_type, \
     f.entity_identifier, f.name, f.is_folder, f.sequence, f.parent_id \
     FROM user_favorites f \
     INNER JOIN workspaces w ON w.id = f.workspace_id \
     WHERE f.deleted_at IS NULL AND f.user_id = $1 AND w.slug = $2 AND f.id = $3",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("user_id", Uuid),
        ("entity_type", Varchar),
        ("entity_identifier", Uuid),
        ("name", Varchar),
        ("is_folder", Bool),
        ("sequence", Float8),
        ("parent_id", Uuid),
    ]
}

plane_query! {
    /// `get_favorite_entities("cycle", user, project_id).get(pk=identifier,
    /// workspace=workspace)` (`plane/app/serializers/favorite.py:86-99`):
    ///
    /// ```sql
    /// cycles.deleted_at IS NULL AND cycles.project_id = project
    ///   AND cycles.id = identifier AND cycles.workspace_id = workspace
    ///   AND EXISTS (project_members, member, is_active, deleted_at IS NULL,
    ///               project.archived_at IS NULL)
    /// ```
    ///
    /// Bind order: `$1` project id, `$2` identifier, `$3` workspace id,
    /// `$4` user id.
    ENTITY_GATE_CYCLE = "workspaces_entity_gate_cycle",
    "SELECT EXISTS ( \
       SELECT 1 FROM cycles c \
       WHERE c.deleted_at IS NULL AND c.project_id = $1 AND c.id = $2 \
         AND c.workspace_id = $3 \
         AND EXISTS (SELECT 1 FROM project_members pm \
                     INNER JOIN projects p ON p.id = pm.project_id \
                     WHERE pm.project_id = $1 AND pm.member_id = $4 \
                       AND pm.is_active AND pm.deleted_at IS NULL \
                       AND p.archived_at IS NULL)) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// [`ENTITY_GATE_CYCLE`] for `modules`.
    ///
    /// Bind order: `$1` project id, `$2` identifier, `$3` workspace id,
    /// `$4` user id.
    ENTITY_GATE_MODULE = "workspaces_entity_gate_module",
    "SELECT EXISTS ( \
       SELECT 1 FROM modules m \
       WHERE m.deleted_at IS NULL AND m.project_id = $1 AND m.id = $2 \
         AND m.workspace_id = $3 \
         AND EXISTS (SELECT 1 FROM project_members pm \
                     INNER JOIN projects p ON p.id = pm.project_id \
                     WHERE pm.project_id = $1 AND pm.member_id = $4 \
                       AND pm.is_active AND pm.deleted_at IS NULL \
                       AND p.archived_at IS NULL)) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// [`ENTITY_GATE_CYCLE`] for `issues`. The model's default manager is the
    /// plain soft-deletion manager (`plane/db/models/issue.py:172`), so an
    /// archived, triage or draft issue **is** favouritable.
    ///
    /// Bind order: `$1` project id, `$2` identifier, `$3` workspace id,
    /// `$4` user id.
    ENTITY_GATE_ISSUE = "workspaces_entity_gate_issue",
    "SELECT EXISTS ( \
       SELECT 1 FROM issues i \
       WHERE i.deleted_at IS NULL AND i.project_id = $1 AND i.id = $2 \
         AND i.workspace_id = $3 \
         AND EXISTS (SELECT 1 FROM project_members pm \
                     INNER JOIN projects p ON p.id = pm.project_id \
                     WHERE pm.project_id = $1 AND pm.member_id = $4 \
                       AND pm.is_active AND pm.deleted_at IS NULL \
                       AND p.archived_at IS NULL)) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `get_favorite_entities("project", user, project_id).get(
    /// pk=identifier, workspace=workspace)`: the `project` branch's `in_project`
    /// is `Q(id=project_id)` (`plane/app/serializers/favorite.py:89-92`), so
    /// both ids must agree.
    ///
    /// Bind order: `$1` project id, `$2` identifier, `$3` workspace id,
    /// `$4` user id.
    ENTITY_GATE_PROJECT = "workspaces_entity_gate_project",
    "SELECT EXISTS ( \
       SELECT 1 FROM projects p \
       WHERE p.deleted_at IS NULL AND p.id = $1 AND p.id = $2 \
         AND p.workspace_id = $3 \
         AND EXISTS (SELECT 1 FROM project_members pm \
                     INNER JOIN projects mp ON mp.id = pm.project_id \
                     WHERE pm.project_id = $1 AND pm.member_id = $4 \
                       AND pm.is_active AND pm.deleted_at IS NULL \
                       AND mp.archived_at IS NULL)) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `get_favorite_entities("page", …).get(pk=…, workspace=workspace)`
    /// (`plane/app/serializers/favorite.py:89-103`): a live page linked to the
    /// project through a **live** `project_pages` row, an active member of the
    /// unarchived project, and `owned_by = user OR (access = 0 AND not a
    /// restricted guest)`. The page's own `archived_at` is not part of the
    /// predicate.
    ///
    /// Bind order: `$1` project id, `$2` identifier, `$3` workspace id,
    /// `$4` user id.
    ENTITY_GATE_PAGE = "workspaces_entity_gate_page",
    "SELECT EXISTS ( \
       SELECT 1 FROM pages pg \
       WHERE pg.deleted_at IS NULL AND pg.id = $2 AND pg.workspace_id = $3 \
         AND EXISTS (SELECT 1 FROM project_pages pp \
                     WHERE pp.page_id = pg.id AND pp.project_id = $1 \
                       AND pp.deleted_at IS NULL) \
         AND EXISTS (SELECT 1 FROM project_members pm \
                     INNER JOIN projects p ON p.id = pm.project_id \
                     WHERE pm.project_id = $1 AND pm.member_id = $4 \
                       AND pm.is_active AND pm.deleted_at IS NULL \
                       AND p.archived_at IS NULL) \
         AND (pg.owned_by_id = $4 \
              OR (pg.access = 0 AND NOT EXISTS ( \
                    SELECT 1 FROM project_members g \
                    INNER JOIN projects gp ON gp.id = g.project_id \
                    WHERE g.project_id = $1 AND g.member_id = $4 \
                      AND g.is_active AND g.deleted_at IS NULL \
                      AND gp.archived_at IS NULL \
                      AND g.role = 5 AND NOT gp.guest_view_all_features)))) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `get_favorite_entities("view", user, project_id)` with a project
    /// (`plane/app/serializers/favorite.py:86-103`): a live view of the project,
    /// an active member of the unarchived project, and `owned_by = user OR
    /// (access = 1 AND not a restricted guest)`.
    ///
    /// Bind order: `$1` project id, `$2` identifier, `$3` workspace id,
    /// `$4` user id.
    ENTITY_GATE_VIEW = "workspaces_entity_gate_view",
    "SELECT EXISTS ( \
       SELECT 1 FROM issue_views v \
       WHERE v.deleted_at IS NULL AND v.project_id = $1 AND v.id = $2 \
         AND v.workspace_id = $3 \
         AND EXISTS (SELECT 1 FROM project_members pm \
                     INNER JOIN projects p ON p.id = pm.project_id \
                     WHERE pm.project_id = $1 AND pm.member_id = $4 \
                       AND pm.is_active AND pm.deleted_at IS NULL \
                       AND p.archived_at IS NULL) \
         AND (v.owned_by_id = $4 \
              OR (v.access = 1 AND NOT EXISTS ( \
                    SELECT 1 FROM project_members g \
                    INNER JOIN projects gp ON gp.id = g.project_id \
                    WHERE g.project_id = $1 AND g.member_id = $4 \
                      AND g.is_active AND g.deleted_at IS NULL \
                      AND gp.archived_at IS NULL \
                      AND g.role = 5 AND NOT gp.guest_view_all_features)))) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `get_favorite_entities("view", user, None)` — the workspace-view branch
    /// (`plane/app/serializers/favorite.py:79-85`): an active workspace member,
    /// `project_id IS NULL`, and `owned_by = user OR (access = 1 AND not a
    /// workspace guest)`.
    ///
    /// Bind order: `$1` identifier, `$2` workspace id, `$3` user id.
    ENTITY_GATE_WORKSPACE_VIEW = "workspaces_entity_gate_workspace_view",
    "SELECT EXISTS ( \
       SELECT 1 FROM issue_views v \
       WHERE v.deleted_at IS NULL AND v.project_id IS NULL AND v.id = $1 \
         AND v.workspace_id = $2 \
         AND EXISTS (SELECT 1 FROM workspace_members wm \
                     WHERE wm.workspace_id = v.workspace_id AND wm.member_id = $3 \
                       AND wm.is_active AND wm.deleted_at IS NULL) \
         AND (v.owned_by_id = $3 \
              OR (v.access = 1 AND NOT EXISTS ( \
                    SELECT 1 FROM workspace_members rg \
                    WHERE rg.workspace_id = v.workspace_id AND rg.member_id = $3 \
                      AND rg.is_active AND rg.deleted_at IS NULL \
                      AND rg.role = 5)))) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `Workspace.objects.get(slug=slug)` (`plane/app/views/workspace/favorite.py:41`):
    /// the live row only — a soft-deleted workspace is a 404 here, unlike the
    /// PATCH/DELETE lookup.
    ///
    /// Bind order: `$1` slug.
    WORKSPACE_ID_BY_SLUG = "workspaces_write_favorite_workspace",
    "SELECT w.id FROM workspaces w WHERE w.slug = $1 AND w.deleted_at IS NULL",
    [("id", Uuid)]
}

plane_query! {
    /// `WorkspaceBaseModel.save`'s `self.workspace = self.project.workspace`
    /// (`plane/db/models/workspace.py:192-195`). The forward FK is read through
    /// `Project._base_manager`, i.e. **without** the `deleted_at` predicate: a
    /// favourite may point at a soft-deleted project's workspace.
    ///
    /// Bind order: `$1` project id.
    PROJECT_WORKSPACE = "workspaces_write_favorite_project_workspace",
    "SELECT p.workspace_id FROM projects p WHERE p.id = $1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `UserFavorite.objects.create(...)` (`plane/app/views/workspace/favorite.py:68-72`),
    /// i.e. `Model.save(force_insert=True)` with `created_by`/`updated_by` from
    /// H01 (`plane/db/models/base.py:23-45`) and `sequence` from H25.
    ///
    /// Bind order: `$1`/`$2` created/updated at, `$3` created_by, `$4` id,
    /// `$5` workspace, `$6` project, `$7` user, `$8` entity type,
    /// `$9` entity identifier, `$10` name, `$11` is_folder, `$12` sequence,
    /// `$13` parent.
    FAVORITE_INSERT = "workspaces_favorite_insert",
    "INSERT INTO user_favorites \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, project_id, user_id, entity_type, entity_identifier, name, \
      is_folder, sequence, parent_id) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    []
}

plane_query! {
    /// `instance.save()` on PATCH (`plane/app/views/workspace/favorite.py:85`):
    /// a full save, so every non-pk column is written — `updated_by` from H01's
    /// update branch, `updated_at` from `auto_now`, and `workspace` re-derived
    /// from `project` first.
    ///
    /// Bind order: `$1`/`$2` created/updated at, `$3` created_by, `$4` updated_by,
    /// `$5` deleted_at, `$6` workspace, `$7` project, `$8` user, `$9` entity
    /// type, `$10` entity identifier, `$11` name, `$12` is_folder,
    /// `$13` sequence, `$14` parent, `$15` id.
    FAVORITE_UPDATE = "workspaces_favorite_update",
    "UPDATE user_favorites SET created_at = $1, updated_at = $2, \
     created_by_id = $3, updated_by_id = $4, deleted_at = $5, workspace_id = $6, \
     project_id = $7, user_id = $8, entity_type = $9, entity_identifier = $10, \
     name = $11, is_folder = $12, sequence = $13, parent_id = $14 \
     WHERE id = $15",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &FAVORITE_EARLY_RETURN,
    &FAVORITE_LOOKUP,
    &ENTITY_GATE_CYCLE,
    &ENTITY_GATE_MODULE,
    &ENTITY_GATE_ISSUE,
    &ENTITY_GATE_PROJECT,
    &ENTITY_GATE_PAGE,
    &ENTITY_GATE_VIEW,
    &ENTITY_GATE_WORKSPACE_VIEW,
    &WORKSPACE_ID_BY_SLUG,
    &PROJECT_WORKSPACE,
    &FAVORITE_INSERT,
    &FAVORITE_UPDATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_favorite_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_favorite_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
