// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/api.py` — the caller's API tokens.
//!
//! * `GET /api/users/api-tokens/` — `ApiTokenEndpoint.get` with no `pk`
//! * `GET /api/users/api-tokens/{pk}/` — `ApiTokenEndpoint.get` with `pk`
//! * 226/227 `POST`/`PATCH`/`DELETE` — [`super::write_tokens`] (set A)
//!
//! `APITokenReadSerializer` (the read side) excludes `token` and computes
//! `is_active` from `expired_at` (`plane/app/serializers/api.py:28-38`).

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::api::APIToken;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_api::APITokenReadSerializer;
use plane_serializers::value::DrfValue;
use uuid::Uuid;

use super::queries;
use super::support;
use super::write_tokens;

/// Register this module's handlers.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.api.ApiTokenEndpoint")?
        .method(Method::GET, get)?
        // Set A: 226 `POST` and 227 `PATCH`/`DELETE`, plus the two signature
        // 500s (`PATCH`/`DELETE` on 226, `POST` on 227).
        .method(Method::POST, write_tokens::post)?
        .method(Method::PATCH, write_tokens::patch)?
        .method(Method::DELETE, write_tokens::delete)?;
    Ok(())
}

/// `APITokenReadSerializer.get_is_active` (`plane/app/serializers/api.py:35-38`).
fn is_active(token: &APIToken) -> bool {
    match token.expired_at {
        None => true,
        Some(expired_at) => plane_core::clock::now() < expired_at,
    }
}

fn token_serializer(token: &APIToken) -> APITokenReadSerializer<'_> {
    APITokenReadSerializer {
        id: token.id,
        is_active: DrfValue::Bool(is_active(token)),
        created_at: token.created_at,
        updated_at: token.updated_at,
        deleted_at: token.deleted_at,
        label: &token.label,
        description: &token.description,
        last_used: token.last_used,
        user_type: i64::from(token.user_type),
        expired_at: token.expired_at,
        created_by: token.created_by_id,
        updated_by: token.updated_by_id,
        user: token.user_id,
        workspace: token.workspace_id,
    }
}

/// `ApiTokenEndpoint.get` (`plane/app/views/api.py:41-49`).
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user_id = support::user_id(ctx)?;
        let client = crate::common::client(ctx)?;
        if let Some(pk) = ctx.kwarg("pk") {
            let pk =
                Uuid::parse_str(pk).map_err(|_| ViewError::Api(plane_core::ApiError::NotFound))?;
            let statement = queries::USER_API_TOKEN_BY_ID.prepare(client).await?;
            let row = client
                .query_opt(&statement, &[&pk, &user_id])
                .await?
                .ok_or(ViewError::ObjectDoesNotExist)?;
            let token = APIToken::from_row(&row, 0)?;
            let body = crate::common::render(ctx, &token_serializer(&token))?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        }

        let statement = queries::USER_API_TOKENS.prepare(client).await?;
        let rows = client.query(&statement, &[&user_id]).await?;
        let mut tokens = Vec::with_capacity(rows.len());
        for row in &rows {
            tokens.push(APIToken::from_row(row, 0)?);
        }
        let serializers: Vec<APITokenReadSerializer> =
            tokens.iter().map(token_serializer).collect();
        let prepared = crate::common::RequestRender::from_ctx(ctx);
        let body = crate::common::render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
