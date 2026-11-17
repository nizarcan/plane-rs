// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `auth` (`deploy/route-groups.yaml`): the `/auth/` surface.
//!
//! | Rust | Python | Routes |
//! | --- | --- | --- |
//! | [`oauth`] | `authentication/views/{app,space}/{google,github,gitlab,gitea}.py` | 365-376, 385-388 |
//!
//! The group is split by flow, one module per plan set (`plans/AUTH_FLOWS.md`
//! §3): set A owns the session routes, set B the magic-code routes and set C
//! the OAuth routes below. Each set adds its module and its line to
//! [`register`].

use plane_drf::registry::{Registry, RegistryError};

pub mod oauth;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    oauth::register(registry)
}
