// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The public reads that only need a board (and, for the anchor route, the
//! project it points at):
//!
//! * `GET /api/public/workspaces/{slug}/project-boards/` —
//!   `WorkspaceProjectDeployBoardEndpoint` (`plane/space/views/project.py:28-51`,
//!   route 237), a **ported 500**: the URL binds `slug` but `get` takes
//!   `anchor` (`urls/intake.py:31` vs `views/project.py:31`), so
//!   `View.dispatch` raises `TypeError` before the body runs, and the second
//!   bug (`:32` stores `DeployBoard.objects.filter(...).values_list`, the
//!   method, then reads `.workspace` off it) is unreachable. The family's
//!   `handle_exception` turns the `TypeError` into
//!   `{"error": "Something went wrong please try again later"}`
//!   (`space/views/base.py:117-120`). Recorded as `500×3` in
//!   `scenarios/read/COVERAGE.md:212`; ported as-is per the brief.
//! * `GET /api/public/workspaces/{slug}/projects/{project_id}/anchor/` —
//!   `WorkspaceProjectAnchorEndpoint` (`project.py:54-62`, route 249):
//!   `.get(workspace__slug, project_id, entity_identifier=project_id,
//!   entity_name="project")` → `DeployBoardSerializer`, or the family 404.
//!
//! The intake-issue list/retrieve of routes 234-236 live here too, because
//! they are the same shape of board read: [`intake_list`] and
//! [`intake_retrieve`].

use http::Method;
use plane_db::models::deploy_board::DeployBoard;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use super::meta::render_board;
use super::ok;
use super::queries;
use crate::common::client;

/// Register 237, 249 and the intake reads.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.space.views.project.WorkspaceProjectDeployBoardEndpoint")?
        .method(Method::GET, workspace_project_deploy_board)?;
    registry
        .api_view("plane.space.views.project.WorkspaceProjectAnchorEndpoint")?
        .method(Method::GET, workspace_project_anchor)?;
    registry
        .viewset("plane.space.views.intake.IntakeIssuePublicViewSet")?
        .action("list", intake_list)?
        .action("retrieve", intake_retrieve)?;
    Ok(())
}

/// `WorkspaceProjectDeployBoardEndpoint.get` — always a 500 (see the module
/// docs).
///
/// The Python's `TypeError` is raised by `View.dispatch` **after**
/// `initial()`, so the permission check and the `DeployBoardAnchorMixin` step
/// have already run; the runtime performs both before calling a handler, and
/// the handler then answers the family's 500 body.
fn workspace_project_deploy_board<'a>(
    _ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `request.method == "GET"` is the only method registered, so the
        // missing `anchor` parameter is always the `TypeError` Django hits:
        // `WorkspaceProjectDeployBoardEndpoint.get() missing 1 required
        // positional argument: 'anchor'` (the URL binds `slug` instead,
        // `space/urls/intake.py:31`).
        Err(ViewError::Unknown(
            "WorkspaceProjectDeployBoardEndpoint.get() missing 1 required positional argument: \
             'anchor' (the URL binds `slug`)"
                .to_owned(),
        ))
    })
}

/// `WorkspaceProjectAnchorEndpoint.get` (`plane/space/views/project.py:57-62`).
fn workspace_project_anchor<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id: Uuid = ctx
            .kwarg("project_id")
            .ok_or(ViewError::Key)?
            .parse()
            .map_err(|_| ViewError::Validation)?;
        let client = client(ctx)?;
        let statement = queries::BOARD_BY_WORKSPACE_PROJECT.prepare(client).await?;
        let row = client.query_opt(&statement, &[&slug, &project_id]).await?;
        let Some(row) = row else {
            // `DeployBoard.DoesNotExist` → `ObjectDoesNotExist` in the family
            // handler (`space/views/base.py:103-107`).
            return Err(ViewError::ObjectDoesNotExist);
        };
        let board = DeployBoard::from_row(&row, 0)?;
        let body = render_board(ctx, &board).await?;
        Ok(ok(body))
    })
}

// ---------------------------------------------------------------------------
// IntakeIssuePublicViewSet (routes 234-236, GET halves)
// ---------------------------------------------------------------------------

/// `IntakeIssuePublicViewSet.list` (`plane/space/views/intake.py:57-106`).
fn intake_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { super::intake_read::list(ctx).await })
}

/// `IntakeIssuePublicViewSet.retrieve` (`plane/space/views/intake.py:250-270`).
fn intake_retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { super::intake_read::retrieve(ctx).await })
}
