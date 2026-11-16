// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `FileAssetEndpoint` and `UserAssetsEndpoint` — the two "v1-style" asset
//! reads (`apps/api/plane/app/views/asset/base.py`).
//!
//! Both classes answer two URL shapes with the same method, and one of the two
//! shapes does not supply the method's positional arguments:
//!
//! | Route | Python | Result |
//! | --- | --- | --- |
//! | `/api/workspaces/{slug}/file-assets/` | `get(self, request, workspace_id, asset_key)` | `TypeError` → 500 |
//! | `/api/workspaces/file-assets/{workspace_id}/{asset_key}/` | same | the list |
//! | `/api/users/file-assets/` | `get(self, request, asset_key)` | `TypeError` → 500 |
//! | `/api/users/file-assets/{asset_key}/` | same | one object or the error body |
//!
//! The 500 is not a guess: `BaseAPIView.dispatch` catches the `TypeError`, logs
//! it and `handle_exception` falls through to
//! `{"error": "Something went wrong please try again later"}`
//! (`plane/app/views/base.py:297-348`), which is what the differential
//! scenario records for those two routes. Ported as it is, with the same two
//! status codes.

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_asset::FileAssetSerializer;

use crate::common::{RequestRender, client};

use super::queries;
use super::support;

/// Register both classes' `GET`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.asset.base.FileAssetEndpoint")?
        .method(Method::GET, file_asset_get)?;
    registry
        .api_view("plane.app.views.asset.base.UserAssetsEndpoint")?
        .method(Method::GET, user_assets_get)?;
    Ok(())
}

/// `FileAssetEndpoint.get` (`base.py:27-38`), on both of its routes.
fn file_asset_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let Some(asset_key) = ctx.kwarg("asset_key") else {
            // `get() missing 2 required positional arguments: 'workspace_id'
            // and 'asset_key'` — the collection route supplies neither.
            return Err(ViewError::Unknown(
                "TypeError: FileAssetEndpoint.get() missing 2 required positional \
                 arguments: 'workspace_id' and 'asset_key'"
                    .to_string(),
            ));
        };
        let workspace_id = super::support::uuid_kwarg(ctx, "workspace_id")?;
        // `asset_key = str(workspace_id) + "/" + asset_key` (`base.py:28`).
        let key = format!("{workspace_id}/{asset_key}");

        let client = client(ctx)?;
        let rows = client
            .query(queries::FILE_ASSETS_BY_ASSET.sql, &[&key])
            .await?;
        let assets = decode(&rows)?;
        let user_id = super::support::user_id(ctx)?;
        let accessible = support::accessible_assets(client, user_id, &assets).await?;
        let files: Vec<&FileAsset> = assets
            .iter()
            .zip(&accessible)
            .filter_map(|(asset, &accessible)| accessible.then_some(asset))
            .collect();

        if files.is_empty() {
            // `{"error": "Asset key does not exist", "status": False}`, 200.
            return support::json_object(
                200,
                &Object::new()
                    .field("error", "Asset key does not exist")
                    .field("status", false),
            );
        }

        // `FileAssetSerializer(files, context={"request": request}, many=True)`
        // (`base.py:32-33`): the absolute URI is DRF's `FileField`.
        let absolute: Vec<String> = files
            .iter()
            .map(|asset| support::build_absolute_uri(ctx, &asset.asset))
            .collect();
        let serializers: Vec<FileAssetSerializer<'_>> = files
            .iter()
            .zip(&absolute)
            .map(|(asset, url)| support::file_asset_serializer(asset, url))
            .collect();
        // `Response({"data": serializer.data, "status": True})`.
        let prepared = RequestRender::from_ctx(ctx);
        let body = plane_serializers::render(
            &support::DataEnvelope { data: &serializers },
            &prepared.render_ctx(),
        )?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `UserAssetsEndpoint.get` (`base.py:87-96`), on both of its routes.
fn user_assets_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let Some(asset_key) = ctx.kwarg("asset_key") else {
            // `get() missing 1 required positional argument: 'asset_key'`.
            return Err(ViewError::Unknown(
                "TypeError: UserAssetsEndpoint.get() missing 1 required positional \
                 argument: 'asset_key'"
                    .to_string(),
            ));
        };
        let user_id = super::support::user_id(ctx)?;

        let rows = client(ctx)?
            .query(
                queries::USER_FILE_ASSETS_BY_ASSET.sql,
                &[&asset_key, &user_id],
            )
            .await?;

        if rows.is_empty() {
            return support::json_object(
                200,
                &Object::new()
                    .field("error", "Asset key does not exist")
                    .field("status", false),
            );
        }

        // `FileAssetSerializer(files, context={"request": request})` — *without*
        // `many=True` (`base.py:90`). DRF renders one object from a `QuerySet`
        // and the first field's `get_attribute` raises `AttributeError`
        // (`rest_framework/serializers.py:530-551`), which Plane's
        // `handle_exception` turns into a 500. Ported as it is.
        Err(ViewError::Unknown(
            "AttributeError: 'QuerySet' object has no attribute 'id' (FileAssetSerializer \
             was given a queryset without many=True)"
                .to_string(),
        ))
    })
}

/// Decode `FileAsset::COLUMNS` from a result set.
fn decode(rows: &[tokio_postgres::Row]) -> Result<Vec<FileAsset>, ViewError> {
    rows.iter()
        .map(|row| FileAsset::from_row(row, 0).map_err(ViewError::from))
        .collect()
}
