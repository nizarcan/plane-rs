// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/quick_link.py`: the caller's quick links.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/quick-links/` | `QuickLinkViewSet.list` | `quick_link.py:60-65` |
//! | `GET /api/workspaces/{slug}/quick-links/{pk}/` | `QuickLinkViewSet.retrieve` | `quick_link.py:45-52` |
//!
//! Both statements are one query: the `workspace__slug` filter is a join, and
//! the serializer reads only this row's columns.

use plane_core::error::APPLICATION_JSON;
use plane_db::models::workspace::WorkspaceUserLink;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::fields::PgJson;
use plane_serializers::generated::app_workspace::WorkspaceUserLinkSerializer;
use uuid::Uuid;

use crate::common::client;
use crate::workspaces::queries;
use crate::workspaces::rows;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.workspace.quick_link.QuickLinkViewSet")?
        .action("list", list)?
        .action("retrieve", retrieve)?;
    Ok(())
}

/// `WorkspaceUserLinkSerializer` (`plane/app/serializers/workspace.py:157-162`).
fn serializer(model: &WorkspaceUserLink) -> WorkspaceUserLinkSerializer<'_> {
    WorkspaceUserLinkSerializer {
        id: model.id,
        created_at: model.created_at,
        updated_at: model.updated_at,
        deleted_at: model.deleted_at,
        title: model.title.as_deref(),
        url: &model.url,
        metadata: Some(PgJson::from(&model.metadata)),
        created_by: model.created_by_id,
        updated_by: model.updated_by_id,
        workspace: model.workspace_id,
        project: model.project_id,
        owner: model.owner_id,
    }
}

/// `QuickLinkViewSet.list` (`quick_link.py:61-65`).
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::QUICK_LINKS_FOR_WORKSPACE.sql)
            .await?;
        let rows = client.query(&statement, &[&slug, &user]).await?;
        let mut decoded = Vec::with_capacity(rows.len());
        for row in &rows {
            decoded.push(WorkspaceUserLink::from_row(row, 0)?);
        }
        let serializers: Vec<_> = decoded.iter().map(serializer).collect();
        let body = rows::render_plain_list(ctx, &serializers)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `QuickLinkViewSet.retrieve` (`quick_link.py:46-52`): a missing link is this
/// view's own `{"error": "Quick link not found."}`, not the family 404.
fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(ViewError::Key);
        };
        // `get(pk=…)` on a `UUIDField`: an unparsable value is Django's
        // `ValidationError`, i.e. the family's 400 (not the 404 below).
        let Ok(pk) = Uuid::parse_str(pk) else {
            return Err(ViewError::Validation);
        };
        let client = client(ctx)?;
        let statement = client.prepare_cached(queries::QUICK_LINK_BY_ID.sql).await?;
        let Some(row) = client.query_opt(&statement, &[&slug, &user, &pk]).await? else {
            return rows::error_body(404, "Quick link not found.");
        };
        let model = WorkspaceUserLink::from_row(&row, 0)?;
        let body = rows::render_plain(ctx, &serializer(&model))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
