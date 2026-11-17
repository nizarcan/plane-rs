// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the auth-session group (`AUTH_FLOWS.md` set A).
//!
//! Every statement names the Python it reproduces. `plane-auth` owns the
//! statements the login/signup adapter runs (`crates/plane-auth/src/queries.rs`)
//! and they are reused here; this catalog holds only the reads and writes the
//! *views* add on top.
//!
//! `tests/schema_check.rs` prepares every statement here against a
//! Django-migrated database, so a renamed column or changed type fails in CI.

use plane_db::plane_query;

plane_query! {
    /// `User.objects.get(pk=…)` (sign-out's `User.objects.get` and the DRF
    /// password views' `User.objects.get(pk=request.user.id)`), the full model
    /// row in `plane_db::models::user::User::from_row` order.
    ///
    /// The default manager has no extra filter: `User` is not a `BaseModel`,
    /// so an inactive row is returned exactly like Django returns it.
    AUTH_USER_BY_ID = "auth_user_by_id",
    "SELECT password, last_login, id, username, mobile_number, email, display_name, \
     first_name, last_name, avatar, avatar_asset_id, cover_image, cover_image_asset_id, \
     date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
     is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
     is_password_autoset, is_password_reset_required, token, last_active, last_login_time, \
     last_logout_time, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, \
     token_updated_at, is_bot, bot_type, user_timezone, is_email_valid, masked_at \
     FROM users \
     WHERE id = $1",
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
    ]
}

plane_query! {
    /// `User.objects.filter(email=email).first()` — the view's existence check,
    /// the credential provider's reload and the deactivation/bot checks all read
    /// this row. `Meta.ordering = ("-created_at",)`, but `email` is unique so
    /// the order is irrelevant.
    AUTH_USER_BY_EMAIL = "auth_user_by_email",
    "SELECT password, last_login, id, username, mobile_number, email, display_name, \
     first_name, last_name, avatar, avatar_asset_id, cover_image, cover_image_asset_id, \
     date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
     is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
     is_password_autoset, is_password_reset_required, token, last_active, last_login_time, \
     last_logout_time, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, \
     token_updated_at, is_bot, bot_type, user_timezone, is_email_valid, masked_at \
     FROM users \
     WHERE email = $1 \
     ORDER BY created_at DESC \
     LIMIT 1",
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
    ]
}

plane_query! {
    /// `User.objects.filter(email=email).first()` for `/auth/email-check/`:
    /// only `is_password_autoset` is read
    /// (`views/app/check.py:80-95`), so the projection keeps it.
    AUTH_EMAIL_CHECK_USER = "auth_email_check_user",
    "SELECT is_password_autoset FROM users WHERE email = $1 \
     ORDER BY created_at DESC LIMIT 1",
    [("is_password_autoset", Bool)]
}

plane_query! {
    /// `Profile.objects.get_or_create(user=user)` (`redirection_path.py:10`),
    /// the read half. The default manager hides soft-deleted rows; `Profile`
    /// is a `TimeAuditModel`, not a `BaseModel`, so it has no `deleted_at`
    /// column at all.
    AUTH_PROFILE_BY_USER = "auth_profile_by_user",
    "SELECT p.\"created_at\" AS created_at, p.\"updated_at\" AS updated_at, \
     p.\"id\" AS id, p.\"user_id\" AS user_id, p.\"theme\" AS theme, \
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
    /// `Workspace.objects.filter(pk=profile.last_workspace_id,
    /// workspace_member__member_id=user.id,
    /// workspace_member__is_active=True).first()` (`redirection_path.py:17-30`).
    ///
    /// Both managers' soft-delete filters apply: the workspace's
    /// (`w.deleted_at IS NULL`) and the joined membership's
    /// (`wm.deleted_at IS NULL`). `workspace_members` has a live unique
    /// `(workspace, member)`, so at most one row matches.
    AUTH_WORKSPACE_BY_ID_FOR_MEMBER = "auth_workspace_by_id_for_member",
    "SELECT w.slug FROM workspaces w \
     JOIN workspace_members wm ON wm.workspace_id = w.id \
     WHERE w.id = $1 AND wm.member_id = $2 AND wm.is_active \
       AND w.deleted_at IS NULL AND wm.deleted_at IS NULL \
     LIMIT 21",
    [("slug", Varchar)]
}

plane_query! {
    /// `Workspace.objects.filter(workspace_member__member_id=user.id,
    /// workspace_member__is_active=True).order_by("created_at").first()`
    /// (`redirection_path.py:32-40`).
    AUTH_FIRST_WORKSPACE_FOR_MEMBER = "auth_first_workspace_for_member",
    "SELECT w.slug FROM workspaces w \
     JOIN workspace_members wm ON wm.workspace_id = w.id \
     WHERE wm.member_id = $1 AND wm.is_active \
       AND w.deleted_at IS NULL AND wm.deleted_at IS NULL \
     ORDER BY w.created_at ASC \
     LIMIT 1",
    [("slug", Varchar)]
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(email=user.email).count()`
    /// (`redirection_path.py:42-43`): every live invite, accepted or not.
    AUTH_INVITE_COUNT_FOR_EMAIL = "auth_invite_count_for_email",
    "SELECT COUNT(*) FROM workspace_member_invites \
     WHERE email = $1 AND deleted_at IS NULL",
    [("count", Int8)]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&plane_db::Query] = &[
    &AUTH_USER_BY_ID,
    &AUTH_USER_BY_EMAIL,
    &AUTH_EMAIL_CHECK_USER,
    &AUTH_PROFILE_BY_USER,
    &AUTH_WORKSPACE_BY_ID_FOR_MEMBER,
    &AUTH_FIRST_WORKSPACE_FOR_MEMBER,
    &AUTH_INVITE_COUNT_FOR_EMAIL,
];
