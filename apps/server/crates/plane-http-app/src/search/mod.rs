// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `search` (`deploy/route-groups.yaml`): Workspace search, entity search and issue search.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/search/`
//! * `/api/workspaces/*/entity-search/`
//! * `/api/workspaces/*/projects/*/search-issues/`
//!
//! One file per Python module under `plane/app/views/`:
//!
//! | Rust | Python |
//! | --- | --- |
//! | [`base`] | `search/base.py` — `GlobalSearchEndpoint`, `SearchEndpoint` |
//! | [`issue`] | `search/issue.py` — `IssueSearchEndpoint` |
//!
//! All three `GET`s are ported. `values` holds the `.values()`-row writer the
//! three share (they return raw dicts, not serializers).

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod issue;
pub mod queries;
pub mod values;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    base::register(registry)?;
    issue::register(registry)?;
    Ok(())
}
