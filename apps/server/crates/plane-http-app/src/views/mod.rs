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
//! | [`write_view`] | the same two viewsets' `create`/`partial_update`/`destroy`; [`write_view_queries`] is their SQL |
//!
//! The four `GET`s of the two viewset families are ported, plus the
//! favourite-views `GET` the runtime's guard refuses (405) and, in
//! [`write_view`], the set-A writes: create (178/180), PATCH and DELETE
//! (179/181) and their `PUT` guard. `WorkspaceViewIssuesViewSet.list`
//! (`GET /api/workspaces/{slug}/issues/`) belongs to the `issues` group, not
//! this one (`deploy/route-groups.yaml`).

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod queries;
pub mod write_queries;

// Set A: view create, update and delete (178–181).
pub mod write_view;
pub mod write_view_queries;

// Set B: view favourites (183/184) and the member `workspace-views` write (198).
mod write_favorite;
mod write_member_views;
mod write_support;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    base::register(registry)?;
    // The saved-view writes (set A): create, PATCH, DELETE and the `PUT` guard
    // that lets both `{pk}` routes flip whole. `write_view` owns the derived
    // `query` column (H24) and the destroy side effects.
    registry
        .viewset("plane.app.views.view.base.WorkspaceViewViewSet")?
        .action("create", write_view::workspace_create)?
        .action("partial_update", write_view::workspace_partial_update)?
        .action("destroy", write_view::workspace_destroy)?
        // `PUT …/views/{pk}/` maps to `UpdateModelMixin.update`, which the
        // `BaseViewSet` guard always refuses (405); `ROUTES.md:631`.
        .action("update", crate::cycles::base::guard_only)?;
    registry
        .viewset("plane.app.views.view.base.IssueViewViewSet")?
        .action("create", write_view::issue_create)?
        .action("partial_update", write_view::issue_partial_update)?
        .action("destroy", write_view::issue_destroy)?
        // `ROUTES.md:630`.
        .action("update", crate::cycles::base::guard_only)?;
    // `…/projects/{project_id}/project-views/` matches this group's
    // `projects/*/project-views/**` selector, so the route is registered here
    // even though the handler lives with the other project settings writes.
    registry
        .api_view("plane.app.views.project.base.ProjectUserViewsEndpoint")?
        .method(
            http::Method::POST,
            crate::projects::settings_write::project_views,
        )?;
    // Set B: view favourites (`user-favorite-views/`, 183/184) and the member
    // preference write whose path this group selects (`workspace-views/`, 198).
    // `base::register` already owns this viewset's guard-405 `list`; the actions
    // here are the two write verbs.
    registry
        .viewset("plane.app.views.view.base.IssueViewFavoriteViewSet")?
        .action("create", write_favorite::create)?
        .action("destroy", write_favorite::destroy)?;
    registry
        .api_view("plane.app.views.workspace.member.WorkspaceMemberUserViewsEndpoint")?
        .method(http::Method::POST, write_member_views::workspace_views)?;
    Ok(())
}
