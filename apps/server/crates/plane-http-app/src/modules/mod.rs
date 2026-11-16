// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `modules` (`deploy/route-groups.yaml`): Modules, module links
//! and module issues.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/modules/`
//! * `/api/workspaces/*/projects/*/modules/**`
//! * `/api/workspaces/*/projects/*/archived-modules/**`
//! * `/api/workspaces/*/projects/*/user-favorite-modules/**`
//!
//! One file per Python module under `plane/app/views/`:
//!
//! | Rust | Python |
//! | --- | --- |
//! | [`base`] | `module/base.py` — `ModuleViewSet`, `ModuleLinkViewSet`, `ModuleUserPropertiesEndpoint`, `ModuleFavoriteViewSet` |
//! | [`archive`] | `module/archive.py` — `ModuleArchiveUnarchiveEndpoint` |
//! | [`issue`] | `module/issue.py` — `ModuleIssueViewSet` |
//! | [`workspace`] | `workspace/module.py` — `WorkspaceModulesEndpoint` |
//!
//! `+ GET …/modules/{module_id}/issues/{issue_id}/` is answered by the
//! runtime's `BaseViewSet` guard (405), so it registers nothing.
//!
//! The write verbs of `ModuleViewSet` (103/104), `ModuleUserPropertiesEndpoint`
//! (112) and `ModuleArchiveUnarchiveEndpoint` (113, plus the 114/115 routes
//! whose handler signature the URL does not satisfy) live in [`write`] and its
//! SQL catalog [`write_queries`].
//!
//! Set B — module issues (105/106/107), module links (108/109) and module
//! favourites (110/111) — lives in `write_issue_module`, `write_link` and
//! `write_favorite`, with the catalog [`write_issue_queries`]. Route 105
//! (`/projects/{project_id}/issues/{issue_id}/modules/`) is path-wise in the
//! `issues` route group, so `issues/mod.rs` registers that action.

use plane_drf::registry::{Registry, RegistryError};

pub mod archive;
pub mod base;
pub mod issue;
pub mod queries;
pub mod workspace;
pub mod write;
pub mod write_issue_queries;
pub mod write_queries;

// Set B: module issues (106/107), module links (108/109) and module
// favourites (110/111).
mod write_favorite;
pub(crate) mod write_issue_module;
mod write_link;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    // Module core, set A: `POST`, `PATCH`, `DELETE` (103/104), the
    // user-properties `PATCH` (112), archive/unarchive (113) and the two
    // archived-module routes the same view answers with a family 500
    // (114/115). `PUT` is the `BaseViewSet` guard's 405.
    registry
        .viewset("plane.app.views.module.base.ModuleViewSet")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?
        .action("create", write::create)?
        // `PUT` is refused by the `BaseViewSet` guard (405); registering the
        // action is what makes the guard reachable.
        .action("update", crate::cycles::base::guard_only)?
        .action("partial_update", write::partial_update)?
        .action("destroy", write::destroy)?;
    registry
        .viewset("plane.app.views.module.issue.ModuleIssueViewSet")?
        .action("list", issue::list)?
        // `GET …/modules/{module_id}/issues/{issue_id}/`: the guard answers 405.
        .action("retrieve", crate::cycles::base::guard_only)?
        // Set B: `POST modules/{module_id}/issues/` (106) and `DELETE
        // modules/{module_id}/issues/{issue_id}/` (107). Route 105
        // (`issues/{issue_id}/modules/`) belongs to the `issues` route group
        // and is registered there.
        .action(
            "create_module_issues",
            write_issue_module::create_module_issues,
        )?
        .action("destroy", write_issue_module::destroy)?;
    registry
        .viewset("plane.app.views.module.base.ModuleLinkViewSet")?
        .action("list", base::links_list)?
        .action("retrieve", base::links_retrieve)?
        // Set B: module links (108/109), `PUT` included.
        .action("create", write_link::create)?
        .action("update", write_link::update)?
        .action("partial_update", write_link::partial_update)?
        .action("destroy", write_link::destroy)?;
    registry
        .viewset("plane.app.views.module.base.ModuleFavoriteViewSet")?
        .action("list", base::favorites_list)?
        // Set B: favourites (110/111).
        .action("create", write_favorite::create)?
        .action("destroy", write_favorite::destroy)?;
    registry
        .api_view("plane.app.views.module.base.ModuleUserPropertiesEndpoint")?
        .method(http::Method::GET, base::user_properties_get)?
        .method(http::Method::PATCH, write::user_properties_patch)?;
    registry
        .api_view("plane.app.views.module.archive.ModuleArchiveUnarchiveEndpoint")?
        .method(http::Method::GET, archive::get)?
        .method(http::Method::POST, write::archive_post)?
        .method(http::Method::DELETE, write::archive_delete)?;
    registry
        .api_view("plane.app.views.workspace.module.WorkspaceModulesEndpoint")?
        .method(http::Method::GET, workspace::get)?;
    Ok(())
}
