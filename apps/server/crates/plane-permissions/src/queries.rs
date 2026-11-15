// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static statements the permission checks run.
//!
//! Every statement is the `EXISTS`/single-row form of one Django ORM query
//! from `plane/app/permissions`, `plane/utils/permissions` or
//! `plane/license/api/permissions`, with the same filters in the same order:
//!
//! * the soft-delete manager's `deleted_at IS NULL` applies to the table the
//!   queryset starts from, and **not** to joined tables. Django joins with the
//!   related model's `_base_manager` (a plain manager), so
//!   `WorkspaceMember.objects.filter(workspace__slug=…)` matches members of a
//!   soft-deleted workspace. `PROJECT_MEMBER_ROLE` and friends keep the join
//!   unfiltered on purpose;
//! * `workspaces.slug`, `projects.identifier` and `project_pages.deleted_at`
//!   are the only joined-table predicates the Python has;
//! * `is_active` is a plain boolean predicate on the membership row;
//! * a `None` kwarg becomes `IS NULL` in Django and matches nothing, because
//!   every column involved is `NOT NULL`. The Rust callers therefore skip the
//!   query and answer `false` instead of sending `NULL`.
//!
//! The result shape is declared for [`plane_db::schema_check`]; the integration
//! test prepares every statement against the fixture database, so a column
//! rename fails in this crate rather than at runtime.

use plane_db::catalog::Query;
use plane_db::plane_query;

plane_query! {
    /// `WorkspaceMember.objects.filter(member=…, workspace__slug=…, is_active=True)`
    /// (`plane/app/permissions/workspace.py:34-39` and every other workspace
    /// check). Only the role is needed; the partial unique constraint on
    /// `(workspace, member) WHERE deleted_at IS NULL` makes the row unique, so a
    /// single `ORDER BY created_at DESC LIMIT 1` answers both `.exists()` and
    /// `.values_list("role").first()`.
    WORKSPACE_MEMBER_ROLE = "permissions_workspace_member_role",
    "SELECT wm.role \
     FROM workspace_members wm \
     JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND wm.member_id = $1 AND w.slug = $2 \
     ORDER BY wm.created_at DESC \
     LIMIT 1",
    [("role", Int2)]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace_id=…, member=…, is_active=True)`
    /// (`plane/app/permissions/workspace.py:125-129`, the `workspace_id` branch
    /// of `WorkspaceMemberPermission`).
    WORKSPACE_MEMBER_ROLE_BY_ID = "permissions_workspace_member_role_by_id",
    "SELECT wm.role \
     FROM workspace_members wm \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND wm.workspace_id = $1 AND wm.member_id = $2 \
     ORDER BY wm.created_at DESC \
     LIMIT 1",
    [("role", Int2)]
}

plane_query! {
    /// `ProjectMember.objects.filter(member=…, workspace__slug=…, project_id=…, is_active=True)`
    /// (`plane/app/permissions/project.py:33-38,68-74,174-179`).
    PROJECT_MEMBER_ROLE = "permissions_project_member_role",
    "SELECT pm.role \
     FROM project_members pm \
     JOIN workspaces w ON w.id = pm.workspace_id \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $1 \
       AND pm.project_id = $2 AND w.slug = $3 \
     ORDER BY pm.created_at DESC \
     LIMIT 1",
    [("role", Int2)]
}

plane_query! {
    /// `ProjectMember.objects.filter(workspace__slug=…, member=…, project__identifier=…, is_active=True)`
    /// (`plane/app/permissions/project.py:141-148`, the
    /// `project_identifier` branch of `ProjectEntityPermission`). Existence is
    /// all that branch asks for; the identifier is unique per workspace among
    /// non-deleted projects but the join does not filter `projects.deleted_at`,
    /// so a soft-deleted project still counts.
    PROJECT_MEMBER_EXISTS_BY_IDENTIFIER = "permissions_project_member_exists_by_identifier",
    "SELECT EXISTS ( \
         SELECT 1 \
         FROM project_members pm \
         JOIN projects p ON p.id = pm.project_id \
         JOIN workspaces w ON w.id = pm.workspace_id \
         WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $1 \
           AND p.identifier = $2 AND w.slug = $3 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// `Issue.objects.filter(id=kwargs["pk"], created_by=request.user)`
    /// (`plane/app/permissions/base.py:51`). `Issue.objects` is the plain
    /// soft-deletion manager: `deleted_at IS NULL` only, no triage/archived
    /// filtering (that is `Issue.issue_objects`).
    CREATOR_ISSUE = "permissions_creator_issue",
    "SELECT EXISTS ( \
         SELECT 1 FROM issues WHERE deleted_at IS NULL AND id = $1 AND created_by_id = $2 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// As [`CREATOR_ISSUE`], for `IssueComment` (`issue.py:451`).
    CREATOR_ISSUE_COMMENT = "permissions_creator_issue_comment",
    "SELECT EXISTS ( \
         SELECT 1 FROM issue_comments WHERE deleted_at IS NULL AND id = $1 AND created_by_id = $2 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// As [`CREATOR_ISSUE`], for `FileAsset` (`asset.py:32`).
    CREATOR_FILE_ASSET = "permissions_creator_file_asset",
    "SELECT EXISTS ( \
         SELECT 1 FROM file_assets WHERE deleted_at IS NULL AND id = $1 AND created_by_id = $2 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// As [`CREATOR_ISSUE`], for `Sticky` (`sticky.py:16`).
    CREATOR_STICKY = "permissions_creator_sticky",
    "SELECT EXISTS ( \
         SELECT 1 FROM stickies WHERE deleted_at IS NULL AND id = $1 AND created_by_id = $2 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// As [`CREATOR_ISSUE`], for `Cycle` (`cycle.py:60`).
    CREATOR_CYCLE = "permissions_creator_cycle",
    "SELECT EXISTS ( \
         SELECT 1 FROM cycles WHERE deleted_at IS NULL AND id = $1 AND created_by_id = $2 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// As [`CREATOR_ISSUE`], for `Module` (`module.py:67`).
    CREATOR_MODULE = "permissions_creator_module",
    "SELECT EXISTS ( \
         SELECT 1 FROM modules WHERE deleted_at IS NULL AND id = $1 AND created_by_id = $2 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// As [`CREATOR_ISSUE`], for `IssueView` (`view.py:58`).
    CREATOR_ISSUE_VIEW = "permissions_creator_issue_view",
    "SELECT EXISTS ( \
         SELECT 1 FROM issue_views WHERE deleted_at IS NULL AND id = $1 AND created_by_id = $2 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// As [`CREATOR_ISSUE`], for `DraftIssue` (`draft.py:16`).
    CREATOR_DRAFT_ISSUE = "permissions_creator_draft_issue",
    "SELECT EXISTS ( \
         SELECT 1 FROM draft_issues WHERE deleted_at IS NULL AND id = $1 AND created_by_id = $2 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// `Page.objects.filter(id=page_id, workspace__slug=slug,
    /// project_pages__project_id=project_id,
    /// project_pages__deleted_at__isnull=True).first()`
    /// (`plane/app/permissions/page.py:48-53`). The page's own soft-delete
    /// filter is the manager's; the link's is explicit. `.first()` orders by
    /// `Page.Meta.ordering` (`-created_at`); `id` is unique, so the row is the
    /// page or nothing.
    PAGE_FOR_PERMISSION = "permissions_page_for_permission",
    "SELECT p.owned_by_id, p.access \
     FROM pages p \
     JOIN project_pages pp ON pp.page_id = p.id \
     JOIN workspaces w ON w.id = p.workspace_id \
     WHERE p.deleted_at IS NULL AND p.id = $1 AND pp.project_id = $2 \
       AND pp.deleted_at IS NULL AND w.slug = $3 \
     ORDER BY p.created_at DESC \
     LIMIT 1",
    [("owned_by_id", Uuid), ("access", Int2)]
}

plane_query! {
    /// `Project.objects.filter(pk=project_id, guest_view_all_features=True).exists()`
    /// (`plane/app/permissions/page.py:63`). `Project.objects` filters
    /// `deleted_at`; `pk=None` matches nothing because the column is `NOT NULL`.
    PROJECT_GUEST_VIEW_ALL = "permissions_project_guest_view_all",
    "SELECT EXISTS ( \
         SELECT 1 FROM projects \
         WHERE deleted_at IS NULL AND id = $1 AND guest_view_all_features \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// `Instance.objects.first()` (`plane/license/api/permissions/instance.py:16`).
    /// `Instance.Meta.ordering` is `-created_at`.
    INSTANCE_LATEST = "permissions_instance_latest",
    "SELECT id FROM instances WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `InstanceAdmin.objects.filter(role__gte=15, instance=instance, user=request.user).exists()`
    /// (`plane/license/api/permissions/instance.py:17`). `role` is an `integer`
    /// here, unlike the `smallint` membership roles.
    INSTANCE_ADMIN_EXISTS = "permissions_instance_admin_exists",
    "SELECT EXISTS ( \
         SELECT 1 FROM instance_admins \
         WHERE deleted_at IS NULL AND role >= 15 AND instance_id = $1 AND user_id = $2 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// `ProjectMember.objects.filter(workspace__slug=…, project_id=…, member=…,
    /// role=GUEST, is_active=True, project__guest_view_all_features=False).exists()`
    /// (`plane/app/permissions/base.py:124-131`). The project predicate becomes
    /// `NOT projects.guest_view_all_features`.
    RESTRICTED_GUEST_EXISTS = "permissions_restricted_guest_exists",
    "SELECT EXISTS ( \
         SELECT 1 \
         FROM project_members pm \
         JOIN projects p ON p.id = pm.project_id \
         JOIN workspaces w ON w.id = pm.workspace_id \
         WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $1 \
           AND pm.project_id = $2 AND pm.role = 5 \
           AND NOT p.guest_view_all_features AND w.slug = $3 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

plane_query! {
    /// `Issue.objects.filter(pk=…, workspace__slug=…, project_id=…,
    /// created_by=request.user).exists()` (`plane/app/permissions/base.py:135-140`).
    ISSUE_OWNED_BY_USER = "permissions_issue_owned_by_user",
    "SELECT EXISTS ( \
         SELECT 1 \
         FROM issues i \
         JOIN workspaces w ON w.id = i.workspace_id \
         WHERE i.deleted_at IS NULL AND i.id = $1 AND i.project_id = $2 \
           AND i.created_by_id = $3 AND w.slug = $4 \
     ) AS exists_flag",
    [("exists_flag", Bool)]
}

/// Every catalogued statement, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &WORKSPACE_MEMBER_ROLE,
    &WORKSPACE_MEMBER_ROLE_BY_ID,
    &PROJECT_MEMBER_ROLE,
    &PROJECT_MEMBER_EXISTS_BY_IDENTIFIER,
    &CREATOR_ISSUE,
    &CREATOR_ISSUE_COMMENT,
    &CREATOR_FILE_ASSET,
    &CREATOR_STICKY,
    &CREATOR_CYCLE,
    &CREATOR_MODULE,
    &CREATOR_ISSUE_VIEW,
    &CREATOR_DRAFT_ISSUE,
    &PAGE_FOR_PERMISSION,
    &PROJECT_GUEST_VIEW_ALL,
    &INSTANCE_LATEST,
    &INSTANCE_ADMIN_EXISTS,
    &RESTRICTED_GUEST_EXISTS,
    &ISSUE_OWNED_BY_USER,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `plane_query!` in this file must appear in `ALL_QUERIES`, and the
    /// names must be unique (the same guard `plane-db` uses).
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
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
            assert!(!query.columns.is_empty(), "{} has no columns", query.name);

            // Placeholders must be $1..$n with no gaps: the callers pass one
            // parameter per index in order.
            let mut indexes = std::collections::BTreeSet::new();
            for part in query.sql.split('$').skip(1) {
                let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
                if !digits.is_empty() {
                    indexes.insert(digits.parse::<u32>().expect("placeholder index"));
                }
            }
            let indexes: Vec<u32> = indexes.into_iter().collect();
            assert_eq!(
                indexes,
                (1..=indexes.len() as u32).collect::<Vec<_>>(),
                "{}: placeholders must be $1..$n",
                query.name
            );
        }
    }
}
