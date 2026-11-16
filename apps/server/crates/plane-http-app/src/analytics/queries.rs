// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `analytics` group.
//!
//! Only the lookups with a fixed shape live here: the saved analytic views and
//! the four "one row" reads the chart branches do. Every aggregate is
//! dynamic — the filter set comes from the query string — and is composed in
//! [`super::sql`] (`DECISIONS.md` D1 amendment).
//!
//! `crates/plane-http-app/tests/schema_check.rs` prepares each statement here
//! against a Django-migrated database.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `AnalyticViewViewset.list`'s queryset
    /// (`plane/app/views/analytic/base.py:192-193`):
    /// `AnalyticView.objects.filter(workspace__slug=slug)` with the model's
    /// `Meta.ordering = ("-created_at",)`.
    ANALYTIC_VIEWS_BY_WORKSPACE = "analytics_views_by_workspace",
    "SELECT analytic_views.\"created_at\", analytic_views.\"updated_at\", analytic_views.\"created_by_id\", \
     analytic_views.\"updated_by_id\", analytic_views.\"deleted_at\", analytic_views.\"id\", analytic_views.\"workspace_id\", \
     analytic_views.\"name\", analytic_views.\"description\", analytic_views.\"query\", analytic_views.\"query_dict\" \
     FROM analytic_views \
     INNER JOIN workspaces ON analytic_views.workspace_id = workspaces.id \
     WHERE analytic_views.deleted_at IS NULL AND workspaces.slug = $1 \
     ORDER BY analytic_views.created_at DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("query", Jsonb),
        ("query_dict", Jsonb),
    ]
}

plane_query! {
    /// `get_object()` in `AnalyticViewViewset.retrieve`
    /// (`plane/app/views/analytic/base.py:192-193`, DRF's `get_object`):
    /// `filter(pk=…, workspace__slug=…)` with `LIMIT 21`.
    ANALYTIC_VIEW_IN_WORKSPACE = "analytics_view_in_workspace",
    "SELECT analytic_views.\"created_at\", analytic_views.\"updated_at\", analytic_views.\"created_by_id\", \
     analytic_views.\"updated_by_id\", analytic_views.\"deleted_at\", analytic_views.\"id\", analytic_views.\"workspace_id\", \
     analytic_views.\"name\", analytic_views.\"description\", analytic_views.\"query\", analytic_views.\"query_dict\" \
     FROM analytic_views \
     INNER JOIN workspaces ON analytic_views.workspace_id = workspaces.id \
     WHERE analytic_views.deleted_at IS NULL AND workspaces.slug = $1 AND analytic_views.id = $2 \
     ORDER BY analytic_views.created_at DESC LIMIT 21",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("query", Jsonb),
        ("query_dict", Jsonb),
    ]
}

plane_query! {
    /// The caller's workspace role in `ProjectStatsEndpoint.get`
    /// (`plane/app/views/analytic/base.py:431-435`):
    /// `WorkspaceMember.objects.filter(workspace__slug=…, member=…, is_active=True).values_list("role").first()`.
    /// `first()` on a `values_list` keeps the model's `Meta.ordering`
    /// (`-created_at`).
    WORKSPACE_MEMBER_ROLE = "analytics_workspace_member_role",
    "SELECT workspace_members.role FROM workspace_members \
     INNER JOIN workspaces ON workspace_members.workspace_id = workspaces.id \
     WHERE workspace_members.deleted_at IS NULL AND workspace_members.is_active \
       AND workspace_members.member_id = $1 AND workspaces.slug = $2 \
     ORDER BY workspace_members.created_at DESC LIMIT 1",
    [("role", Int2)]
}

plane_query! {
    /// `Cycle.objects.filter(id=cycle_id).first()` in
    /// `ProjectAdvanceAnalyticsChartEndpoint.work_item_completion_chart`
    /// (`plane/app/views/analytic/project_analytics.py:196`).
    ///
    /// `Cycle.start_date`/`end_date` are `DateTimeField`s (the Python calls
    /// `.date()` on both); `Module`'s are `DateField`s.
    CYCLE_WINDOW = "analytics_cycle_window",
    "SELECT cycles.start_date, cycles.end_date FROM cycles \
     WHERE cycles.deleted_at IS NULL AND cycles.id = $1 \
     ORDER BY cycles.created_at DESC LIMIT 1",
    [("start_date", Timestamptz), ("end_date", Timestamptz)]
}

plane_query! {
    /// `Module.objects.filter(id=module_id).first()`
    /// (`plane/app/views/analytic/project_analytics.py:208`).
    MODULE_WINDOW = "analytics_module_window",
    "SELECT modules.start_date, modules.target_date FROM modules \
     WHERE modules.deleted_at IS NULL AND modules.id = $1 \
     ORDER BY modules.created_at DESC LIMIT 1",
    [("start_date", Date), ("target_date", Date)]
}

plane_query! {
    /// `Project.objects.filter(id=project_id).first()`
    /// (`plane/app/views/analytic/project_analytics.py:217`), which reads only
    /// `created_at`.
    PROJECT_CREATED_AT = "analytics_project_created_at",
    "SELECT projects.created_at FROM projects \
     WHERE projects.deleted_at IS NULL AND projects.id = $1 \
     ORDER BY projects.created_at DESC LIMIT 1",
    [("created_at", Timestamptz)]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &ANALYTIC_VIEWS_BY_WORKSPACE,
    &ANALYTIC_VIEW_IN_WORKSPACE,
    &WORKSPACE_MEMBER_ROLE,
    &CYCLE_WINDOW,
    &MODULE_WINDOW,
    &PROJECT_CREATED_AT,
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
