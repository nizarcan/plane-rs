// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/v1/users/me/` (route 314).
//!
//! Python: `plane/api/views/user.py:34-41` — `UserLiteSerializer(request.user)`.
//! With `APIKeyAuthentication` `request.user` is `api_token.user`, a full
//! `users` row loaded through the forward-FK descriptor, which uses the *base*
//! manager (no `deleted_at` filter; `users` has no such column).
//!
//! The route's `use_read_replica` is `false` (`routes.json`), so the runtime's
//! connection is the primary and the row read matches Django's.

use deadpool_postgres::Client;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;
use plane_serializers::generated::api_user::UserLiteSerializer;
use uuid::Uuid;

use crate::users_queries as queries;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.user.UserEndpoint")?
        .method(http::Method::GET, me)?;
    Ok(())
}

/// `UserEndpoint.get` (`plane/api/views/user.py:34-41`).
pub fn me<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;
        let row = load_user(client, user_id).await?;

        // `UserLiteSerializer.avatar_url` is `user.avatar_url`
        // (`plane/db/models/user.py:142-151`).
        let urls = plane_http_app::projects::lite::load_asset_urls(
            client,
            &row.avatar_asset_id.into_iter().collect(),
        )
        .await?;
        let avatar_url = plane_http_app::projects::lite::asset_or_text(
            row.avatar_asset_id,
            row.avatar.clone(),
            &urls,
        )?;

        let serializer = UserLiteSerializer {
            id: row.id,
            first_name: &row.first_name,
            last_name: &row.last_name,
            email: row.email.as_deref(),
            avatar: &row.avatar,
            avatar_url: avatar_url.as_deref(),
            display_name: &row.display_name,
        };
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))
            .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

struct UserRow {
    id: Uuid,
    first_name: String,
    last_name: String,
    email: Option<String>,
    avatar: String,
    avatar_asset_id: Option<Uuid>,
    display_name: String,
}

async fn load_user(client: &Client, user_id: Uuid) -> Result<UserRow, ViewError> {
    let row = client
        .query_opt(queries::USER_BY_ID.sql, &[&user_id])
        .await
        .map_err(ViewError::from)?;
    let Some(row) = row else {
        // The API token's FK guarantees the row exists; a miss is Django's
        // `RelatedObjectDoesNotExist` (a 500) but is unreachable in practice.
        return Err(ViewError::Unknown(
            "the API token's user does not exist".to_string(),
        ));
    };
    Ok(UserRow {
        id: row.get(0),
        first_name: row.get(1),
        last_name: row.get(2),
        email: row.get(3),
        avatar: row.get(4),
        avatar_asset_id: row.get(5),
        display_name: row.get(6),
    })
}
