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
//! | `GET …/estimates/` | `WorkspaceEstimatesEndpoint.get` | `plane/app/views/workspace/estimate.py:22` |

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod queries;
pub mod ser;
pub mod workspace;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.estimate.base.ProjectEstimatePointEndpoint")?
        .method(http::Method::GET, base::project_estimates)?;

    registry
        .viewset("plane.app.views.estimate.base.BulkEstimatePointEndpoint")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?;

    registry
        .api_view("plane.app.views.workspace.estimate.WorkspaceEstimatesEndpoint")?
        .method(http::Method::GET, workspace::get)?;
    Ok(())
}
