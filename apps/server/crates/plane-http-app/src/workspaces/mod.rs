// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `workspaces` (`deploy/route-groups.yaml`): Workspaces, members, invitations, themes and workspace-level preferences.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/`
//! * `/api/workspaces/{slug}/`
//! * `/api/workspace-slug-check/`
//! * `/api/workspaces/*/members/**`
//! * `/api/workspaces/*/project-members/`
//! * `/api/workspaces/*/workspace-members/**`
//! * `/api/workspaces/*/invitations/**`
//! * `/api/workspaces/*/workspace-themes/**`
//! * `/api/workspaces/*/user-stats/**`
//! * `/api/workspaces/*/user-activity/**`
//! * `/api/workspaces/*/user-profile/**`
//! * `/api/workspaces/*/user-issues/**`
//! * `/api/workspaces/*/user-properties/`
//! * `/api/workspaces/*/user-favorites/**`
//! * `/api/workspaces/*/quick-links/**`
//! * `/api/workspaces/*/home-preferences/**`
//! * `/api/workspaces/*/recent-visits/`
//! * `/api/workspaces/*/sidebar-preferences/`
//!
//! ## Layout
//!
//! One file per Python module under `plane/app/views/workspace/`, same stem:
//!
//! | File | Python |
//! | --- | --- |
//! | [`base`] | `base.py` — slug check, `WorkSpaceViewSet`, `WorkspaceThemeViewSet` |
//! | [`activity_export`] | `base.py` — `ExportWorkspaceUserActivityEndpoint` (route 203, the synchronous CSV) |
//! | [`favorite`] | `favorite.py` — favourites and favourite folders |
//! | [`home`] | `home.py` — home widget preferences |
//! | [`invite`] | `invite.py` — invitations and the invite landing page |
//! | [`member`] | `member.py` — members, own membership, project-member map |
//! | [`quick_link`] | `quick_link.py` — quick links |
//! | [`recent_visit`] | `recent_visit.py` — recent visits |
//! | [`user`] | `user.py` — user stats, activity, profile, issues, properties |
//! | [`user_preference`] | `user_preference.py` — sidebar preferences |
//! | [`rows`] | shared row decoding and response helpers |
//!
//! ## Three routes are 500s upstream, and stay 500s
//!
//! `GET /user-favorites/{favorite_id}/` and
//! `GET /home-preferences/{key}/` resolve to methods whose signatures lack the
//! extra kwarg, so DRF raises `TypeError` and the family handler answers
//! `500 {"error": "Something went wrong please try again later"}`. They are
//! ported as they are, with the bug named in the handler. The write verbs have
//! three more of the same kind: 212 `PATCH`/`DELETE` and 213 `POST`
//! ([`write_favorite`]).
//!
//! `GET /api/workspaces/` never reaches its handler: `allow_permission` reads
//! `kwargs["slug"]` on a route without a `slug`
//! (`plane/app/permissions/base.py:62`), so the runtime answers
//! `400 {"error": "The required key does not exist."}`. The handler is ported
//! anyway.

use plane_drf::registry::{Registry, RegistryError};

pub mod activity_export;
pub mod base;
pub mod favorite;
pub mod home;
pub mod home_write;
pub mod invite;
pub mod invite_write;
pub mod member;
pub mod member_write;
pub mod queries;
pub mod quick_link;
pub mod quick_link_write;
pub mod recent_visit;
pub mod rows;
pub mod theme_write;
pub mod user;
pub mod user_preference;
pub mod user_preference_write;
pub mod user_properties_write;
pub mod write;
pub mod write_a_queries;
pub mod write_b_queries;
pub mod write_c_queries;
pub mod write_favorite;
pub mod write_favorite_queries;
pub mod write_queries;

/// Register this group's handlers, in the order they were ported.
///
/// `apps/server/HANDLERS.md` has the step list. Return the first
/// [`RegistryError`]: a route that cannot be registered must fail startup
/// rather than fall back to a 501 nobody notices.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    base::register(registry)?;
    theme_write::register(registry)?;
    write::register(registry)?;
    invite::register(registry)?;
    invite_write::register(registry)?;
    member::register(registry)?;
    member_write::register(registry)?;
    favorite::register(registry)?;
    write_favorite::register(registry)?;
    quick_link::register(registry)?;
    quick_link_write::register(registry)?;
    home::register(registry)?;
    home_write::register(registry)?;
    recent_visit::register(registry)?;
    user_preference::register(registry)?;
    user_preference_write::register(registry)?;
    user::register(registry)?;
    // set C's `PATCH /user-properties/` (207).
    user_properties_write::register(registry)?;
    // `POST /user-activity/{user_id}/export/` (203), the synchronous CSV.
    registry
        .api_view("plane.app.views.workspace.base.ExportWorkspaceUserActivityEndpoint")?
        .method(http::Method::POST, activity_export::post)?;
    Ok(())
}
