// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `stickies` (`deploy/route-groups.yaml`): Workspace stickies.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/stickies/**`
//!
//! One file per Python module under `plane/app/views/`:
//!
//! | Rust | Python |
//! | --- | --- |
//! | [`base`] | `workspace/sticky.py` — `WorkspaceStickyViewSet` |
//!
//! `GET …/stickies/` is ported; `GET …/stickies/{pk}/` is answered by the
//! runtime's `BaseViewSet` guard (405) and registers a handler that can never
//! run, exactly like the module/cycle viewsets do.

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod queries;
pub mod write;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.workspace.sticky.WorkspaceStickyViewSet")?
        .action("list", base::list)?
        // `GET …/stickies/{pk}/`: `retrieve` is a DRF mixin action and the
        // viewset's only permission class is `IsAuthenticated`, so the guard
        // answers 405 (`plane/app/views/base.py:103-198`).
        .action("retrieve", base::guard_only)?
        .action("create", write::create)?
        .action("partial_update", write::partial_update)?
        .action("destroy", write::destroy)?;
    Ok(())
}
