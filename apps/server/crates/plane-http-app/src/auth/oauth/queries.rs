// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the OAuth routes.
//!
//! The only database reads outside the shared auth adapter are
//! `get_redirection_path`'s four queries (`utils/redirection_path.py:8-47`),
//! run by the app callbacks when the login has no `next_path`.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Profile.objects.get_or_create(user=user)`'s lookup
    /// (`utils/redirection_path.py:10`). `Profile` has no `deleted_at`
    /// (`TimeAuditModel`, `plane/db/models/user.py:200`), so there is no
    /// soft-delete predicate. `user_id` is a `OneToOneField`, so `get` cannot
    /// see two rows.
    PROFILE_BY_USER = "auth_oauth_profile_by_user",
    "SELECT is_onboarded, last_workspace_id FROM profiles WHERE user_id = $1",
    [
        ("is_onboarded", Bool),
        ("last_workspace_id", Uuid),
    ]
}

plane_query! {
    /// The `profile.last_workspace_id` branch (`redirection_path.py:19-30`):
    /// `Workspace.objects.filter(pk=…, workspace_member__member_id=…,
    /// workspace_member__is_active=True)`.
    ///
    /// Django's reverse join applies **only** `workspaces.deleted_at IS NULL`
    /// (the queryset's own manager); a soft-deleted `workspace_members` row
    /// still satisfies the join, which is why there is no `deleted_at`
    /// predicate on it. Verified against the ORM's SQL.
    WORKSPACE_SLUG_FOR_LAST = "auth_oauth_workspace_slug_for_last",
    "SELECT w.slug \
     FROM workspaces w \
     INNER JOIN workspace_members wm ON w.id = wm.workspace_id \
     WHERE w.deleted_at IS NULL AND wm.is_active AND w.id = $1 AND wm.member_id = $2 \
     LIMIT 1",
    [("slug", Varchar)]
}

plane_query! {
    /// The fallback-workspace branch (`redirection_path.py:32-40`):
    /// `Workspace.objects.filter(workspace_member__member_id=…,
    /// workspace_member__is_active=True).order_by("created_at").first()`.
    FALLBACK_WORKSPACE_SLUG = "auth_oauth_fallback_workspace_slug",
    "SELECT w.slug \
     FROM workspaces w \
     INNER JOIN workspace_members wm ON w.id = wm.workspace_id \
     WHERE w.deleted_at IS NULL AND wm.is_active AND wm.member_id = $1 \
     ORDER BY w.created_at ASC \
     LIMIT 1",
    [("slug", Varchar)]
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(email=user.email).count()`
    /// (`redirection_path.py:43`): the default manager's soft-delete
    /// predicate, and the email comparison is **exact**.
    INVITE_EXISTS_FOR_EMAIL = "auth_oauth_invite_exists_for_email",
    "SELECT EXISTS (SELECT 1 FROM workspace_member_invites \
     WHERE deleted_at IS NULL AND email = $1) AS \"exists\"",
    [("exists", Bool)]
}

/// Every statement declared above, for `tests/schema_check.rs`.
pub static ALL_QUERIES: &[&Query] = &[
    &PROFILE_BY_USER,
    &WORKSPACE_SLUG_FOR_LAST,
    &FALLBACK_WORKSPACE_SLUG,
    &INVITE_EXISTS_FOR_EMAIL,
];
