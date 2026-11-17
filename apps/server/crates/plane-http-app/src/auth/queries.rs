// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `auth` group.
//!
//! Three sets share this file (`plans/AUTH_FLOWS.md` §3): the session routes
//! (set A), the magic-link routes (set B) and the OAuth routes (set C).
//! `plane-auth` owns the statements the login/signup adapter runs
//! (`crates/plane-auth/src/queries.rs`) and they are reused here; this catalog
//! holds only the reads and writes the *views* add on top — the session views'
//! user reads and the single `get_redirection_path`
//! (`plane/authentication/utils/redirection_path.py:8-47`) implementation that
//! every flow calls through [`super::support::redirection_path`].
//!
//! Every statement keeps Django's ORM semantics exactly:
//!
//! * `Workspace.objects` is a `SoftDeletionManager` (`plane/db/mixins.py:57`), so
//!   the workspace side filters `deleted_at IS NULL`;
//! * the reverse FK filter `workspace_member__…` does **not** apply the related
//!   manager: Django joins `workspace_members` through the model's
//!   `_base_manager` (a plain `Manager`) and filters only the named columns, so a
//!   soft-deleted membership still counts. Both joins below are therefore
//!   unfiltered on `workspace_members.deleted_at`;
//! * `WorkspaceMemberInvite.objects.filter(email=…).count()` **does** use the
//!   soft-deletion manager.
//!
//! `tests/schema_check.rs` prepares every statement here against a
//! Django-migrated database, so a renamed column or changed type fails in CI.

use plane_db::Query;
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
    /// `Workspace.objects.filter(pk=profile.last_workspace_id,
    /// workspace_member__member_id=user.id, workspace_member__is_active=True)
    /// .exists()` / `.first()` (`redirection_path.py:17-32`).
    ///
    /// `.first()` keeps `Workspace.Meta.ordering = ("-created_at",)`
    /// (`plane/db/models/workspace.py:182`), so the tiebreak is `created_at
    /// DESC`; with a primary-key filter there is at most one workspace anyway.
    AUTH_LAST_WORKSPACE_SLUG = "auth_last_workspace_slug",
    "SELECT w.slug FROM workspaces w \
     INNER JOIN workspace_members wm ON wm.workspace_id = w.id \
     WHERE w.id = $1 AND wm.member_id = $2 AND wm.is_active AND w.deleted_at IS NULL \
     ORDER BY w.created_at DESC \
     LIMIT 1",
    [("slug", Varchar)]
}

plane_query! {
    /// `Workspace.objects.filter(workspace_member__member_id=user.id,
    /// workspace_member__is_active=True).order_by("created_at").first()`
    /// (`redirection_path.py:34-40`).
    AUTH_MEMBER_WORKSPACE_SLUG = "auth_member_workspace_slug",
    "SELECT w.slug FROM workspaces w \
     INNER JOIN workspace_members wm ON wm.workspace_id = w.id \
     WHERE wm.member_id = $1 AND wm.is_active AND w.deleted_at IS NULL \
     ORDER BY w.created_at ASC \
     LIMIT 1",
    [("slug", Varchar)]
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(email=user.email).count()`
    /// (`redirection_path.py:42-44`); only truthiness is observable, so this is
    /// an `EXISTS`.
    AUTH_PENDING_INVITE_EXISTS = "auth_pending_invite_exists",
    "SELECT EXISTS (SELECT 1 FROM workspace_member_invites \
     WHERE email = $1 AND deleted_at IS NULL) AS \"exists\"",
    [("exists", Bool)]
}

/// Every catalogued statement, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &AUTH_USER_BY_ID,
    &AUTH_USER_BY_EMAIL,
    &AUTH_EMAIL_CHECK_USER,
    &AUTH_LAST_WORKSPACE_SLUG,
    &AUTH_MEMBER_WORKSPACE_SLUG,
    &AUTH_PENDING_INVITE_EXISTS,
];
