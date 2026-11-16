// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the project-member write paths (routes 139–146).
//!
//! Every statement is the ORM's own predicate: the default manager's
//! `deleted_at IS NULL`, the `workspace__slug` join Django adds for the lookup
//! (it never filters the workspace's own `deleted_at`), and the model's
//! ordering where the view iterates a queryset. The handlers in
//! [`super::member_write`] and [`super::invite_write`] bind in the documented
//! order.

use plane_db::Query;
use plane_db::plane_query;

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

plane_query! {
    /// `Project.objects.get(pk=project_id, workspace__slug=slug)`
    /// (`plane/app/views/project/member.py:90`). The view reads only
    /// `project.workspace_id`; a miss is `DoesNotExist` → 404.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    PROJECT_LOOKUP = "members_project_lookup",
    "SELECT p.id, p.workspace_id FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 AND p.id = $2 LIMIT 1",
    [("id", Uuid), ("workspace_id", Uuid)]
}

plane_query! {
    /// `WorkspaceMember.objects.get(workspace__slug=slug, member=<uuid>,
    /// is_active=True)`, used by route 144's role guards (`member.py:101`) and
    /// route 145's two workspace-role fetches (`member.py:241,246`).
    ///
    /// Bind order: `$1` workspace slug, `$2` member id.
    WORKSPACE_MEMBER_ROLE = "members_workspace_member_role",
    "SELECT wm.id, wm.role FROM workspace_members wm \
     INNER JOIN workspaces w ON (wm.workspace_id = w.id) \
     WHERE wm.deleted_at IS NULL AND w.slug = $1 AND wm.member_id = $2 \
       AND wm.is_active LIMIT 1",
    [("id", Uuid), ("role", Int2)]
}

plane_query! {
    /// `WorkspaceMember.objects.get(member=request.user, workspace__slug=slug,
    /// is_active=True)` — route 141 (`invite.py:88`).
    ///
    /// Bind order: `$1` member id, `$2` workspace slug.
    WORKSPACE_MEMBER_SELF = "members_workspace_member_self",
    "SELECT wm.id, wm.role, wm.workspace_id FROM workspace_members wm \
     INNER JOIN workspaces w ON (wm.workspace_id = w.id) \
     WHERE wm.deleted_at IS NULL AND wm.member_id = $1 AND w.slug = $2 \
       AND wm.is_active LIMIT 1",
    [("id", Uuid), ("role", Int2), ("workspace_id", Uuid)]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace__slug=slug, member=user).first()`
    /// — route 143 (`invite.py:205`). No `is_active` filter: an inactive row is
    /// found and reactivated.
    ///
    /// Bind order: `$1` workspace slug, `$2` member id.
    WORKSPACE_MEMBER_BY_MEMBER = "members_workspace_member_by_member",
    "SELECT wm.id, wm.role, wm.is_active FROM workspace_members wm \
     INNER JOIN workspaces w ON (wm.workspace_id = w.id) \
     WHERE wm.deleted_at IS NULL AND w.slug = $1 AND wm.member_id = $2 \
     LIMIT 1",
    [("id", Uuid), ("role", Int2), ("is_active", Bool)]
}

plane_query! {
    /// Route 143's invite lookup
    /// (`invite.py:160`: `ProjectMemberInvite.objects.get(pk=pk,
    /// project_id=project_id, workspace__slug=slug)`). The view needs
    /// `token`/`email`/`role`/`responded_at` and the two FK ids for the save.
    ///
    /// Bind order: `$1` invite id, `$2` project id, `$3` workspace slug.
    PROJECT_INVITE_BY_PK = "members_project_invite_by_pk",
    "SELECT pmi.id, pmi.email, pmi.token, pmi.role, pmi.accepted, pmi.responded_at, \
       pmi.project_id, pmi.workspace_id, pmi.message, pmi.created_at, \
       pmi.created_by_id \
     FROM project_member_invites pmi \
     INNER JOIN workspaces w ON (pmi.workspace_id = w.id) \
     WHERE pmi.deleted_at IS NULL AND pmi.id = $1 AND pmi.project_id = $2 \
       AND w.slug = $3 LIMIT 1",
    [
        ("id", Uuid),
        ("email", Varchar),
        ("token", Varchar),
        ("role", Int2),
        ("accepted", Bool),
        ("responded_at", Timestamptz),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("message", Text),
        ("created_at", Timestamptz),
        ("created_by_id", Uuid),
    ]
}

plane_query! {
    /// Route 140's `get_object()` over `ProjectInvitationsViewset.get_queryset`
    /// (`invite.py:45-55`): workspace slug, project id and the default
    /// manager's `deleted_at IS NULL`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` invite id.
    PROJECT_INVITE_DESTROY_TARGET = "members_project_invite_destroy_target",
    "SELECT pmi.id FROM project_member_invites pmi \
     INNER JOIN workspaces w ON (pmi.workspace_id = w.id) \
     WHERE pmi.deleted_at IS NULL AND w.slug = $1 AND pmi.project_id = $2 \
       AND pmi.id = $3 LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// Route 145's `ProjectMember.objects.get(pk=pk, workspace__slug=slug,
    /// project_id=project_id, is_active=True)` (`member.py:224-226`).
    ///
    /// Bind order: `$1` row id, `$2` project id, `$3` workspace slug.
    PROJECT_MEMBER_TARGET = "members_project_member_target",
    "SELECT pm.created_at, pm.updated_at, pm.created_by_id, pm.updated_by_id, \
       pm.deleted_at, pm.id, pm.workspace_id, pm.project_id, pm.member_id, \
       pm.comment, pm.role, pm.view_props, pm.default_props, pm.preferences, \
       pm.sort_order, pm.is_active \
     FROM project_members pm \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.id = $1 \
       AND pm.project_id = $2 AND w.slug = $3 LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
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
    /// The caller's own active membership — route 145's role checks
    /// (`member.py:263-268`), route 145's destroy (`member.py:319-324`) and
    /// route 146's leave (`member.py:350-355`).
    ///
    /// Bind order: `$1` project id, `$2` member id, `$3` workspace slug.
    PROJECT_MEMBER_SELF = "members_project_member_self",
    "SELECT pm.id, pm.role FROM project_members pm \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.project_id = $1 \
       AND pm.member_id = $2 AND w.slug = $3 LIMIT 1",
    [("id", Uuid), ("role", Int2)]
}

plane_query! {
    /// Route 145's destroy target: `ProjectMember.objects.get(workspace__slug,
    /// project_id, pk, member__is_bot=False, is_active=True)`
    /// (`member.py:310-317`). `member__is_bot=False` is Django's INNER JOIN to
    /// `users` with the predicate.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` row id.
    PROJECT_MEMBER_DESTROY_TARGET = "members_project_member_destroy_target",
    "SELECT pm.id, pm.role FROM project_members pm \
     INNER JOIN users u ON (pm.member_id = u.id) \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND w.slug = $1 AND pm.project_id = $2 \
       AND pm.id = $3 AND NOT u.is_bot AND pm.is_active LIMIT 1",
    [("id", Uuid), ("role", Int2)]
}

plane_query! {
    /// Route 146's last-admin count
    /// (`member.py:357-363`): active project admins in the workspace/project.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    PROJECT_ADMIN_COUNT = "members_project_admin_count",
    "SELECT COUNT(*) FROM project_members pm \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND w.slug = $1 AND pm.project_id = $2 \
       AND pm.role = 20 AND pm.is_active",
    [("count", Int8)]
}

plane_query! {
    /// Route 144's reactivation select: `ProjectMember.objects.filter(
    /// project_id=project_id, member_id__in=[…])` (`member.py:126-129`).
    ///
    /// Bind order: `$1` project id, `$2` member id array.
    PROJECT_MEMBERS_FOR_UPDATE = "members_project_members_for_update",
    "SELECT pm.id, pm.member_id FROM project_members pm \
     WHERE pm.deleted_at IS NULL AND pm.project_id = $1 \
       AND pm.member_id = ANY($2)",
    [("id", Uuid), ("member_id", Uuid)]
}

plane_query! {
    /// Route 144's sort-order map
    /// (`member.py:134-142`): `values("user_id").annotate(Min("sort_order"))`
    /// over the workspace slug and the requested member ids.
    ///
    /// Bind order: `$1` workspace slug, `$2` member id array.
    PROJECT_USER_PROPERTY_MIN = "members_project_user_property_min",
    "SELECT pup.user_id, MIN(pup.sort_order) AS min_sort_order FROM project_user_properties pup \
     INNER JOIN workspaces w ON (pup.workspace_id = w.id) \
     WHERE pup.deleted_at IS NULL AND w.slug = $1 AND pup.user_id = ANY($2) \
     GROUP BY pup.user_id",
    [("user_id", Uuid), ("min_sort_order", Float8)]
}

plane_query! {
    /// Route 144's response and email queryset:
    /// `ProjectMember.objects.filter(project_id=project_id, member_id__in=[…])`
    /// (`member.py:149-152`), ordered by the model's `("-created_at",)`.
    ///
    /// Bind order: `$1` project id, `$2` member id array.
    PROJECT_MEMBERS_FOR_RESPONSE = "members_project_members_for_response",
    "SELECT pm.id, pm.member_id, pm.role, pm.project_id, pm.created_at \
     FROM project_members pm \
     WHERE pm.deleted_at IS NULL AND pm.project_id = $1 \
       AND pm.member_id = ANY($2) \
     ORDER BY pm.created_at DESC",
    [
        ("id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
        ("project_id", Uuid),
        ("created_at", Timestamptz),
    ]
}

plane_query! {
    /// The joined row a `ProjectMemberSerializer` response renders after a
    /// PATCH (`member.py:280-285`). `PROJECT_MEMBER_ONE` (`queries.rs`) cannot
    /// be reused: it filters `is_active` and `NOT u.is_bot`, while the PATCH
    /// lookup filters neither bot nor (beyond the get itself) activity.
    ///
    /// Bind order: `$1` row id, `$2` project id, `$3` workspace slug.
    PROJECT_MEMBER_WRITE_ONE = "members_project_member_write_one",
    "SELECT pm.created_at, pm.updated_at, pm.created_by_id, pm.updated_by_id, \
       pm.deleted_at, pm.id, pm.project_id, pm.workspace_id, pm.member_id, \
       pm.comment, pm.role, pm.view_props, pm.default_props, pm.preferences, \
       pm.sort_order, pm.is_active, \
       p.id AS p_id, p.identifier AS p_identifier, p.name AS p_name, \
       p.cover_image AS p_cover_image, \
       p.cover_image_asset_id AS p_cover_image_asset_id, \
       p.logo_props AS p_logo_props, p.description AS p_description, \
       u.id AS u_id, u.first_name AS u_first_name, u.last_name AS u_last_name, \
       u.avatar AS u_avatar, u.avatar_asset_id AS u_avatar_asset_id, \
       u.is_bot AS u_is_bot, u.display_name AS u_display_name, \
       u.email AS u_email, u.last_login_medium AS u_last_login_medium, \
       w.name AS w_name, w.slug AS w_slug, w.id AS w_id, w.logo AS w_logo, \
       w.logo_asset_id AS w_logo_asset_id \
     FROM project_members pm \
     LEFT OUTER JOIN users u ON (pm.member_id = u.id) \
     INNER JOIN projects p ON (pm.project_id = p.id) \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND pm.id = $1 AND pm.project_id = $2 \
       AND w.slug = $3 LIMIT 1",
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
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
        ("u_id", Uuid),
        ("u_first_name", Varchar),
        ("u_last_name", Varchar),
        ("u_avatar", Text),
        ("u_avatar_asset_id", Uuid),
        ("u_is_bot", Bool),
        ("u_display_name", Varchar),
        ("u_email", Varchar),
        ("u_last_login_medium", Varchar),
        ("w_name", Varchar),
        ("w_slug", Varchar),
        ("w_id", Uuid),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
    ]
}

// ---------------------------------------------------------------------------
// Route 141: joining projects
// ---------------------------------------------------------------------------

plane_query! {
    /// `Project.objects.filter(id__in=project_ids,
    /// workspace__slug=slug).only("id", "network")` (`invite.py:91-92`).
    ///
    /// Bind order: `$1` project id array, `$2` workspace slug.
    PROJECTS_BY_IDS = "members_projects_by_ids",
    "SELECT p.id, p.network FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     WHERE p.deleted_at IS NULL AND p.id = ANY($1) AND w.slug = $2",
    [("id", Uuid), ("network", Int2)]
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

plane_query! {
    /// `ProjectMember.objects.create` / `bulk_create` (`member.py:144-151`,
    /// `invite.py:174-186`). Defaults are the model's: `comment` NULL,
    /// `view_props`/`default_props` `get_default_props()` (`project.py:39`),
    /// `preferences` `get_default_preferences()` (`project.py:64`),
    /// `sort_order` 65535, `is_active` true (`project.py:260-275`).
    ///
    /// `bulk_create(ignore_conflicts=True)` is `ON CONFLICT DO NOTHING`; a
    /// soft-deleted row does not conflict because the unique index is partial.
    ///
    /// Bind order: created/updated, id, role, created_by, member, project,
    /// updated_by, workspace, view_props, default_props, preferences.
    PROJECT_MEMBER_INSERT = "members_project_member_insert",
    "INSERT INTO project_members \
     (created_at, updated_at, id, comment, role, created_by_id, member_id, \
      project_id, updated_by_id, workspace_id, view_props, default_props, \
      sort_order, preferences, is_active, deleted_at) \
     VALUES ($1, $2, $3, NULL, $4, $5, $6, $7, $8, $9, $10, $11, 65535, $12, \
       TRUE, NULL) \
     ON CONFLICT DO NOTHING",
    []
}

plane_query! {
    /// `ProjectUserProperty.objects.bulk_create(..., ignore_conflicts=True)`
    /// (`member.py:157`, `invite.py:188-201`) with the model's callable
    /// defaults (`plane/db/models/project.py:392-418`) and the computed
    /// `sort_order`.
    ///
    /// Bind order: created/updated, id, display_properties, created_by,
    /// project, updated_by, user, workspace, display_filters, filters,
    /// rich_filters, preferences, sort_order.
    PROJECT_USER_PROPERTY_INSERT = "members_project_user_property_insert",
    "INSERT INTO project_user_properties \
     (created_at, updated_at, id, display_properties, created_by_id, project_id, \
      updated_by_id, user_id, workspace_id, display_filters, filters, deleted_at, \
      rich_filters, preferences, sort_order) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NULL, $12, $13, $14) \
     ON CONFLICT DO NOTHING",
    []
}

plane_query! {
    /// `ProjectMember.objects.bulk_update(…, ["is_active", "role"])`
    /// (`member.py:132`): a bare UPDATE with no save hook and no audit columns.
    ///
    /// Bind order: `$1` role, `$2` row id.
    PROJECT_MEMBER_SET_ROLE_ACTIVE = "members_project_member_set_role_active",
    "UPDATE project_members SET is_active = TRUE, role = $1 WHERE id = $2",
    []
}

plane_query! {
    /// `ProjectMember.save()` after `is_active`/`role` changed in memory
    /// (`member.py:293`, `invite.py:245-247`): full-row UPDATE, `updated_at`
    /// and `updated_by_id` from H01/`auto_now`, `created_at`/`created_by_id`
    /// unchanged.
    ///
    /// Bind order: everything but the id, then the id.
    PROJECT_MEMBER_SAVE = "members_project_member_save",
    "UPDATE project_members SET created_at = $1, updated_at = $2, id = $3, \
       comment = $4, role = $5, created_by_id = $6, member_id = $7, \
       project_id = $8, updated_by_id = $9, workspace_id = $10, view_props = $11, \
       default_props = $12, sort_order = $13, preferences = $14, is_active = $15, \
       deleted_at = $16 \
     WHERE id = $17",
    []
}

plane_query! {
    /// The destroy/leave deactivation (`member.py:341-342`, `:371-372`):
    /// `is_active = False; save()` — the save's `auto_now` timestamp and H01's
    /// `updated_by`.
    ///
    /// Bind order: `$1` updated_at, `$2` updated_by, `$3` row id.
    PROJECT_MEMBER_DEACTIVATE = "members_project_member_deactivate",
    "UPDATE project_members SET is_active = FALSE, updated_at = $1, \
       updated_by_id = $2 \
     WHERE id = $3",
    []
}

plane_query! {
    /// Route 143's `project_invite.save()` (`invite.py:196-198`): full-row
    /// UPDATE with `accepted`, `responded_at`, `auto_now`'s `updated_at` and
    /// H01's `updated_by`.
    ///
    /// Bind order: created_at, updated_at, id, email, accepted, token, message,
    /// responded_at, role, created_by, project, updated_by, workspace, id.
    PROJECT_INVITE_SAVE = "members_project_invite_save",
    "UPDATE project_member_invites SET created_at = $1, updated_at = $2, \
       id = $3, email = $4, accepted = $5, token = $6, message = $7, \
       responded_at = $8, role = $9, created_by_id = $10, project_id = $11, \
       updated_by_id = $12, workspace_id = $13, deleted_at = NULL \
     WHERE id = $14",
    []
}

plane_query! {
    /// `WorkspaceMember.objects.create(workspace_id=…, member=user, role=…)`
    /// (`invite.py:206-210`) with the model defaults (`workspace.py:198-217`):
    /// `company_role` NULL, `view_props`/`default_props` `get_default_props()`,
    /// `issue_props` `get_issue_props()`, `is_active` true, and the three
    /// `dict` fields.
    ///
    /// Bind order: created/updated, id, role, created_by, member, workspace,
    /// view_props, default_props, issue_props.
    WORKSPACE_MEMBER_INSERT = "members_workspace_member_insert",
    "INSERT INTO workspace_members \
     (created_at, updated_at, id, role, created_by_id, member_id, updated_by_id, \
      workspace_id, company_role, view_props, default_props, issue_props, \
      is_active, deleted_at, explored_features, getting_started_checklist, tips) \
     VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, NULL, $8, $9, $10, TRUE, NULL, \
       '{}', '{}', '{}')",
    []
}

plane_query! {
    /// `workspace_member.is_active = True; workspace_member.save()`
    /// (`invite.py:212-214`): the observable change is the flag plus
    /// `auto_now`/H01.
    ///
    /// Bind order: `$1` updated_at, `$2` updated_by, `$3` row id.
    WORKSPACE_MEMBER_ACTIVATE = "members_workspace_member_activate",
    "UPDATE workspace_members SET is_active = TRUE, updated_at = $1, \
       updated_by_id = $2 \
     WHERE id = $3",
    []
}

plane_query! {
    /// Route 143's membership lookup
    /// (`invite.py:236-240`): `ProjectMember.objects.filter(workspace_id=…,
    /// project_id=…, member=user).first()` — inactive rows included.
    ///
    /// Bind order: `$1` workspace id, `$2` project id, `$3` member id.
    PROJECT_MEMBER_BY_MEMBER = "members_project_member_by_member",
    "SELECT pm.id, pm.role, pm.is_active FROM project_members pm \
     WHERE pm.deleted_at IS NULL AND pm.workspace_id = $1 \
       AND pm.project_id = $2 AND pm.member_id = $3 \
     ORDER BY pm.created_at DESC LIMIT 1",
    [("id", Uuid), ("role", Int2), ("is_active", Bool)]
}

plane_query! {
    /// Route 141's reactivation (`invite.py:146-149`):
    /// `ProjectMember.objects.filter(workspace__slug=slug,
    /// project_id__in=…, member=user, is_active=False).update(is_active=True,
    /// role=workspace_role, updated_by=user, updated_at=now)` — a bare queryset
    /// UPDATE, no save hook and no `ProjectUserProperty`.
    ///
    /// Bind order: `$1` role, `$2` updated_by, `$3` updated_at, `$4` slug,
    /// `$5` project id array, `$6` member id.
    PROJECT_MEMBER_REACTIVATE = "members_project_member_reactivate",
    "UPDATE project_members pm SET is_active = TRUE, role = $1, \
       updated_by_id = $2, updated_at = $3 \
     FROM workspaces w \
     WHERE w.id = pm.workspace_id AND w.slug = $4 \
       AND pm.project_id = ANY($5) AND pm.member_id = $6 \
       AND pm.is_active = FALSE AND pm.deleted_at IS NULL",
    []
}

/// Every catalogued statement of this file, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_LOOKUP,
    &WORKSPACE_MEMBER_ROLE,
    &WORKSPACE_MEMBER_SELF,
    &WORKSPACE_MEMBER_BY_MEMBER,
    &PROJECT_INVITE_BY_PK,
    &PROJECT_INVITE_DESTROY_TARGET,
    &PROJECT_MEMBER_TARGET,
    &PROJECT_MEMBER_SELF,
    &PROJECT_MEMBER_DESTROY_TARGET,
    &PROJECT_ADMIN_COUNT,
    &PROJECT_MEMBERS_FOR_UPDATE,
    &PROJECT_USER_PROPERTY_MIN,
    &PROJECT_MEMBERS_FOR_RESPONSE,
    &PROJECT_MEMBER_WRITE_ONE,
    &PROJECTS_BY_IDS,
    &PROJECT_MEMBER_INSERT,
    &PROJECT_USER_PROPERTY_INSERT,
    &PROJECT_MEMBER_SET_ROLE_ACTIVE,
    &PROJECT_MEMBER_SAVE,
    &PROJECT_MEMBER_DEACTIVATE,
    &PROJECT_INVITE_SAVE,
    &WORKSPACE_MEMBER_INSERT,
    &WORKSPACE_MEMBER_ACTIVATE,
    &PROJECT_MEMBER_BY_MEMBER,
    &PROJECT_MEMBER_REACTIVATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("member_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "member_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
