// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `users-me` (`deploy/route-groups.yaml`): the signed-in user's
//! own profile, settings, sessions, activity and tokens.
//!
//! Routes, grouped by the Python module each handler ports
//! (`HANDLERS.md`'s naming rule):
//!
//! | Rust module | Python |
//! | --- | --- |
//! | [`user_base`] | `plane/app/views/user/base.py` |
//! | [`write_email`] | `plane/app/views/user/base.py` (165/166) |
//! | [`write_deactivate`] | `plane/app/views/user/base.py` (162 DELETE) |
//! | [`workspace_base`] | `plane/app/views/workspace/base.py` |
//! | [`workspace_user`] | `plane/app/views/workspace/user.py` |
//! | [`workspace_invite`] | `plane/app/views/workspace/invite.py` |
//! | [`project_invite`] | `plane/app/views/project/invite.py` |
//! | [`project_member`] | `plane/app/views/project/member.py` |
//! | [`api`] | `plane/app/views/api.py` |
//!
//! Everything is registered here one route at a time; a route whose handler is
//! not registered keeps answering the pipeline's 501 and stays on Django. The
//! group's routes and their verification state are listed in `ROUTES.md`,
//! which only the orchestrator edits.
//!
//! ## Side effects
//!
//! None of the 18 `GET`s writes a row, publishes a task or touches the
//! response cache. Three of them add `Cache-Control: private, max-age=12`
//! (`@cache_control`) and the runtime supplies the rest of the headers.

use plane_drf::registry::{Registry, RegistryError};

pub mod api;
pub mod project_invite;
pub mod project_member;
pub mod queries;
pub mod support;
pub mod user_base;
pub mod workspace_base;
pub mod workspace_invite;
pub mod workspace_user;
pub mod write_deactivate;
pub mod write_email;
pub mod write_queries;

/// Register this group's handlers, in the order they were ported.
///
/// Return the first [`RegistryError`]: a route that cannot be registered must
/// fail startup rather than fall back to a 501 nobody notices.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    user_base::register(registry)?;
    workspace_base::register(registry)?;
    workspace_user::register(registry)?;
    workspace_invite::register(registry)?;
    project_invite::register(registry)?;
    project_member::register(registry)?;
    api::register(registry)?;
    Ok(())
}
