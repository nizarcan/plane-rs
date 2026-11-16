// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.notification.base.UserNotificationPreferenceEndpoint.get`
//! (`plane/app/views/notification/base.py:296-304`).
//!
//! `UserNotificationPreference.objects.get(user=request.user)` — one row per
//! user in a healthy database, but the fixture (and any workspace-scoped
//! preference that was created in addition to the account-wide one) has two,
//! and `MultipleObjectsReturned` is **not** an `ObjectDoesNotExist`: it escapes
//! Plane's family handler's specific branches and lands on the generic
//! `500 {"error": "Something went wrong please try again later"}`
//! (`plane/app/views/base.py:235-239`). `LIMIT 2` distinguishes the three
//! cases; a miss stays `404 {"error": "The required object does not exist."}`.

use plane_core::error::APPLICATION_JSON;
use plane_db::models::notification::UserNotificationPreference;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_notification::UserNotificationPreferenceSerializer;

use super::queries;
use super::render::plain_context;
use crate::issues::lists::list_common::{client, user_id};

/// Register the route.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.notification.base.UserNotificationPreferenceEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

/// `UserNotificationPreferenceEndpoint.get`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(queries::USER_NOTIFICATION_PREFERENCE.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&user])
            .await
            .map_err(ViewError::from)?;
        let row = match rows.as_slice() {
            [] => return Err(ViewError::ObjectDoesNotExist),
            [row] => row,
            // `MultipleObjectsReturned` reaches the generic 500.
            _ => {
                return Err(ViewError::Unknown(
                    "UserNotificationPreference matching query does not exist.".to_owned(),
                ));
            }
        };
        let preference = UserNotificationPreference::from_row(row, 0)?;

        let serializer = UserNotificationPreferenceSerializer {
            id: preference.id,
            created_at: preference.created_at,
            updated_at: preference.updated_at,
            deleted_at: preference.deleted_at,
            property_change: preference.property_change,
            state_change: preference.state_change,
            comment: preference.comment,
            mention: preference.mention,
            issue_completed: preference.issue_completed,
            created_by: preference.created_by_id,
            updated_by: preference.updated_by_id,
            user: preference.user_id,
            workspace: preference.workspace_id,
            project: preference.project_id,
        };
        let context = plain_context(ctx);
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            plane_serializers::render(&serializer, &context)?,
        ))
    })
}
