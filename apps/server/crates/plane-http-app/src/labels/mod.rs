// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `labels` (`deploy/route-groups.yaml`): Workspace and project
//! labels, issue labels.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/labels/`
//! * `/api/workspaces/*/projects/*/labels/**`
//! * `/api/workspaces/*/projects/*/issue-labels/**`
//! * `/api/workspaces/*/projects/*/bulk-create-labels/`
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `GET …/projects/{project_id}/issue-labels/` | `list` | `plane/app/views/issue/label.py:24` |
//! | `GET …/projects/{project_id}/issue-labels/{pk}/` | `retrieve` | `plane/app/views/issue/label.py:26` |
//! | `GET …/labels/` | `WorkspaceLabelsEndpoint.get` | `plane/app/views/workspace/label.py:22` |

use plane_drf::registry::{Registry, RegistryError};

pub mod label;
pub mod not_found;
pub mod queries;
pub mod workspace;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.label.LabelViewSet")?
        .action("list", label::list)?
        .action("retrieve", label::retrieve)?;
    registry
        .api_view("plane.app.views.workspace.label.WorkspaceLabelsEndpoint")?
        .method(http::Method::GET, workspace::get)?;
    Ok(())
}
