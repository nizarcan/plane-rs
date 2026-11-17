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
//! `GET`/`PUT`/`PATCH …/cycles/{cycle_id}/cycle-issues/{issue_id}/` and `GET
//! …/user-favorite-cycles/` are answered by the runtime's `BaseViewSet` guard
//! (`405 {"detail": "Method \"GET\" not allowed."}` for the first, the verb
//! named in each), so they register [`base::guard_only`] and nothing else.

use plane_drf::registry::{Registry, RegistryError};

pub mod archive;
pub mod base;
pub mod issue;
pub mod issue_list;
pub mod queries;
pub mod workspace;
pub mod write;
pub mod write_queries;

// Set B: cycle issues and the transfer snapshot (routes 33, 34, 38).
mod write_issue;
pub mod write_issue_queries;
mod write_transfer;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.cycle.base.CycleViewSet")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?
        .action("create", write::create)?
        // `PUT` is refused by the `BaseViewSet` guard (405); registering the
        // action is what makes the guard reachable.
        .action("update", base::guard_only)?
        .action("partial_update", write::partial_update)?
        .action("destroy", write::destroy)?;
    registry
        .api_view("plane.app.views.cycle.base.CycleDateCheckEndpoint")?
        .method(http::Method::POST, write::date_check)?;
    registry
        .viewset("plane.app.views.cycle.issue.CycleIssueViewSet")?
        .action("list", issue::list)?
        // `GET`/`PUT`/`PATCH …/cycle-issues/{issue_id}/`: the viewset implements
        // none of the three actions, declares only `IsAuthenticated`
        // (`plane/app/views/cycle/issue.py:40-49`) and `routes.json` 34's
        // `guard_actions` marks `retrieve`, `update` and `partial_update`
        // mixin-provided, so the `BaseViewSet` guard answers
        // `405 {"detail": "Method \"PUT\" not allowed."}` — also for a
        // non-member, because the permission class passes first.
        .action("retrieve", base::guard_only)?
        .action("update", base::guard_only)?
        .action("partial_update", base::guard_only)?;
    registry
        .viewset("plane.app.views.cycle.base.CycleFavoriteViewSet")?
        // `GET …/user-favorite-cycles/`: `ListModelMixin` with no authorizing
        // permission class, so the guard answers 405.
        .action("list", base::guard_only)?
        .action("create", write::favorite_create)?
        .action("destroy", write::favorite_destroy)?;
    registry
        .api_view("plane.app.views.cycle.base.CycleUserPropertiesEndpoint")?
        .method(http::Method::GET, base::user_properties_get)?
        .method(http::Method::PATCH, write::user_properties_patch)?;
    registry
        .api_view("plane.app.views.cycle.base.CycleProgressEndpoint")?
        .method(http::Method::GET, base::progress)?;
    registry
        .api_view("plane.app.views.cycle.base.CycleAnalyticsEndpoint")?
        .method(http::Method::GET, base::analytics)?;
    registry
        .api_view("plane.app.views.cycle.archive.CycleArchiveUnarchiveEndpoint")?
        .method(http::Method::GET, archive::get)?
        .method(http::Method::POST, write::archive_post)?
        .method(http::Method::DELETE, write::archive_delete)?;
    registry
        .api_view("plane.app.views.workspace.cycle.WorkspaceCyclesEndpoint")?
        .method(http::Method::GET, workspace::get)?;
    // Set B: `POST cycle-issues/` (33), `DELETE cycle-issues/{issue_id}/` (34)
    // and `POST transfer-issues/` (38).
    write_issue::register(registry)?;
    write_transfer::register(registry)?;
    Ok(())
}
