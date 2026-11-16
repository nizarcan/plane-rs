// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `meta` (`deploy/route-groups.yaml`): Static reference data
//! (timezones, Unsplash proxy).
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/timezones/`
//! * `/api/unsplash/`
//!
//! `timezones` is the worked example of `apps/server/HANDLERS.md`: a read with
//! no database, no serializer and one cache decorator, ported end to end —
//! including Django's page cache (`plane_cache::page`, DECISIONS.md D5a).
//!
//! `unsplash` is **not** ported and stays on Django. It reads the
//! `UNSPLASH_ACCESS_KEY` instance configuration and, when a key is set, calls
//! `https://api.unsplash.com` with `requests` and returns Unsplash's own JSON
//! verbatim (`plane/app/views/external/base.py:215-243`). Porting it needs the
//! outbound HTTP client and its SSRF guards (`DECISIONS.md` D9, Phase 5), and
//! a port that returned `[]` without them would be a stub that only looks
//! right on a deployment with no key configured.

use plane_drf::registry::{Registry, RegistryError};

pub mod queries;
pub mod timezones;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    timezones::register(registry)?;
    Ok(())
}
