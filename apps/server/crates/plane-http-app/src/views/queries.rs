// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `views` group.
//!
//! The two workspace-view statements are **not** here: their `order_by` comes
//! from the request (through `VIEW_ORDER_BY_ALLOWLIST`) and the guest filter
//! changes their parameter list, so they are dynamic SQL and go through
//! `Client::prepare` (`DECISIONS.md` D1's 2026-11-16 amendment). See
//! `crate::views::base::workspace_views_sql`.
//!
//! ## `IssueViewViewSet` (`plane/app/views/view/base.py:291-383`)
//!
//! `get_queryset()` is
//! `IssueView.objects.all().filter(workspace__slug=slug).filter(project_id=…)
//! .filter(project__project_projectmember__member=user,
//! project__project_projectmember__is_active=True,
//! project__archived_at__isnull=True).filter(Q(owned_by=user) |
//! Q(access=1)).select_related("project").select_related("workspace")
//! .annotate(is_favorite=Exists(user_favorite subquery))
//! .order_by("-is_favorite", "name").distinct()`.
//!
//! Django emits the two `select_related` joins plus the `project_members` join
//! and selects the joined models' columns as well; only the `issue_views`
//! columns and `is_favorite` reach the serializer, and because the joins are
//! many-to-one the `DISTINCT` row set is the same without them. The one
//! predicate Django does **not** add is `project_members.deleted_at IS NULL`:
//! `filter(project__project_projectmember__member=…)` is a join, and Django
//! never applies a related model's default manager to a join (only to a
//! queryset of that model). It is reproduced here for the same reason the
//! ledger lists it.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Project.objects.get(id=project_id)` (`base.py:334`): the only column the
    /// view reads is `guest_view_all_features`. The lookup is **not** scoped to
    /// the URL workspace, and the manager's `deleted_at IS NULL` predicate
    /// applies — a soft-deleted project is `DoesNotExist` (404) even though
    /// `allow_permission` accepted the caller's `ProjectMember` row.
    PROJECT_GUEST_FLAG = "views_project_guest_flag",
    "SELECT guest_view_all_features FROM projects WHERE id = $1 AND deleted_at IS NULL LIMIT 1",
    [("guest_view_all_features", Bool)]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace__slug=slug, member=user,
    /// role=5, is_active=True).exists()` (`base.py:88`, `:131`): the
    /// workspace-guest test both `WorkspaceViewViewSet` branches run.
    WORKSPACE_GUEST_EXISTS = "views_workspace_guest_exists",
    "SELECT 1 AS \"exists\" FROM workspace_members wm \
     INNER JOIN workspaces w ON wm.workspace_id = w.id \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND wm.member_id = $2 \
       AND wm.role = 5 AND w.slug = $1 \
     LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `ProjectMember.objects.filter(workspace__slug=slug, project_id=…,
    /// member=user, role=5, is_active=True).exists()` (`base.py:335-342`,
    /// `:359-366`): the project-guest test `IssueViewViewSet` runs.
    PROJECT_GUEST_EXISTS = "views_project_guest_exists",
    "SELECT 1 AS \"exists\" FROM project_members pm \
     INNER JOIN workspaces w ON pm.workspace_id = w.id \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $3 \
       AND pm.project_id = $2 AND pm.role = 5 AND w.slug = $1 \
     LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `IssueViewViewSet.list`'s queryset (`base.py:305-329`); see the module
    /// docs for the predicates.
    ISSUE_VIEW_LIST = "views_issue_view_list",
    "SELECT DISTINCT iv.id, EXISTS(SELECT 1 FROM user_favorites uf \
       INNER JOIN workspaces uw ON uf.workspace_id = uw.id \
       WHERE uf.deleted_at IS NULL AND uf.entity_identifier = iv.id \
         AND uf.entity_type = 'view' AND uf.project_id = $2 AND uf.user_id = $3 \
         AND uw.slug = $1 LIMIT 1) AS is_favorite, \
     iv.created_at, iv.updated_at, iv.deleted_at, iv.name, iv.description, \
     iv.query, iv.filters, iv.display_filters, iv.display_properties, \
     iv.rich_filters, iv.access, iv.sort_order, iv.logo_props, iv.is_locked, \
     iv.archived_at, iv.created_by_id, iv.updated_by_id, iv.workspace_id, \
     iv.project_id, iv.owned_by_id \
     FROM issue_views iv \
     INNER JOIN workspaces w ON iv.workspace_id = w.id \
     INNER JOIN projects p ON iv.project_id = p.id \
     INNER JOIN project_members pm ON p.id = pm.project_id \
     WHERE iv.deleted_at IS NULL AND w.slug = $1 AND iv.project_id = $2 \
       AND p.archived_at IS NULL AND pm.is_active AND pm.member_id = $3 \
       AND (iv.owned_by_id = $3 OR iv.access = 1) \
     ORDER BY 2 DESC, iv.name ASC",
    [
        ("id", Uuid),
        ("is_favorite", Bool),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("name", Varchar),
        ("description", Text),
        ("query", Jsonb),
        ("filters", Jsonb),
        ("display_filters", Jsonb),
        ("display_properties", Jsonb),
        ("rich_filters", Jsonb),
        ("access", Int2),
        ("sort_order", Float8),
        ("logo_props", Jsonb),
        ("is_locked", Bool),
        ("archived_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("owned_by_id", Uuid),
    ]
}

plane_query! {
    /// [`ISSUE_VIEW_LIST`] plus the restricted-guest filter `owned_by=user`
    /// (`base.py:343-345`).
    ISSUE_VIEW_LIST_RESTRICTED = "views_issue_view_list_restricted",
    "SELECT DISTINCT iv.id, EXISTS(SELECT 1 FROM user_favorites uf \
       INNER JOIN workspaces uw ON uf.workspace_id = uw.id \
       WHERE uf.deleted_at IS NULL AND uf.entity_identifier = iv.id \
         AND uf.entity_type = 'view' AND uf.project_id = $2 AND uf.user_id = $3 \
         AND uw.slug = $1 LIMIT 1) AS is_favorite, \
     iv.created_at, iv.updated_at, iv.deleted_at, iv.name, iv.description, \
     iv.query, iv.filters, iv.display_filters, iv.display_properties, \
     iv.rich_filters, iv.access, iv.sort_order, iv.logo_props, iv.is_locked, \
     iv.archived_at, iv.created_by_id, iv.updated_by_id, iv.workspace_id, \
     iv.project_id, iv.owned_by_id \
     FROM issue_views iv \
     INNER JOIN workspaces w ON iv.workspace_id = w.id \
     INNER JOIN projects p ON iv.project_id = p.id \
     INNER JOIN project_members pm ON p.id = pm.project_id \
     WHERE iv.deleted_at IS NULL AND w.slug = $1 AND iv.project_id = $2 \
       AND p.archived_at IS NULL AND pm.is_active AND pm.member_id = $3 \
       AND (iv.owned_by_id = $3 OR iv.access = 1) AND iv.owned_by_id = $3 \
     ORDER BY 2 DESC, iv.name ASC",
    [
        ("id", Uuid),
        ("is_favorite", Bool),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("name", Varchar),
        ("description", Text),
        ("query", Jsonb),
        ("filters", Jsonb),
        ("display_filters", Jsonb),
        ("display_properties", Jsonb),
        ("rich_filters", Jsonb),
        ("access", Int2),
        ("sort_order", Float8),
        ("logo_props", Jsonb),
        ("is_locked", Bool),
        ("archived_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("owned_by_id", Uuid),
    ]
}

plane_query! {
    /// `IssueViewViewSet.retrieve`'s
    /// `self.get_queryset().filter(pk=pk, project_id=project_id).first()`
    /// (`base.py:352`). No guest filter: the guest case is a 403 *after* the
    /// row is found (`:359-373`).
    ISSUE_VIEW_BY_PK = "views_issue_view_by_pk",
    "SELECT DISTINCT iv.id, EXISTS(SELECT 1 FROM user_favorites uf \
       INNER JOIN workspaces uw ON uf.workspace_id = uw.id \
       WHERE uf.deleted_at IS NULL AND uf.entity_identifier = iv.id \
         AND uf.entity_type = 'view' AND uf.project_id = $2 AND uf.user_id = $3 \
         AND uw.slug = $1 LIMIT 1) AS is_favorite, \
     iv.created_at, iv.updated_at, iv.deleted_at, iv.name, iv.description, \
     iv.query, iv.filters, iv.display_filters, iv.display_properties, \
     iv.rich_filters, iv.access, iv.sort_order, iv.logo_props, iv.is_locked, \
     iv.archived_at, iv.created_by_id, iv.updated_by_id, iv.workspace_id, \
     iv.project_id, iv.owned_by_id \
     FROM issue_views iv \
     INNER JOIN workspaces w ON iv.workspace_id = w.id \
     INNER JOIN projects p ON iv.project_id = p.id \
     INNER JOIN project_members pm ON p.id = pm.project_id \
     WHERE iv.deleted_at IS NULL AND w.slug = $1 AND iv.project_id = $2 \
       AND p.archived_at IS NULL AND pm.is_active AND pm.member_id = $3 \
       AND (iv.owned_by_id = $3 OR iv.access = 1) AND iv.id = $4 \
     ORDER BY 2 DESC, iv.name ASC \
     LIMIT 1",
    [
        ("id", Uuid),
        ("is_favorite", Bool),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("name", Varchar),
        ("description", Text),
        ("query", Jsonb),
        ("filters", Jsonb),
        ("display_filters", Jsonb),
        ("display_properties", Jsonb),
        ("rich_filters", Jsonb),
        ("access", Int2),
        ("sort_order", Float8),
        ("logo_props", Jsonb),
        ("is_locked", Bool),
        ("archived_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("owned_by_id", Uuid),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_GUEST_FLAG,
    &WORKSPACE_GUEST_EXISTS,
    &PROJECT_GUEST_EXISTS,
    &ISSUE_VIEW_LIST,
    &ISSUE_VIEW_LIST_RESTRICTED,
    &ISSUE_VIEW_BY_PK,
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
