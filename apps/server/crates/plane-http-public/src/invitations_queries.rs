// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the v1 invitation routes (340–343).
//!
//! Python: `plane/api/views/invite.py:36-154`. `get_queryset()` is
//! `filter_queryset(WorkspaceMemberInvite.objects.filter(workspace__slug=slug))`
//! (`:36-37`); the default manager adds `deleted_at IS NULL` and the model's
//! ordering is `-created_at` (`plane/db/models/workspace.py:243-256`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `WorkspaceInvitationsViewset.list` (`plane/api/views/invite.py:55-58`):
    /// the seven `WorkspaceInviteSerializer` fields, model order.
    INVITES = "v1_invitations_list",
    "SELECT i.id, i.email, i.role, i.created_at, i.updated_at, \
     i.responded_at, i.accepted \
     FROM workspace_member_invites i \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND w.slug = $1 \
     ORDER BY i.created_at DESC",
    [
        ("id", Uuid),
        ("email", Varchar),
        ("role", Int2),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("responded_at", Timestamptz),
        ("accepted", Bool),
    ]
}

plane_query! {
    /// `self.get_queryset().get(pk=pk)` (`plane/api/views/invite.py:39-40`).
    INVITE_BY_ID = "v1_invitations_by_id",
    "SELECT i.id, i.email, i.role, i.created_at, i.updated_at, \
     i.responded_at, i.accepted, i.token, i.message, i.created_by_id, \
     i.updated_by_id, i.deleted_at, i.workspace_id \
     FROM workspace_member_invites i \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND w.slug = $1 AND i.id = $2 \
     LIMIT 1",
    [
        ("id", Uuid),
        ("email", Varchar),
        ("role", Int2),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("responded_at", Timestamptz),
        ("accepted", Bool),
        ("token", Varchar),
        ("message", Text),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("workspace_id", Uuid),
    ]
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.filter(email=…, workspace__slug=slug)
    ///  .exists()` (`plane/api/serializers/invite.py:53-57`). A PATCH/PUT that
    /// keeps the instance's own email matches the instance itself, which is
    /// why a full `PUT` with an unchanged email is rejected.
    INVITE_EMAIL_EXISTS = "v1_invitations_email_exists",
    "SELECT EXISTS (SELECT 1 FROM workspace_member_invites i \
       INNER JOIN workspaces w ON w.id = i.workspace_id \
       WHERE i.deleted_at IS NULL AND w.slug = $1 AND i.email = $2) AS present",
    [("present", Bool)]
}

plane_query! {
    /// `WorkspaceMemberInvite.objects.create(...)` from
    /// `WorkspaceInvitationsViewset.create` (`plane/api/views/invite.py:93`).
    ///
    /// The serializer has no `token` field and `create()` never sets one, but
    /// that does **not** fail: Django's `Field.get_default()` returns `""` for
    /// a `CharField` without a default (`django/db/models/fields/__init__.py`),
    /// so the row is written with an empty token. (The read plan
    /// `plans/V1_API.md:302-305` expected a NOT NULL 400; the oracle disagrees
    /// and the row is created — ported as it is.)
    INVITE_INSERT = "v1_invitations_insert",
    "INSERT INTO workspace_member_invites (created_at, updated_at, \
     created_by_id, updated_by_id, deleted_at, id, workspace_id, email, \
     accepted, token, message, responded_at, role) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    []
}

plane_query! {
    /// `serializer.save()` on an existing invite (`plane/api/views/invite.py:123`,
    /// `:120`) — a full-row `UPDATE` from the in-memory instance, after
    /// `BaseModel.save` stamps `updated_at`/`updated_by`.
    INVITE_UPDATE = "v1_invitations_update",
    "UPDATE workspace_member_invites SET created_at = $1, updated_at = $2, \
     created_by_id = $3, updated_by_id = $4, deleted_at = $5, id = $6, \
     workspace_id = $7, email = $8, accepted = $9, token = $10, \
     message = $11, responded_at = $12, role = $13 \
     WHERE id = $6",
    []
}

/// Every catalogued statement of this route family, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &INVITES,
    &INVITE_BY_ID,
    &INVITE_EMAIL_EXISTS,
    &INVITE_INSERT,
    &INVITE_UPDATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("invitations_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
