// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `AUTH_FLOWS.md` set A — the session routes under `/auth/`.
//!
//! | Route | Python | File |
//! | --- | --- | --- |
//! | 352-355 | `views/{app,space}/email.py` | [`email`] |
//! | 356-357 | `views/{app,space}/signout.py` | [`signout`] |
//! | 358 | `views/common.py:29-36` (`CSRFTokenEndpoint`) | [`csrf`] |
//! | 377-378 | `views/{app,space}/check.py` | [`check`] |
//! | 379-384 | `views/{app,space}/password_management.py`, `views/common.py:48-139` | [`password`] |
//!
//! Sets B (`magic.rs`) and C (`oauth/`) register their own routes through this
//! module's [`register`]; the module list is the merge point.

pub mod check;
pub mod csrf;
pub mod email;
pub mod password;
pub mod queries;
pub mod signout;
pub(crate) mod support;

use plane_drf::registry::{Registry, RegistryError};

/// Register every session route this set owns.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    email::register(registry)?;
    signout::register(registry)?;
    csrf::register(registry)?;
    check::register(registry)?;
    password::register(registry)?;
    Ok(())
}
