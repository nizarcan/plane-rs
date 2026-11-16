// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `issues` (`deploy/route-groups.yaml`): Work items (issues), drafts, comments, links, attachments and reactions.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/issues/`
//! * `/api/workspaces/*/work-items/**`
//! * `/api/workspaces/*/draft-issues/**`
//! * `/api/workspaces/*/draft-to-issue/**`
//! * `/api/workspaces/*/export-issues/`
//! * `/api/workspaces/*/projects/*/issues/`
//! * `/api/workspaces/*/projects/*/issues/**`
//! * `/api/workspaces/*/projects/*/issues-detail/`
//! * `/api/workspaces/*/projects/*/v2/issues/**`
//! * `/api/workspaces/*/projects/*/work-items/**`
//! * `/api/workspaces/*/projects/*/archived-issues/`
//! * `/api/workspaces/*/projects/*/deleted-issues/`
//! * `/api/workspaces/*/projects/*/issue-dates/`
//! * `/api/workspaces/*/projects/*/bulk-delete-issues/`
//! * `/api/workspaces/*/projects/*/bulk-archive-issues/`
//! * `/api/workspaces/*/projects/*/comments/*/reactions/**`
//! * `/api/workspaces/*/projects/*/user-properties/`
//!
//! Nothing is registered yet: every route of this group still answers Django,
//! so `register` is empty. It stops being empty one route at a time, each one
//! only after its `scenarios/read/` (and, for a write, its write) scenario is
//! clean against Django — `apps/server/HANDLERS.md` is the procedure. Once a
//! group is fully ported, the orchestrator flips its `Owner` column in
//! `ROUTES.md`; a domain agent never edits that file.

use plane_drf::registry::{Registry, RegistryError};

pub mod lists;
pub mod queries;
mod sub_resources;

/// Register this group's handlers, in the order they were ported.
///
/// `apps/server/HANDLERS.md` has the step list. Return the first
/// [`RegistryError`]: a route that cannot be registered must fail startup
/// rather than fall back to a 501 nobody notices.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    sub_resources::register_sub_resources(registry)?;
    lists::register_lists(registry)?;
    // `…/projects/{project_id}/user-properties/` matches this group's
    // `projects/*/user-properties/` selector, so the route is registered here
    // even though the handler lives with the other project settings writes.
    registry
        .api_view("plane.app.views.issue.base.ProjectUserDisplayPropertyEndpoint")?
        .method(
            http::Method::PATCH,
            crate::projects::settings_write::user_properties,
        )?;
    Ok(())
}
