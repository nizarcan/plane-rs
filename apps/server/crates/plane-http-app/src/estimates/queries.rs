// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `estimates` group.
//!
//! Every statement names its columns explicitly and mirrors the ORM's joins and
//! ordering (`Estimate.Meta.ordering = ("name",)`,
//! `EstimatePoint.Meta.ordering = ("value",)`), so the row order is the one
//! Django produces.
//!
//! The two `prefetch_related("points")` statements use `estimate_id = ANY($1)`
//! instead of an `IN (…)` list whose text grows with the page: same shape for
//! the planner, one cacheable statement.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Project.objects.get(workspace__slug=slug, pk=project_id)`
    /// (`plane/app/views/estimate/base.py:39`). The default manager filters
    /// soft-deleted rows; `Meta.ordering = ("-created_at",)` is kept so the
    /// statement is the ORM's, with `LIMIT 21` replaced by `LIMIT 1` (a primary
    /// key lookup cannot match twice).
    ///
    /// Bind order: `$1` project id, `$2` workspace slug.
    PROJECT_BY_SLUG_AND_ID = "estimates_project_by_slug_and_id",
    "SELECT p.\"created_at\", p.\"updated_at\", p.\"created_by_id\", \
     p.\"updated_by_id\", p.\"deleted_at\", p.\"id\", p.\"name\", \
     p.\"description\", p.\"description_text\", p.\"description_html\", \
     p.\"network\", p.\"workspace_id\", p.\"identifier\", \
     p.\"default_assignee_id\", p.\"project_lead_id\", p.\"emoji\", \
     p.\"icon_prop\", p.\"module_view\", p.\"cycle_view\", \
     p.\"issue_views_view\", p.\"page_view\", p.\"intake_view\", \
     p.\"is_time_tracking_enabled\", p.\"is_issue_type_enabled\", \
     p.\"guest_view_all_features\", p.\"cover_image\", \
     p.\"cover_image_asset_id\", p.\"estimate_id\", p.\"archive_in\", \
     p.\"close_in\", p.\"logo_props\", p.\"default_state_id\", p.\"archived_at\", \
     p.\"timezone\", p.\"external_source\", p.\"external_id\" \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     WHERE p.deleted_at IS NULL AND p.id = $1 AND w.slug = $2 \
     ORDER BY p.\"created_at\" DESC LIMIT 1",
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

plane_query! {
    /// `EstimatePoint.objects.filter(estimate_id=…, project_id=…, workspace__slug=…)`
    /// (`plane/app/views/estimate/base.py:41-46`).
    ///
    /// Bind order: `$1` estimate id, `$2` project id, `$3` workspace slug.
    ESTIMATE_POINTS_BY_PROJECT = "estimates_points_by_project",
    "SELECT ep.\"created_at\", ep.\"updated_at\", ep.\"created_by_id\", \
     ep.\"updated_by_id\", ep.\"deleted_at\", ep.\"id\", ep.\"project_id\", \
     ep.\"workspace_id\", ep.\"estimate_id\", ep.\"key\", ep.\"description\", \
     ep.\"value\" \
     FROM estimate_points ep \
     INNER JOIN workspaces w ON (ep.workspace_id = w.id) \
     WHERE ep.deleted_at IS NULL AND ep.estimate_id = $1 \
       AND ep.project_id = $2 AND w.slug = $3 \
     ORDER BY ep.\"value\" ASC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("estimate_id", Uuid),
        ("key", Int4),
        ("description", Text),
        ("value", Varchar),
    ]
}

plane_query! {
    /// `BulkEstimatePointEndpoint.list`'s queryset
    /// (`plane/app/views/estimate/base.py:63-70`).
    ///
    /// Bind order: `$1` project id, `$2` workspace slug.
    ESTIMATES_BY_PROJECT = "estimates_by_project",
    "SELECT e.\"created_at\", e.\"updated_at\", e.\"created_by_id\", \
     e.\"updated_by_id\", e.\"deleted_at\", e.\"id\", e.\"project_id\", \
     e.\"workspace_id\", e.\"name\", e.\"description\", e.\"type\", \
     e.\"last_used\" \
     FROM estimates e \
     INNER JOIN projects p ON (e.project_id = p.id) \
     INNER JOIN workspaces w ON (e.workspace_id = w.id) \
     WHERE e.deleted_at IS NULL AND e.project_id = $1 AND w.slug = $2 \
     ORDER BY e.\"name\" ASC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("type", Varchar),
        ("last_used", Bool),
    ]
}

plane_query! {
    /// `prefetch_related("points")` for one page of estimates: Django issues
    /// one query for the whole set, in `EstimatePoint.Meta.ordering` order.
    ///
    /// Bind: `$1` an array of estimate ids.
    ESTIMATE_POINTS_BY_ESTIMATES = "estimates_points_by_estimates",
    "SELECT ep.\"created_at\", ep.\"updated_at\", ep.\"created_by_id\", \
     ep.\"updated_by_id\", ep.\"deleted_at\", ep.\"id\", ep.\"project_id\", \
     ep.\"workspace_id\", ep.\"estimate_id\", ep.\"key\", ep.\"description\", \
     ep.\"value\" \
     FROM estimate_points ep \
     WHERE ep.deleted_at IS NULL AND ep.estimate_id = ANY($1) \
     ORDER BY ep.\"value\" ASC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("estimate_id", Uuid),
        ("key", Int4),
        ("description", Text),
        ("value", Varchar),
    ]
}

plane_query! {
    /// `Estimate.objects.get(pk=…, workspace__slug=…, project_id=…)`
    /// (`plane/app/views/estimate/base.py:104`).
    ///
    /// Bind order: `$1` estimate id, `$2` workspace slug, `$3` project id.
    ESTIMATE_BY_ID = "estimates_estimate_by_id",
    "SELECT e.\"created_at\", e.\"updated_at\", e.\"created_by_id\", \
     e.\"updated_by_id\", e.\"deleted_at\", e.\"id\", e.\"project_id\", \
     e.\"workspace_id\", e.\"name\", e.\"description\", e.\"type\", \
     e.\"last_used\" \
     FROM estimates e \
     INNER JOIN workspaces w ON (e.workspace_id = w.id) \
     WHERE e.deleted_at IS NULL AND e.id = $1 AND w.slug = $2 \
       AND e.project_id = $3",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("type", Varchar),
        ("last_used", Bool),
    ]
}

plane_query! {
    /// The single-instance `points` query `EstimateReadSerializer(estimate)`
    /// triggers when it walks `estimate.points` (a reverse manager, so one
    /// query per instance — `plane/app/views/estimate/base.py:105`).
    ///
    /// Bind: `$1` estimate id.
    ESTIMATE_POINTS_OF_ESTIMATE = "estimates_points_of_estimate",
    "SELECT ep.\"created_at\", ep.\"updated_at\", ep.\"created_by_id\", \
     ep.\"updated_by_id\", ep.\"deleted_at\", ep.\"id\", ep.\"project_id\", \
     ep.\"workspace_id\", ep.\"estimate_id\", ep.\"key\", ep.\"description\", \
     ep.\"value\" \
     FROM estimate_points ep \
     WHERE ep.deleted_at IS NULL AND ep.estimate_id = $1 \
     ORDER BY ep.\"value\" ASC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("estimate_id", Uuid),
        ("key", Int4),
        ("description", Text),
        ("value", Varchar),
    ]
}

plane_query! {
    /// `WorkspaceEstimatesEndpoint.get` (`plane/app/views/workspace/estimate.py:22-30`):
    /// the estimates referenced by the workspace's projects, with the project
    /// `pk__in=` subquery the ORM builds from `values_list("estimate_id")`.
    ///
    /// Bind order: `$1` workspace slug (used twice, as Django does).
    WORKSPACE_ESTIMATES = "estimates_workspace_estimates",
    "SELECT e.\"created_at\", e.\"updated_at\", e.\"created_by_id\", \
     e.\"updated_by_id\", e.\"deleted_at\", e.\"id\", e.\"project_id\", \
     e.\"workspace_id\", e.\"name\", e.\"description\", e.\"type\", \
     e.\"last_used\" \
     FROM estimates e \
     INNER JOIN projects p ON (e.project_id = p.id) \
     INNER JOIN workspaces w ON (e.workspace_id = w.id) \
     WHERE e.deleted_at IS NULL AND e.id IN ( \
       SELECT p2.estimate_id FROM projects p2 \
       INNER JOIN workspaces w2 ON (p2.workspace_id = w2.id) \
       WHERE p2.deleted_at IS NULL AND p2.estimate_id IS NOT NULL \
         AND w2.slug = $1 \
     ) AND w.slug = $1 \
     ORDER BY e.\"name\" ASC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("type", Varchar),
        ("last_used", Bool),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_BY_SLUG_AND_ID,
    &ESTIMATE_POINTS_BY_PROJECT,
    &ESTIMATES_BY_PROJECT,
    &ESTIMATE_POINTS_BY_ESTIMATES,
    &ESTIMATE_BY_ID,
    &ESTIMATE_POINTS_OF_ESTIMATE,
    &WORKSPACE_ESTIMATES,
];
