// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two public asset `GET`s.
//!
//! Python: `plane/space/views/asset.py` — `EntityAssetEndpoint.get`
//! (`:46-85`), mounted twice:
//!
//! | Route | URL | Behaviour |
//! | --- | --- | --- |
//! | 255 | `assets/v2/anchor/{anchor}/` | **always 500** |
//! | 256 | `assets/v2/anchor/{anchor}/{pk}/` | 302 to a presigned URL |
//!
//! 255 is a ported Django bug: the collection URL binds only `anchor`, but
//! `get` declares `pk` (`:46`), so `View.dispatch`'s `handler(request, **kwargs)`
//! raises `TypeError: get() missing 1 required positional argument: 'pk'` and
//! the family `handle_exception` answers
//! `{"error": "Something went wrong please try again later"}`
//! (`space/views/base.py:117-120`). `scenarios/read/COVERAGE.md:228` records
//! the `500×3`.
//!
//! 256's body, in order:
//!
//! 1. `DeployBoard.objects.filter(anchor=anchor).first()` — **no**
//!    `entity_name` filter, so a page board answers too. A miss is the view's
//!    own `{"error": "Requested resource could not be found."}` (`:48-54`),
//!    which is a *different* body from every other view's
//!    (`board::requested_resource_not_found`).
//! 2. `FileAsset.objects.get(workspace_id=…, project_id=…, pk=…,
//!    entity_type__in=[ISSUE_DESCRIPTION, COMMENT_DESCRIPTION])` (`:57-65`).
//!    A miss is an uncaught `FileAsset.DoesNotExist` → the family 404 body.
//! 3. `asset.is_uploaded` false → `{"error": "The requested asset could not be
//!    found."}` 404 (`:67-72`).
//! 4. Otherwise `HttpResponseRedirect(S3Storage(request=request).
//!    generate_presigned_url(object_name=asset.asset, filename=
//!    asset.attributes.get("name"), disposition="inline", content_type=
//!    asset.attributes.get("type")))` (`:74-85`).
//!
//! The write verbs of both routes belong to set B: `255`/`256` POST, PATCH and
//! DELETE are not registered here, so they keep answering the pipeline's 501.

use http::Method;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::board;
use super::queries;
use crate::assets::support;
use crate::common::client;

/// Register the `GET` of both asset routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.space.views.asset.EntityAssetEndpoint")?
        .method(Method::GET, get)?;
    Ok(())
}

/// `EntityAssetEndpoint.get` (`plane/space/views/asset.py:46-85`).
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // The collection route binds no `pk`, which is the Python's `TypeError`
        // (`:46`): both URLs dispatch to this one view.
        let Some(raw_pk) = ctx.kwarg("pk") else {
            return Err(ViewError::Unknown(
                "EntityAssetEndpoint.get() missing 1 required positional argument: 'pk'".to_owned(),
            ));
        };
        let pk: uuid::Uuid = raw_pk.parse().map_err(|_| ViewError::Validation)?;
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let client = client(ctx)?;
        let Some(deploy_board) = board::board_by_anchor(client, anchor).await? else {
            return Ok(board::requested_resource_not_found());
        };
        let statement = queries::FILE_ASSET_FOR_BOARD.prepare(client).await?;
        let row = client
            .query_opt(
                &statement,
                &[&deploy_board.workspace_id, &deploy_board.project_id, &pk],
            )
            .await?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let asset = FileAsset::from_row(&row, 0)?;
        if !asset.is_uploaded {
            return Ok(crate::write::error_body(
                404,
                "The requested asset could not be found.",
            ));
        }
        let content_type = support::attribute_type(&asset);
        support::presigned_redirect(ctx, &asset, "inline", content_type.as_deref()).await
    })
}
