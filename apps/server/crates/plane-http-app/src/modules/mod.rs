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

use plane_drf::registry::{Registry, RegistryError};

pub mod archive;
pub mod base;
pub mod issue;
pub mod queries;
pub mod workspace;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.module.base.ModuleViewSet")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?;
    registry
        .viewset("plane.app.views.module.issue.ModuleIssueViewSet")?
        .action("list", issue::list)?
        // `GET …/modules/{module_id}/issues/{issue_id}/`: the guard answers 405.
        .action("retrieve", crate::cycles::base::guard_only)?;
    registry
        .viewset("plane.app.views.module.base.ModuleLinkViewSet")?
        .action("list", base::links_list)?
        .action("retrieve", base::links_retrieve)?;
    registry
        .viewset("plane.app.views.module.base.ModuleFavoriteViewSet")?
        .action("list", base::favorites_list)?;
    registry
        .api_view("plane.app.views.module.base.ModuleUserPropertiesEndpoint")?
        .method(http::Method::GET, base::user_properties_get)?;
    registry
        .api_view("plane.app.views.module.archive.ModuleArchiveUnarchiveEndpoint")?
        .method(http::Method::GET, archive::get)?;
    registry
        .api_view("plane.app.views.workspace.module.WorkspaceModulesEndpoint")?
        .method(http::Method::GET, workspace::get)?;
    Ok(())
}
