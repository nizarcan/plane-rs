// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static statements of [`super::lookup`].
//!
//! Both are `Query`s, not dynamic SQL: the shape is fixed (one table, every
//! column, one or two bound parameters), so they go through the per-connection
//! statement cache (`DECISIONS.md` D1). Dynamic SQL — `fields=`, filters,
//! grouping — must not be prepared cached; see `HANDLERS.md` "performance".
//!
//! The column list is the generated model's `COLUMNS` in `from_row` order, so
//! `Workspace::from_row(row, 0)` / `Project::from_row(row, 0)` decode these
//! rows. `tests/schema_check.rs` prepares both against a Django-migrated
//! database and fails if a column name or type drifts.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Workspace.objects.get(slug=slug)`:
    /// `plane/middleware/db_routing.py`, `plane/db/mixins.py:57` (the default
    /// manager hides soft-deleted rows), then `django/db/models/query.py`.
    ///
    /// `slug` is the only lookup the workspace-scoped routes use, and it is
    /// exact (`Workspace.save` slugifies once, on creation).
    WORKSPACE_BY_SLUG = "app_workspace_by_slug",
    "SELECT created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
     name, logo, logo_asset_id, owner_id, slug, organization_size, timezone, \
     background_color \
     FROM workspaces \
     WHERE slug = $1 AND deleted_at IS NULL",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("logo", Text),
        ("logo_asset_id", Uuid),
        ("owner_id", Uuid),
        ("slug", Varchar),
        ("organization_size", Varchar),
        ("timezone", Varchar),
        ("background_color", Varchar),
    ]
}

plane_query! {
    /// `Project.objects.get(pk=project_id, workspace__slug=slug)` with the
    /// workspace already loaded.
    ///
    /// Django joins `workspaces` to filter on the slug; the handler has the
    /// workspace id by then, so the join becomes `workspace_id = $1`. The
    /// project's own manager predicate still applies. The *workspace's*
    /// `deleted_at` is deliberately not consulted, because Django's
    /// `workspace__slug=` join does not filter soft-deleted workspaces either
    /// (a member of a soft-deleted workspace still passes; see the ledger's
    /// "suspected upstream bugs").
    PROJECT_IN_WORKSPACE = "app_project_in_workspace",
    "SELECT created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
     name, description, description_text, description_html, network, \
     workspace_id, identifier, default_assignee_id, project_lead_id, emoji, \
     icon_prop, module_view, cycle_view, issue_views_view, page_view, \
     intake_view, is_time_tracking_enabled, is_issue_type_enabled, \
     guest_view_all_features, cover_image, cover_image_asset_id, estimate_id, \
     archive_in, close_in, logo_props, default_state_id, archived_at, \
     timezone, external_source, external_id \
     FROM projects \
     WHERE id = $2 AND workspace_id = $1 AND deleted_at IS NULL",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("description_text", Jsonb),
        ("description_html", Jsonb),
        ("network", Int2),
        ("workspace_id", Uuid),
        ("identifier", Varchar),
        ("default_assignee_id", Uuid),
        ("project_lead_id", Uuid),
        ("emoji", Varchar),
        ("icon_prop", Jsonb),
        ("module_view", Bool),
        ("cycle_view", Bool),
        ("issue_views_view", Bool),
        ("page_view", Bool),
        ("intake_view", Bool),
        ("is_time_tracking_enabled", Bool),
        ("is_issue_type_enabled", Bool),
        ("guest_view_all_features", Bool),
        ("cover_image", Text),
        ("cover_image_asset_id", Uuid),
        ("estimate_id", Uuid),
        ("archive_in", Int4),
        ("close_in", Int4),
        ("logo_props", Jsonb),
        ("default_state_id", Uuid),
        ("archived_at", Timestamptz),
        ("timezone", Varchar),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

/// Every statement in this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&WORKSPACE_BY_SLUG, &PROJECT_IN_WORKSPACE];

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
