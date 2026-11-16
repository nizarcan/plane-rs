// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `cycles` (`deploy/route-groups.yaml`): Cycles and cycle issues.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/cycles/`
//! * `/api/workspaces/*/projects/*/cycles/**`
//! * `/api/workspaces/*/projects/*/archived-cycles/**`
//! * `/api/workspaces/*/projects/*/user-favorite-cycles/**`
//!
//! One file per Python module under `plane/app/views/`:
//!
//! | Rust | Python |
//! | --- | --- |
//! | [`base`] | `cycle/base.py` — `CycleViewSet`, `CycleUserPropertiesEndpoint`, `CycleProgressEndpoint`, `CycleAnalyticsEndpoint` |
//! | [`archive`] | `cycle/archive.py` — `CycleArchiveUnarchiveEndpoint` |
//! | [`issue`] | `cycle/issue.py` — `CycleIssueViewSet` |
//! | [`workspace`] | `workspace/cycle.py` — `WorkspaceCyclesEndpoint` |
//!
//! `+ GET …/cycles/{cycle_id}/cycle-issues/{issue_id}/` and `+ GET
//! …/user-favorite-cycles/` are answered by the runtime's `BaseViewSet` guard
//! (`405 {"detail": "Method \"GET\" not allowed."}`), so they register nothing.

use plane_drf::registry::{Registry, RegistryError};

pub mod archive;
pub mod base;
pub mod issue;
pub mod issue_list;
pub mod queries;
pub mod workspace;

// Set B: cycle issues and the transfer snapshot (routes 33, 34, 38).
mod write_issue;
pub mod write_issue_queries;
mod write_transfer;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.cycle.base.CycleViewSet")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?;
    registry
        .viewset("plane.app.views.cycle.issue.CycleIssueViewSet")?
        .action("list", issue::list)?
        // `GET …/cycle-issues/{issue_id}/`: the guard answers 405.
        .action("retrieve", base::guard_only)?;
    registry
        .viewset("plane.app.views.cycle.base.CycleFavoriteViewSet")?
        // `GET …/user-favorite-cycles/`: `ListModelMixin` with no authorizing
        // permission class, so the guard answers 405.
        .action("list", base::guard_only)?;
    registry
        .api_view("plane.app.views.cycle.base.CycleUserPropertiesEndpoint")?
        .method(http::Method::GET, base::user_properties_get)?;
    registry
        .api_view("plane.app.views.cycle.base.CycleProgressEndpoint")?
        .method(http::Method::GET, base::progress)?;
    registry
        .api_view("plane.app.views.cycle.base.CycleAnalyticsEndpoint")?
        .method(http::Method::GET, base::analytics)?;
    registry
        .api_view("plane.app.views.cycle.archive.CycleArchiveUnarchiveEndpoint")?
        .method(http::Method::GET, archive::get)?;
    registry
        .api_view("plane.app.views.workspace.cycle.WorkspaceCyclesEndpoint")?
        .method(http::Method::GET, workspace::get)?;
    // Set B: `POST cycle-issues/` (33), `DELETE cycle-issues/{issue_id}/` (34)
    // and `POST transfer-issues/` (38).
    write_issue::register(registry)?;
    write_transfer::register(registry)?;
    Ok(())
}
