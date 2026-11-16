// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `intake` (`deploy/route-groups.yaml`): Intake, inboxes and their issues.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/projects/*/intakes/**`
//! * `/api/workspaces/*/projects/*/intake-issues/**`
//! * `/api/workspaces/*/projects/*/inboxes/**`
//! * `/api/workspaces/*/projects/*/inbox-issues/**`
//! * `/api/workspaces/*/projects/*/intake-work-items/**`
//! * `/api/workspaces/*/projects/*/intake-state/`
//!
//! | Route | Registered | Python |
//! | --- | --- | --- |
//! | `GET …/intakes/` | `IntakeViewSet.list` | `plane/app/views/intake/base.py:74` |
//! | `GET …/intakes/{pk}/` | `IntakeViewSet.retrieve` (guard's 405) | `:79`/`base.py:103-198` |
//! | `GET …/inboxes/` | the same `IntakeViewSet.list` | `:74` |
//! | `GET …/inboxes/{pk}/` | the same guard-only `retrieve` | — |
//! | `GET …/intake-issues/` | `IntakeIssueViewSet.list` | `:178` |
//! | `GET …/intake-issues/{pk}/` | `IntakeIssueViewSet.retrieve` | `:507` |
//! | `GET …/inbox-issues/` | the same `list` | `:178` |
//! | `GET …/inbox-issues/{pk}/` | the same `retrieve` | `:507` |
//! | `GET …/intake-work-items/{work_item_id}/description-versions/` | `IntakeWorkItemDescriptionVersionEndpoint.get` | `:583` |
//! | `GET …/description-versions/{pk}/` | the same `get` | `:583` |
//! | `GET …/intake-state/` | `IntakeStateEndpoint.get` | `state/base.py:151` |
//!
//! Every write verb of these views stays on Django: they are part of the same
//! route rows and are ported in Phase 5.

use plane_drf::registry::{Registry, RegistryError};

pub mod asset;
pub mod base;
pub mod issue;
pub mod queries;
pub mod render;
pub mod state;
pub mod versions;

/// Register this group's handlers, in route order.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    base::register(registry)?;
    issue::register(registry)?;
    versions::register(registry)?;
    state::register(registry)?;
    Ok(())
}
