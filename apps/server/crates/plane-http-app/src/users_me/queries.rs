// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `users-me` group.
//!
//! Each statement names the Python it reproduces. The Django ORM's statements
//! are quoted in the doc comments where the shape is not obvious; the two
//! genuinely dynamic ones (the activity page's ordering and the workspace
//! list's filters) are built by the handlers and are therefore *not* in this
//! catalog (`DECISIONS.md` D1 amendment).
//!
//! `tests/schema_check.rs` prepares every statement here against a
//! Django-migrated database, so a renamed column or changed type fails in CI.

use plane_db::plane_query;

plane_query! {
    /// `User.objects.get(pk=request.user.id)` plus the two properties
    /// `UserMeSerializer` reads: `avatar_url` and `cover_image_url`
    /// (`plane/db/models/user.py:149-167`).
    ///
    /// Django loads the user in the auth layer and then queries `file_assets`
    /// once per non-null avatar/cover asset. Folding the two left joins into
    /// the user lookup answers with the same values in one round trip. The
    /// asset joins deliberately carry no `deleted_at` predicate: a related
    /// object is fetched through `_base_manager`
    /// (`django/db/models/fields/related_descriptors.py`), so a soft-deleted
    /// avatar still renders.
    USER_BY_ID_WITH_ASSETS = "users_me_user_by_id_with_assets",
    "SELECT u.\"password\" AS password, u.\"last_login\" AS last_login, u.\"id\" AS id, \
     u.\"username\" AS username, u.\"mobile_number\" AS mobile_number, u.\"email\" AS email, \
     u.\"display_name\" AS display_name, u.\"first_name\" AS first_name, \
     u.\"last_name\" AS last_name, u.\"avatar\" AS avatar, \
     u.\"avatar_asset_id\" AS avatar_asset_id, u.\"cover_image\" AS cover_image, \
     u.\"cover_image_asset_id\" AS cover_image_asset_id, u.\"date_joined\" AS date_joined, \
     u.\"created_at\" AS created_at, u.\"updated_at\" AS updated_at, \
     u.\"last_location\" AS last_location, u.\"created_location\" AS created_location, \
     u.\"is_superuser\" AS is_superuser, u.\"is_managed\" AS is_managed, \
     u.\"is_password_expired\" AS is_password_expired, u.\"is_active\" AS is_active, \
     u.\"is_staff\" AS is_staff, u.\"is_email_verified\" AS is_email_verified, \
     u.\"is_password_autoset\" AS is_password_autoset, \
     u.\"is_password_reset_required\" AS is_password_reset_required, u.\"token\" AS token, \
     u.\"last_active\" AS last_active, u.\"last_login_time\" AS last_login_time, \
     u.\"last_logout_time\" AS last_logout_time, u.\"last_login_ip\" AS last_login_ip, \
     u.\"last_logout_ip\" AS last_logout_ip, u.\"last_login_medium\" AS last_login_medium, \
     u.\"last_login_uagent\" AS last_login_uagent, \
     u.\"token_updated_at\" AS token_updated_at, u.\"is_bot\" AS is_bot, \
     u.\"bot_type\" AS bot_type, u.\"user_timezone\" AS user_timezone, \
     u.\"is_email_valid\" AS is_email_valid, u.\"masked_at\" AS masked_at, \
     aa.\"id\" AS avatar_id, aa.\"entity_type\" AS avatar_entity_type, \
     aaw.\"slug\" AS avatar_workspace_slug, aa.\"project_id\" AS avatar_project_id, \
     aa.\"issue_id\" AS avatar_issue_id, \
     ca.\"id\" AS cover_id, ca.\"entity_type\" AS cover_entity_type, \
     caw.\"slug\" AS cover_workspace_slug, ca.\"project_id\" AS cover_project_id, \
     ca.\"issue_id\" AS cover_issue_id \
     FROM users u \
     LEFT JOIN file_assets aa ON aa.\"id\" = u.\"avatar_asset_id\" \
     LEFT JOIN workspaces aaw ON aaw.\"id\" = aa.\"workspace_id\" \
     LEFT JOIN file_assets ca ON ca.\"id\" = u.\"cover_image_asset_id\" \
     LEFT JOIN workspaces caw ON caw.\"id\" = ca.\"workspace_id\" \
     WHERE u.\"id\" = $1 LIMIT 21",
    [
        ("password", Varchar),
        ("last_login", Timestamptz),
        ("id", Uuid),
        ("username", Varchar),
        ("mobile_number", Varchar),
        ("email", Varchar),
        ("display_name", Varchar),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("cover_image", Varchar),
        ("cover_image_asset_id", Uuid),
        ("date_joined", Timestamptz),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("last_location", Varchar),
        ("created_location", Varchar),
        ("is_superuser", Bool),
        ("is_managed", Bool),
        ("is_password_expired", Bool),
        ("is_active", Bool),
        ("is_staff", Bool),
        ("is_email_verified", Bool),
        ("is_password_autoset", Bool),
        ("is_password_reset_required", Bool),
        ("token", Varchar),
        ("last_active", Timestamptz),
        ("last_login_time", Timestamptz),
        ("last_logout_time", Timestamptz),
        ("last_login_ip", Varchar),
        ("last_logout_ip", Varchar),
        ("last_login_medium", Varchar),
        ("last_login_uagent", Text),
        ("token_updated_at", Timestamptz),
        ("is_bot", Bool),
        ("bot_type", Varchar),
        ("user_timezone", Varchar),
        ("is_email_valid", Bool),
        ("masked_at", Timestamptz),
        ("avatar_id", Uuid),
        ("avatar_entity_type", Varchar),
        ("avatar_workspace_slug", Varchar),
        ("avatar_project_id", Uuid),
        ("avatar_issue_id", Uuid),
        ("cover_id", Uuid),
        ("cover_entity_type", Varchar),
        ("cover_workspace_slug", Varchar),
        ("cover_project_id", Uuid),
        ("cover_issue_id", Uuid),
    ]
}

plane_query! {
    /// `Profile.objects.get(user=request.user)` (`plane/app/views/user/base.py:446`).
    USER_PROFILE = "users_me_profile_by_user",
    "SELECT p.\"created_at\" AS created_at, p.\"updated_at\" AS updated_at, p.\"id\" AS id, \
     p.\"user_id\" AS user_id, p.\"theme\" AS theme, \
     p.\"is_app_rail_docked\" AS is_app_rail_docked, \
     p.\"is_tour_completed\" AS is_tour_completed, p.\"onboarding_step\" AS onboarding_step, \
     p.\"use_case\" AS use_case, p.\"role\" AS role, p.\"is_onboarded\" AS is_onboarded, \
     p.\"last_workspace_id\" AS last_workspace_id, \
     p.\"billing_address_country\" AS billing_address_country, \
     p.\"billing_address\" AS billing_address, \
     p.\"has_billing_address\" AS has_billing_address, p.\"company_name\" AS company_name, \
     p.\"notification_view_mode\" AS notification_view_mode, \
     p.\"is_smooth_cursor_enabled\" AS is_smooth_cursor_enabled, \
     p.\"is_mobile_onboarded\" AS is_mobile_onboarded, \
     p.\"mobile_onboarding_step\" AS mobile_onboarding_step, \
     p.\"mobile_timezone_auto_set\" AS mobile_timezone_auto_set, \
     p.\"language\" AS language, p.\"start_of_the_week\" AS start_of_the_week, \
     p.\"goals\" AS goals, p.\"background_color\" AS background_color, \
     p.\"is_navigation_tour_completed\" AS is_navigation_tour_completed, \
     p.\"has_marketing_email_consent\" AS has_marketing_email_consent, \
     p.\"is_subscribed_to_changelog\" AS is_subscribed_to_changelog, \
     p.\"product_tour\" AS product_tour \
     FROM profiles p WHERE p.\"user_id\" = $1 LIMIT 21",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("id", Uuid),
        ("user_id", Uuid),
        ("theme", Jsonb),
        ("is_app_rail_docked", Bool),
        ("is_tour_completed", Bool),
        ("onboarding_step", Jsonb),
        ("use_case", Text),
        ("role", Varchar),
        ("is_onboarded", Bool),
        ("last_workspace_id", Uuid),
        ("billing_address_country", Varchar),
        ("billing_address", Jsonb),
        ("has_billing_address", Bool),
        ("company_name", Varchar),
        ("notification_view_mode", Varchar),
        ("is_smooth_cursor_enabled", Bool),
        ("is_mobile_onboarded", Bool),
        ("mobile_onboarding_step", Jsonb),
        ("mobile_timezone_auto_set", Bool),
        ("language", Varchar),
        ("start_of_the_week", Int2),
        ("goals", Jsonb),
        ("background_color", Varchar),
        ("is_navigation_tour_completed", Bool),
        ("has_marketing_email_consent", Bool),
        ("is_subscribed_to_changelog", Bool),
        ("product_tour", Jsonb),
    ]
}

plane_query! {
    /// `Account.objects.filter(user=request.user)`
    /// (`plane/app/views/user/base.py:433`), default ordering `-created_at`.
    USER_ACCOUNTS = "users_me_accounts_by_user",
    "SELECT a.\"created_at\" AS created_at, a.\"updated_at\" AS updated_at, a.\"id\" AS id, \
     a.\"user_id\" AS user_id, a.\"provider_account_id\" AS provider_account_id, \
     a.\"provider\" AS provider, a.\"access_token\" AS access_token, \
     a.\"access_token_expired_at\" AS access_token_expired_at, \
     a.\"refresh_token\" AS refresh_token, \
     a.\"refresh_token_expired_at\" AS refresh_token_expired_at, \
     a.\"last_connected_at\" AS last_connected_at, a.\"id_token\" AS id_token, \
     a.\"metadata\" AS metadata \
     FROM accounts a WHERE a.\"user_id\" = $1 ORDER BY a.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("id", Uuid),
        ("user_id", Uuid),
        ("provider_account_id", Varchar),
        ("provider", Varchar),
        ("access_token", Text),
        ("access_token_expired_at", Timestamptz),
        ("refresh_token", Text),
        ("refresh_token_expired_at", Timestamptz),
        ("last_connected_at", Timestamptz),
        ("id_token", Text),
        ("metadata", Jsonb),
    ]
}

plane_query! {
    /// `Account.objects.get(pk=pk, user=request.user)`
    /// (`plane/app/views/user/base.py:430`).
    USER_ACCOUNT_BY_ID = "users_me_account_by_id",
    "SELECT a.\"created_at\" AS created_at, a.\"updated_at\" AS updated_at, a.\"id\" AS id, \
     a.\"user_id\" AS user_id, a.\"provider_account_id\" AS provider_account_id, \
     a.\"provider\" AS provider, a.\"access_token\" AS access_token, \
     a.\"access_token_expired_at\" AS access_token_expired_at, \
     a.\"refresh_token\" AS refresh_token, \
     a.\"refresh_token_expired_at\" AS refresh_token_expired_at, \
     a.\"last_connected_at\" AS last_connected_at, a.\"id_token\" AS id_token, \
     a.\"metadata\" AS metadata \
     FROM accounts a WHERE a.\"id\" = $1 AND a.\"user_id\" = $2 LIMIT 21",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("id", Uuid),
        ("user_id", Uuid),
        ("provider_account_id", Varchar),
        ("provider", Varchar),
        ("access_token", Text),
        ("access_token_expired_at", Timestamptz),
        ("refresh_token", Text),
        ("refresh_token_expired_at", Timestamptz),
        ("last_connected_at", Timestamptz),
        ("id_token", Text),
        ("metadata", Jsonb),
    ]
}

plane_query! {
    /// `Instance.objects.first()` and
    /// `InstanceAdmin.objects.filter(instance=instance, user=request.user).exists()`
    /// (`plane/app/views/user/base.py:95-97`), folded into one statement.
    ///
    /// With no instance row Django runs the admin query with
    /// `instance_id IS NULL`, which can never match (the column is NOT NULL);
    /// the subquery reproduces that as no row, which the handler reads as
    /// `false`.
    INSTANCE_ADMIN = "users_me_instance_admin",
    "SELECT i.\"id\" AS instance_id, \
     EXISTS (SELECT 1 FROM instance_admins ia \
             WHERE ia.\"deleted_at\" IS NULL AND ia.\"instance_id\" = i.\"id\" \
               AND ia.\"user_id\" = $1) AS is_admin \
     FROM instances i WHERE i.\"deleted_at\" IS NULL \
     ORDER BY i.\"created_at\" DESC LIMIT 1",
    [("instance_id", Uuid), ("is_admin", Bool)]
}

plane_query! {
    /// `IssueActivity.objects.filter(actor=request.user).count()`
    /// (`plane/db/mixins.py:57` manager predicate), the paginator's
    /// `total_count`.
    USER_ACTIVITY_COUNT = "users_me_activity_count",
    "SELECT COUNT(*)::int8 AS total FROM issue_activities a \
     WHERE a.\"deleted_at\" IS NULL AND a.\"actor_id\" = $1",
    [("total", Int8)]
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(email=obj.email).count()`
    /// (`plane/app/serializers/user.py:113`).
    USER_INVITE_COUNT = "users_me_invite_count",
    "SELECT COUNT(*)::int8 AS total FROM workspace_member_invites i \
     WHERE i.\"deleted_at\" IS NULL AND i.\"email\" = $1",
    [("total", Int8)]
}

plane_query! {
    /// The last-visited branch of `UserMeSettingsSerializer.get_workspace`
    /// (`plane/app/serializers/user.py:117-130`):
    /// `Workspace.objects.filter(pk=profile.last_workspace_id,
    /// workspace_member__member=obj.id, workspace_member__is_active=True).first()`
    /// plus `workspace.logo_asset.asset_url`.
    ///
    /// The membership test is an `EXISTS` here; Django emits an INNER JOIN,
    /// which yields the same rows for a unique `(workspace, member)` pair and
    /// `LIMIT 1` collapses either.
    WORKSPACE_FOR_USER_BY_ID = "users_me_workspace_for_user_by_id",
    "SELECT w.\"created_at\" AS created_at, w.\"updated_at\" AS updated_at, \
     w.\"created_by_id\" AS created_by_id, w.\"updated_by_id\" AS updated_by_id, \
     w.\"deleted_at\" AS deleted_at, w.\"id\" AS id, w.\"name\" AS name, \
     w.\"logo\" AS logo, w.\"logo_asset_id\" AS logo_asset_id, w.\"owner_id\" AS owner_id, \
     w.\"slug\" AS slug, w.\"organization_size\" AS organization_size, \
     w.\"timezone\" AS timezone, w.\"background_color\" AS background_color, \
     la.\"id\" AS logo_id, la.\"entity_type\" AS logo_entity_type, \
     law.\"slug\" AS logo_workspace_slug, la.\"project_id\" AS logo_project_id, \
     la.\"issue_id\" AS logo_issue_id \
     FROM workspaces w \
     LEFT JOIN file_assets la ON la.\"id\" = w.\"logo_asset_id\" \
     LEFT JOIN workspaces law ON law.\"id\" = la.\"workspace_id\" \
     WHERE w.\"deleted_at\" IS NULL AND w.\"id\" = $1 \
       AND EXISTS (SELECT 1 FROM workspace_members wm \
                   WHERE wm.\"workspace_id\" = w.\"id\" AND wm.\"member_id\" = $2 \
                     AND wm.\"is_active\") \
     ORDER BY w.\"created_at\" DESC LIMIT 1",
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
        ("logo_id", Uuid),
        ("logo_entity_type", Varchar),
        ("logo_workspace_slug", Varchar),
        ("logo_project_id", Uuid),
        ("logo_issue_id", Uuid),
    ]
}

plane_query! {
    /// The fallback branch of `UserMeSettingsSerializer.get_workspace`
    /// (`plane/app/serializers/user.py:137-143`):
    /// `Workspace.objects.filter(workspace_member__member_id=obj.id,
    /// workspace_member__is_active=True).order_by("created_at").first()`.
    ///
    /// The relation join does not apply `WorkspaceMember`'s manager predicate
    /// (a related join only carries the predicates Django adds), so
    /// `workspace_members.deleted_at` is intentionally absent, as in the
    /// captured SQL.
    FALLBACK_WORKSPACE_FOR_USER = "users_me_fallback_workspace_for_user",
    "SELECT w.\"created_at\" AS created_at, w.\"updated_at\" AS updated_at, \
     w.\"created_by_id\" AS created_by_id, w.\"updated_by_id\" AS updated_by_id, \
     w.\"deleted_at\" AS deleted_at, w.\"id\" AS id, w.\"name\" AS name, \
     w.\"logo\" AS logo, w.\"logo_asset_id\" AS logo_asset_id, w.\"owner_id\" AS owner_id, \
     w.\"slug\" AS slug, w.\"organization_size\" AS organization_size, \
     w.\"timezone\" AS timezone, w.\"background_color\" AS background_color, \
     la.\"id\" AS logo_id, la.\"entity_type\" AS logo_entity_type, \
     law.\"slug\" AS logo_workspace_slug, la.\"project_id\" AS logo_project_id, \
     la.\"issue_id\" AS logo_issue_id \
     FROM workspaces w \
     LEFT JOIN file_assets la ON la.\"id\" = w.\"logo_asset_id\" \
     LEFT JOIN workspaces law ON law.\"id\" = la.\"workspace_id\" \
     WHERE w.\"deleted_at\" IS NULL \
       AND EXISTS (SELECT 1 FROM workspace_members wm \
                   WHERE wm.\"workspace_id\" = w.\"id\" AND wm.\"member_id\" = $1 \
                     AND wm.\"is_active\") \
     ORDER BY w.\"created_at\" ASC LIMIT 1",
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
        ("logo_id", Uuid),
        ("logo_entity_type", Varchar),
        ("logo_workspace_slug", Varchar),
        ("logo_project_id", Uuid),
        ("logo_issue_id", Uuid),
    ]
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(email=request.user.email)
    /// .select_related("workspace")` (`plane/app/views/workspace/invite.py:263-266`),
    /// default ordering `-created_at`, plus the workspace's `logo_asset`.
    INVITES_BY_EMAIL = "users_me_invites_by_email",
    "SELECT i.\"created_at\" AS created_at, i.\"updated_at\" AS updated_at, \
     i.\"created_by_id\" AS created_by_id, i.\"updated_by_id\" AS updated_by_id, \
     i.\"deleted_at\" AS deleted_at, i.\"id\" AS id, i.\"workspace_id\" AS workspace_id, \
     i.\"email\" AS email, i.\"accepted\" AS accepted, i.\"token\" AS token, \
     i.\"message\" AS message, i.\"responded_at\" AS responded_at, i.\"role\" AS role, \
     w.\"created_at\" AS w_created_at, w.\"updated_at\" AS w_updated_at, \
     w.\"created_by_id\" AS w_created_by_id, w.\"updated_by_id\" AS w_updated_by_id, \
     w.\"deleted_at\" AS w_deleted_at, w.\"id\" AS w_id, w.\"name\" AS w_name, \
     w.\"logo\" AS w_logo, w.\"logo_asset_id\" AS w_logo_asset_id, \
     w.\"owner_id\" AS w_owner_id, w.\"slug\" AS w_slug, \
     w.\"organization_size\" AS w_organization_size, w.\"timezone\" AS w_timezone, \
     w.\"background_color\" AS w_background_color, \
     la.\"id\" AS logo_id, la.\"entity_type\" AS logo_entity_type, \
     law.\"slug\" AS logo_workspace_slug, la.\"project_id\" AS logo_project_id, \
     la.\"issue_id\" AS logo_issue_id \
     FROM workspace_member_invites i \
     INNER JOIN workspaces w ON i.\"workspace_id\" = w.\"id\" \
     LEFT JOIN file_assets la ON la.\"id\" = w.\"logo_asset_id\" \
     LEFT JOIN workspaces law ON law.\"id\" = la.\"workspace_id\" \
     WHERE i.\"deleted_at\" IS NULL AND i.\"email\" = $1 \
     ORDER BY i.\"created_at\" DESC",
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
        ("w_created_at", Timestamptz),
        ("w_updated_at", Timestamptz),
        ("w_created_by_id", Uuid),
        ("w_updated_by_id", Uuid),
        ("w_deleted_at", Timestamptz),
        ("w_id", Uuid),
        ("w_name", Varchar),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
        ("w_owner_id", Uuid),
        ("w_slug", Varchar),
        ("w_organization_size", Varchar),
        ("w_timezone", Varchar),
        ("w_background_color", Varchar),
        ("logo_id", Uuid),
        ("logo_entity_type", Varchar),
        ("logo_workspace_slug", Varchar),
        ("logo_project_id", Uuid),
        ("logo_issue_id", Uuid),
    ]
}

plane_query! {
    /// `ProjectMemberInvite.objects.filter(email=request.user.email)
    /// .select_related("workspace", "workspace__owner", "project")`
    /// (`plane/app/views/project/invite.py:136-142`), default ordering
    /// `-created_at`.
    ///
    /// The `GET` of this viewset never reaches the handler: `BaseViewSet`'s
    /// action guard refuses the mixin-provided `list` first
    /// (`plane/app/views/base.py:103-172`). The statement exists so the
    /// handler is a real port rather than a stub, and so the guard can be
    /// verified against a route whose handler is complete.
    PROJECT_INVITES_BY_EMAIL = "users_me_project_invites_by_email",
    "SELECT i.\"created_at\" AS created_at, i.\"updated_at\" AS updated_at, \
     i.\"created_by_id\" AS created_by_id, i.\"updated_by_id\" AS updated_by_id, \
     i.\"deleted_at\" AS deleted_at, i.\"id\" AS id, i.\"project_id\" AS project_id, \
     i.\"workspace_id\" AS workspace_id, i.\"email\" AS email, i.\"accepted\" AS accepted, \
     i.\"token\" AS token, i.\"message\" AS message, i.\"responded_at\" AS responded_at, \
     i.\"role\" AS role, \
     p.\"id\" AS p_id, p.\"identifier\" AS p_identifier, p.\"name\" AS p_name, \
     p.\"cover_image\" AS p_cover_image, \
     p.\"cover_image_asset_id\" AS p_cover_image_asset_id, p.\"logo_props\" AS p_logo_props, \
     p.\"description\" AS p_description, \
     ca.\"id\" AS cover_id, ca.\"entity_type\" AS cover_entity_type, \
     caw.\"slug\" AS cover_workspace_slug, ca.\"project_id\" AS cover_project_id, \
     ca.\"issue_id\" AS cover_issue_id, \
     w.\"created_at\" AS w_created_at, w.\"updated_at\" AS w_updated_at, \
     w.\"created_by_id\" AS w_created_by_id, w.\"updated_by_id\" AS w_updated_by_id, \
     w.\"deleted_at\" AS w_deleted_at, w.\"id\" AS w_id, w.\"name\" AS w_name, \
     w.\"logo\" AS w_logo, w.\"logo_asset_id\" AS w_logo_asset_id, \
     w.\"owner_id\" AS w_owner_id, w.\"slug\" AS w_slug, \
     w.\"organization_size\" AS w_organization_size, w.\"timezone\" AS w_timezone, \
     w.\"background_color\" AS w_background_color, \
     la.\"id\" AS logo_id, la.\"entity_type\" AS logo_entity_type, \
     law.\"slug\" AS logo_workspace_slug, la.\"project_id\" AS logo_project_id, \
     la.\"issue_id\" AS logo_issue_id \
     FROM project_member_invites i \
     INNER JOIN projects p ON i.\"project_id\" = p.\"id\" \
     INNER JOIN workspaces w ON i.\"workspace_id\" = w.\"id\" \
     LEFT JOIN file_assets ca ON ca.\"id\" = p.\"cover_image_asset_id\" \
     LEFT JOIN workspaces caw ON caw.\"id\" = ca.\"workspace_id\" \
     LEFT JOIN file_assets la ON la.\"id\" = w.\"logo_asset_id\" \
     LEFT JOIN workspaces law ON law.\"id\" = la.\"workspace_id\" \
     WHERE i.\"deleted_at\" IS NULL AND i.\"email\" = $1 \
     ORDER BY i.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("email", Varchar),
        ("accepted", Bool),
        ("token", Varchar),
        ("message", Text),
        ("responded_at", Timestamptz),
        ("role", Int2),
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
        ("cover_id", Uuid),
        ("cover_entity_type", Varchar),
        ("cover_workspace_slug", Varchar),
        ("cover_project_id", Uuid),
        ("cover_issue_id", Uuid),
        ("w_created_at", Timestamptz),
        ("w_updated_at", Timestamptz),
        ("w_created_by_id", Uuid),
        ("w_updated_by_id", Uuid),
        ("w_deleted_at", Timestamptz),
        ("w_id", Uuid),
        ("w_name", Varchar),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
        ("w_owner_id", Uuid),
        ("w_slug", Varchar),
        ("w_organization_size", Varchar),
        ("w_timezone", Varchar),
        ("w_background_color", Varchar),
        ("logo_id", Uuid),
        ("logo_entity_type", Varchar),
        ("logo_workspace_slug", Varchar),
        ("logo_project_id", Uuid),
        ("logo_issue_id", Uuid),
    ]
}

plane_query! {
    /// `User.objects.get(pk=value)` — the `ModelChoiceFilter` validation
    /// django-filter runs for `?owner=` (`filterset_fields = ["owner"]`,
    /// `plane/app/views/workspace/base.py:178`). One row means the choice is
    /// valid.
    USER_EXISTS = "users_me_user_exists",
    "SELECT u.\"id\" AS id FROM users u WHERE u.\"id\" = $1 LIMIT 21",
    [("id", Uuid)]
}

plane_query! {
    /// `ProjectMember.objects.filter(workspace__slug=slug,
    /// member_id=request.user.id, is_active=True,
    /// member__member_workspace__workspace__slug=slug,
    /// member__member_workspace__is_active=True).values("project_id", "role")`
    /// (`plane/app/views/project/member.py:396-402`), default ordering
    /// `-created_at`.
    ///
    /// The two slug joins are folded into `EXISTS` membership tests: the
    /// project member's own workspace must have this slug, and the same user
    /// must be an active member of it (which the permission class already
    /// proved, but the view repeats it).
    USER_PROJECT_ROLES = "users_me_project_roles",
    "SELECT pm.\"project_id\" AS project_id, pm.\"role\" AS role \
     FROM project_members pm \
     WHERE pm.\"deleted_at\" IS NULL AND pm.\"is_active\" \
       AND pm.\"member_id\" = $2 \
       AND EXISTS (SELECT 1 FROM workspaces pw \
                   WHERE pw.\"id\" = pm.\"workspace_id\" AND pw.\"slug\" = $1) \
       AND EXISTS (SELECT 1 FROM workspace_members wm \
                   INNER JOIN workspaces mw ON mw.\"id\" = wm.\"workspace_id\" \
                   WHERE wm.\"member_id\" = pm.\"member_id\" \
                     AND wm.\"is_active\" AND mw.\"slug\" = $1) \
     ORDER BY pm.\"created_at\" DESC",
    [("project_id", Uuid), ("role", Int2)]
}

plane_query! {
    /// `APIToken.objects.filter(user=request.user)`
    /// (`plane/app/views/api.py:43-45`), default ordering `-created_at`.
    USER_API_TOKENS = "users_me_api_tokens",
    "SELECT t.\"created_at\" AS created_at, t.\"updated_at\" AS updated_at, \
     t.\"created_by_id\" AS created_by_id, t.\"updated_by_id\" AS updated_by_id, \
     t.\"deleted_at\" AS deleted_at, t.\"id\" AS id, t.\"label\" AS label, \
     t.\"description\" AS description, t.\"is_active\" AS is_active, \
     t.\"last_used\" AS last_used, t.\"token\" AS token, t.\"user_id\" AS user_id, \
     t.\"user_type\" AS user_type, t.\"workspace_id\" AS workspace_id, \
     t.\"expired_at\" AS expired_at \
     FROM api_tokens t \
     WHERE t.\"deleted_at\" IS NULL AND t.\"user_id\" = $1 \
     ORDER BY t.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("label", Varchar),
        ("description", Text),
        ("is_active", Bool),
        ("last_used", Timestamptz),
        ("token", Varchar),
        ("user_id", Uuid),
        ("user_type", Int2),
        ("workspace_id", Uuid),
        ("expired_at", Timestamptz),
    ]
}

plane_query! {
    /// `APIToken.objects.get(user=request.user, pk=pk)`
    /// (`plane/app/views/api.py:47-49`).
    USER_API_TOKEN_BY_ID = "users_me_api_token_by_id",
    "SELECT t.\"created_at\" AS created_at, t.\"updated_at\" AS updated_at, \
     t.\"created_by_id\" AS created_by_id, t.\"updated_by_id\" AS updated_by_id, \
     t.\"deleted_at\" AS deleted_at, t.\"id\" AS id, t.\"label\" AS label, \
     t.\"description\" AS description, t.\"is_active\" AS is_active, \
     t.\"last_used\" AS last_used, t.\"token\" AS token, t.\"user_id\" AS user_id, \
     t.\"user_type\" AS user_type, t.\"workspace_id\" AS workspace_id, \
     t.\"expired_at\" AS expired_at \
     FROM api_tokens t \
     WHERE t.\"deleted_at\" IS NULL AND t.\"id\" = $1 AND t.\"user_id\" = $2 LIMIT 21",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("label", Varchar),
        ("description", Text),
        ("is_active", Bool),
        ("last_used", Timestamptz),
        ("token", Varchar),
        ("user_id", Uuid),
        ("user_type", Int2),
        ("workspace_id", Uuid),
        ("expired_at", Timestamptz),
    ]
}

plane_query! {
    /// `IssueActivity.objects.filter(actor=request.user,
    /// workspace__slug=slug, created_at__date__gte=date.today()-6 months)
    /// .annotate(created_date=Cast("created_at", DateField())).values("created_date")
    /// .annotate(activity_count=Count("created_date")).order_by("created_date")`
    /// (`plane/app/views/workspace/user.py:567-577`).
    ACTIVITY_GRAPH = "users_me_activity_graph",
    "SELECT (a.\"created_at\")::date AS created_date, \
     COUNT((a.\"created_at\")::date)::int8 AS activity_count \
     FROM issue_activities a \
     INNER JOIN workspaces w ON a.\"workspace_id\" = w.\"id\" \
     WHERE a.\"deleted_at\" IS NULL AND a.\"actor_id\" = $1 \
       AND (a.\"created_at\" AT TIME ZONE 'UTC')::date >= $2 \
       AND w.\"slug\" = $3 \
     GROUP BY 1 ORDER BY 1 ASC",
    [("created_date", Date), ("activity_count", Int8)]
}

plane_query! {
    /// `Issue.issue_objects.filter(assignees__in=[request.user],
    /// workspace__slug=slug, completed_at__month=month,
    /// completed_at__isnull=False).annotate(completed_week=ExtractWeek("completed_at"))
    /// .annotate(week=F("completed_week") % 4).values("week")
    /// .annotate(completed_count=Count("completed_week")).order_by("week")`
    /// (`plane/app/views/workspace/user.py:586-598`).
    ///
    /// `EXTRACT` returns `numeric` in Postgres; Django converts it to `int`
    /// through the expression's `IntegerField`, so the cast keeps the same JSON
    /// number.
    ISSUES_COMPLETED_GRAPH = "users_me_issues_completed_graph",
    "SELECT (EXTRACT(WEEK FROM i.\"completed_at\" AT TIME ZONE 'UTC') % 4)::int4 AS week, \
     COUNT(EXTRACT(WEEK FROM i.\"completed_at\" AT TIME ZONE 'UTC'))::int8 AS completed_count \
     FROM issues i \
     LEFT OUTER JOIN states s ON i.\"state_id\" = s.\"id\" \
     INNER JOIN projects p ON i.\"project_id\" = p.\"id\" \
     INNER JOIN issue_assignees ia ON i.\"id\" = ia.\"issue_id\" \
     INNER JOIN workspaces w ON i.\"workspace_id\" = w.\"id\" \
     WHERE i.\"deleted_at\" IS NULL \
       AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) \
       AND NOT (i.\"archived_at\" IS NOT NULL) \
       AND NOT (p.\"archived_at\" IS NOT NULL) \
       AND NOT (i.\"is_draft\") \
       AND ia.\"assignee_id\" = $1 \
       AND i.\"completed_at\" IS NOT NULL \
       AND EXTRACT(MONTH FROM i.\"completed_at\" AT TIME ZONE 'UTC') = $2::int8 \
       AND w.\"slug\" = $3 \
     GROUP BY 1 ORDER BY 1 ASC",
    [("week", Int4), ("completed_count", Int8)]
}

plane_query! {
    /// The four counters of `UserWorkspaceDashboardEndpoint.get`
    /// (`plane/app/views/workspace/base.py:264-281`), folded into one scan of
    /// the same join.
    ///
    /// Django runs four `COUNT(*)` queries whose FROM/WHERE differ only in the
    /// state predicate; `FILTER` reproduces each predicate on the one shape
    /// (`LEFT JOIN states`, because `Issue.issue_objects` excludes triage and
    /// the completed counter's join is an inner join, which a NULL state fails
    /// either way).
    DASHBOARD_COUNTS = "users_me_dashboard_counts",
    "SELECT \
       COUNT(*) FILTER (WHERE TRUE)::int8 AS assigned_issues_count, \
       COUNT(*) FILTER (WHERE NOT (s.\"group\" IN ('completed', 'cancelled') \
                                   AND s.\"group\" IS NOT NULL))::int8 AS pending_issues_count, \
       COUNT(*) FILTER (WHERE s.\"group\" = 'completed')::int8 AS completed_issues_count, \
       COUNT(*) FILTER (WHERE EXTRACT(WEEK FROM i.\"target_date\") = $3::int8)::int8 \
         AS issues_due_week_count \
     FROM issues i \
     LEFT OUTER JOIN states s ON i.\"state_id\" = s.\"id\" \
     INNER JOIN projects p ON i.\"project_id\" = p.\"id\" \
     INNER JOIN issue_assignees ia ON i.\"id\" = ia.\"issue_id\" \
     INNER JOIN workspaces w ON i.\"workspace_id\" = w.\"id\" \
     WHERE i.\"deleted_at\" IS NULL \
       AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) \
       AND NOT (i.\"archived_at\" IS NOT NULL) \
       AND NOT (p.\"archived_at\" IS NOT NULL) \
       AND NOT (i.\"is_draft\") \
       AND ia.\"assignee_id\" = $1 \
       AND w.\"slug\" = $2",
    [
        ("assigned_issues_count", Int8),
        ("pending_issues_count", Int8),
        ("completed_issues_count", Int8),
        ("issues_due_week_count", Int8),
    ]
}

plane_query! {
    /// The `issue_activities` part of the same view
    /// (`plane/app/views/workspace/base.py:236-246`), three months back.
    DASHBOARD_ACTIVITY_GRAPH = "users_me_dashboard_activity_graph",
    "SELECT (a.\"created_at\")::date AS created_date, \
     COUNT((a.\"created_at\")::date)::int8 AS activity_count \
     FROM issue_activities a \
     INNER JOIN workspaces w ON a.\"workspace_id\" = w.\"id\" \
     WHERE a.\"deleted_at\" IS NULL AND a.\"actor_id\" = $1 \
       AND (a.\"created_at\" AT TIME ZONE 'UTC')::date >= $2 \
       AND w.\"slug\" = $3 \
     GROUP BY 1 ORDER BY 1 ASC",
    [("created_date", Date), ("activity_count", Int8)]
}

plane_query! {
    /// The `completed_issues` part of the same view
    /// (`plane/app/views/workspace/base.py:250-262`): `WeekInMonth(F("day_of_month"))`,
    /// `((day - 1) / 7) + 1`, grouped and ordered by week.
    DASHBOARD_COMPLETED_ISSUES = "users_me_dashboard_completed_issues",
    "SELECT (((EXTRACT(DAY FROM i.\"completed_at\" AT TIME ZONE 'UTC') - 1) / 7) + 1)::INTEGER \
       AS week_in_month, \
     COUNT(i.\"id\")::int8 AS completed_count \
     FROM issues i \
     LEFT OUTER JOIN states s ON i.\"state_id\" = s.\"id\" \
     INNER JOIN projects p ON i.\"project_id\" = p.\"id\" \
     INNER JOIN issue_assignees ia ON i.\"id\" = ia.\"issue_id\" \
     INNER JOIN workspaces w ON i.\"workspace_id\" = w.\"id\" \
     WHERE i.\"deleted_at\" IS NULL \
       AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) \
       AND NOT (i.\"archived_at\" IS NOT NULL) \
       AND NOT (p.\"archived_at\" IS NOT NULL) \
       AND NOT (i.\"is_draft\") \
       AND ia.\"assignee_id\" = $1 \
       AND i.\"completed_at\" IS NOT NULL \
       AND EXTRACT(MONTH FROM i.\"completed_at\" AT TIME ZONE 'UTC') = $2::int8 \
       AND w.\"slug\" = $3 \
     GROUP BY 1 ORDER BY 1 ASC",
    [("week_in_month", Int4), ("completed_count", Int8)]
}

plane_query! {
    /// `state_distribution` (`plane/app/views/workspace/base.py:283-289`):
    /// `annotate(state_group=F("state__group")).values("state_group")
    /// .annotate(state_count=Count("state_group")).order_by("state_group")`.
    DASHBOARD_STATE_DISTRIBUTION = "users_me_dashboard_state_distribution",
    "SELECT s.\"group\" AS state_group, COUNT(s.\"group\")::int8 AS state_count \
     FROM issues i \
     LEFT OUTER JOIN states s ON i.\"state_id\" = s.\"id\" \
     INNER JOIN projects p ON i.\"project_id\" = p.\"id\" \
     INNER JOIN issue_assignees ia ON i.\"id\" = ia.\"issue_id\" \
     INNER JOIN workspaces w ON i.\"workspace_id\" = w.\"id\" \
     WHERE i.\"deleted_at\" IS NULL \
       AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) \
       AND NOT (i.\"archived_at\" IS NOT NULL) \
       AND NOT (p.\"archived_at\" IS NOT NULL) \
       AND NOT (i.\"is_draft\") \
       AND ia.\"assignee_id\" = $1 \
       AND w.\"slug\" = $2 \
     GROUP BY 1 ORDER BY 1 ASC",
    [("state_group", Varchar), ("state_count", Int8)]
}

plane_query! {
    /// `overdue_issues` (`plane/app/views/workspace/base.py:291-297`):
    /// not completed/cancelled, `target_date < timezone.now()`,
    /// `completed_at IS NULL`, Meta ordering `-created_at`.
    DASHBOARD_OVERDUE_ISSUES = "users_me_dashboard_overdue_issues",
    "SELECT i.\"id\" AS id, i.\"name\" AS name, w.\"slug\" AS workspace__slug, \
     i.\"project_id\" AS project_id, i.\"target_date\" AS target_date \
     FROM issues i \
     LEFT OUTER JOIN states s ON i.\"state_id\" = s.\"id\" \
     INNER JOIN projects p ON i.\"project_id\" = p.\"id\" \
     INNER JOIN issue_assignees ia ON i.\"id\" = ia.\"issue_id\" \
     INNER JOIN workspaces w ON i.\"workspace_id\" = w.\"id\" \
     WHERE i.\"deleted_at\" IS NULL \
       AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) \
       AND NOT (i.\"archived_at\" IS NOT NULL) \
       AND NOT (p.\"archived_at\" IS NOT NULL) \
       AND NOT (i.\"is_draft\") \
       AND NOT (s.\"group\" IN ('completed', 'cancelled') AND s.\"group\" IS NOT NULL) \
       AND ia.\"assignee_id\" = $1 \
       AND i.\"completed_at\" IS NULL \
       AND i.\"target_date\" < $2 \
       AND w.\"slug\" = $3 \
     ORDER BY i.\"created_at\" DESC",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("workspace__slug", Varchar),
        ("project_id", Uuid),
        ("target_date", Date),
    ]
}

plane_query! {
    /// `upcoming_issues` (`plane/app/views/workspace/base.py:299-305`):
    /// not completed/cancelled, `start_date >= timezone.now()`,
    /// `completed_at IS NULL`, Meta ordering `-created_at`.
    DASHBOARD_UPCOMING_ISSUES = "users_me_dashboard_upcoming_issues",
    "SELECT i.\"id\" AS id, i.\"name\" AS name, w.\"slug\" AS workspace__slug, \
     i.\"project_id\" AS project_id, i.\"start_date\" AS start_date \
     FROM issues i \
     LEFT OUTER JOIN states s ON i.\"state_id\" = s.\"id\" \
     INNER JOIN projects p ON i.\"project_id\" = p.\"id\" \
     INNER JOIN issue_assignees ia ON i.\"id\" = ia.\"issue_id\" \
     INNER JOIN workspaces w ON i.\"workspace_id\" = w.\"id\" \
     WHERE i.\"deleted_at\" IS NULL \
       AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) \
       AND NOT (i.\"archived_at\" IS NOT NULL) \
       AND NOT (p.\"archived_at\" IS NOT NULL) \
       AND NOT (i.\"is_draft\") \
       AND NOT (s.\"group\" IN ('completed', 'cancelled') AND s.\"group\" IS NOT NULL) \
       AND ia.\"assignee_id\" = $1 \
       AND i.\"completed_at\" IS NULL \
       AND i.\"start_date\" >= $2 \
       AND w.\"slug\" = $3 \
     ORDER BY i.\"created_at\" DESC",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("workspace__slug", Varchar),
        ("project_id", Uuid),
        ("start_date", Date),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&plane_db::Query] = &[
    &USER_BY_ID_WITH_ASSETS,
    &USER_PROFILE,
    &USER_ACCOUNTS,
    &USER_ACCOUNT_BY_ID,
    &INSTANCE_ADMIN,
    &USER_ACTIVITY_COUNT,
    &USER_INVITE_COUNT,
    &WORKSPACE_FOR_USER_BY_ID,
    &FALLBACK_WORKSPACE_FOR_USER,
    &INVITES_BY_EMAIL,
    &PROJECT_INVITES_BY_EMAIL,
    &USER_EXISTS,
    &USER_PROJECT_ROLES,
    &USER_API_TOKENS,
    &USER_API_TOKEN_BY_ID,
    &ACTIVITY_GRAPH,
    &ISSUES_COMPLETED_GRAPH,
    &DASHBOARD_COUNTS,
    &DASHBOARD_ACTIVITY_GRAPH,
    &DASHBOARD_COMPLETED_ISSUES,
    &DASHBOARD_STATE_DISTRIBUTION,
    &DASHBOARD_OVERDUE_ISSUES,
    &DASHBOARD_UPCOMING_ISSUES,
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
