// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IssueAttachmentV2Endpoint.get` — the two `GET`s of
//! `plane/app/views/issue/attachment.py:203-241`.
//!
//! One method serves both routes:
//!
//! | Route | Result |
//! | --- | --- |
//! | `…/issues/{issue_id}/attachments/` | the issue's uploaded `ISSUE_ATTACHMENT` assets |
//! | `…/issues/{issue_id}/attachments/{pk}/` | `302` to a presigned URL, or a 400/404 |
//!
//! Both start with `issue_hidden_from_guest` (`plane/app/permissions/base.py:106`):
//! a *restricted* project guest — role GUEST on a project whose
//! `guest_view_all_features` is off — may only reach the attachments of issues
//! they created, and anything else is
//! `403 {"error": "You are not allowed to view this issue"}`. The other v2 asset
//! routes use `FileAsset.is_project_accessible_to` instead; this one does not
//! call it at all (`attachment.py:203-241`), so a plain project guest who passes
//! `issue_hidden_from_guest` sees the whole attachment list.
//!
//! The list serializer is built **without** a request context, so its `asset`
//! field is the storage name and not an absolute URI (see
//! [`super::support::issue_attachment_serializer`]); `asset_url` is the model
//! property (`plane/db/models/asset.py:83-104`) and needs the asset's workspace
//! slug, which the statement joins in instead of Django's one query per row.

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use crate::common::{RequestRender, client, render_list};

use super::queries;
use super::support;

/// Register the class's `GET` (both routes share it, as in the Python).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.attachment.IssueAttachmentV2Endpoint")?
        .method(Method::GET, get)?;
    Ok(())
}

/// `IssueAttachmentV2Endpoint.get` (`attachment.py:203-241`).
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let issue_id = support::uuid_kwarg(ctx, "issue_id")?;
        let user_id = support::user_id(ctx)?;
        let client = client(ctx)?;

        if support::issue_hidden_from_guest(client, slug, project_id, issue_id, user_id).await? {
            return support::json_object(
                403,
                &Object::new().field("error", "You are not allowed to view this issue"),
            );
        }

        if ctx.kwarg("pk").is_none() {
            // The collection route: every uploaded attachment of the issue.
            let rows = client
                .query(
                    queries::ISSUE_ATTACHMENTS_BY_ISSUE.sql,
                    &[&issue_id, &slug, &project_id],
                )
                .await?;
            let (assets, slugs) = decode_with_slug(&rows)?;
            let urls: Vec<Option<String>> = assets
                .iter()
                .zip(&slugs)
                .map(|(asset, slug)| support::asset_url(asset.entity_type.as_deref(), slug, asset))
                .collect();
            let serializers: Vec<_> = assets
                .iter()
                .zip(&urls)
                .map(|(asset, url)| support::issue_attachment_serializer(asset, url.as_deref()))
                .collect();
            let body = render_list(&serializers, &RequestRender::from_ctx(ctx).render_ctx())?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        }

        // The detail route.
        let asset_id = support::uuid_kwarg(ctx, "pk")?;
        let row = client
            .query_opt(
                queries::ISSUE_ATTACHMENT_BY_ID.sql,
                &[&asset_id, &slug, &project_id, &issue_id],
            )
            .await?;
        let Some(row) = row else {
            // `FileAsset.objects.get(...)` → `DoesNotExist` → 404
            // `{"error": "The required object does not exist."}`.
            return Err(ViewError::ObjectDoesNotExist);
        };
        let asset = FileAsset::from_row(&row, 0)?;
        if !asset.is_uploaded {
            return support::json_object(
                400,
                &Object::new()
                    .field("error", "The asset is not uploaded.")
                    .field("status", false),
            );
        }
        support::presigned_redirect(ctx, &asset, "attachment", None).await
    })
}

/// Decode `FileAsset` rows plus the joined `workspaces.slug` column.
fn decode_with_slug(
    rows: &[tokio_postgres::Row],
) -> Result<(Vec<FileAsset>, Vec<String>), ViewError> {
    let mut assets = Vec::with_capacity(rows.len());
    let mut slugs = Vec::with_capacity(rows.len());
    for row in rows {
        assets.push(FileAsset::from_row(row, 0)?);
        // `FileAsset::COLUMN_COUNT` columns precede the join's extra column.
        slugs.push(row.try_get(FileAsset::COLUMN_COUNT)?);
    }
    Ok((assets, slugs))
}
