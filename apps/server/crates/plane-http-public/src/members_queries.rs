// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the v1 member routes (292–298).
//!
//! Python: `plane/api/views/member.py:39-337`. None of these views paginates
//! through a serializer that honours `fields=`/`expand=`; the two lite lists
//! paginate, the two plain lists do not.
//!
//! Every statement reproduces a Django queryset, so the manager predicates are
//! part of the SQL: `deleted_at IS NULL` on `workspace_members`,
//! `project_members` and `projects` (`plane/db/mixins.py:57`). `users` is a
//! `TimeAuditModel` with no `deleted_at` column at all
//! (`plane/db/models/user.py:56`), so no statement here filters one.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Workspace.objects.filter(slug=slug).exists()`
    /// (`plane/api/views/member.py:84,136,190,269,316`).
    WORKSPACE_EXISTS = "v1_members_workspace_exists",
    "SELECT EXISTS (SELECT 1 FROM workspaces w \
       WHERE w.deleted_at IS NULL AND w.slug = $1) AS present",
    [("present", Bool)]
}

plane_query! {
    /// `Project.objects.filter(id=project_id, workspace__slug=slug).exists()`
    /// (`member.py:322`). `workspace__slug` is a join and does not filter the
    /// workspace's own `deleted_at`.
    PROJECT_EXISTS = "v1_members_project_exists",
    "SELECT EXISTS (SELECT 1 FROM projects p \
       INNER JOIN workspaces w ON w.id = p.workspace_id \
       WHERE p.deleted_at IS NULL AND p.id = $1 AND w.slug = $2) AS present",
    [("present", Bool)]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace__slug=slug)
    ///  .select_related("member")` — default ordering `-created_at`
    /// (`plane/db/models/workspace.py:224`). `is_active` is **not** filtered
    /// (`member.py:90`), so inactive members are part of the list.
    WORKSPACE_MEMBERS = "v1_members_workspace_members",
    "SELECT wm.id, wm.member_id, wm.role, wm.is_active, \
     u.id AS user_id, u.first_name, u.last_name, u.email, u.avatar, \
     u.avatar_asset_id, u.display_name, u.is_bot \
     FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     INNER JOIN users u ON u.id = wm.member_id \
     WHERE wm.deleted_at IS NULL AND w.slug = $1 \
     ORDER BY wm.created_at DESC",
    [
        ("id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
        ("is_active", Bool),
        ("user_id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("email", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("display_name", Varchar),
        ("is_bot", Bool),
    ]
}

plane_query! {
    /// [`WORKSPACE_MEMBERS`] sliced by `OffsetPaginator`
    /// (`plane/api/views/member.py:275-282`). `LIMIT $2` is `limit + 1`.
    WORKSPACE_MEMBERS_PAGE = "v1_members_workspace_members_page",
    "SELECT wm.id, wm.member_id, wm.role, wm.is_active, \
     u.id AS user_id, u.first_name, u.last_name, u.email, u.avatar, \
     u.avatar_asset_id, u.display_name, u.is_bot \
     FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     INNER JOIN users u ON u.id = wm.member_id \
     WHERE wm.deleted_at IS NULL AND w.slug = $1 \
     ORDER BY wm.created_at DESC",
    [
        ("id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
        ("is_active", Bool),
        ("user_id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("email", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("display_name", Varchar),
        ("is_bot", Bool),
    ]
}

plane_query! {
    /// `workspace_members.count()` for the same queryset (`paginator.py:169`).
    WORKSPACE_MEMBERS_COUNT = "v1_members_workspace_members_count",
    "SELECT COUNT(*) FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE wm.deleted_at IS NULL AND w.slug = $1",
    [("count", Int8)]
}

plane_query! {
    /// `User.objects.filter(id__in=project_members)` (`member.py:143-149`).
    ///
    /// The inner queryset is `ProjectMember.objects.filter(project_id=…,
    /// workspace__slug=…).values_list("member_id", flat=True)` — the manager's
    /// soft-delete predicate applies, `is_active` does not. `users` has no
    /// `deleted_at`; its default ordering is `-created_at`.
    PROJECT_MEMBER_USERS = "v1_members_project_member_users",
    "SELECT u.id, u.first_name, u.last_name, u.email, u.avatar, \
     u.avatar_asset_id, u.display_name \
     FROM users u \
     WHERE u.id IN ( \
       SELECT pm.member_id FROM project_members pm \
       INNER JOIN workspaces w ON w.id = pm.workspace_id \
       WHERE pm.deleted_at IS NULL AND pm.project_id = $1 AND w.slug = $2 \
     ) \
     ORDER BY u.created_at DESC",
    [
        ("id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("email", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("display_name", Varchar),
    ]
}

plane_query! {
    /// `ProjectMember.objects.get(project_id=…, workspace__slug=…, pk=pk)` then
    /// `User.objects.get(id=project_members.member_id)`
    /// (`member.py:197-199`).
    ///
    /// The two Python queries become one join. A null `member_id` is Django's
    /// `User.objects.get(id=None)` — no row, hence `DoesNotExist`, hence the
    /// family 404.
    PROJECT_MEMBER_DETAIL_USER = "v1_members_project_member_detail_user",
    "SELECT u.id, u.first_name, u.last_name, u.email, u.avatar, \
     u.avatar_asset_id, u.display_name \
     FROM project_members pm \
     INNER JOIN workspaces w ON w.id = pm.workspace_id \
     INNER JOIN users u ON u.id = pm.member_id \
     WHERE pm.deleted_at IS NULL AND pm.project_id = $1 AND w.slug = $2 \
       AND pm.id = $3 \
     LIMIT 1",
    [
        ("id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("email", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("display_name", Varchar),
    ]
}

plane_query! {
    /// `ProjectMember.objects.filter(project_id=…, workspace__slug=…)
    ///  .select_related("member").order_by("-created_at")` sliced
    /// (`member.py:328-336`). `member` is a nullable FK, so the join is
    /// `LEFT OUTER`; a null member makes the lite serializer raise
    /// `AttributeError`, which the handler turns into a 500.
    PROJECT_MEMBERS_PAGE = "v1_members_project_members_page",
    "SELECT pm.id, pm.member_id, pm.role, pm.is_active, \
     u.id AS user_id, u.first_name, u.last_name, u.email, u.avatar, \
     u.avatar_asset_id, u.display_name, u.is_bot \
     FROM project_members pm \
     INNER JOIN workspaces w ON w.id = pm.workspace_id \
     LEFT OUTER JOIN users u ON u.id = pm.member_id \
     WHERE pm.deleted_at IS NULL AND pm.project_id = $1 AND w.slug = $2 \
     ORDER BY pm.created_at DESC",
    [
        ("id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
        ("is_active", Bool),
        ("user_id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("email", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("display_name", Varchar),
        ("is_bot", Bool),
    ]
}

plane_query! {
    /// `project_members.count()` for [`PROJECT_MEMBERS_PAGE`].
    PROJECT_MEMBERS_COUNT = "v1_members_project_members_count",
    "SELECT COUNT(*) FROM project_members pm \
     INNER JOIN workspaces w ON w.id = pm.workspace_id \
     WHERE pm.deleted_at IS NULL AND pm.project_id = $1 AND w.slug = $2",
    [("count", Int8)]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace__slug=slug, member=value,
    ///  is_active=True).exists()` (`api/serializers/member.py:27-28`) and the
    /// role read in `validate` (`:53-57`).
    WORKSPACE_MEMBER_ROLE = "v1_members_workspace_member_role",
    "SELECT wm.role FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE wm.deleted_at IS NULL AND wm.is_active \
       AND w.slug = $1 AND wm.member_id = $2 \
     LIMIT 1",
    [("role", Int2)]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace__slug=slug,
    ///  member_id=…, is_active=True).exists()` — the PATCH pre-check
    /// (`plane/api/views/member.py:214-216`).
    WORKSPACE_MEMBER_ACTIVE = "v1_members_workspace_member_active",
    "SELECT EXISTS (SELECT 1 FROM workspace_members wm \
       INNER JOIN workspaces w ON w.id = wm.workspace_id \
       WHERE wm.deleted_at IS NULL AND wm.is_active \
         AND w.slug = $1 AND wm.member_id = $2) AS present",
    [("present", Bool)]
}

plane_query! {
    /// `ProjectMember.objects.get(project_id=…, workspace__slug=…, pk=pk)`
    /// (`member.py:212,232`).
    PROJECT_MEMBER_BY_PK = "v1_members_project_member_by_pk",
    "SELECT pm.created_at, pm.updated_at, pm.created_by_id, pm.updated_by_id, \
     pm.deleted_at, pm.id, pm.project_id, pm.workspace_id, pm.member_id, \
     pm.comment, pm.role, pm.view_props, pm.default_props, pm.preferences, \
     pm.sort_order, pm.is_active \
     FROM project_members pm \
     INNER JOIN workspaces w ON w.id = pm.workspace_id \
     WHERE pm.deleted_at IS NULL AND pm.project_id = $1 AND w.slug = $2 \
       AND pm.id = $3 \
     LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("member_id", Uuid),
        ("comment", Text),
        ("role", Int2),
        ("view_props", Jsonb),
        ("default_props", Jsonb),
        ("preferences", Jsonb),
        ("sort_order", Float8),
        ("is_active", Bool),
    ]
}

plane_query! {
    /// `ProjectMember.save()`'s `self.project.workspace_id`
    /// (`plane/db/models/project.py:277`) — the forward FK descriptor uses the
    /// *base* manager, so a soft-deleted project still resolves and a missing
    /// one is `Project.DoesNotExist` (the family 404).
    PROJECT_WORKSPACE_ID_ANY = "v1_members_project_workspace_id_any",
    "SELECT p.workspace_id FROM projects p WHERE p.id = $1 LIMIT 1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `ProjectMember.objects.create(...)` (`member.py:163`), after
    /// `BaseModel.save` (`plane/db/models/base.py:23-45`).
    PROJECT_MEMBER_INSERT = "v1_members_project_member_insert",
    "INSERT INTO project_members (created_at, updated_at, created_by_id, \
     updated_by_id, deleted_at, id, project_id, workspace_id, member_id, \
     comment, role, view_props, default_props, preferences, sort_order, \
     is_active) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
     $15, $16)",
    []
}

plane_query! {
    /// `ProjectUserProperty.objects.filter(workspace_id=…, user=…)
    ///  .aggregate(Min("sort_order"))` (`plane/db/models/project.py:277-280`).
    PROJECT_USER_PROPERTY_MIN = "v1_members_project_user_property_min",
    "SELECT MIN(pup.sort_order) FROM project_user_properties pup \
     WHERE pup.deleted_at IS NULL AND pup.workspace_id = $1 \
       AND pup.user_id = $2",
    [("min", Float8)]
}

plane_query! {
    /// `ProjectUserProperty.objects.create(...)` from `ProjectMember.save()`
    /// (`plane/db/models/project.py:281-288`).
    PROJECT_USER_PROPERTY_INSERT = "v1_members_project_user_property_insert",
    "INSERT INTO project_user_properties (created_at, updated_at, created_by_id, \
     updated_by_id, deleted_at, id, project_id, workspace_id, user_id, filters, \
     display_filters, display_properties, rich_filters, preferences, sort_order) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    []
}

plane_query! {
    /// The receipt of a PATCH on `ProjectMember` (`plane/db/models/project.py:271`):
    /// `serializer.save()` writes every validated field and then a full-row
    /// `save()`, so `BaseModel.save` stamps `updated_at`/`updated_by`.
    PROJECT_MEMBER_UPDATE = "v1_members_project_member_update",
    "UPDATE project_members SET created_at = $1, updated_at = $2, \
     created_by_id = $3, updated_by_id = $4, deleted_at = $5, project_id = $6, \
     workspace_id = $7, member_id = $8, comment = $9, role = $10, \
     view_props = $11, default_props = $12, preferences = $13, \
     sort_order = $14, is_active = $15 \
     WHERE id = $16",
    []
}

plane_query! {
    /// `project_member.is_active = False; project_member.save()`
    /// (`plane/api/views/member.py:232-234`).
    PROJECT_MEMBER_SET_INACTIVE = "v1_members_project_member_set_inactive",
    "UPDATE project_members SET updated_at = $1, updated_by_id = $2, \
     is_active = false \
     WHERE id = $3",
    []
}

/// Every catalogued statement of this route family, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &WORKSPACE_EXISTS,
    &PROJECT_EXISTS,
    &WORKSPACE_MEMBERS,
    &WORKSPACE_MEMBERS_PAGE,
    &WORKSPACE_MEMBERS_COUNT,
    &PROJECT_MEMBER_USERS,
    &PROJECT_MEMBER_DETAIL_USER,
    &PROJECT_MEMBERS_PAGE,
    &PROJECT_MEMBERS_COUNT,
    &WORKSPACE_MEMBER_ROLE,
    &WORKSPACE_MEMBER_ACTIVE,
    &PROJECT_MEMBER_BY_PK,
    &PROJECT_WORKSPACE_ID_ANY,
    &PROJECT_MEMBER_INSERT,
    &PROJECT_USER_PROPERTY_MIN,
    &PROJECT_USER_PROPERTY_INSERT,
    &PROJECT_MEMBER_UPDATE,
    &PROJECT_MEMBER_SET_INACTIVE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("members_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
