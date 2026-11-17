// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! OAuth routes of the `auth` group — set C of `plans/AUTH_FLOWS.md` §3.
//!
//! | Routes | Rust | Python |
//! | --- | --- | --- |
//! | 365/366, 369/370, 373/374, 385/386 | [`initiate`], [`callback`] | `views/app/{google,github,gitlab,gitea}.py` |
//! | 367/368, 371/372, 375/376, 387/388 | [`initiate`], [`callback`] | `views/space/{google,github,gitlab,gitea}.py` |
//!
//! The provider implementations live in `plane_auth::auth_flow::providers`
//! (`plane-auth/src/auth_flow/providers/`), because the claim mapping is
//! provider logic with no HTTP surface and the unit tests record the Python
//! classes directly. Everything the views share with the rest of `/auth/` —
//! state validation, the adapter, the redirect builders, the error codes — is
//! `plane_auth::auth_flow`.
//!
//! ## Non-obvious behaviour, ported as it is
//!
//! * `validate_oauth_state` runs **before** the `code` check and deletes the
//!   stored state on success (`utils/oauth_state.py:9-19`), so a callback with
//!   a valid state and no code consumes it.
//! * The callback reads `next_path` from the session before the state check and
//!   never clears it.
//! * The app Gitea view builds every URL with `urljoin(session["host"], …)`
//!   (`views/app/gitea.py:45,76,101`), so an absolute session host survives.
//! * The space Gitea view concatenates the base and the path with no separator
//!   on success but with `?` on every error (`views/space/gitea.py:49,101`).
//! * No view reads `IS_*_ENABLED`; the providers gate on their own credentials
//!   (`AUTH_FLOWS.md` §2i).

use plane_drf::registry::{Registry, RegistryError};

pub mod callback;
pub mod initiate;
pub mod support;

/// Register this flow's handlers, in `routes.json`'s order.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    initiate::register(registry)?;
    callback::register(registry)?;
    Ok(())
}
