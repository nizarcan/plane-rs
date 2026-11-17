// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `analytics` (`deploy/route-groups.yaml`): Workspace and project analytics endpoints.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/analytics/`
//! * `/api/workspaces/*/analytic-view/**`
//! * `/api/workspaces/*/saved-analytic-view/**`
//! * `/api/workspaces/*/export-analytics/`
//! * `/api/workspaces/*/default-analytics/`
//! * `/api/workspaces/*/project-stats/`
//! * `/api/workspaces/*/advance-analytics/`
//! * `/api/workspaces/*/advance-analytics-stats/`
//! * `/api/workspaces/*/advance-analytics-charts/`
//! * `/api/workspaces/*/projects/*/advance-analytics/`
//! * `/api/workspaces/*/projects/*/advance-analytics-stats/`
//! * `/api/workspaces/*/projects/*/advance-analytics-charts/`
//!
//! ## Layout
//!
//! One file per Python module under `plane/app/views/analytic/`, plus the two
//! helpers the views import:
//!
//! | File | Python |
//! | --- | --- |
//! | [`base`] | `base.py` — `AnalyticsEndpoint`, `AnalyticViewViewset`, `SavedAnalyticEndpoint` |
//! | [`default`] | `base.py` — `DefaultAnalyticsEndpoint`, `ProjectStatsEndpoint` |
//! | [`advance`] | `advance.py`, `plane/utils/date_utils.py` |
//! | [`project`] | `project_analytics.py` |
//! | [`plot`] | `plane/utils/analytics_plot.py` — `build_graph_plot` |
//! | [`chart`] | `plane/utils/build_chart.py` — `build_analytics_chart` |
//! | [`sql`] | the `Issue` querysets the views build, as composable SQL |
//! | [`stored`] | the stored `analytic_views.query` dictionary |
//! | [`queries`] | the static catalog |
//!
//! ## What is ported
//!
//! Every `GET` of the group, plus the stored-view writes (routes 1 and 2:
//! `POST …/analytic-view/` and `PATCH`/`DELETE …/analytic-view/{pk}/`).
//! `POST …/export-analytics/` is **not** ported: it enqueues
//! `analytic_export_task` and has no read part.
//!
//! ## Side effects
//!
//! The `GET`s write nothing, invalidate nothing and enqueue nothing: they are
//! pure reads over `issues`, `projects`, `analytic_views` and the m2m through
//! tables. The stored-view writes are one INSERT / full-row UPDATE, and a
//! DELETE adds H03's soft delete plus its `soft_delete_related_objects`
//! cascade message ([`write`]).

use http::Method;
use plane_drf::registry::{Registry, RegistryError};

pub mod advance;
pub mod base;
pub mod chart;
pub mod default;
pub mod plot;
pub mod project;
pub mod queries;
pub mod sql;
pub mod stored;
pub mod write;
pub mod write_queries;

/// Register this group's handlers, in the order they were ported.
///
/// `apps/server/HANDLERS.md` has the step list. Return the first
/// [`RegistryError`]: a route that cannot be registered must fail startup
/// rather than fall back to a 501 nobody notices.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.analytic.base.AnalyticsEndpoint")?
        .method(Method::GET, base::analytics)?;
    registry
        .viewset("plane.app.views.analytic.base.AnalyticViewViewset")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?
        .action("create", write::create)?
        .action("partial_update", write::partial_update)?
        .action("destroy", write::destroy)?;
    registry
        .api_view("plane.app.views.analytic.base.SavedAnalyticEndpoint")?
        .method(Method::GET, base::saved_analytic)?;
    registry
        .api_view("plane.app.views.analytic.base.DefaultAnalyticsEndpoint")?
        .method(Method::GET, default::default_analytics)?;
    registry
        .api_view("plane.app.views.analytic.base.ProjectStatsEndpoint")?
        .method(Method::GET, default::project_stats)?;
    registry
        .api_view("plane.app.views.analytic.advance.AdvanceAnalyticsEndpoint")?
        .method(Method::GET, advance::advance_analytics)?;
    registry
        .api_view("plane.app.views.analytic.advance.AdvanceAnalyticsStatsEndpoint")?
        .method(Method::GET, advance::advance_analytics_stats)?;
    registry
        .api_view("plane.app.views.analytic.advance.AdvanceAnalyticsChartEndpoint")?
        .method(Method::GET, advance::advance_analytics_chart)?;
    registry
        .api_view("plane.app.views.analytic.project_analytics.ProjectAdvanceAnalyticsEndpoint")?
        .method(Method::GET, project::advance_analytics)?;
    registry
        .api_view(
            "plane.app.views.analytic.project_analytics.ProjectAdvanceAnalyticsStatsEndpoint",
        )?
        .method(Method::GET, project::advance_analytics_stats)?;
    registry
        .api_view(
            "plane.app.views.analytic.project_analytics.ProjectAdvanceAnalyticsChartEndpoint",
        )?
        .method(Method::GET, project::advance_analytics_chart)?;
    Ok(())
}
