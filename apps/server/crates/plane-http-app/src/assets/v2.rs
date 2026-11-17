// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `v2` asset reads of `plane/app/views/asset/v2.py`.
//!
//! Five of the six registered `GET`s answer `302` redirects to a presigned S3
//! URL (the `Location` bytes are `S3Storage.generate_presigned_url`'s output —
//! `crates/plane-storage` reproduces them exactly, DECISIONS.md D6). The sixth
//! is [`asset_check_get`], `{"exists": bool}`.
//!
//! Two of the handler methods also serve a second URL shape that does not
//! supply their trailing kwarg, so Python raises `TypeError` and
//! `handle_exception` answers `500` (`WorkspaceFileAssetEndpoint.get` on the
//! workspace collection and `ProjectAssetEndpoint.get` on the project
//! collection). Both are ported as they are; the differential scenario records
//! the same 500 for both targets.
//!
//! Every route except the `AllowAny` static one asks
//! `FileAsset.is_project_accessible_to` before it answers, and all of them reuse
//! the same four error bodies:
//!
//! | Body | Status | Python |
//! | --- | --- | --- |
//! | `{"error": "The required object does not exist."}` | 404 | `Model.DoesNotExist` (`base.py:318-322`) |
//! | `{"error": "The requested asset could not be found."}` | 404 | hand-built (`v2.py:508`) |
//! | `{"error": "You don't have access to this asset."}` | 403 | hand-built (`v2.py:501`) |
//! | `{"error": "Invalid entity type.", "status": false}` | 400 | hand-built (`v2.py:549`) |

use http::Method;
use plane_core::json::Object;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use crate::common::client;

use super::entities;
use super::queries;
use super::support;

/// The entity types `StaticFileAssetEndpoint.get` will serve
/// (`plane/app/views/asset/v2.py:542-547`).
const STATIC_ENTITY_TYPES: [&str; 4] = [
    "USER_AVATAR",
    "USER_COVER",
    "WORKSPACE_LOGO",
    "PROJECT_COVER",
];

/// Register the six classes' `GET`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.asset.v2.WorkspaceFileAssetEndpoint")?
        .method(Method::GET, workspace_file_asset_get)?;
    registry
        .api_view("plane.app.views.asset.v2.StaticFileAssetEndpoint")?
        .method(Method::GET, static_file_asset_get)?;
    registry
        .api_view("plane.app.views.asset.v2.ProjectAssetEndpoint")?
        .method(Method::GET, project_asset_get)?;
    registry
        .api_view("plane.app.views.asset.v2.AssetCheckEndpoint")?
        .method(Method::GET, asset_check_get)?;
    registry
        .api_view("plane.app.views.asset.v2.WorkspaceAssetDownloadEndpoint")?
        .method(Method::GET, workspace_asset_download_get)?;
    registry
        .api_view("plane.app.views.asset.v2.ProjectAssetDownloadEndpoint")?
        .method(Method::GET, project_asset_download_get)?;
    Ok(())
}

/// `WorkspaceFileAssetEndpoint.get` (`v2.py:495-522`).
fn workspace_file_asset_get<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("asset_id").is_none() {
            // `/api/assets/v2/workspaces/{slug}/` supplies no `asset_id`:
            // `get() missing 1 required positional argument: 'asset_id'` → 500.
            return Err(ViewError::Unknown(
                "TypeError: WorkspaceFileAssetEndpoint.get() missing 1 required \
                 positional argument: 'asset_id'"
                    .to_string(),
            ));
        }
        let slug = support::kwarg(ctx, "slug")?;
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;

        let row = client(ctx)?
            .query_opt(
                queries::FILE_ASSET_BY_ID_IN_WORKSPACE.sql,
                &[&asset_id, &slug],
            )
            .await?;
        let Some(asset) = decode_optional(row)? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if !accessible(ctx, &asset).await? {
            return entities::no_access();
        }
        if !asset.is_uploaded {
            return entities::asset_not_found();
        }
        support::presigned_redirect(ctx, &asset, "attachment", None).await
    })
}

/// `StaticFileAssetEndpoint.get` (`v2.py:530-564`) — `AllowAny`.
fn static_file_asset_get<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let statement = queries::FILE_ASSET_BY_ID.prepare(client(ctx)?).await?;
        let row = client(ctx)?.query_opt(&statement, &[&asset_id]).await?;
        let Some(asset) = decode_optional(row)? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if !asset.is_uploaded {
            return entities::asset_not_found();
        }
        let entity_type = asset.entity_type.as_deref().unwrap_or_default();
        if !STATIC_ENTITY_TYPES.contains(&entity_type) {
            return entities::invalid_entity_type();
        }
        // Inline only for an allow-listed image type, which `plane-storage`
        // re-checks against `settings.INLINE_SAFE_MIME_TYPES`
        // (`storage.py:133-137`); anything else is served as an attachment.
        let attributes = support::Attributes::parse(asset.attributes.as_str());
        let content_type = attributes.mime_type().map(str::to_string);
        support::presigned_redirect(ctx, &asset, "inline", content_type.as_deref()).await
    })
}

/// `ProjectAssetEndpoint.get` (`v2.py:720-747`).
fn project_asset_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("pk").is_none() {
            // `/api/assets/v2/workspaces/{slug}/projects/{project_id}/` supplies
            // no `pk`: `get() missing 1 required positional argument: 'pk'` →
            // 500.
            return Err(ViewError::Unknown(
                "TypeError: ProjectAssetEndpoint.get() missing 1 required positional \
                 argument: 'pk'"
                    .to_string(),
            ));
        }
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let pk = support::uuid_kwarg(ctx, "pk")?;

        let row = client(ctx)?
            .query_opt(
                queries::FILE_ASSET_IN_PROJECT.sql,
                &[&slug, &project_id, &pk],
            )
            .await?;
        let Some(asset) = decode_optional(row)? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if !accessible(ctx, &asset).await? {
            return entities::no_access();
        }
        if !asset.is_uploaded {
            return entities::asset_not_found();
        }
        support::presigned_redirect(ctx, &asset, "attachment", None).await
    })
}

/// `AssetCheckEndpoint.get` (`v2.py:832-837`).
fn asset_check_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let row = client(ctx)?
            .query_opt(
                queries::FILE_ASSET_EXISTS_IN_WORKSPACE.sql,
                &[&asset_id, &slug],
            )
            .await?;
        // `exists = asset is not None and asset.is_project_accessible_to(user)`
        let exists = match decode_optional(row)? {
            Some(asset) => accessible(ctx, &asset).await?,
            None => false,
        };
        support::json_object(200, &Object::new().field("exists", exists))
    })
}

/// `WorkspaceAssetDownloadEndpoint.get` (`v2.py:962-990`).
fn workspace_asset_download_get<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let row = client(ctx)?
            .query_opt(
                queries::FILE_ASSET_UPLOADED_IN_WORKSPACE.sql,
                &[&asset_id, &slug],
            )
            .await?;
        // The view catches `DoesNotExist` itself and answers its own 404.
        let Some(asset) = decode_optional(row)? else {
            return entities::asset_not_found();
        };
        if !accessible(ctx, &asset).await? {
            return entities::no_access();
        }
        support::presigned_redirect(ctx, &asset, "attachment", None).await
    })
}

/// `ProjectAssetDownloadEndpoint.get` (`v2.py:997-1025`).
fn project_asset_download_get<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let row = client(ctx)?
            .query_opt(
                queries::FILE_ASSET_UPLOADED_IN_PROJECT.sql,
                &[&asset_id, &slug, &project_id],
            )
            .await?;
        let Some(asset) = decode_optional(row)? else {
            return entities::asset_not_found();
        };
        if !accessible(ctx, &asset).await? {
            return entities::no_access();
        }
        support::presigned_redirect(ctx, &asset, "attachment", None).await
    })
}

/// `FileAsset.is_project_accessible_to(request.user)` for one row.
async fn accessible(ctx: &ViewCtx<'_>, asset: &FileAsset) -> Result<bool, ViewError> {
    let client = client(ctx)?;
    let user_id = support::user_id(ctx)?;
    let flags = support::accessible_assets(client, user_id, std::slice::from_ref(asset)).await?;
    Ok(flags.first().copied().unwrap_or(false))
}

/// Decode one `FileAsset` row, if the statement produced one.
fn decode_optional(row: Option<tokio_postgres::Row>) -> Result<Option<FileAsset>, ViewError> {
    match row {
        Some(row) => Ok(Some(FileAsset::from_row(&row, 0)?)),
        None => Ok(None),
    }
}
