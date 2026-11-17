// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The statements of workspace-core set B (`plans/WORKSPACES_CORE.md` §3):
//! routes 188, 189, 190, 191, 194 and 195.
//!
//! `workspaces/write_queries.rs` (the P0's file) holds what the sets share —
//! the caller's membership (`WORKSPACE_MEMBER_ACTIVE`), the `leave` guard's
//! admin count, the slug `UniqueValidator`. This file holds what only set B
//! runs. Three statements are lifted from other modules rather than repeated:
//! [`crate::workspaces::queries::INVITE_BY_ID`] (188/189/191's invite lookup),
//! [`crate::workspaces::write_a_queries::WORKSPACE_MEMBER_CREATE`] (190's
//! `bulk_create`) and `plane_db::hooks::soft_delete::delete_instance`
//! (189/191's `instance.delete()`).
//!
//! Every predicate is Django's, taken from the compiled queryset, and every
//! statement is prepared by `tests/schema_check.rs` against a Django-migrated
//! database. Three rules of Django's SQL recur:
//!
//! * the default manager's `deleted_at IS NULL` applies to the model the view
//!   queried, never to a model it merely joined (`plane/db/mixins.py:56-58`) —
//!   so a membership or invitation of a soft-deleted workspace still resolves,
//!   while `Workspace.objects.get` (`plane/app/views/workspace/invite.py:69`)
//!   does hide the workspace;
//! * `Meta.ordering` is part of `get()`/`first()`: every model here orders
//!   `-created_at`, and `get()` adds `LIMIT 21`
//!   (`django/db/models/query.py:get`);
//! * a queryset `update()` writes exactly the named columns — no `updated_by`,
//!   no `auto_now` on a column the call site did not name
//!   (`django/db/models/query.py:1200-1220`).

use plane_db::Query;
use plane_db::plane_query;

// ---------------------------------------------------------------------------
// 188 — `WorkspaceInvitationsViewset.create`
// ---------------------------------------------------------------------------

plane_query! {
    /// `Workspace.objects.get(slug=slug)` (`invite.py:69`).
    ///
    /// `Workspace.objects` is the soft-deletion manager, so a soft-deleted
    /// workspace is the family 404 here even though the class permission
    /// (`WorkSpaceAdminPermission`, a `workspace_members` join without a
    /// `workspaces.deleted_at` predicate) let the request through. `Meta.ordering`
    /// is `-created_at` and `.get()` appends `LIMIT 21`.
    ///
    /// Bind order: `$1` slug.
    WORKSPACE_ID_BY_SLUG = "workspaces_b_workspace_id_by_slug",
    "SELECT id FROM workspaces \
     WHERE deleted_at IS NULL AND slug = $1 \
     ORDER BY created_at DESC LIMIT 21",
    [("id", Uuid)]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace_id=…, member__email__in=[…],
    /// is_active=True).select_related("member", "member__avatar_asset")`
    /// (`invite.py:72-85`) — the "already a member" pre-check, whose
    /// truthiness *and* whose 400 body are the same queryset.
    ///
    /// `member__email__in` is an INNER JOIN on `users` with a case-**sensitive**
    /// `IN`; `avatar_asset` is a nullable FK, hence the LEFT OUTER JOIN. The
    /// join carries no `file_assets.deleted_at` predicate, exactly like the
    /// read route's `WORKSPACE_MEMBERS`.
    ///
    /// The `IN` list is bound as one `uuid`-free `text[]`: Django compiles it
    /// to a literal list, and `= ANY($2)` is the same predicate. An empty list
    /// is Django's `EmptyResultSet` (no statement at all) and the handler skips
    /// the query.
    ///
    /// Bind order: `$1` workspace id, `$2` email array.
    MEMBERS_BY_EMAIL = "workspaces_b_members_by_email",
    "SELECT wm.created_at, wm.updated_at, wm.created_by_id, wm.updated_by_id, wm.deleted_at, \
     wm.id, wm.workspace_id, wm.member_id, wm.role, wm.company_role, wm.view_props, \
     wm.default_props, wm.issue_props, wm.is_active, wm.getting_started_checklist, wm.tips, \
     wm.explored_features, \
     u.id AS user_id, u.email AS user_email, u.first_name AS user_first_name, \
     u.last_name AS user_last_name, u.avatar AS user_avatar, \
     u.avatar_asset_id AS user_avatar_asset_id, u.is_bot AS user_is_bot, \
     u.display_name AS user_display_name, u.last_login_medium AS user_last_login_medium, \
     fa.entity_type AS avatar_entity_type \
     FROM workspace_members wm \
     INNER JOIN users u ON u.id = wm.member_id \
     LEFT OUTER JOIN file_assets fa ON fa.id = u.avatar_asset_id \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND u.email = ANY($2) \
       AND wm.workspace_id = $1 \
     ORDER BY wm.created_at DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
        ("company_role", Text),
        ("view_props", Jsonb),
        ("default_props", Jsonb),
        ("issue_props", Jsonb),
        ("is_active", Bool),
        ("getting_started_checklist", Jsonb),
        ("tips", Jsonb),
        ("explored_features", Jsonb),
        ("user_id", Uuid),
        ("user_email", Varchar),
        ("user_first_name", Varchar),
        ("user_last_name", Varchar),
        ("user_avatar", Text),
        ("user_avatar_asset_id", Uuid),
        ("user_is_bot", Bool),
        ("user_display_name", Varchar),
        ("user_last_login_medium", Varchar),
        ("avatar_entity_type", Varchar),
    ]
}

plane_query! {
    /// One row of `WorkspaceMemberInvite.objects.bulk_create([…], batch_size=10,
    /// ignore_conflicts=True)` (`invite.py:112-114`).
    ///
    /// The in-memory objects are built in the loop above it, so every column is
    /// bound: `email` is the **normalised** address, `token` the freshly minted
    /// JWT, `role` the raw body value through `PositiveSmallIntegerField`'s
    /// `int()`, `created_by` the request user, and `accepted`/`message`/
    /// `responded_at`/`updated_by` their model defaults.
    ///
    /// `ignore_conflicts=True` is `ON CONFLICT DO NOTHING`; the live-row unique
    /// index is `(email, workspace)` where `deleted_at IS NULL`
    /// (`plane/db/models/workspace.py:241-256`), so a soft-deleted duplicate
    /// inserts. Django runs the whole `bulk_create` in one
    /// `transaction.atomic(savepoint=False)` (`django/db/models/query.py`), so
    /// the handler opens the same transaction around its per-row inserts.
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` id, `$5` workspace, `$6` email, `$7` token, `$8` role.
    INVITE_INSERT = "workspaces_b_invite_insert",
    "INSERT INTO workspace_member_invites \
       (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
        workspace_id, email, accepted, token, message, responded_at, role) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, FALSE, $7, NULL, NULL, $8) \
     ON CONFLICT DO NOTHING",
    []
}

// ---------------------------------------------------------------------------
// 189 — `WorkspaceInvitationsViewset.partial_update` / `.destroy`
// ---------------------------------------------------------------------------

plane_query! {
    /// `serializer.save()` for 189's `partial_update` (`invite.py:150-151`):
    /// `ModelSerializer.update` assigns `role` and calls `instance.save()`,
    /// which is `auto_now`'s `updated_at` plus H01's `updated_by`
    /// (`plane/db/models/base.py:23-44`).
    ///
    /// The three columns are the only ones that change; the full-row save
    /// writes the rest of `created_at`/`email`/`token`/… back unchanged, which
    /// a primary-key-keyed UPDATE cannot observe.
    ///
    /// Bind order: `$1` role, `$2` updated_at, `$3` updated_by, `$4` id.
    INVITE_UPDATE_ROLE = "workspaces_b_invite_update_role",
    "UPDATE workspace_member_invites \
     SET role = $1, updated_at = $2, updated_by_id = $3 \
     WHERE id = $4",
    []
}

// ---------------------------------------------------------------------------
// 190 — `UserWorkspaceInvitationsViewSet.create`
// ---------------------------------------------------------------------------

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(pk__in=invitations,
    /// email=request.user.email).order_by("-created_at")` (`invite.py:279-281`).
    ///
    /// The manager's `deleted_at IS NULL` applies to the invitation; the
    /// `workspaces` join only supplies `slug` for the per-row cache key
    /// (`invite.py:285-290`), which Django fetches with a second query per row.
    /// The join has no `deleted_at` predicate, so an invitation of a
    /// soft-deleted workspace still invalidates its (unreachable) member list.
    ///
    /// An empty `pk__in` is Django's `EmptyResultSet` and the handler skips the
    /// statement.
    ///
    /// Bind order: `$1` email, `$2` pk array.
    INVITES_FOR_USER_BY_PKS = "workspaces_b_invites_for_user_by_pks",
    "SELECT i.created_at, i.updated_at, i.created_by_id, i.updated_by_id, i.deleted_at, i.id, \
     i.workspace_id, i.email, i.accepted, i.token, i.message, i.responded_at, i.role, \
     w.slug AS ws_slug \
     FROM workspace_member_invites i \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND i.email = $1 AND i.id = ANY($2) \
     ORDER BY i.created_at DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("email", Varchar),
        ("accepted", Bool),
        ("token", Varchar),
        ("message", Text),
        ("responded_at", Timestamptz),
        ("role", Int2),
        ("ws_slug", Varchar),
    ]
}

plane_query! {
    /// `WorkspaceMember.objects.bulk_create([...], ignore_conflicts=True)` for
    /// 190 (`plane/app/views/workspace/invite.py:296-308`).
    ///
    /// The same column list as
    /// [`crate::workspaces::write_a_queries::WORKSPACE_MEMBER_CREATE`], with the
    /// conflict clause `bulk_create` adds. Every input object shares the
    /// request user and one invitation's role; `company_role` is the model's
    /// `NULL` default (the view does not pass it), which is why this cannot
    /// reuse set A's statement: a workspace create binds the body's
    /// `company_role`.
    ///
    /// Django runs the whole `bulk_create` in one
    /// `transaction.atomic(savepoint=False)`, so the handler opens one too.
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` id, `$4` role,
    /// `$5` created_by, `$6` member, `$7` workspace, `$8` view_props,
    /// `$9` default_props, `$10` issue_props.
    MEMBER_INSERT_IGNORE = "workspaces_b_member_insert_ignore",
    "INSERT INTO workspace_members \
       (created_at, updated_at, id, role, created_by_id, member_id, updated_by_id, \
        workspace_id, company_role, view_props, default_props, issue_props, is_active, \
        deleted_at, explored_features, getting_started_checklist, tips) \
     VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, NULL, $8, $9, $10, TRUE, NULL, '{}', '{}', '{}') \
     ON CONFLICT DO NOTHING",
    []
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace_id=…, member=request.user)
    /// .update(is_active=True, role=invitation.role)` (`invite.py:292-294`).
    ///
    /// A queryset `update()`: no `updated_at`, no `updated_by_id`, and no
    /// `is_bot`/`is_active` predicate beyond the manager's `deleted_at IS NULL`.
    /// The member is reactivated even if the invitation's role is lower than
    /// the stored one — the caller's body decides.
    ///
    /// Bind order: `$1` role, `$2` member id, `$3` workspace id.
    MEMBER_SET_ACTIVE_ROLE = "workspaces_b_member_set_active_role",
    "UPDATE workspace_members SET is_active = TRUE, role = $1 \
     WHERE deleted_at IS NULL AND member_id = $2 AND workspace_id = $3",
    []
}

plane_query! {
    /// `workspace_invitations.delete()` for 190 (`invite.py:311`) — H04
    /// (`plane/db/mixins.py:33-38`): `UPDATE … SET deleted_at=now` over the
    /// same queryset, so `deleted_at IS NULL`, `email` and the pk list are the
    /// predicate, and **only** `deleted_at` is written.
    ///
    /// An empty pk list never reaches here (an empty `pk__in` is Django's
    /// `EmptyResultSet`).
    ///
    /// Bind order: `$2` email, `$3` pk array (`$1` is `deleted_at`, added by
    /// `soft_delete::delete_where`).
    INVITES_DELETE_FOR_USER = "workspaces_b_invites_delete_for_user",
    "UPDATE workspace_member_invites SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND email = $2 AND id = ANY($3)",
    []
}

// ---------------------------------------------------------------------------
// 191 — `WorkspaceJoinEndpoint.post`
// ---------------------------------------------------------------------------

plane_query! {
    /// `User.objects.filter(email=workspace_invite.email).first()`
    /// (`invite.py:207`).
    ///
    /// `UserManager` has no soft-delete predicate (`plane/db/models/user.py:136`)
    /// and `Meta.ordering` is `-created_at`, so `.first()` is
    /// `ORDER BY created_at DESC LIMIT 1`. The comparison is case-**sensitive**
    /// while the view's pre-check is case-insensitive — the bug the fixture
    /// pins.
    ///
    /// Only the columns H30 rewrites and the UPDATE writes are selected.
    ///
    /// Bind order: `$1` email.
    USER_FOR_INVITE = "workspaces_b_user_for_invite",
    "SELECT id, email, display_name, is_superuser, is_staff, token, token_updated_at \
     FROM users WHERE email = $1 \
     ORDER BY created_at DESC LIMIT 1",
    [
        ("id", Uuid),
        ("email", Varchar),
        ("display_name", Varchar),
        ("is_superuser", Bool),
        ("is_staff", Bool),
        ("token", Varchar),
        ("token_updated_at", Timestamptz),
    ]
}

plane_query! {
    /// `user.save()` after the acceptance (`invite.py:228-229`) — H30
    /// (`plane/db/models/user.py:169-187`) plus `auto_now`'s `updated_at`.
    ///
    /// `user.last_workspace_id = workspace_invite.workspace.id` above it is a
    /// no-op: `last_workspace_id` lives on `Profile`, not `User`
    /// (`plane/db/models/user.py:236`), so it only sets an unused attribute on
    /// the instance. The other concrete columns keep their values.
    ///
    /// Bind order: `$1` email, `$2` display_name, `$3` is_staff, `$4` token,
    /// `$5` token_updated_at, `$6` updated_at, `$7` id.
    USER_SAVE_FOR_INVITE = "workspaces_b_user_save_for_invite",
    "UPDATE users SET email = $1, display_name = $2, is_staff = $3, token = $4, \
       token_updated_at = $5, updated_at = $6 \
     WHERE id = $7",
    []
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace=…, member=user).first()`
    /// (`invite.py:212-214`).
    ///
    /// The manager hides soft-deleted rows and `.first()` orders
    /// `-created_at`, so a soft-deleted membership is invisible and the accept
    /// creates a second row (the unique index is partial).
    ///
    /// Bind order: `$1` workspace id, `$2` member id.
    MEMBER_BY_WORKSPACE_AND_USER = "workspaces_b_member_by_workspace_and_user",
    "SELECT wm.created_at, wm.updated_at, wm.created_by_id, wm.updated_by_id, wm.deleted_at, \
     wm.id, wm.workspace_id, wm.member_id, wm.role, wm.company_role, wm.view_props, \
     wm.default_props, wm.issue_props, wm.is_active, wm.getting_started_checklist, wm.tips, \
     wm.explored_features \
     FROM workspace_members wm \
     WHERE wm.deleted_at IS NULL AND wm.workspace_id = $1 AND wm.member_id = $2 \
     ORDER BY wm.created_at DESC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
        ("company_role", Text),
        ("view_props", Jsonb),
        ("default_props", Jsonb),
        ("issue_props", Jsonb),
        ("is_active", Bool),
        ("getting_started_checklist", Jsonb),
        ("tips", Jsonb),
        ("explored_features", Jsonb),
    ]
}

plane_query! {
    /// The reactivate branch of the acceptance (`invite.py:215-218`):
    /// `workspace_member.is_active = True; role = …; save()` — a **full** save,
    /// so `auto_now`'s `updated_at` and H01's `updated_by` move as well.
    ///
    /// Bind order: `$1` updated_at, `$2` updated_by, `$3` is_active,
    /// `$4` role, `$5` id.
    MEMBER_ACCEPT_SAVE = "workspaces_b_member_accept_save",
    "UPDATE workspace_members \
     SET updated_at = $1, updated_by_id = $2, is_active = $3, role = $4 \
     WHERE id = $5",
    []
}

plane_query! {
    /// `workspace_invite.save()` after `accepted`/`responded_at` are assigned
    /// (`invite.py:201-203`): a full save, so `auto_now`'s `updated_at` and
    /// H01's `updated_by` are the other two moving columns.
    ///
    /// Bind order: `$1` accepted, `$2` responded_at, `$3` updated_at,
    /// `$4` updated_by, `$5` id.
    INVITE_ACCEPT_SAVE = "workspaces_b_invite_accept_save",
    "UPDATE workspace_member_invites \
     SET accepted = $1, responded_at = $2, updated_at = $3, updated_by_id = $4 \
     WHERE id = $5",
    []
}

// ---------------------------------------------------------------------------
// 194 / 195 — `WorkSpaceMemberViewSet.partial_update` / `.destroy` / `.leave`
// ---------------------------------------------------------------------------

plane_query! {
    /// 194 `partial_update`'s target (`plane/app/views/workspace/member.py:98-
    /// 100`): `select_related("member", "member__avatar_asset").get(pk=pk,
    /// workspace__slug=slug, member__is_bot=False, is_active=True)`.
    ///
    /// `NOT users.is_bot` is an INNER JOIN predicate; `avatar_asset` is the
    /// nullable LEFT OUTER JOIN. `Meta.ordering` is `-created_at` and `.get()`
    /// appends `LIMIT 21`.
    ///
    /// Bind order: `$1` slug, `$2` pk.
    MEMBER_TARGET_FOR_UPDATE = "workspaces_b_member_target_for_update",
    "SELECT wm.created_at, wm.updated_at, wm.created_by_id, wm.updated_by_id, wm.deleted_at, \
     wm.id, wm.workspace_id, wm.member_id, wm.role, wm.company_role, wm.view_props, \
     wm.default_props, wm.issue_props, wm.is_active, wm.getting_started_checklist, wm.tips, \
     wm.explored_features, \
     u.id AS user_id, u.email AS user_email, u.first_name AS user_first_name, \
     u.last_name AS user_last_name, u.avatar AS user_avatar, \
     u.avatar_asset_id AS user_avatar_asset_id, u.is_bot AS user_is_bot, \
     u.display_name AS user_display_name, u.last_login_medium AS user_last_login_medium, \
     fa.entity_type AS avatar_entity_type \
     FROM workspace_members wm \
     INNER JOIN users u ON u.id = wm.member_id \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     LEFT OUTER JOIN file_assets fa ON fa.id = u.avatar_asset_id \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND NOT u.is_bot \
       AND wm.id = $2 AND w.slug = $1 \
     ORDER BY wm.created_at DESC LIMIT 21",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
        ("company_role", Text),
        ("view_props", Jsonb),
        ("default_props", Jsonb),
        ("issue_props", Jsonb),
        ("is_active", Bool),
        ("getting_started_checklist", Jsonb),
        ("tips", Jsonb),
        ("explored_features", Jsonb),
        ("user_id", Uuid),
        ("user_email", Varchar),
        ("user_first_name", Varchar),
        ("user_last_name", Varchar),
        ("user_avatar", Text),
        ("user_avatar_asset_id", Uuid),
        ("user_is_bot", Bool),
        ("user_display_name", Varchar),
        ("user_last_login_medium", Varchar),
        ("avatar_entity_type", Varchar),
    ]
}

plane_query! {
    /// 194 `destroy`'s target (`member.py:144-146`): the same predicates as
    /// [`MEMBER_TARGET_FOR_UPDATE`] without the `select_related` columns.
    ///
    /// Bind order: `$1` slug, `$2` pk.
    MEMBER_TARGET_FOR_DESTROY = "workspaces_b_member_target_for_destroy",
    "SELECT wm.created_at, wm.updated_at, wm.created_by_id, wm.updated_by_id, wm.deleted_at, \
     wm.id, wm.workspace_id, wm.member_id, wm.role, wm.company_role, wm.view_props, \
     wm.default_props, wm.issue_props, wm.is_active, wm.getting_started_checklist, wm.tips, \
     wm.explored_features \
     FROM workspace_members wm \
     INNER JOIN users u ON u.id = wm.member_id \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND NOT u.is_bot \
       AND wm.id = $2 AND w.slug = $1 \
     ORDER BY wm.created_at DESC LIMIT 21",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
        ("company_role", Text),
        ("view_props", Jsonb),
        ("default_props", Jsonb),
        ("issue_props", Jsonb),
        ("is_active", Bool),
        ("getting_started_checklist", Jsonb),
        ("tips", Jsonb),
        ("explored_features", Jsonb),
    ]
}

plane_query! {
    /// `is_sole_admin_of_any_project(slug, user_id)`
    /// (`plane/app/views/workspace/member.py:31-43`), 194 PATCH/DELETE and
    /// 195's guard:
    ///
    /// ```python
    /// Project.objects.annotate(
    ///     total_members=Count("project_projectmember"),
    ///     member_with_role=Count(
    ///         "project_projectmember",
    ///         filter=Q(project_projectmember__member_id=user_id,
    ///                  project_projectmember__role=20),
    ///     ),
    /// ).filter(total_members=1, member_with_role=1, workspace__slug=slug).exists()
    /// ```
    ///
    /// The join is a bare LEFT OUTER JOIN: `Count()` over a reverse relation
    /// adds no manager predicate, so **soft-deleted and inactive** project
    /// memberships count towards `total_members`. The `projects` manager's
    /// `deleted_at IS NULL` does apply, and the `workspaces` join carries none.
    /// Ported as it is (`MODEL_HOOKS.md`, `PROGRESS.md` 2026-11-17). One project
    /// row is enough.
    ///
    /// Bind order: `$1` slug, `$2` user id.
    SOLE_ADMIN_OF_ANY_PROJECT = "workspaces_b_sole_admin_of_any_project",
    "SELECT EXISTS ( \
       SELECT 1 FROM projects \
       LEFT OUTER JOIN project_members ON (projects.id = project_members.project_id) \
       INNER JOIN workspaces ON (projects.workspace_id = workspaces.id) \
       WHERE projects.deleted_at IS NULL AND workspaces.slug = $1 \
       GROUP BY projects.id \
       HAVING COUNT(project_members.id) = 1 \
          AND COUNT(project_members.id) \
                FILTER (WHERE project_members.member_id = $2 AND project_members.role = 20) = 1 \
     ) AS sole_admin",
    [("sole_admin", Bool)]
}

plane_query! {
    /// 194 PATCH's guest demotion (`member.py:131-134`):
    /// `ProjectMember.objects.filter(workspace_id=…, member_id=…).update(
    /// role=5, updated_at=now)`.
    ///
    /// The queryset names neither `is_active` nor `deleted_at` beyond the
    /// manager's own, so an **inactive** live membership is demoted too; only a
    /// soft-deleted one is skipped.
    ///
    /// Bind order: `$1` role, `$2` updated_at, `$3` member id,
    /// `$4` workspace id.
    PROJECT_MEMBERS_DEMOTE = "workspaces_b_project_members_demote",
    "UPDATE project_members SET role = $1, updated_at = $2 \
     WHERE deleted_at IS NULL AND member_id = $3 AND workspace_id = $4",
    []
}

plane_query! {
    /// 194 DELETE and 195 (`member.py:174-176,215-217`):
    /// `ProjectMember.objects.filter(workspace__slug=slug, member_id=…,
    /// is_active=True).update(is_active=False, updated_at=now)`.
    ///
    /// Django compiles an `UPDATE` with a joined filter as
    /// `WHERE id IN (SELECT … INNER JOIN …)`. The join's `workspaces` has no
    /// `deleted_at` predicate; `is_active=True` is in the subquery, so an
    /// already-inactive membership keeps its timestamp.
    ///
    /// Bind order: `$1` updated_at, `$2` member id, `$3` slug.
    PROJECT_MEMBERS_DEACTIVATE_WS = "workspaces_b_project_members_deactivate_ws",
    "UPDATE project_members SET is_active = FALSE, updated_at = $1 \
     WHERE id IN ( \
       SELECT U0.id FROM project_members U0 \
       INNER JOIN workspaces U2 ON (U0.workspace_id = U2.id) \
       WHERE (U0.deleted_at IS NULL AND U0.is_active AND U0.member_id = $2 AND U2.slug = $3) \
     )",
    []
}

plane_query! {
    /// 194 PATCH's membership write (`member.py:137`):
    /// `workspace_member.save(update_fields=["role", "updated_at", "updated_by"])`.
    ///
    /// `update_fields` does not stop H01 (`plane/db/models/base.py:23-44`): the
    /// in-memory `updated_by` moves, and only the named columns reach the
    /// statement — which is why `is_active`, `view_props` and the rest are not
    /// reverted by a concurrent write.
    ///
    /// Bind order: `$1` role, `$2` updated_at, `$3` updated_by, `$4` id.
    MEMBER_UPDATE_ROLE = "workspaces_b_member_update_role",
    "UPDATE workspace_members \
     SET role = $1, updated_at = $2, updated_by_id = $3 \
     WHERE id = $4",
    []
}

plane_query! {
    /// 194 DELETE and 195's `workspace_member.is_active = False;
    /// workspace_member.save()` (`member.py:178-179,220-221`).
    ///
    /// A full save whose only moving columns are these three: `deleted_at`
    /// stays NULL (the membership row is never deleted) and no
    /// `soft_delete_related_objects` task is published.
    ///
    /// Bind order: `$1` updated_at, `$2` updated_by, `$3` id.
    MEMBER_DEACTIVATE = "workspaces_b_member_deactivate",
    "UPDATE workspace_members SET is_active = FALSE, updated_at = $1, updated_by_id = $2 \
     WHERE id = $3",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &WORKSPACE_ID_BY_SLUG,
    &MEMBERS_BY_EMAIL,
    &INVITE_INSERT,
    &INVITE_UPDATE_ROLE,
    &INVITES_FOR_USER_BY_PKS,
    &MEMBER_INSERT_IGNORE,
    &MEMBER_SET_ACTIVE_ROLE,
    &INVITES_DELETE_FOR_USER,
    &USER_FOR_INVITE,
    &USER_SAVE_FOR_INVITE,
    &MEMBER_BY_WORKSPACE_AND_USER,
    &MEMBER_ACCEPT_SAVE,
    &INVITE_ACCEPT_SAVE,
    &MEMBER_TARGET_FOR_UPDATE,
    &MEMBER_TARGET_FOR_DESTROY,
    &SOLE_ADMIN_OF_ANY_PROJECT,
    &PROJECT_MEMBERS_DEMOTE,
    &PROJECT_MEMBERS_DEACTIVATE_WS,
    &MEMBER_UPDATE_ROLE,
    &MEMBER_DEACTIVATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_b_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_b_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
