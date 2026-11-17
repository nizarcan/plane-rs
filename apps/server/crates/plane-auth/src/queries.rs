// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-auth`'s statement catalog.
//!
//! Every statement lives here with the exact result columns it expects;
//! `tests/schema_check.rs` prepares them all against a Django-migrated database
//! (DECISIONS.md D1). Adding a statement means adding a
//! [`plane_query!`](plane_db::plane_query) invocation (or a [`Query`] literal
//! for a write), listing it in [`ALL_QUERIES`], and bumping the count in
//! `catalog_is_complete`.
//!
//! Column names and types were read from a clone of `plane_tpl` (Django's
//! migrations, unmodified).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `SessionStore._get_session_from_db`
    /// (`django/contrib/sessions/backends/db.py:30-42`):
    /// `SELECT … WHERE session_key = %s AND expire_date > now()`.
    LOAD_SESSION_BY_KEY = "auth_load_session_by_key",
    "SELECT session_data, expire_date \
     FROM sessions \
     WHERE session_key = $1 AND expire_date > now()",
    [
        ("session_data", Text),
        ("expire_date", Timestamptz),
    ]
}

plane_query! {
    /// `SessionStore.exists` (`db.py:52-53`), used while generating a key.
    SESSION_EXISTS = "auth_session_exists",
    "SELECT 1::int4 AS present FROM sessions WHERE session_key = $1",
    [("present", Int4)]
}

plane_query! {
    /// `Model.save(force_insert=True)` for a `sessions` row: every concrete
    /// column (`SessionStore.create_model_instance`, `db.py:77-100`).
    INSERT_SESSION = "auth_insert_session",
    "INSERT INTO sessions (session_key, session_data, expire_date, user_id, device_info) \
     VALUES ($1, $2, $3, $4, $5)",
    []
}

plane_query! {
    /// `Model.save(force_update=True)`: every non-PK column, so `expire_date` is
    /// refreshed on each save exactly as Django's `auto_now`-less model does.
    UPDATE_SESSION = "auth_update_session",
    "UPDATE sessions \
     SET session_data = $2, expire_date = $3, user_id = $4, device_info = $5 \
     WHERE session_key = $1",
    []
}

plane_query! {
    /// `SessionStore.delete` (`db.py:142-151`).
    DELETE_SESSION = "auth_delete_session",
    "DELETE FROM sessions WHERE session_key = $1",
    []
}

plane_query! {
    /// `Session.objects.filter(user_id=request.user.id).delete()`
    /// (`plane/app/views/user/base.py:342`). `sessions.user_id` is a
    /// `varchar(50)` (`plane/db/models/session.py:45-51`), so Django binds
    /// `str(uuid)` — the lower-case dashed form.
    DELETE_SESSIONS_FOR_USER = "auth_delete_sessions_for_user",
    "DELETE FROM sessions WHERE user_id = $1",
    []
}

plane_query! {
    /// `ModelBackend.get_user` (`django/contrib/auth/backends.py:229-235`) and
    /// `get_by_natural_key` for the email lookup. `users` does not use
    /// `BaseModel`, so there is no `deleted_at` column.
    ///
    /// The select list is the generated `plane_db::models::user::User::COLUMNS`
    /// in its order, because Django's `request.user` **is** the full model row:
    /// every column is loaded once here, and a handler that needs another field
    /// (the `users_me` group reads the avatar/cover asset ids) does not issue a
    /// second `users` query. The 13-column projection `AuthUser` keeps is
    /// decoded from this row by [`crate::user::AuthUser::from_user_row`].
    LOAD_USER_BY_ID = "auth_load_user_by_id",
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
    /// `UserManager.get_by_natural_key` with `USERNAME_FIELD = "email"`:
    /// `User.objects.get(email=…)`, an exact match (`User.save` lower-cases the
    /// stored value; `SignInAuthEndpoint` lower-cases the input).
    LOAD_USER_BY_EMAIL = "auth_load_user_by_email",
    "SELECT id, password, username, email, display_name, first_name, last_name, \
     user_timezone, is_active, is_staff, is_superuser, is_email_verified, is_bot, \
     date_joined \
     FROM users \
     WHERE email = $1",
    [
        ("id", Uuid),
        ("password", Varchar),
        ("username", Varchar),
        ("email", Varchar),
        ("display_name", Varchar),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("user_timezone", Varchar),
        ("is_active", Bool),
        ("is_staff", Bool),
        ("is_superuser", Bool),
        ("is_email_verified", Bool),
        ("is_bot", Bool),
        ("date_joined", Timestamptz),
    ]
}

plane_query! {
    /// `APIKeyAuthentication.validate_api_token`
    /// (`plane/api/middleware/api_authentication.py:28-44`): the token must not
    /// be expired, must be active, its user must be active, and
    /// `APIToken.objects` (a `SoftDeletionManager`) hides soft-deleted rows.
    LOAD_API_TOKEN = "auth_load_api_token",
    "SELECT t.id, t.token, u.id AS user_id, u.password, u.username, u.email, u.display_name, \
     u.first_name, u.last_name, u.user_timezone, u.is_active, u.is_staff, \
     u.is_superuser, u.is_email_verified, u.is_bot, u.date_joined \
     FROM api_tokens t \
     JOIN users u ON u.id = t.user_id \
     WHERE (t.expired_at > now() OR t.expired_at IS NULL) \
       AND t.token = $1 \
       AND t.is_active \
       AND u.is_active \
       AND t.deleted_at IS NULL",
    [
        ("id", Uuid),
        ("token", Varchar),
        ("user_id", Uuid),
        ("password", Varchar),
        ("username", Varchar),
        ("email", Varchar),
        ("display_name", Varchar),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("user_timezone", Varchar),
        ("is_active", Bool),
        ("is_staff", Bool),
        ("is_superuser", Bool),
        ("is_email_verified", Bool),
        ("is_bot", Bool),
        ("date_joined", Timestamptz),
    ]
}

plane_query! {
    /// `api_token.save(update_fields=["last_used"])`
    /// (`plane/api/middleware/api_authentication.py:42`): **only** that column.
    /// `updated_at`, `updated_by` and the audit columns are not written
    /// (MODEL_HOOKS.md §1.6 "`update_fields` wins", §4.3).
    TOUCH_API_TOKEN = "auth_touch_api_token",
    "UPDATE api_tokens SET last_used = $2 WHERE id = $1",
    []
}

plane_query! {
    /// `AbstractBaseUser.check_password`'s `setter`:
    /// `save(update_fields=["password"])`
    /// (`django/contrib/auth/base_user.py:106-112`). Only that column:
    /// `User.save`'s token rotation and `updated_at` stay in memory when
    /// `update_fields` excludes them (MODEL_HOOKS.md §1.6).
    UPDATE_USER_PASSWORD = "auth_update_user_password",
    "UPDATE users SET password = $2 WHERE id = $1",
    []
}

plane_query! {
    /// `User.objects.filter(email=email).first()` in the auth adapter
    /// (`plane/authentication/adapter/base.py:355`) and the login views. The
    /// full 40-column row, because `save_user_data`, H30 and the returned
    /// `request.user` are the model instance.
    LOAD_USER_FULL_BY_EMAIL = "auth_load_user_full_by_email",
    "SELECT password, last_login, id, username, mobile_number, email, display_name, \
     first_name, last_name, avatar, avatar_asset_id, cover_image, cover_image_asset_id, \
     date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
     is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
     is_password_autoset, is_password_reset_required, token, last_active, last_login_time, \
     last_logout_time, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, \
     token_updated_at, is_bot, bot_type, user_timezone, is_email_valid, masked_at \
     FROM users \
     WHERE email = $1 \
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
    /// `User(email=…, username=…)` + `Model.save()`: Django's `INSERT` lists
    /// every concrete column, with `auto_now_add`/`auto_now`/defaults already
    /// evaluated in Python (`django/db/models/base.py:925-1008`). Bind order
    /// matches the column order of `LOAD_USER_FULL_BY_EMAIL`.
    INSERT_USER = "auth_insert_user",
    "INSERT INTO users (\
     password, last_login, id, username, mobile_number, email, display_name, \
     first_name, last_name, avatar, avatar_asset_id, cover_image, cover_image_asset_id, \
     date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
     is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
     is_password_autoset, is_password_reset_required, token, last_active, last_login_time, \
     last_logout_time, last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, \
     token_updated_at, is_bot, bot_type, user_timezone, is_email_valid, masked_at) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
     $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, \
     $33, $34, $35, $36, $37, $38, $39, $40)",
    []
}

plane_query! {
    /// `Adapter.save_user_data` (`adapter/base.py:231-251`) + H30 + `auto_now`:
    /// only the columns the method and the hook change are written. Django's
    /// full `save()` rewrites every column with its unchanged value, which is
    /// not observable; the audit column `updated_at` moves.
    UPDATE_USER_LOGIN = "auth_update_user_login",
    "UPDATE users SET \
     email = $1, display_name = $2, is_staff = $3, token = $4, token_updated_at = $5, \
     avatar = $6, avatar_asset_id = $7, last_active = $8, last_login_time = $9, \
     last_login_medium = $10, last_login_ip = $11, last_login_uagent = $12, \
     is_active = $13, updated_at = $14 \
     WHERE id = $15",
    []
}

plane_query! {
    /// `sync_user_data`'s trailing `user.save()` (`adapter/base.py:303-304`),
    /// before `save_user_data` runs its own full save. The columns the sync
    /// assigns plus H30 and `auto_now`.
    UPDATE_USER_SYNC = "auth_update_user_sync",
    "UPDATE users SET \
     email = $1, display_name = $2, is_staff = $3, token = $4, token_updated_at = $5, \
     first_name = $6, last_name = $7, avatar = $8, avatar_asset_id = $9, updated_at = $10 \
     WHERE id = $11",
    []
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(email=email).exists()` in
    /// `__check_signup` (`adapter/base.py:108`): exact email, and the default
    /// manager hides soft-deleted invites. This is an **unconditional** check,
    /// even when `ENABLE_SIGNUP` is `"1"`.
    INVITE_EXISTS_FOR_EMAIL = "auth_invite_exists_for_email",
    "SELECT EXISTS (SELECT 1 FROM workspace_member_invites \
     WHERE email = $1 AND deleted_at IS NULL) AS \"exists\"",
    [("exists", Bool)]
}

plane_query! {
    /// `User.objects.filter(email=email).exists()` — the variant lookup in
    /// `MagicCodeProvider._raise_for_email` (`magic_code.py:126`) and the
    /// exhaustion branch of `initiate` (`:103`). Exact match, like every other
    /// `users` lookup on this path.
    USER_EXISTS_BY_EMAIL = "auth_user_exists_by_email",
    "SELECT EXISTS (SELECT 1 FROM users WHERE email = $1) AS \"exists\"",
    [("exists", Bool)]
}

plane_query! {
    /// `Profile.objects.create(user=user)` (`adapter/base.py:397`): every
    /// column with its model default. `background_color` is random and
    /// normalised by the harness (`scenarios/normalize.yaml`).
    INSERT_PROFILE = "auth_insert_profile",
    "INSERT INTO profiles (\
     id, created_at, updated_at, user_id, theme, is_app_rail_docked, is_tour_completed, \
     onboarding_step, use_case, role, is_onboarded, last_workspace_id, \
     billing_address_country, billing_address, has_billing_address, company_name, \
     notification_view_mode, is_smooth_cursor_enabled, is_mobile_onboarded, \
     mobile_onboarding_step, mobile_timezone_auto_set, language, start_of_the_week, \
     goals, background_color, is_navigation_tour_completed, has_marketing_email_consent, \
     is_subscribed_to_changelog, product_tour) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
     $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29)",
    []
}

plane_query! {
    /// H31: `create_user_notification` (`plane/db/models/user.py:298-312`), a
    /// `post_save` receiver on `User` that creates one row when
    /// `created and not instance.is_bot`.
    INSERT_USER_NOTIFICATION_PREFERENCE = "auth_insert_user_notification_preference",
    "INSERT INTO user_notification_preferences (\
     id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, user_id, \
     workspace_id, project_id, property_change, state_change, comment, mention, \
     issue_completed) \
     VALUES ($1, $2, $3, NULL, NULL, NULL, $4, NULL, NULL, $5, $6, $7, $8, $9)",
    []
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(email=…, accepted=True)`
    /// (`workspace_project_join.py:18`), with the workspace slug the cache
    /// invalidation loop reads through the FK (`:34`).
    ///
    /// `Meta.ordering = ("-created_at",)`, so the insert order is newest first.
    WORKSPACE_INVITES_FOR_EMAIL = "auth_workspace_invites_for_email",
    "SELECT wmi.id, wmi.workspace_id, wmi.role, w.slug \
     FROM workspace_member_invites wmi \
     JOIN workspaces w ON w.id = wmi.workspace_id \
     WHERE wmi.email = $1 AND wmi.accepted AND wmi.deleted_at IS NULL \
     ORDER BY wmi.created_at DESC",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("role", Int2),
        ("slug", Varchar),
    ]
}

plane_query! {
    /// `ProjectMemberInvite.objects.filter(email=…, accepted=True)`
    /// (`workspace_project_join.py:41`), `Meta.ordering = ("-created_at",)`.
    PROJECT_INVITES_FOR_EMAIL = "auth_project_invites_for_email",
    "SELECT id, workspace_id, project_id, role, created_by_id \
     FROM project_member_invites \
     WHERE email = $1 AND accepted AND deleted_at IS NULL \
     ORDER BY created_at DESC",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("role", Int2),
        ("created_by_id", Uuid),
    ]
}

plane_query! {
    /// `WorkspaceMember.objects.bulk_create([...], ignore_conflicts=True)`
    /// (`workspace_project_join.py:21-29,44-52`). `TimeAuditModel`'s
    /// `auto_now_add`/`auto_now` are evaluated per instance by `pre_save`, even
    /// in a bulk insert (`django/db/models/query.py:1809-1826`), and the JSON
    /// columns keep their model defaults.
    INSERT_WORKSPACE_MEMBER = "auth_insert_workspace_member",
    "INSERT INTO workspace_members (\
     id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, \
     member_id, role, company_role, view_props, default_props, issue_props, is_active, \
     getting_started_checklist, tips, explored_features) \
     VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, NULL, $8, $9, $10, TRUE, '{}', '{}', '{}') \
     ON CONFLICT DO NOTHING",
    []
}

plane_query! {
    /// `ProjectMember.objects.bulk_create([...], ignore_conflicts=True)`
    /// (`workspace_project_join.py:55-65`).
    ///
    /// **Upstream bug, ported as-is:** the list omits `project_id`, which is
    /// `NOT NULL` (`ProjectBaseModel.project`, `project.py:231`), and
    /// `bulk_create` bypasses `ProjectBaseModel.save`'s
    /// `self.workspace = self.project.workspace`. Any accepted project invite
    /// therefore raises `IntegrityError` and the signup/login is a 500, after
    /// the workspace memberships above have already been committed.
    INSERT_PROJECT_MEMBER = "auth_insert_project_member",
    "INSERT INTO project_members (\
     id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, project_id, \
     workspace_id, member_id, comment, role, view_props, default_props, preferences, \
     sort_order, is_active) \
     VALUES ($1, $2, $3, $4, NULL, NULL, NULL, $5, $6, NULL, $7, $8, $9, $10, 65535, TRUE) \
     ON CONFLICT DO NOTHING",
    []
}

plane_query! {
    /// `workspace_member_invites.delete()` on a `SoftDeletionQuerySet`
    /// (`plane/db/mixins.py:33-38`): `update(deleted_at=now())`, no
    /// `updated_at` movement.
    SOFT_DELETE_WORKSPACE_INVITES = "auth_soft_delete_workspace_invites",
    "UPDATE workspace_member_invites SET deleted_at = $2 \
     WHERE email = $1 AND accepted AND deleted_at IS NULL",
    []
}

plane_query! {
    /// `project_member_invites.delete()` on a `SoftDeletionQuerySet`
    /// (`plane/db/mixins.py:33-38`).
    SOFT_DELETE_PROJECT_INVITES = "auth_soft_delete_project_invites",
    "UPDATE project_member_invites SET deleted_at = $2 \
     WHERE email = $1 AND accepted AND deleted_at IS NULL",
    []
}

plane_query! {
    /// `Account.objects.filter(user=…, provider=…, provider_account_id=…).first()`
    /// (`authentication/adapter/oauth.py:104-108`). `Meta.ordering =
    /// ("-created_at",)` decides which row `.first()` returns; `accounts` is
    /// not soft-deletable, so there is no `deleted_at` filter.
    ///
    /// The select list is the generated `plane_db::models::user::Account`
    /// column order.
    ACCOUNT_BY_USER_PROVIDER_ID = "auth_account_by_user_provider_id",
    "SELECT created_at, updated_at, id, user_id, provider_account_id, provider, \
     access_token, access_token_expired_at, refresh_token, refresh_token_expired_at, \
     last_connected_at, id_token, metadata \
     FROM accounts \
     WHERE user_id = $1 AND provider = $2 AND provider_account_id = $3 \
     ORDER BY created_at DESC \
     LIMIT 1",
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
    /// `Account.objects.create(...)` (`adapter/oauth.py:120-130`): every
    /// column, in the model's order, with `metadata`'s `dict` default.
    ///
    /// **Upstream behaviour, ported as-is:** a missing `access_token` or
    /// `provider_account_id` is `NULL` here, which violates `NOT NULL`; the
    /// caller swallows the `IntegrityError` (`oauth.py:131-132`).
    INSERT_ACCOUNT = "auth_insert_account",
    "INSERT INTO accounts (\
     created_at, updated_at, id, user_id, provider_account_id, provider, access_token, \
     access_token_expired_at, refresh_token, refresh_token_expired_at, last_connected_at, \
     id_token, metadata) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    []
}

plane_query! {
    /// `account.save()` with no `update_fields` (`adapter/oauth.py:110-117`):
    /// every non-PK column in `local_concrete_fields` order. `created_at` and
    /// `metadata` keep the loaded values — `auto_now_add` does not re-fire on
    /// an update, and the Python never assigns `metadata`.
    UPDATE_ACCOUNT = "auth_update_account",
    "UPDATE accounts SET \
     created_at = $1, updated_at = $2, user_id = $3, provider_account_id = $4, \
     provider = $5, access_token = $6, access_token_expired_at = $7, \
     refresh_token = $8, refresh_token_expired_at = $9, last_connected_at = $10, \
     id_token = $11, metadata = $12 \
     WHERE id = $13",
    []
}

plane_query! {
    /// `FileAsset.objects.get(pk=user.avatar_asset_id, user_id=user.id).asset`
    /// (`adapter/base.py:257`), through the soft-delete manager
    /// (`plane/db/mixins.py:57`).
    FILE_ASSET_OBJECT_KEY = "auth_file_asset_object_key",
    "SELECT asset FROM file_assets \
     WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
    [("asset", Varchar)]
}

plane_query! {
    /// `asset.delete()` on a model instance (`adapter/base.py:262`): Django's
    /// `Collector.delete()` issues a hard `DELETE` (the soft-delete override is
    /// on the queryset, not the instance), and nothing cascades into
    /// `file_assets`.
    DELETE_FILE_ASSET_BY_ID = "auth_delete_file_asset_by_id",
    "DELETE FROM file_assets WHERE id = $1",
    []
}

plane_query! {
    /// `FileAsset.objects.create(...)` for the OAuth avatar
    /// (`adapter/base.py:213-222`). All 24 columns, in the order
    /// `FileAsset.objects.create` binds them (`app/assets/write_queries.rs`'s
    /// `FILE_ASSET_INSERT`, duplicated here because `plane-auth` cannot depend
    /// on `plane-http-app`).
    ///
    /// Bind order: created_at, updated_at, id, attributes, asset, created_by,
    /// updated_by, workspace, is_deleted, deleted_at, is_archived, comment,
    /// entity_type, external_id, external_source, is_uploaded, issue, page,
    /// project, size, storage_metadata, user, draft_issue, entity_identifier.
    INSERT_USER_AVATAR_ASSET = "auth_insert_user_avatar_asset",
    "INSERT INTO file_assets (created_at, updated_at, id, attributes, asset, \
     created_by_id, updated_by_id, workspace_id, is_deleted, deleted_at, \
     is_archived, comment_id, entity_type, external_id, external_source, \
     is_uploaded, issue_id, page_id, project_id, size, storage_metadata, \
     user_id, draft_issue_id, entity_identifier) VALUES \
     ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
      $17, $18, $19, $20, $21, $22, $23, $24)",
    []
}

/// Every catalogued statement, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &LOAD_SESSION_BY_KEY,
    &SESSION_EXISTS,
    &INSERT_SESSION,
    &UPDATE_SESSION,
    &DELETE_SESSION,
    &DELETE_SESSIONS_FOR_USER,
    &LOAD_USER_BY_ID,
    &LOAD_USER_BY_EMAIL,
    &LOAD_USER_FULL_BY_EMAIL,
    &INSERT_USER,
    &UPDATE_USER_LOGIN,
    &UPDATE_USER_SYNC,
    &INVITE_EXISTS_FOR_EMAIL,
    &USER_EXISTS_BY_EMAIL,
    &INSERT_PROFILE,
    &INSERT_USER_NOTIFICATION_PREFERENCE,
    &WORKSPACE_INVITES_FOR_EMAIL,
    &PROJECT_INVITES_FOR_EMAIL,
    &INSERT_WORKSPACE_MEMBER,
    &INSERT_PROJECT_MEMBER,
    &SOFT_DELETE_WORKSPACE_INVITES,
    &SOFT_DELETE_PROJECT_INVITES,
    &LOAD_API_TOKEN,
    &TOUCH_API_TOKEN,
    &UPDATE_USER_PASSWORD,
    &ACCOUNT_BY_USER_PROVIDER_ID,
    &INSERT_ACCOUNT,
    &UPDATE_ACCOUNT,
    &FILE_ASSET_OBJECT_KEY,
    &DELETE_FILE_ASSET_BY_ID,
    &INSERT_USER_AVATAR_ASSET,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_complete_and_unique() {
        let source = include_str!("queries.rs");
        // `concat!` keeps this match from counting its own pattern string.
        let declarations = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declarations,
            ALL_QUERIES.len(),
            "queries.rs declares {declarations} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );

        let mut names = BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(!query.name.is_empty());
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
            let mut columns = BTreeSet::new();
            for column in query.columns {
                assert!(
                    columns.insert(column.name),
                    "{} repeats column {:?}",
                    query.name,
                    column.name
                );
            }
        }
    }
}
