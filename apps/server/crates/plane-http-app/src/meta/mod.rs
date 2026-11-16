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
//! `unsplash` is the proxy of `plane/app/views/external/base.py:215-243`. With
//! no `UNSPLASH_ACCESS_KEY` in the instance configuration it answers `[]`
//! (`Response([], 200)`); with a key it forwards to
//! `https://api.unsplash.com` through `plane_net`'s `UNSPLASH` preset
//! (DECISIONS.md D9) and returns Unsplash's own JSON and status. The key is read
//! the way `get_configuration_value` reads it, including the Fernet decryption
//! of an encrypted instance-configuration row.

use plane_drf::registry::{Registry, RegistryError};

pub mod queries;
pub mod timezones;
pub mod unsplash;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    timezones::register(registry)?;
    unsplash::register(registry)?;
    Ok(())
}
