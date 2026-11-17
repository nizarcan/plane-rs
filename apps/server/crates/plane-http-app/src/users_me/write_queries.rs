// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `users-me` write paths.
//!
//! Every statement is the SQL a Django call site issues:
//!
//! * `UserEndpoint.partial_update` (162 PATCH) and `update_email` (166),
//!   `deactivate` (162 DELETE): `user.save()` writes **every** column
//!   (`plane/db/models/base.py:23-45` sets the audit columns, `user.py:169-187`
//!   rewrites five of them);
//! * `ProfileEndpoint.patch` (167), `UpdateUserOnBoardedEndpoint` (171) and
//!   `UpdateUserTourCompletedEndpoint` (172): `profile.save()`;
//! * `ApiTokenEndpoint.post`/`patch`/`delete` (226/227): `APIToken`'s insert,
//!   full-row update and soft delete;
//! * `UserEndpoint.deactivate`'s scans and its invite sweep
//!   (`plane/app/views/user/base.py:296-344`).
//!
//! `tests/schema_check.rs` prepares every statement against a Django-migrated
//! database, so a column rename fails in CI.
//!
//! **Bind order.** Each `*_UPDATE` lists the columns in the same order as the
//! matching `SELECT` (`queries::USER_BY_ID`, `queries::USER_PROFILE`,
//! `queries::USER_API_TOKENS`), with `id` last. That keeps one row struct and
//! one bind vector for both directions.

use plane_db::plane_query;

plane_query! {
    /// `user.save()` with no `update_fields`
    /// (`plane/app/serializers/base.py`, `Model.save()`).
    ///
    /// Bind order: the 39 non-`id` columns of [`super::queries::USER_BY_ID`]
    /// in its order, then the row id. H30 (`hooks::user::before_save`) must
    /// run before the binds are built.
    USER_UPDATE = "users_me_user_update",
    "UPDATE users SET \
     \"password\" = $1, \"last_login\" = $2, \"username\" = $3, \
     \"mobile_number\" = $4, \"email\" = $5, \"display_name\" = $6, \
     \"first_name\" = $7, \"last_name\" = $8, \"avatar\" = $9, \
     \"avatar_asset_id\" = $10, \"cover_image\" = $11, \
     \"cover_image_asset_id\" = $12, \"date_joined\" = $13, \
     \"created_at\" = $14, \"updated_at\" = $15, \"last_location\" = $16, \
     \"created_location\" = $17, \"is_superuser\" = $18, \
     \"is_managed\" = $19, \"is_password_expired\" = $20, \
     \"is_active\" = $21, \"is_staff\" = $22, \
     \"is_email_verified\" = $23, \"is_password_autoset\" = $24, \
     \"is_password_reset_required\" = $25, \"token\" = $26, \
     \"last_active\" = $27, \"last_login_time\" = $28, \
     \"last_logout_time\" = $29, \"last_login_ip\" = $30, \
     \"last_logout_ip\" = $31, \"last_login_medium\" = $32, \
     \"last_login_uagent\" = $33, \"token_updated_at\" = $34, \
     \"is_bot\" = $35, \"bot_type\" = $36, \"user_timezone\" = $37, \
     \"is_email_valid\" = $38, \"masked_at\" = $39 \
     WHERE \"id\" = $40",
    []
}

plane_query! {
    /// `profile.save()` (`plane/app/views/user/base.py:446-458,390,401`): the
    /// `Profile` model has no `save()` override, so `Model.save()` writes all
    /// 28 non-`id` columns.
    ///
    /// Bind order: [`super::queries::USER_PROFILE`]'s columns without `id`,
    /// then `id`.
    PROFILE_UPDATE = "users_me_profile_update",
    "UPDATE profiles SET \
     \"created_at\" = $1, \"updated_at\" = $2, \"user_id\" = $3, \
     \"theme\" = $4, \"is_app_rail_docked\" = $5, \
     \"is_tour_completed\" = $6, \"onboarding_step\" = $7, \
     \"use_case\" = $8, \"role\" = $9, \"is_onboarded\" = $10, \
     \"last_workspace_id\" = $11, \"billing_address_country\" = $12, \
     \"billing_address\" = $13, \"has_billing_address\" = $14, \
     \"company_name\" = $15, \"notification_view_mode\" = $16, \
     \"is_smooth_cursor_enabled\" = $17, \"is_mobile_onboarded\" = $18, \
     \"mobile_onboarding_step\" = $19, \"mobile_timezone_auto_set\" = $20, \
     \"language\" = $21, \"start_of_the_week\" = $22, \"goals\" = $23, \
     \"background_color\" = $24, \"is_navigation_tour_completed\" = $25, \
     \"has_marketing_email_consent\" = $26, \
     \"is_subscribed_to_changelog\" = $27, \"product_tour\" = $28 \
     WHERE \"id\" = $29",
    []
}

plane_query! {
    /// `APIToken.objects.create(...)` (`plane/app/views/api.py:22-27`):
    /// `BaseModel.save` stamps `created_at`/`updated_at` and sets
    /// `created_by = request.user`, `updated_by = NULL`; every other column
    /// gets the model default or the view's raw value.
    ///
    /// Bind order is the table's model order (the `USER_API_TOKENS` select
    /// order).
    API_TOKEN_INSERT = "users_me_api_token_insert",
    "INSERT INTO api_tokens \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      label, description, is_active, last_used, token, user_id, user_type, \
      workspace_id, expired_at) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    []
}

plane_query! {
    /// `APIToken.objects.get(user=request.user, pk=pk)`
    /// (`plane/app/views/api.py:47,52,57`): the `SoftDeletionManager`
    /// predicate plus the full row.
    ///
    /// Bind order: the token id, then the user id.
    API_TOKEN_BY_PK = "users_me_api_token_by_pk",
    "SELECT t.\"created_at\" AS created_at, t.\"updated_at\" AS updated_at, \
     t.\"created_by_id\" AS created_by_id, t.\"updated_by_id\" AS updated_by_id, \
     t.\"deleted_at\" AS deleted_at, t.\"id\" AS id, t.\"label\" AS label, \
     t.\"description\" AS description, t.\"is_active\" AS is_active, \
     t.\"last_used\" AS last_used, t.\"token\" AS token, t.\"user_id\" AS user_id, \
     t.\"user_type\" AS user_type, t.\"workspace_id\" AS workspace_id, \
     t.\"expired_at\" AS expired_at \
     FROM api_tokens t \
     WHERE t.\"deleted_at\" IS NULL AND t.\"id\" = $1 AND t.\"user_id\" = $2 \
     LIMIT 21",
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
    /// `serializer.save()` on 227 PATCH: `BaseModel.save` sets
    /// `updated_by = request.user` and `updated_at = now()`; three columns
    /// (`deleted_at`, `created_by`, `updated_by`) are writable through
    /// `APITokenSerializer` and come from the validated body.
    ///
    /// Bind order: [`API_TOKEN_BY_PK`]'s columns without `id`, then the id.
    API_TOKEN_UPDATE = "users_me_api_token_update",
    "UPDATE api_tokens SET \
     \"created_at\" = $1, \"updated_at\" = $2, \"created_by_id\" = $3, \
     \"updated_by_id\" = $4, \"deleted_at\" = $5, \"label\" = $6, \
     \"description\" = $7, \"is_active\" = $8, \"last_used\" = $9, \
     \"token\" = $10, \"user_id\" = $11, \"user_type\" = $12, \
     \"workspace_id\" = $13, \"expired_at\" = $14 \
     WHERE \"id\" = $15",
    []
}

plane_query! {
    /// `InstanceAdmin.objects.filter(user=user).exists()`
    /// (`plane/app/views/user/base.py:279-283`), the deactivation guard.
    /// `BaseModel`'s `SoftDeletionManager` hides soft-deleted rows; Django's
    /// `exists()` is `SELECT 1 … LIMIT 1`.
    INSTANCE_ADMIN_EXISTS = "users_me_instance_admin_exists",
    "SELECT 1::int4 AS present FROM instance_admins \
     WHERE \"deleted_at\" IS NULL AND \"user_id\" = $1 LIMIT 1",
    [("present", Int4)]
}

plane_query! {
    /// `ProjectMember.objects.filter(member=request.user, is_active=True)`
    /// (`plane/app/views/user/base.py:288`), iterated for the (dead) guard and
    /// then `bulk_update(..., ["is_active"])`.
    ///
    /// The two annotations are constant per row and cannot change the branch
    /// (see `plans/USERS_ME.md` §4), so only the ids are read.
    /// `ProjectBaseModel.Meta.ordering` is `("-created_at",)`, which Django
    /// carries into the statement.
    PROJECT_MEMBER_SCAN = "users_me_project_member_scan",
    "SELECT pm.\"id\" AS id FROM project_members pm \
     WHERE pm.\"deleted_at\" IS NULL AND pm.\"member_id\" = $1 \
       AND pm.\"is_active\" \
     ORDER BY pm.\"created_at\" DESC",
    [("id", Uuid)]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(member=request.user, is_active=True)`
    /// (`plane/app/views/user/base.py:309`).
    ///
    /// `WorkspaceMember.Meta.ordering` is `("-created_at",)`
    /// (`plane/db/models/workspace.py:227`), so the iteration has that order
    /// even though the rows are only flipped.
    WORKSPACE_MEMBER_SCAN = "users_me_workspace_member_scan",
    "SELECT wm.\"id\" AS id FROM workspace_members wm \
     WHERE wm.\"deleted_at\" IS NULL AND wm.\"member_id\" = $1 \
       AND wm.\"is_active\" \
     ORDER BY wm.\"created_at\" DESC",
    [("id", Uuid)]
}

plane_query! {
    /// `ProjectMember.objects.bulk_update(projects_to_deactivate,
    /// ["is_active"])` (`plane/app/views/user/base.py:334`).
    ///
    /// `bulk_update` writes only the named column: no `save()` hook runs, so
    /// `updated_at`/`updated_by_id` are untouched. One statement per row keeps
    /// the handler identical to Django's per-batch `CASE` statement in every
    /// observable way (the row set is fixed by the scan above).
    ///
    /// Bind order: the row id.
    PROJECT_MEMBER_DEACTIVATE = "users_me_project_member_deactivate",
    "UPDATE project_members SET \"is_active\" = FALSE WHERE \"id\" = $1",
    []
}

plane_query! {
    /// `WorkspaceMember.objects.bulk_update(workspaces_to_deactivate,
    /// ["is_active"])` (`plane/app/views/user/base.py:336`).
    ///
    /// Same shape as [`PROJECT_MEMBER_DEACTIVATE`].
    WORKSPACE_MEMBER_DEACTIVATE = "users_me_workspace_member_deactivate",
    "UPDATE workspace_members SET \"is_active\" = FALSE WHERE \"id\" = $1",
    []
}

plane_query! {
    /// `User.objects.filter(email=new_email).exclude(id=user.id).exists()`
    /// (`plane/app/views/user/base.py:136,251`): the two "address already
    /// taken" checks of 165/166.
    ///
    /// Bind order: the address, then the caller's id.
    USER_EMAIL_EXISTS = "users_me_user_email_exists",
    "SELECT 1::int4 AS present FROM users \
     WHERE \"email\" = $1 AND NOT (\"id\" = $2) LIMIT 1",
    [("present", Int4)]
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(email=user.email).delete()`
    /// (`plane/app/views/user/base.py:337`).
    ///
    /// `WorkspaceMemberInvite` uses `SoftDeletionQuerySet.delete`, which is
    /// `self.update(deleted_at=timezone.now())` (`plane/db/mixins.py:31-35`):
    /// a bare `UPDATE` with **no** `updated_at` and no cascade. The queryset
    /// still carries the manager's `deleted_at IS NULL` predicate, so the
    /// update only touches live rows. The timestamp is the caller's
    /// `timezone.now()`, not the database clock.
    ///
    /// Bind order: the e-mail (possibly NULL; `filter(email=None)` is
    /// `email IS NULL`, which `IS NOT DISTINCT FROM` reproduces), then the
    /// timestamp.
    INVITE_DELETE_BY_EMAIL = "users_me_invite_delete_by_email",
    "UPDATE workspace_member_invites SET \"deleted_at\" = $2 \
     WHERE \"deleted_at\" IS NULL AND \"email\" IS NOT DISTINCT FROM $1",
    []
}

/// Every catalogued statement of the write paths, in declaration order.
pub static ALL_QUERIES: &[&plane_db::Query] = &[
    &USER_UPDATE,
    &PROFILE_UPDATE,
    &API_TOKEN_INSERT,
    &API_TOKEN_BY_PK,
    &API_TOKEN_UPDATE,
    &INSTANCE_ADMIN_EXISTS,
    &PROJECT_MEMBER_SCAN,
    &WORKSPACE_MEMBER_SCAN,
    &PROJECT_MEMBER_DEACTIVATE,
    &WORKSPACE_MEMBER_DEACTIVATE,
    &USER_EMAIL_EXISTS,
    &INVITE_DELETE_BY_EMAIL,
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
        }
    }
}
