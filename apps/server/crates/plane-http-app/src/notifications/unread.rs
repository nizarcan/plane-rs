// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.notification.base.UnreadNotificationEndpoint.get`
//! (`plane/app/views/notification/base.py:201-234`).
//!
//! Two counts over the same rows: unread, unarchived, unsnoozed notifications
//! excluding `sender__icontains="mentioned"`, and the same including it. The
//! view is `use_read_replica = True`, which `plane-drf` applies from
//! `routes.json`.
//!
//! The body is `{"total_unread_notifications_count": …, "mention_unread_notifications_count": …}`
//! in that order, built by the view (not a serializer) and rendered by DRF's
//! `JSONRenderer`, so `plane-query`'s `Json` supplies the key order.

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::json::Json;

use super::queries;
use crate::issues::lists::list_common::{client, kwarg_text, user_id};

/// Register the route.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.notification.base.UnreadNotificationEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

/// `UnreadNotificationEndpoint.get`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(queries::UNREAD_COUNTS.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_one(&statement, &[&user, &slug])
            .await
            .map_err(ViewError::from)?;
        let total: i64 = row.get(0);
        let mention: i64 = row.get(1);

        let body = Json::object(vec![
            ("total_unread_notifications_count", Json::Int(total)),
            ("mention_unread_notifications_count", Json::Int(mention)),
        ]);
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body.to_drf()))
    })
}
