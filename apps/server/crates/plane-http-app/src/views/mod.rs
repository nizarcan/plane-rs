// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `views` (`deploy/route-groups.yaml`): Saved workspace and project views.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/views/**`
//! * `/api/workspaces/*/workspace-views/**`
//! * `/api/workspaces/*/projects/*/views/**`
//! * `/api/workspaces/*/projects/*/project-views/**`
//! * `/api/workspaces/*/projects/*/user-favorite-views/**`
//!
//! One file per Python module under `plane/app/views/`:
//!
//! | Rust | Python |
//! | --- | --- |
//! | [`base`] | `view/base.py` — `WorkspaceViewViewSet`, `IssueViewViewSet`, `IssueViewFavoriteViewSet`, `WorkspaceViewIssuesViewSet` |
//!
//! The four `GET`s of the two viewset families are ported, plus the
//! favourite-views `GET` the runtime's guard refuses (405).
//! `WorkspaceViewIssuesViewSet.list` (`GET /api/workspaces/{slug}/issues/`)
//! belongs to the `issues` group, not this one (`deploy/route-groups.yaml`).

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod queries;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    base::register(registry)?;
    // `…/projects/{project_id}/project-views/` matches this group's
    // `projects/*/project-views/**` selector, so the route is registered here
    // even though the handler lives with the other project settings writes.
    registry
        .api_view("plane.app.views.project.base.ProjectUserViewsEndpoint")?
        .method(
            http::Method::POST,
            crate::projects::settings_write::project_views,
        )?;
    Ok(())
}
