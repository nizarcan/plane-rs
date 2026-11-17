// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The statements the workspace-core **writes** share (`plans/WORKSPACES_CORE.md`
//! §2e), declared here so the three sets (A lifecycle, B members/invitations,
//! C preferences) do not each derive them.
//!
//! Each set owns its own file for what only it runs (`workspaces/write.rs`'s
//! create/update statements, `invite_write_queries.rs`, `member_write_queries.rs`,
//! …); this one holds the lookups that appear in more than one set, plus the
//! H04 statement route 138 uses.
//!
//! Every predicate is Django's, taken from the compiled queryset rather than
//! written by hand, and every statement is prepared by
//! `tests/schema_check.rs` against a Django-migrated database. Two rules of
//! Django's SQL recur:
//!
//! * the default manager's `deleted_at IS NULL` applies to the model the view
//!   queried, never to a model it merely joined (`plane/db/mixins.py:56-58`) —
//!   so a membership of a soft-deleted workspace still resolves;
//! * `Meta.ordering` is part of `get()`: `workspace_members` orders
//!   `-created_at` and `.get()` adds `LIMIT 21`
//!   (`django/db/models/query.py:get`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `WorkspaceMember.objects.get(workspace__slug=slug, member=request.user,
    /// is_active=True)` — the requester's own membership row.
    ///
    /// Three routes need it before they can decide anything:
    /// `WorkspaceInvitationsViewset.create`'s role cap (`invite.py:62-66`),
    /// `WorkSpaceMemberViewSet.partial_update`'s self check (`member.py:99-105`)
    /// and `.destroy`/`.leave` (`member.py:166-170,215-218`).
    ///
    /// A soft-deleted **workspace** does not hide the row: the `workspaces` join
    /// carries no `deleted_at` predicate (only the queried model's manager
    /// applies). A soft-deleted **membership** does, so this is the family 404.
    ///
    /// Bind order: `$1` user id, `$2` slug.
    WORKSPACE_MEMBER_ACTIVE = "workspaces_write_member_active",
    "SELECT wm.id, wm.workspace_id, wm.member_id, wm.role \
     FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND wm.member_id = $1 AND w.slug = $2 \
     ORDER BY wm.created_at DESC LIMIT 21",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
    ]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace__slug=slug, role=20,
    /// is_active=True).count()` (`plane/app/views/workspace/member.py:218-
    /// 221`) — the "only admin" guard of `leave`.
    ///
    /// `.count()` drops `Meta.ordering`, so there is no `ORDER BY`. The
    /// `workspaces` join has no `deleted_at` predicate, like the row above.
    ///
    /// Bind order: `$1` slug.
    WORKSPACE_ADMIN_COUNT = "workspaces_write_admin_count",
    "SELECT count(*) FROM workspace_members wm \
     INNER JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND wm.role = 20 AND w.slug = $1",
    [("count", Int8)]
}

plane_query! {
    /// DRF's `UniqueValidator` for `Workspace.slug`
    /// (`rest_framework/utils/field_mapping.py:65-93`):
    /// `Workspace.objects.filter(slug=value).exists()`, and on an update
    /// `.exclude(pk=instance.pk)` (`rest_framework/validators.py:99-113`).
    ///
    /// `Workspace.objects` is the soft-deletion manager, so a **soft-deleted**
    /// workspace keeps its slug without colliding: that is why
    /// `fix-deleted`'s slug can be reused, and why `create`'s 409 branch
    /// (`base.py:146-151`) is only reachable through a race or a `slug` the
    /// unique index still holds.
    ///
    /// The exclusion is folded into the statement so create and update share
    /// one prepared shape: `$2` is the instance pk on an update and NULL on a
    /// create, where `($2::uuid IS NULL OR id <> $2)` is always true.
    ///
    /// Bind order: `$1` slug, `$2` excluded pk or NULL.
    WORKSPACE_SLUG_TAKEN = "workspaces_write_slug_taken",
    "SELECT EXISTS ( \
       SELECT 1 FROM workspaces \
       WHERE deleted_at IS NULL AND slug = $1 AND ($2::uuid IS NULL OR id <> $2) \
     ) AS taken",
    [("taken", Bool)]
}

plane_query! {
    /// H04 for `ProjectIdentifierEndpoint.delete`
    /// (`plane/app/views/project/base.py:470`):
    /// `ProjectIdentifier.objects.filter(name=name, workspace__slug=slug)
    ///  .delete()`.
    ///
    /// `SoftDeletionQuerySet.delete` is `self.update(deleted_at=now)`
    /// (`plane/db/mixins.py:33-38`), and Django compiles an `UPDATE` with a
    /// joined filter as `WHERE id IN (SELECT … INNER JOIN …)`
    /// (`django/db/models/sql/compiler.py`, `SQLUpdateCompiler`). The name is
    /// the **upper-cased** request value (`base.py:466`) and the join's
    /// `workspaces` has no `deleted_at` predicate.
    ///
    /// Bind order: `$1` deleted_at, `$2` identifier name, `$3` slug.
    PROJECT_IDENTIFIERS_SOFT_DELETE = "workspaces_write_identifiers_soft_delete",
    "UPDATE project_identifiers SET deleted_at = $1 \
     WHERE id IN ( \
       SELECT U0.id FROM project_identifiers U0 \
       INNER JOIN workspaces U1 ON (U0.workspace_id = U1.id) \
       WHERE (U0.deleted_at IS NULL AND U0.name = $2 AND U1.slug = $3) \
     )",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &WORKSPACE_MEMBER_ACTIVE,
    &WORKSPACE_ADMIN_COUNT,
    &WORKSPACE_SLUG_TAKEN,
    &PROJECT_IDENTIFIERS_SOFT_DELETE,
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
