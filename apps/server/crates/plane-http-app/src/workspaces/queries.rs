// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `workspaces` group.
//!
//! Every statement here has a fixed shape and is prepared through the
//! connection's statement cache (`DECISIONS.md` D1). The statements that depend
//! on request data — the issue filters of `/user-stats/` and `/user-issues/`,
//! the `order_by` of `/user-activity/`, and the `entity_data` lookups — are
//! composed with `plane_query` in the handler modules and sent as unnamed
//! statements.
//!
//! Each `SELECT` list names the model columns in `from_row` order, so
//! `Model::from_row(row, start)` decodes it, and `tests/schema_check.rs`
//! prepares it against a Django-migrated database.
//!
//! The Python each statement ports is named in its doc comment. Two rules of
//! Django's SQL are preserved:
//!
//! * the default manager's `deleted_at IS NULL` predicate applies to the model
//!   the view queried, never to the models it merely joined
//!   (`plane/db/mixins.py:57`);
//! * a `select_related` is an `INNER JOIN`, except where the FK is nullable —
//!   `project_members.member` is — in which case Django emits `LEFT OUTER`
//!   (`django/db/models/sql/compiler.py`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Workspace.objects.filter(slug=slug).exists()`
    /// (`plane/app/views/workspace/base.py:225`). Django's `.exists()` is a
    /// `SELECT 1 … LIMIT 1`; the result is the same boolean.
    WORKSPACE_SLUG_EXISTS = "workspaces_slug_exists",
    "SELECT EXISTS (SELECT 1 FROM workspaces WHERE slug = $1 AND deleted_at IS NULL) AS present",
    [("present", Bool)]
}

plane_query! {
    /// `WorkSpaceViewSet.get_queryset()` + `get_object()`
    /// (`plane/app/views/workspace/base.py:64-80,171`,
    /// `rest_framework/mixins.py` `RetrieveModelMixin.retrieve`):
    ///
    /// ```text
    /// Workspace.objects.select_related("owner").order_by("name")
    ///     .filter(workspace_member__member=user, workspace_member__is_active=True)
    ///     .annotate(total_members=<count of active non-bot members>)
    ///     .get(slug=slug)
    /// ```
    ///
    /// The `owner` join only feeds a field the serializer never reads, so it is
    /// omitted. `total_members` is the correlated `Count` subquery of
    /// `base.py:65-70`; the subquery keeps the manager's `deleted_at IS NULL`
    /// because it queries `WorkspaceMember`, and the `users` join is the
    /// `member__is_bot=False` filter.
    WORKSPACE_FOR_MEMBER = "workspaces_for_member",
    "SELECT w.created_at, w.updated_at, w.created_by_id, w.updated_by_id, w.deleted_at, \
     w.id, w.name, w.logo, w.logo_asset_id, w.owner_id, w.slug, w.organization_size, \
     w.timezone, w.background_color, fa.entity_type AS logo_entity_type, \
     (SELECT count(*) FROM workspace_members cnt \
        INNER JOIN users ucnt ON ucnt.id = cnt.member_id \
       WHERE cnt.workspace_id = w.id AND cnt.deleted_at IS NULL AND cnt.is_active \
         AND NOT ucnt.is_bot) AS total_members \
     FROM workspaces w \
     INNER JOIN workspace_members wm ON wm.workspace_id = w.id \
     LEFT OUTER JOIN file_assets fa ON fa.id = w.logo_asset_id \
     WHERE wm.is_active AND wm.member_id = $1 AND w.slug = $2 AND w.deleted_at IS NULL",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("logo", Text),
        ("logo_asset_id", Uuid),
        ("owner_id", Uuid),
        ("slug", Varchar),
        ("organization_size", Varchar),
        ("timezone", Varchar),
        ("background_color", Varchar),
        ("logo_entity_type", Varchar),
        ("total_members", Int8),
    ]
}

plane_query! {
    /// `WorkSpaceMemberViewSet.get_queryset()`
    /// (`plane/app/views/workspace/member.py:57-63`):
    /// `WorkspaceMember.objects.filter(workspace__slug=slug)
    ///  .select_related("member", "member__avatar_asset")`, default ordering
    /// `-created_at` (`plane/db/models/workspace.py:224`).
    ///
    /// `member__avatar_asset` is the nullable FK `avatar_url` reads
    /// (`plane/db/models/user.py:143-151`), so it is a `LEFT OUTER` join.
    WORKSPACE_MEMBERS = "workspaces_members",
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
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     INNER JOIN users u ON u.id = wm.member_id \
     LEFT OUTER JOIN file_assets fa ON fa.id = u.avatar_asset_id \
     WHERE wm.deleted_at IS NULL AND w.slug = $1 \
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
    /// `WorkSpaceMemberViewSet.retrieve`: `self.get_queryset().get(pk=pk)`
    /// (`plane/app/views/workspace/member.py:83`). Same statement with the
    /// member's primary key.
    WORKSPACE_MEMBER_BY_ID = "workspaces_member_by_id",
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
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     INNER JOIN users u ON u.id = wm.member_id \
     LEFT OUTER JOIN file_assets fa ON fa.id = u.avatar_asset_id \
     WHERE wm.deleted_at IS NULL AND w.slug = $1 AND wm.id = $2",
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
    /// `WorkspaceMemberUserEndpoint.get`
    /// (`plane/app/views/workspace/member.py:237-251`):
    /// the caller's active membership with the `draft_issue_count`
    /// `Coalesce(Subquery(...), 0)` annotation and `.first()`.
    ///
    /// `.first()` is `ORDER BY created_at DESC LIMIT 1` on the model's default
    /// ordering; `get()` would clear it, `first()` does not.
    WORKSPACE_MEMBER_ME = "workspaces_member_me",
    "SELECT wm.created_at, wm.updated_at, wm.created_by_id, wm.updated_by_id, wm.deleted_at, \
     wm.id, wm.workspace_id, wm.member_id, wm.role, wm.company_role, wm.view_props, \
     wm.default_props, wm.issue_props, wm.is_active, wm.getting_started_checklist, wm.tips, \
     wm.explored_features, \
     COALESCE((SELECT count(*) FROM draft_issues di \
                WHERE di.deleted_at IS NULL AND di.created_by_id = $1 \
                  AND di.workspace_id = wm.workspace_id \
                GROUP BY di.workspace_id), 0) AS draft_issue_count \
     FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND wm.member_id = $1 AND w.slug = $2 \
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
        ("draft_issue_count", Int8),
    ]
}

plane_query! {
    /// `WorkspaceInvitationsViewset.get_queryset()`
    /// (`plane/app/views/workspace/invite.py:44-50`):
    /// `WorkspaceMemberInvite.objects.filter(workspace__slug=slug)
    ///  .select_related("workspace", "workspace__owner", "created_by")`.
    ///
    /// Only the nested `WorkspaceLiteSerializer` reads the workspace, so the
    /// `owner` join is omitted. `logo_url` reads the nullable `logo_asset`
    /// (`plane/db/models/workspace.py:145-154`), which Django loads lazily per
    /// row; the join here removes those round trips.
    INVITES_FOR_WORKSPACE = "workspaces_invites",
    "SELECT i.created_at, i.updated_at, i.created_by_id, i.updated_by_id, i.deleted_at, i.id, \
     i.workspace_id, i.email, i.accepted, i.token, i.message, i.responded_at, i.role, \
     w.name AS ws_name, w.slug AS ws_slug, w.id AS ws_id, \
     w.logo_asset_id AS ws_logo_asset_id, w.logo AS ws_logo, \
     fa.entity_type AS ws_logo_entity_type \
     FROM workspace_member_invites i \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     LEFT OUTER JOIN file_assets fa ON fa.id = w.logo_asset_id \
     WHERE i.deleted_at IS NULL AND w.slug = $1 \
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
        ("ws_name", Varchar),
        ("ws_slug", Varchar),
        ("ws_id", Uuid),
        ("ws_logo_asset_id", Uuid),
        ("ws_logo", Text),
        ("ws_logo_entity_type", Varchar),
    ]
}

plane_query! {
    /// `WorkspaceInvitationsViewset` `RetrieveModelMixin.retrieve`:
    /// the same queryset with the invite's primary key
    /// (`plane/app/views/workspace/invite.py:44-50`).
    INVITE_BY_ID = "workspaces_invite_by_id",
    "SELECT i.created_at, i.updated_at, i.created_by_id, i.updated_by_id, i.deleted_at, i.id, \
     i.workspace_id, i.email, i.accepted, i.token, i.message, i.responded_at, i.role, \
     w.name AS ws_name, w.slug AS ws_slug, w.id AS ws_id, \
     w.logo_asset_id AS ws_logo_asset_id, w.logo AS ws_logo, \
     fa.entity_type AS ws_logo_entity_type \
     FROM workspace_member_invites i \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     LEFT OUTER JOIN file_assets fa ON fa.id = w.logo_asset_id \
     WHERE i.deleted_at IS NULL AND w.slug = $1 AND i.id = $2",
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
        ("ws_name", Varchar),
        ("ws_slug", Varchar),
        ("ws_id", Uuid),
        ("ws_logo_asset_id", Uuid),
        ("ws_logo", Text),
        ("ws_logo_entity_type", Varchar),
    ]
}

plane_query! {
    /// `WorkspaceThemeViewSet.get_queryset()`
    /// (`plane/app/views/workspace/base.py:328-329`), default ordering
    /// `-created_at`.
    THEMES_FOR_WORKSPACE = "workspaces_themes",
    "SELECT t.created_at, t.updated_at, t.created_by_id, t.updated_by_id, t.deleted_at, t.id, \
     t.workspace_id, t.name, t.actor_id, t.colors \
     FROM workspace_themes t \
     INNER JOIN workspaces w ON w.id = t.workspace_id \
     WHERE t.deleted_at IS NULL AND w.slug = $1 \
     ORDER BY t.created_at DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("actor_id", Uuid),
        ("colors", Jsonb),
    ]
}

plane_query! {
    /// `WorkspaceThemeViewSet` `RetrieveModelMixin.retrieve`
    /// (`plane/app/views/workspace/base.py:328-329`).
    THEME_BY_ID = "workspaces_theme_by_id",
    "SELECT t.created_at, t.updated_at, t.created_by_id, t.updated_by_id, t.deleted_at, t.id, \
     t.workspace_id, t.name, t.actor_id, t.colors \
     FROM workspace_themes t \
     INNER JOIN workspaces w ON w.id = t.workspace_id \
     WHERE t.deleted_at IS NULL AND w.slug = $1 AND t.id = $2",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("actor_id", Uuid),
        ("colors", Jsonb),
    ]
}

plane_query! {
    /// `QuickLinkViewSet.list` (`plane/app/views/workspace/quick_link.py:61-65`),
    /// default ordering `-created_at`.
    QUICK_LINKS_FOR_WORKSPACE = "workspaces_quick_links",
    "SELECT q.created_at, q.updated_at, q.created_by_id, q.updated_by_id, q.deleted_at, q.id, \
     q.workspace_id, q.project_id, q.title, q.url, q.metadata, q.owner_id \
     FROM workspace_user_links q \
     INNER JOIN workspaces w ON w.id = q.workspace_id \
     WHERE q.deleted_at IS NULL AND q.owner_id = $2 AND w.slug = $1 \
     ORDER BY q.created_at DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("title", Varchar),
        ("url", Text),
        ("metadata", Jsonb),
        ("owner_id", Uuid),
    ]
}

plane_query! {
    /// `QuickLinkViewSet.retrieve` (`plane/app/views/workspace/quick_link.py:46-52`).
    QUICK_LINK_BY_ID = "workspaces_quick_link_by_id",
    "SELECT q.created_at, q.updated_at, q.created_by_id, q.updated_by_id, q.deleted_at, q.id, \
     q.workspace_id, q.project_id, q.title, q.url, q.metadata, q.owner_id \
     FROM workspace_user_links q \
     INNER JOIN workspaces w ON w.id = q.workspace_id \
     WHERE q.deleted_at IS NULL AND q.owner_id = $2 AND w.slug = $1 AND q.id = $3",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("title", Varchar),
        ("url", Text),
        ("metadata", Jsonb),
        ("owner_id", Uuid),
    ]
}

plane_query! {
    /// `Workspace.objects.get(slug=slug)` — the first statement of
    /// `WorkspaceHomePreferenceViewSet.get`, `WorkspaceUserPreferenceViewSet.get`
    /// and `WorkspaceUserPropertiesEndpoint.get`
    /// (`plane/app/views/workspace/home.py:25`, `user_preference.py:27`,
    /// `user.py:289`). A missing row is `DoesNotExist`, i.e. the family's 404.
    WORKSPACE_ID_BY_SLUG = "workspaces_id_by_slug",
    "SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL",
    [("id", Uuid)]
}

plane_query! {
    /// `WorkspaceHomePreference.objects.filter(user=request.user,
    /// workspace_id=workspace.id).values("key", "is_enabled", "config",
    /// "sort_order")` (`plane/app/views/workspace/home.py:60-64`).
    ///
    /// Default ordering `-created_at`; the dict keys keep the `values()` order,
    /// which is why the projection order is part of the contract.
    HOME_PREFERENCES_FOR_WORKSPACE = "workspaces_home_preferences",
    "SELECT h.key, h.is_enabled, h.config, h.sort_order \
     FROM workspace_home_preferences h \
     WHERE h.deleted_at IS NULL AND h.user_id = $1 AND h.workspace_id = $2 \
     ORDER BY h.created_at DESC",
    [
        ("key", Varchar),
        ("is_enabled", Bool),
        ("config", Jsonb),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `WorkspaceUserPreference.objects.filter(user=request.user,
    /// workspace_id=workspace.id).order_by("sort_order").values("key",
    /// "is_pinned", "sort_order")`
    /// (`plane/app/views/workspace/user_preference.py:63-67`).
    USER_PREFERENCES_FOR_WORKSPACE = "workspaces_user_preferences",
    "SELECT p.key, p.is_pinned, p.sort_order \
     FROM workspace_user_preferences p \
     WHERE p.deleted_at IS NULL AND p.user_id = $1 AND p.workspace_id = $2 \
     ORDER BY p.sort_order ASC",
    [
        ("key", Varchar),
        ("is_pinned", Bool),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `WorkspaceFavoriteEndpoint.get`
    /// (`plane/app/views/workspace/favorite.py:27-34`). The two `LEFT OUTER`
    /// joins are Django's, not an optimisation: the `OR` mixes
    /// `project_id IS NULL` with a condition on the joined member row, so a
    /// favourite without a project must survive the join.
    FAVORITES_FOR_WORKSPACE = "workspaces_favorites",
    "SELECT f.created_at, f.updated_at, f.created_by_id, f.updated_by_id, f.deleted_at, f.id, \
     f.workspace_id, f.project_id, f.user_id, f.entity_type, f.entity_identifier, f.name, \
     f.is_folder, f.sequence, f.parent_id \
     FROM user_favorites f \
     INNER JOIN workspaces w ON w.id = f.workspace_id \
     LEFT OUTER JOIN projects p ON p.id = f.project_id \
     LEFT OUTER JOIN project_members pm ON pm.project_id = p.id \
     WHERE f.deleted_at IS NULL AND f.parent_id IS NULL AND f.user_id = $2 \
       AND w.slug = $1 \
       AND ((f.project_id IS NULL AND NOT (f.entity_type = 'page')) \
            OR (f.project_id IS NOT NULL AND pm.member_id = $2 AND pm.is_active)) \
     ORDER BY f.created_at DESC",
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
    /// `WorkspaceFavoriteGroupEndpoint.get`
    /// (`plane/app/views/workspace/favorite.py:99-106`): the same statement with
    /// `parent_id=favorite_id` and the `page` exclusion dropped.
    FAVORITE_CHILDREN = "workspaces_favorite_children",
    "SELECT f.created_at, f.updated_at, f.created_by_id, f.updated_by_id, f.deleted_at, f.id, \
     f.workspace_id, f.project_id, f.user_id, f.entity_type, f.entity_identifier, f.name, \
     f.is_folder, f.sequence, f.parent_id \
     FROM user_favorites f \
     INNER JOIN workspaces w ON w.id = f.workspace_id \
     LEFT OUTER JOIN projects p ON p.id = f.project_id \
     LEFT OUTER JOIN project_members pm ON pm.project_id = p.id \
     WHERE f.deleted_at IS NULL AND f.parent_id = $3 AND f.user_id = $2 \
       AND w.slug = $1 \
       AND (f.project_id IS NULL \
            OR (f.project_id IS NOT NULL AND pm.member_id = $2 AND pm.is_active)) \
     ORDER BY f.created_at DESC",
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
    /// `UserRecentVisitViewSet.list` (`plane/app/views/workspace/recent_visit.py:25-35`):
    /// `filter(workspace__slug, user).filter(entity_name=<param>)
    ///  .filter(entity_name__in=["issue", "page", "project"])[:20]`.
    ///
    /// The optional `entity_name=` parameter and the fixed three-name list are
    /// composed in the handler, so this statement takes the list as an array.
    RECENT_VISITS_FOR_WORKSPACE = "workspaces_recent_visits",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, r.deleted_at, r.id, \
     r.workspace_id, r.project_id, r.entity_identifier, r.entity_name, r.user_id, r.visited_at \
     FROM user_recent_visits r \
     INNER JOIN workspaces w ON w.id = r.workspace_id \
     WHERE r.deleted_at IS NULL AND r.user_id = $2 AND w.slug = $1 \
       AND r.entity_name = ANY($3::text[]) \
     ORDER BY r.created_at DESC LIMIT 20",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("entity_identifier", Uuid),
        ("entity_name", Varchar),
        ("user_id", Uuid),
        ("visited_at", Timestamptz),
    ]
}

plane_query! {
    /// `WorkspaceProjectMemberEndpoint.get`
    /// (`plane/app/views/workspace/member.py:260-272`).
    ///
    /// The Python passes the `project_ids` queryset to `project_id__in`, so
    /// Django keeps it as a subquery; the `select_related` joins
    /// `project`, `member` and `workspace`, of which only the workspace is
    /// needed to resolve the slug.
    PROJECT_MEMBERS_FOR_WORKSPACE = "workspaces_project_members",
    "SELECT pm.id, pm.role, pm.member_id, pm.project_id, pm.created_at \
     FROM project_members pm \
     INNER JOIN workspaces w ON w.id = pm.workspace_id \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND w.slug = $1 \
       AND pm.project_id IN (SELECT DISTINCT pm2.project_id FROM project_members pm2 \
                              WHERE pm2.deleted_at IS NULL AND pm2.is_active \
                                AND pm2.member_id = $2) \
     ORDER BY pm.created_at DESC",
    [
        ("id", Uuid),
        ("role", Int2),
        ("member_id", Uuid),
        ("project_id", Uuid),
        ("created_at", Timestamptz),
    ]
}

plane_query! {
    /// `WorkspaceUserPropertiesEndpoint.get`'s `get_or_create`
    /// (`plane/app/views/workspace/user.py:291-292`): `.get()` clears the
    /// default ordering and caps the result at `MAX_GET_RESULTS` (21); the
    /// handler treats more than one row as `MultipleObjectsReturned`.
    USER_PROPERTIES_FOR_WORKSPACE = "workspaces_user_properties",
    "SELECT p.created_at, p.updated_at, p.created_by_id, p.updated_by_id, p.deleted_at, p.id, \
     p.workspace_id, p.user_id, p.filters, p.display_filters, p.display_properties, \
     p.rich_filters, p.navigation_project_limit, p.navigation_control_preference \
     FROM workspace_user_properties p \
     WHERE p.deleted_at IS NULL AND p.user_id = $1 AND p.workspace_id = $2 \
     LIMIT 21",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("user_id", Uuid),
        ("filters", Jsonb),
        ("display_filters", Jsonb),
        ("display_properties", Jsonb),
        ("rich_filters", Jsonb),
        ("navigation_project_limit", Int4),
        ("navigation_control_preference", Varchar),
    ]
}

plane_query! {
    /// `WorkspaceUserProfileEndpoint.get`'s first statement
    /// (`plane/app/views/workspace/user.py:301-303`): the caller's active
    /// membership. `get()` has no `ORDER BY` and a `LIMIT 21`.
    USER_PROFILE_REQUESTER = "workspaces_user_profile_requester",
    "SELECT wm.role FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL \
     LIMIT 21",
    [("role", Int2)]
}

plane_query! {
    /// `WorkspaceUserProfileEndpoint.get`'s second statement
    /// (`plane/app/views/workspace/user.py:307-310`):
    /// `WorkspaceMember.objects.select_related("member")
    ///  .get(workspace__slug=slug, member_id=user_id, is_active=True)`.
    ///
    /// `user_data` reads `avatar_url` and `cover_image_url`
    /// (`plane/db/models/user.py:143-162`), which Django loads with one lazy
    /// query each per row; both joins are here.
    USER_PROFILE_TARGET = "workspaces_user_profile_target",
    "SELECT wm.created_at, wm.updated_at, wm.created_by_id, wm.updated_by_id, wm.deleted_at, \
     wm.id, wm.workspace_id, wm.member_id, wm.role, wm.company_role, wm.view_props, \
     wm.default_props, wm.issue_props, wm.is_active, wm.getting_started_checklist, wm.tips, \
     wm.explored_features, \
     u.email, u.first_name, u.last_name, u.display_name, u.user_timezone, u.avatar, \
     u.avatar_asset_id, u.cover_image, u.cover_image_asset_id, u.date_joined, \
     av.entity_type AS avatar_entity_type, cv.entity_type AS cover_entity_type \
     FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     INNER JOIN users u ON u.id = wm.member_id \
     LEFT OUTER JOIN file_assets av ON av.id = u.avatar_asset_id \
     LEFT OUTER JOIN file_assets cv ON cv.id = u.cover_image_asset_id \
     WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL \
     LIMIT 21",
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
        ("email", Varchar),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("display_name", Varchar),
        ("user_timezone", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("cover_image", Varchar),
        ("cover_image_asset_id", Uuid),
        ("date_joined", Timestamptz),
        ("avatar_entity_type", Varchar),
        ("cover_entity_type", Varchar),
    ]
}

plane_query! {
    /// `WorkspaceUserProfileEndpoint.get`'s project aggregates
    /// (`plane/app/views/workspace/user.py:315-389`).
    ///
    /// The upstream query multiplies `created_issues` by the number of rows the
    /// `LEFT OUTER JOIN issue_assignees` produces, because that aggregate's
    /// `FILTER` does not mention the assignee: an issue created by the profile's
    /// user counts once per assignee. That is a bug, and it is reproduced by
    /// keeping the same join.
    USER_PROFILE_PROJECTS = "workspaces_user_profile_projects",
    "SELECT pr.id, pr.logo_props, \
     COUNT(i.id) FILTER (WHERE i.archived_at IS NULL AND i.created_by_id = $3 \
       AND NOT i.is_draft \
       AND ((SELECT U0.role FROM project_members U0 \
              WHERE U0.deleted_at IS NULL AND U0.is_active AND U0.member_id = $2 \
                AND U0.project_id = pr.id ORDER BY U0.created_at DESC LIMIT 1) > 5 \
            OR pr.guest_view_all_features OR i.created_by_id = $2)) AS created_issues, \
     COUNT(i.id) FILTER (WHERE i.archived_at IS NULL AND ia.assignee_id = $3 \
       AND NOT i.is_draft \
       AND ((SELECT U0.role FROM project_members U0 \
              WHERE U0.deleted_at IS NULL AND U0.is_active AND U0.member_id = $2 \
                AND U0.project_id = pr.id ORDER BY U0.created_at DESC LIMIT 1) > 5 \
            OR pr.guest_view_all_features OR i.created_by_id = $2)) AS assigned_issues, \
     COUNT(i.id) FILTER (WHERE i.archived_at IS NULL AND ia.assignee_id = $3 \
       AND i.completed_at IS NOT NULL AND NOT i.is_draft \
       AND ((SELECT U0.role FROM project_members U0 \
              WHERE U0.deleted_at IS NULL AND U0.is_active AND U0.member_id = $2 \
                AND U0.project_id = pr.id ORDER BY U0.created_at DESC LIMIT 1) > 5 \
            OR pr.guest_view_all_features OR i.created_by_id = $2)) AS completed_issues, \
     COUNT(i.id) FILTER (WHERE i.archived_at IS NULL AND ia.assignee_id = $3 \
       AND NOT i.is_draft AND s.group IN ('backlog', 'unstarted', 'started') \
       AND ((SELECT U0.role FROM project_members U0 \
              WHERE U0.deleted_at IS NULL AND U0.is_active AND U0.member_id = $2 \
                AND U0.project_id = pr.id ORDER BY U0.created_at DESC LIMIT 1) > 5 \
            OR pr.guest_view_all_features OR i.created_by_id = $2)) AS pending_issues \
     FROM projects pr \
     INNER JOIN project_members pm ON pm.project_id = pr.id \
     INNER JOIN workspaces w ON w.id = pr.workspace_id \
     LEFT OUTER JOIN issues i ON i.project_id = pr.id \
     LEFT OUTER JOIN issue_assignees ia ON ia.issue_id = i.id \
     LEFT OUTER JOIN states s ON s.id = i.state_id \
     WHERE pr.deleted_at IS NULL AND pr.archived_at IS NULL AND pm.is_active \
       AND pm.member_id = $2 AND w.slug = $1 \
     GROUP BY pr.id, pr.logo_props",
    [
        ("id", Uuid),
        ("logo_props", Jsonb),
        ("created_issues", Int8),
        ("assigned_issues", Int8),
        ("completed_issues", Int8),
        ("pending_issues", Int8),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &WORKSPACE_SLUG_EXISTS,
    &WORKSPACE_FOR_MEMBER,
    &WORKSPACE_MEMBERS,
    &WORKSPACE_MEMBER_BY_ID,
    &WORKSPACE_MEMBER_ME,
    &INVITES_FOR_WORKSPACE,
    &INVITE_BY_ID,
    &THEMES_FOR_WORKSPACE,
    &THEME_BY_ID,
    &QUICK_LINKS_FOR_WORKSPACE,
    &QUICK_LINK_BY_ID,
    &WORKSPACE_ID_BY_SLUG,
    &HOME_PREFERENCES_FOR_WORKSPACE,
    &USER_PREFERENCES_FOR_WORKSPACE,
    &FAVORITES_FOR_WORKSPACE,
    &FAVORITE_CHILDREN,
    &RECENT_VISITS_FOR_WORKSPACE,
    &PROJECT_MEMBERS_FOR_WORKSPACE,
    &USER_PROPERTIES_FOR_WORKSPACE,
    &USER_PROFILE_REQUESTER,
    &USER_PROFILE_TARGET,
    &USER_PROFILE_PROJECTS,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
            assert!(!query.columns.is_empty());
        }
    }
}
