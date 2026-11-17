// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `estimates` (`deploy/route-groups.yaml`): Estimates and
//! estimate points.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/estimates/`
//! * `/api/workspaces/*/projects/*/project-estimates/`
//! * `/api/workspaces/*/projects/*/estimates/**`
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `GET …/projects/{project_id}/project-estimates/` | `ProjectEstimatePointEndpoint.get` | `plane/app/views/estimate/base.py:35` |
//! | `GET …/projects/{project_id}/estimates/` | `BulkEstimatePointEndpoint.list` | `plane/app/views/estimate/base.py:59` |
//! | `GET …/projects/{project_id}/estimates/{estimate_id}/` | `BulkEstimatePointEndpoint.retrieve` | `plane/app/views/estimate/base.py:103` |
//! | `POST …/projects/{project_id}/estimates/` | `BulkEstimatePointEndpoint.create` | `plane/app/views/estimate/base.py:63` |
//! | `PATCH …/projects/{project_id}/estimates/{estimate_id}/` | `BulkEstimatePointEndpoint.partial_update` | `plane/app/views/estimate/base.py:108` |
//! | `DELETE …/projects/{project_id}/estimates/{estimate_id}/` | `BulkEstimatePointEndpoint.destroy` | `plane/app/views/estimate/base.py:146` |
//! | `POST …/estimates/{estimate_id}/estimate-points/` | `EstimatePointEndpoint.create` | `plane/app/views/estimate/base.py:155` |
//! | `PATCH …/estimates/{estimate_id}/estimate-points/{estimate_point_id}/` | `EstimatePointEndpoint.partial_update` | `plane/app/views/estimate/base.py:182` |
//! | `DELETE …/estimates/{estimate_id}/estimate-points/{estimate_point_id}/` | `EstimatePointEndpoint.destroy` | `plane/app/views/estimate/base.py:197` |
//! | `GET …/estimates/` | `WorkspaceEstimatesEndpoint.get` | `plane/app/views/workspace/estimate.py:22` |

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod queries;
pub mod render;
pub mod ser;
pub mod workspace;
pub mod write_estimate;
pub mod write_estimate_queries;
pub mod write_point;
pub mod write_point_queries;
pub mod write_queries;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.estimate.base.ProjectEstimatePointEndpoint")?
        .method(http::Method::GET, base::project_estimates)?;

    registry
        .viewset("plane.app.views.estimate.base.BulkEstimatePointEndpoint")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?
        .action("create", write_estimate::create)?
        .action("partial_update", write_estimate::patch)?
        .action("destroy", write_estimate::destroy)?;

    registry
        .viewset("plane.app.views.estimate.base.EstimatePointEndpoint")?
        .action("create", write_point::create)?
        .action("partial_update", write_point::partial_update)?
        .action("destroy", write_point::destroy)?;

    registry
        .api_view("plane.app.views.workspace.estimate.WorkspaceEstimatesEndpoint")?
        .method(http::Method::GET, workspace::get)?;
    Ok(())
}
