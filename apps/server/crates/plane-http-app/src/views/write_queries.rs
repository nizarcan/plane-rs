// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `views` writes (set B).
//!
//! | Statement | Python |
//! | --- | --- |
//! | [`VIEW_FAVORITE_GATE`] | `get_favorite_entities("view", …)` (`serializers/favorite.py:76-104`) |
//! | [`PROJECT_WORKSPACE`] | `WorkspaceBaseModel.save`'s `self.project.workspace` (`db/models/workspace.py:192-195`) |
//! | [`USER_FAVORITE_INSERT`] | `UserFavorite.objects.create` (`view/base.py:461-468`) |
//! | [`USER_FAVORITE_LOOKUP`] | the favourite `destroy` lookup (`view/base.py:472-479`) |
//! | [`USER_FAVORITE_HARD_DELETE`] | `delete(soft=False)` (`db/mixins.py:72-78`) |
//! | [`WORKSPACE_MEMBER_FOR_VIEWS`] | `WorkspaceMember.objects.get(…)` (`workspace/member.py:225`) |
//! | [`WORKSPACE_MEMBER_SAVE_VIEW_PROPS`] | `workspace_member.save()` (`workspace/member.py:227`, H01) |
//!
//! The two view reads of this group live in [`super::queries`]; this catalog is
//! the writes only, so the two files stay independent of each other.

use plane_db::plane_query;

plane_query! {
    /// `get_favorite_entities("view", user_id, project_id).get(pk=…,
    /// workspace__slug=slug)` (`serializers/favorite.py:84-98`,
    /// `view/base.py:462-464`).
    ///
    /// The project branch (183 always has a `project_id`) is
    /// `IssueView.objects.filter(project_id=project_id, Exists(member))
    /// .filter(Q(owned_by_id=user_id) | (Q(access=1) & ~Exists(restricted_guest)))
    /// .get(pk=…, workspace__slug=…)`, where
    ///
    /// * `member` = `ProjectMember.objects.filter(project_id=…, member_id=…,
    ///   is_active=True, project__archived_at__isnull=True)`, and
    /// * `restricted_guest` = `member.filter(role=5,
    ///   project__guest_view_all_features=False)`.
    ///
    /// Bind order: `$1` project id, `$2` view id, `$3` slug, `$4` user id.
    VIEW_FAVORITE_GATE = "views_write_favorite_gate",
    "SELECT EXISTS (SELECT 1 FROM issue_views iv \
     INNER JOIN workspaces w ON w.id = iv.workspace_id \
     WHERE iv.deleted_at IS NULL AND iv.project_id = $1 AND iv.id = $2 \
       AND w.slug = $3 \
       AND EXISTS (SELECT 1 FROM project_members pm \
                   INNER JOIN projects p ON p.id = pm.project_id \
                   WHERE pm.project_id = $1 AND pm.member_id = $4 \
                     AND pm.is_active AND pm.deleted_at IS NULL \
                     AND p.archived_at IS NULL) \
       AND (iv.owned_by_id = $4 OR (iv.access = 1 AND NOT EXISTS ( \
             SELECT 1 FROM project_members rg \
             INNER JOIN projects rp ON rp.id = rg.project_id \
             WHERE rg.project_id = $1 AND rg.member_id = $4 \
               AND rg.is_active AND rg.deleted_at IS NULL \
               AND rp.archived_at IS NULL AND rg.role = 5 \
               AND rp.guest_view_all_features = FALSE)))) AS \"exists\"",
    [("exists", Bool)]
}

plane_query! {
    /// `self.project.workspace` in `WorkspaceBaseModel.save`
    /// (`plane/db/models/workspace.py:192-195`) — the FK descriptor reads the
    /// **base** manager (`django/db/models/fields/related_descriptors.py`), so
    /// a soft-deleted project still resolves its workspace.
    PROJECT_WORKSPACE = "views_write_project_workspace",
    "SELECT p.workspace_id FROM projects p WHERE p.id = $1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `UserFavorite.objects.create(user=…, entity_identifier=…,
    /// entity_type="view", project_id=…)` (`view/base.py:465-471`).
    /// `workspace` comes from the project (above), `sequence` from H25, and
    /// `name`/`parent_id` stay NULL with `is_folder` false.
    ///
    /// Bind order: `$1`/`$2` created/updated at, `$3` created_by, `$4` id,
    /// `$5` workspace, `$6` project, `$7` user, `$8` entity_identifier, `$9`
    /// sequence.
    USER_FAVORITE_INSERT = "views_write_favorite_insert",
    "INSERT INTO user_favorites \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, project_id, user_id, entity_type, entity_identifier, name, \
      is_folder, sequence, parent_id) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, 'view', $8, NULL, \
       FALSE, $9, NULL)",
    []
}

plane_query! {
    /// `UserFavorite.objects.get(project=project_id, user=…,
    /// workspace__slug=slug, entity_type="view", entity_identifier=view_id)`
    /// (`view/base.py:472-479`).
    ///
    /// Bind order: `$1` project id, `$2` user id, `$3` slug, `$4` view id.
    USER_FAVORITE_LOOKUP = "views_write_favorite_lookup",
    "SELECT uf.id FROM user_favorites uf \
     INNER JOIN workspaces w ON w.id = uf.workspace_id \
     WHERE uf.deleted_at IS NULL AND uf.project_id = $1 \
       AND uf.entity_type = 'view' AND uf.user_id = $2 AND w.slug = $3 \
       AND uf.entity_identifier = $4 LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `view_favorite.delete(soft=False)` (`view/base.py:479`,
    /// `db/mixins.py:72-78`): `super().delete()` on one instance, i.e. one hard
    /// `DELETE` and no cascade task.
    ///
    /// Bind order: `$1` favourite id.
    USER_FAVORITE_HARD_DELETE = "views_write_favorite_hard_delete",
    "DELETE FROM user_favorites WHERE id = $1",
    []
}

plane_query! {
    /// `WorkspaceMember.objects.get(workspace__slug=slug,
    /// member=request.user, is_active=True)` (`workspace/member.py:225`).
    /// The manager's `deleted_at IS NULL` applies; a miss is
    /// `DoesNotExist` → the family 404.
    ///
    /// Bind order: `$1` slug, `$2` user id.
    WORKSPACE_MEMBER_FOR_VIEWS = "views_write_workspace_member_for_views",
    "SELECT wm.id, wm.view_props FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE wm.deleted_at IS NULL AND w.slug = $1 AND wm.member_id = $2 \
       AND wm.is_active LIMIT 1",
    [("id", Uuid), ("view_props", Jsonb)]
}

plane_query! {
    /// `workspace_member.save()` (`workspace/member.py:227`) — H01 changes only
    /// `updated_by`, `auto_now` only `updated_at`, and `view_props` is the one
    /// assigned column; every other column is rewritten with its own loaded
    /// value, so the row state is this UPDATE's.
    ///
    /// Bind order: `$1` view_props, `$2` updated_at, `$3` updated_by, `$4` id.
    WORKSPACE_MEMBER_SAVE_VIEW_PROPS = "views_write_workspace_member_save_view_props",
    "UPDATE workspace_members SET view_props = $1, updated_at = $2, \
     updated_by_id = $3 WHERE id = $4",
    []
}

/// Every catalogued statement of the `views` writes, in declaration order.
pub static ALL_QUERIES: &[&plane_db::Query] = &[
    &VIEW_FAVORITE_GATE,
    &PROJECT_WORKSPACE,
    &USER_FAVORITE_INSERT,
    &USER_FAVORITE_LOOKUP,
    &USER_FAVORITE_HARD_DELETE,
    &WORKSPACE_MEMBER_FOR_VIEWS,
    &WORKSPACE_MEMBER_SAVE_VIEW_PROPS,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
            assert!(!query.sql.is_empty());
        }
    }
}
