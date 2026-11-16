// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.project.base.DeployBoardViewSet` — the two read actions.
//!
//! Python: `apps/api/plane/app/views/project/base.py:539-561`.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `GET …/projects/{project_id}/project-deploy-boards/` | `list` | `:555-561` |
//! | `GET …/projects/{project_id}/project-deploy-boards/{pk}/` | `retrieve` | `:544-553` |
//!
//! `list` looks the board up by `entity_name="project"` +
//! `entity_identifier=project_id` and `.first()`, so a project with no board
//! serialises `None`. `DeployBoardSerializer(None).data` is `get_initial()`:
//! every field is read-only (`required=False`) and has no default, so DRF
//! raises `SkipField` for each and the body is `{}` — ported as it is
//! (`response_initial`).
//!
//! `retrieve` goes through `get_object()`, which also filters `project_id`, so a
//! board of another project in the same workspace is a 404.

use std::collections::HashSet;

use plane_core::error::APPLICATION_JSON;
use plane_db::models::deploy_board::DeployBoard;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::PgJson;
use plane_serializers::generated::app_project::DeployBoardSerializer;

use crate::common::{client, lookup, render};
use crate::projects::joined;
use crate::projects::lite;
use crate::projects::queries;

/// `DeployBoardSerializer(None).data`, in the serializer's field order.
const EMPTY_BOARD: &[u8] = b"{\"deleted_at\":null,\"is_comments_enabled\":false,\"is_reactions_enabled\":false,\"is_votes_enabled\":false,\"view_props\":null,\"is_activity_enabled\":false,\"is_disabled\":false,\"created_by\":null,\"updated_by\":null,\"intake\":null}";

/// `DeployBoardViewSet.list` (`plane/app/views/project/base.py:555-561`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;

        let statement = queries::PROJECT_DEPLOY_BOARD.prepare(client(ctx)?).await?;
        let row = client(ctx)?
            .query_opt(&statement, &[&project_id, &slug])
            .await?;
        let Some(row) = row else {
            // `DeployBoardSerializer(None).data` is `get_initial()`: the fields
            // with a model default or with `allow_null=True` render their
            // initial value, the read-only ones raise `SkipField`
            // (`rest_framework/serializers.py:364-372,530-551`). For this
            // serializer that is a constant object.
            return Ok(ViewResponse::raw(
                200,
                APPLICATION_JSON,
                EMPTY_BOARD.to_vec(),
            ));
        };
        let board = DeployBoard::from_row(&row, 0)?;
        let body = render_board(ctx, &board, &row, 18).await?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `DeployBoardViewSet.retrieve` (`plane/app/views/project/base.py:544-553`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;

        let row = client(ctx)?
            .query_opt(
                queries::PROJECT_DEPLOY_BOARD_ONE.sql,
                &[&project_id, &project_id, &slug, &pk],
            )
            .await?;
        let Some(row) = row else {
            // DRF's `get_object()`: `Http404("No Deploy Board matches …")`.
            return crate::projects::not_found::no_match("DeployBoard");
        };
        let board = DeployBoard::from_row(&row, 0)?;
        let body = render_board(ctx, &board, &row, 18).await?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `DeployBoardSerializer(board)`, with the nested project/workspace columns
/// taken from the board's own statement (both relations are joined in
/// `queries::PROJECT_DEPLOY_BOARD*`) and their `*_url` properties from one
/// batched asset query.
///
/// `project` is nullable: a board with no project renders
/// `"project_details": null`, which is what Django's lazy accessor produces for
/// a null FK.
async fn render_board(
    ctx: &ViewCtx<'_>,
    board: &DeployBoard,
    row: &tokio_postgres::Row,
    start: usize,
) -> Result<Vec<u8>, ViewError> {
    let client = client(ctx)?;
    let mut project = if board.project_id.is_some() {
        Some(joined::project_at(row, start)?)
    } else {
        None
    };
    let mut workspace = Some(joined::workspace_at(row, start + 7)?);
    let mut asset_ids = HashSet::new();
    if let Some(project) = &project {
        asset_ids.extend(project.cover_image_asset_id);
    }
    if let Some(workspace) = &workspace {
        asset_ids.extend(workspace.logo_asset_id);
    }
    let urls = lite::load_asset_urls(client, &asset_ids).await?;
    if let Some(project) = &mut project {
        project.cover_image_url = lite::asset_or_text(
            project.cover_image_asset_id,
            project.cover_image.clone().unwrap_or_default(),
            &urls,
        )?;
    }
    if let Some(workspace) = &mut workspace {
        workspace.logo_url = lite::asset_or_text(
            workspace.logo_asset_id,
            workspace.logo.clone().unwrap_or_default(),
            &urls,
        )?;
    }

    let project_lite = project.as_ref().map(lite::LiteProject::lite);
    let workspace_lite = workspace.as_ref().map(lite::LiteWorkspace::lite);

    let serializer = DeployBoardSerializer {
        id: board.id,
        project_details: project_lite.as_ref(),
        workspace_detail: workspace_lite.as_ref(),
        created_at: board.created_at,
        updated_at: board.updated_at,
        deleted_at: board.deleted_at,
        entity_identifier: board.entity_identifier,
        entity_name: board.entity_name.as_deref(),
        anchor: &board.anchor,
        is_comments_enabled: board.is_comments_enabled,
        is_reactions_enabled: board.is_reactions_enabled,
        is_votes_enabled: board.is_votes_enabled,
        view_props: Some(PgJson::from(&board.view_props)),
        is_activity_enabled: board.is_activity_enabled,
        is_disabled: board.is_disabled,
        created_by: board.created_by_id,
        updated_by: board.updated_by_id,
        workspace: board.workspace_id,
        project: board.project_id,
        intake: board.intake_id,
    };
    render(ctx, &serializer)
}
