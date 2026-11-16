// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `states` (`deploy/route-groups.yaml`): Project states and the
//! default-state action.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/states/**`
//! * `/api/workspaces/*/projects/*/states/**`
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `GET …/projects/{project_id}/states/` | `list` | `plane/app/views/state/base.py:82` |
//! | `GET …/projects/{project_id}/states/{pk}/` | `retrieve` | `plane/app/views/state/base.py:47` |
//! | `GET …/states/` | `WorkspaceStatesEndpoint.get` | `plane/app/views/workspace/state.py:21` |

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod not_found;
pub mod queries;
pub mod workspace;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.state.base.StateViewSet")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?;
    registry
        .api_view("plane.app.views.workspace.state.WorkspaceStatesEndpoint")?
        .method(http::Method::GET, workspace::get)?;
    Ok(())
}
