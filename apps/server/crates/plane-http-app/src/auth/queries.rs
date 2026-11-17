// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `auth` group (set B, magic link).
//!
//! `get_redirection_path` (`plane/authentication/utils/redirection_path.py:8-47`)
//! is the only function that reads here; the user and profile statements it
//! needs are already catalogued by `plane-auth`
//! (`queries::USER_EXISTS_BY_EMAIL`, `queries::LOAD_PROFILE_FOR_USER`,
//! `queries::INSERT_PROFILE`).
//!
//! Every statement keeps Django's ORM semantics exactly:
//!
//! * `Workspace.objects` is a `SoftDeletionManager` (`plane/db/mixins.py:57`), so
//!   the workspace side filters `deleted_at IS NULL`;
//! * the reverse FK filter `workspace_member__…` does **not** apply the related
//!   manager: Django joins `workspace_members` and filters only the named
//!   columns, so a soft-deleted membership still counts. Both joins below are
//!   therefore unfiltered on `workspace_members.deleted_at`;
//! * `WorkspaceMemberInvite.objects.filter(email=…).count()` **does** use the
//!   soft-deletion manager.

use plane_db::Query;
use plane_db::plane_query;

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
    &AUTH_LAST_WORKSPACE_SLUG,
    &AUTH_MEMBER_WORKSPACE_SLUG,
    &AUTH_PENDING_INVITE_EXISTS,
];
