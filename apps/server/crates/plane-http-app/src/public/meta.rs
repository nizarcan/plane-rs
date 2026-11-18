// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ProjectMetaDataEndpoint` and `ProjectDeployBoardPublicSettingsEndpoint`.
//!
//! Python:
//!
//! * `plane/space/views/meta.py:16-32` — `GET anchor/{anchor}/meta/`
//!   (route 246). The board is a `.get(anchor, entity_name="project")`, whose
//!   miss the view catches itself and answers `{"error": "Project is not
//!   published"}`; the project is a second `.get(id=entity_identifier)` with
//!   the same body. The serializer is the **space** `ProjectLiteSerializer`
//!   (7 fields, `plane/space/serializer/project.py:11-25`).
//! * `plane/space/views/project.py:19-25` — `GET anchor/{anchor}/settings/`
//!   (route 247). The board is `.get(anchor, entity_name="project")`; a miss
//!   is an uncaught `DeployBoard.DoesNotExist` → the family's 404 body. The
//!   serializer is the **app** `DeployBoardSerializer`
//!   (`plane/app/serializers/project.py:…`), which nests the app
//!   `ProjectLiteSerializer` and `WorkspaceLiteSerializer`.
//!
//! Both are `AllowAny` and both run through `BaseAPIView`, i.e. the space
//! family's `handle_exception` and the `DeployBoardAnchorMixin` 404 (which the
//! runtime performs before the handler).

use http::Method;
use plane_db::models::deploy_board::DeployBoard;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::PgJson;
use plane_serializers::generated::app_project::{DeployBoardSerializer, ProjectLiteSerializer};
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use plane_serializers::generated::space_project::ProjectLiteSerializer as SpaceProjectLiteSerializer;

use super::board;
use super::queries;
use crate::common::client;
use crate::issues::sub_resources::sub_common::{ProjectLiteRow, WorkspaceLiteRow};
use crate::public::{ok, plain_context};
use crate::write::error_body;

/// Register `ProjectMetaDataEndpoint.get` and
/// `ProjectDeployBoardPublicSettingsEndpoint.get`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.space.views.meta.ProjectMetaDataEndpoint")?
        .method(Method::GET, get_meta)?;
    registry
        .api_view("plane.space.views.project.ProjectDeployBoardPublicSettingsEndpoint")?
        .method(Method::GET, get_settings)?;
    Ok(())
}

/// `ProjectMetaDataEndpoint.get` (`plane/space/views/meta.py:19-32`).
fn get_meta<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let client = client(ctx)?;
        // The view catches `DoesNotExist` and answers its own body
        // (`meta.py:21-23`), so this is the `.first()` form with the view's 404
        // rather than the family one.
        let Some(board) = board::published_board(client, anchor).await? else {
            return Ok(error_body(404, "Project is not published"));
        };
        // `project_id = deploy_board.entity_identifier; Project.objects.get(id=project_id)`
        // (`meta.py:26-27`). A NULL `entity_identifier` makes the lookup
        // `id IS NULL`, a miss (`meta.py:28-29`).
        let statement = queries::PROJECT_LITE_SPACE.prepare(client).await?;
        let Some(project_id) = board.entity_identifier else {
            return Ok(error_body(404, "Project is not published"));
        };
        let Some(row) = client.query_opt(&statement, &[&project_id]).await? else {
            return Ok(error_body(404, "Project is not published"));
        };
        let icon_prop: Option<plane_db::JsonText> = row.get(4);
        let serializer = SpaceProjectLiteSerializer {
            id: row.get(0),
            identifier: row.get(1),
            name: row.get(2),
            cover_image: row.get(3),
            icon_prop: icon_prop.as_ref().map(PgJson::from_json_text),
            emoji: row.get(5),
            description: row.get(6),
        };
        let prepared = plain_context(ctx);
        let body = plane_serializers::render(&serializer, &prepared).map_err(ViewError::from)?;
        Ok(ok(body))
    })
}

/// `ProjectDeployBoardPublicSettingsEndpoint.get`
/// (`plane/space/views/project.py:22-25`).
fn get_settings<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let client = client(ctx)?;
        let board = board::require_published_board(client, anchor).await?;
        let body = render_board(ctx, &board).await?;
        Ok(ok(body))
    })
}

/// `DeployBoardSerializer(project_deploy_board).data` — the shared renderer of
/// routes 247 and 249.
///
/// `project_details` and `workspace_detail` are `select_related`-style FK
/// reads (`project.py:13-14`); a NULL FK renders `null`
/// (`rest_framework/serializers.py` `to_representation`).
pub async fn render_board(ctx: &ViewCtx<'_>, board: &DeployBoard) -> Result<Vec<u8>, ViewError> {
    let client = client(ctx)?;
    let project = match board.project_id {
        Some(project_id) => {
            let statement = queries::PROJECT_LITE_APP.prepare(client).await?;
            let row = client.query_opt(&statement, &[&project_id]).await?;
            row.as_ref()
                .map(|row| ProjectLiteRow::from_row(row, 0))
                .transpose()?
                .flatten()
        }
        None => None,
    };
    let workspace = {
        let statement = queries::WORKSPACE_LITE.prepare(client).await?;
        let row = client.query_opt(&statement, &[&board.workspace_id]).await?;
        row.as_ref()
            .map(|row| WorkspaceLiteRow::from_row(row, 0))
            .transpose()?
            .flatten()
    };
    let project_serializer: Option<ProjectLiteSerializer<'_>> =
        project.as_ref().map(ProjectLiteRow::serializer);
    let workspace_serializer: Option<WorkspaceLiteSerializer<'_>> =
        workspace.as_ref().map(WorkspaceLiteRow::serializer);
    let serializer = DeployBoardSerializer {
        id: board.id,
        project_details: project_serializer.as_ref(),
        workspace_detail: workspace_serializer.as_ref(),
        created_at: board.created_at,
        updated_at: board.updated_at,
        deleted_at: board.deleted_at,
        entity_identifier: board.entity_identifier,
        entity_name: board.entity_name.as_deref(),
        anchor: &board.anchor,
        is_comments_enabled: board.is_comments_enabled,
        is_reactions_enabled: board.is_reactions_enabled,
        is_votes_enabled: board.is_votes_enabled,
        view_props: Some(PgJson::from_json_text(&board.view_props)),
        is_activity_enabled: board.is_activity_enabled,
        is_disabled: board.is_disabled,
        created_by: board.created_by_id,
        updated_by: board.updated_by_id,
        workspace: board.workspace_id,
        project: board.project_id,
        intake: board.intake_id,
    };
    let prepared = plain_context(ctx);
    plane_serializers::render(&serializer, &prepared).map_err(ViewError::from)
}
