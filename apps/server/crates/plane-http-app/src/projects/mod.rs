// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `projects` (`deploy/route-groups.yaml`): Projects, project
//! members, invitations, favourites and settings.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/projects/**`
//! * `/api/workspaces/*/project-identifiers/`
//! * `/api/workspaces/*/user-favorite-projects/**`
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `GET …/projects/` | `ProjectViewSet.list` | `plane/app/views/project/base.py:147` |
//! | `GET …/projects/details/` | `ProjectViewSet.list_detail` | `:102` |
//! | `GET …/projects/{pk}/` | `ProjectViewSet.retrieve` | `:226` |
//! | `GET …/project-identifiers/` | `ProjectIdentifierEndpoint.get` | `:446` |
//! | `GET …/projects/{project_id}/invitations/` | `ProjectInvitationsViewset.list` | `invite.py:57` |
//! | `GET …/projects/{project_id}/invitations/{pk}/` | `ProjectInvitationsViewset.retrieve` | `invite.py:61` |
//! | `GET …/projects/{project_id}/join/{pk}/` | `ProjectJoinEndpoint.get` | `invite.py:302` |
//! | `GET …/projects/{project_id}/members/` | `ProjectMemberViewSet.list` | `member.py:166` |
//! | `GET …/projects/{project_id}/members/{pk}/` | `ProjectMemberViewSet.retrieve` | `member.py:183` |
//! | `GET …/projects/{project_id}/project-members/me/` | `ProjectMemberUserEndpoint.get` | `member.py:378` |
//! | `GET …/projects/{project_id}/preferences/member/{member_id}/` | `ProjectMemberPreferenceEndpoint.get` | `member.py:434` |
//! | `GET …/user-favorite-projects/` | `ProjectFavoritesViewSet.list` (405) | `base.py:499` |
//! | `GET …/projects/{project_id}/project-deploy-boards/` | `DeployBoardViewSet.list` | `base.py:555` |
//! | `GET …/projects/{project_id}/project-deploy-boards/{pk}/` | `DeployBoardViewSet.retrieve` | `base.py:544` |
//!
//! `ProjectFavoritesViewSet.list` is DRF's `ListModelMixin`, reached through the
//! router but carrying no authorization; `BaseViewSet.initial` refuses it with
//! `MethodNotAllowed` before the handler runs
//! (`plane/app/views/base.py:182-198`). The registry still needs the action so
//! the route is reachable and the guard can answer 405; [`favorites_list`] is
//! unreachable by construction and says so if it is ever called.

use plane_core::error::ApiError;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

pub mod archive_write;
pub mod base;
pub mod board;
pub mod deploy_board_write;
pub mod favorite_write;
pub mod identifier;
pub mod invite;
pub mod invite_write;
pub mod joined;
pub mod lite;
pub mod member;
pub mod member_queries;
pub mod member_write;
pub mod not_found;
pub mod queries;
pub mod serializer;
pub mod settings_write;
pub mod write_queries;
pub mod write_support;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.project.base.ProjectViewSet")?
        .action("list", base::list)?
        .action("list_detail", base::list_detail)?
        .action("retrieve", base::retrieve)?;

    registry
        .api_view("plane.app.views.project.base.ProjectIdentifierEndpoint")?
        .method(http::Method::GET, identifier::get)?;

    registry
        .viewset("plane.app.views.project.invite.ProjectInvitationsViewset")?
        .action("list", invite::list)?
        .action("retrieve", invite::retrieve)?
        .action("create", invite_write::create)?
        .action("destroy", invite_write::destroy)?;

    registry
        .api_view("plane.app.views.project.invite.ProjectJoinEndpoint")?
        .method(http::Method::GET, invite::join)?
        .method(http::Method::POST, invite_write::join)?;

    registry
        .viewset("plane.app.views.project.member.ProjectMemberViewSet")?
        .action("list", member::list)?
        .action("retrieve", member::retrieve)?
        .action("create", member_write::create)?
        .action("partial_update", member_write::partial_update)?
        .action("destroy", member_write::destroy)?
        .action("leave", member_write::leave)?;

    registry
        .api_view("plane.app.views.project.member.ProjectMemberUserEndpoint")?
        .method(http::Method::GET, member::me)?;

    registry
        .api_view("plane.app.views.project.member.ProjectMemberPreferenceEndpoint")?
        .method(http::Method::GET, member::preference)?;

    // `GET` on `ProjectFavoritesViewSet` is refused by `BaseViewSet`'s guard
    // (`routes.json` records it in `viewset_guard.refused_methods`), so the
    // action exists but can never be dispatched to.
    registry
        .viewset("plane.app.views.project.base.ProjectFavoritesViewSet")?
        .action("list", favorites_list)?;
    favorite_write::register(registry)?;

    registry
        .viewset("plane.app.views.project.base.DeployBoardViewSet")?
        .action("list", board::list)?
        .action("retrieve", board::retrieve)?;
    deploy_board_write::register(registry)?;
    archive_write::register(registry)?;
    settings_write::register(registry)?;
    Ok(())
}

/// Unreachable: `BaseViewSet._resolved_action_is_authorized` returns 405 for
/// this action before dispatch. Kept explicit (not a silent 200) so a registry
/// or guard regression fails loudly instead of serving an unauthorized list.
fn favorites_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        Err(ViewError::Api(ApiError::PermissionDenied(format!(
            "{} must never be dispatched: BaseViewSet's guard answers 405",
            ctx.view
        ))))
    })
}
