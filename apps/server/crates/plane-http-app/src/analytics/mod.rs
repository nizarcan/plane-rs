// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `analytics` (`deploy/route-groups.yaml`): Workspace and project analytics endpoints.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/analytics/`
//! * `/api/workspaces/*/analytic-view/**`
//! * `/api/workspaces/*/saved-analytic-view/**`
//! * `/api/workspaces/*/export-analytics/`
//! * `/api/workspaces/*/default-analytics/`
//! * `/api/workspaces/*/project-stats/`
//! * `/api/workspaces/*/advance-analytics/`
//! * `/api/workspaces/*/advance-analytics-stats/`
//! * `/api/workspaces/*/advance-analytics-charts/`
//! * `/api/workspaces/*/projects/*/advance-analytics/`
//! * `/api/workspaces/*/projects/*/advance-analytics-stats/`
//! * `/api/workspaces/*/projects/*/advance-analytics-charts/`
//!
//! Nothing is registered yet: every route of this group still answers Django,
//! so `register` is empty. It stops being empty one route at a time, each one
//! only after its `scenarios/read/` (and, for a write, its write) scenario is
//! clean against Django — `apps/server/HANDLERS.md` is the procedure. Once a
//! group is fully ported, the orchestrator flips its `Owner` column in
//! `ROUTES.md`; a domain agent never edits that file.

use plane_drf::registry::{Registry, RegistryError};

pub mod queries;

/// Register this group's handlers, in the order they were ported.
///
/// `apps/server/HANDLERS.md` has the step list. Return the first
/// [`RegistryError`]: a route that cannot be registered must fail startup
/// rather than fall back to a 501 nobody notices.
pub fn register(_registry: &mut Registry) -> Result<(), RegistryError> {
    Ok(())
}
