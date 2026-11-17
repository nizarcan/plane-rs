// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.notification.base.UserNotificationPreferenceEndpoint`
//! (`plane/app/views/notification/base.py:296-313`), route 122.
//!
//! | Method | Python | Handler |
//! | --- | --- | --- |
//! | `GET` | `get` (`:301-304`) | [`get`] |
//! | `PATCH` | `patch` (`:307-313`) | [`patch`] |
//!
//! `UserNotificationPreference.objects.get(user=request.user)` — one row per
//! user in a healthy database, but the fixture (and any workspace-scoped
//! preference that was created in addition to the account-wide one) has two,
//! and `MultipleObjectsReturned` is **not** an `ObjectDoesNotExist`: it escapes
//! Plane's family handler's specific branches and lands on the generic
//! `500 {"error": "Something went wrong please try again later"}`
//! (`plane/app/views/base.py:235-239`). `LIMIT 2` distinguishes the three
//! cases; a miss stays `404 {"error": "The required object does not exist."}`.
//! The lookup runs before `request.data` is touched (`:308` then `:309`), so a
//! malformed body on a zero-row user is the 404 and on a two-row user the 500.
//!
//! `patch` has no role check at all (`BaseAPIView` plus `IsAuthenticated`), and
//! `UserNotificationPreferenceSerializer`'s `Meta.read_only_fields` make
//! `user`, `workspace`, `project`, `created_by`, `updated_by` and `deleted_at`
//! unwritable; the five boolean preferences are the whole body
//! (`plane/app/serializers/notification.py:25-29`). `serializer.save()` is
//! `ModelSerializer.update` + `instance.save()`, i.e. every column plus H01's
//! `updated_by` and `auto_now`'s `updated_at` (`plane/db/models/base.py:23-45`).

use plane_core::error::APPLICATION_JSON;
use plane_db::hooks::audit::Audit;
use plane_db::models::notification::UserNotificationPreference;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_notification::UserNotificationPreferenceSerializer;

use super::queries;
use super::render::plain_context;
use super::write_bulk_queries;
use crate::issues::lists::list_common::{client, user_id};
use crate::write::{self, Serializer};

/// Register the route.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.notification.base.UserNotificationPreferenceEndpoint")?
        .method(http::Method::GET, get)?
        .method(http::Method::PATCH, patch)?;
    Ok(())
}

/// `UserNotificationPreference.objects.get(user=request.user)` (`:302`,`:308`).
async fn load(
    client: &deadpool_postgres::Client,
    user: uuid::Uuid,
) -> Result<UserNotificationPreference, ViewError> {
    let statement = client
        .prepare_cached(queries::USER_NOTIFICATION_PREFERENCE.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&user])
        .await
        .map_err(ViewError::from)?;
    match rows.as_slice() {
        [] => Err(ViewError::ObjectDoesNotExist),
        [row] => Ok(UserNotificationPreference::from_row(row, 0)?),
        // `MultipleObjectsReturned` reaches the generic 500.
        _ => Err(ViewError::Unknown(
            "UserNotificationPreference matching query does not exist.".to_owned(),
        )),
    }
}

/// `UserNotificationPreferenceSerializer(instance).data`.
fn value(preference: &UserNotificationPreference) -> UserNotificationPreferenceSerializer {
    UserNotificationPreferenceSerializer {
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
    }
}

fn render(
    ctx: &ViewCtx<'_>,
    preference: &UserNotificationPreference,
) -> Result<ViewResponse, ViewError> {
    let context = plain_context(ctx);
    Ok(ViewResponse::raw(
        200,
        APPLICATION_JSON,
        plane_serializers::render(&value(preference), &context)?,
    ))
}

/// `UserNotificationPreferenceEndpoint.get`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let preference = load(client, user).await?;
        render(ctx, &preference)
    })
}

/// `UserNotificationPreferenceEndpoint.patch` (`base.py:307-313`).
fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // The lookup is first (`base.py:308`): a two-row user is the 500 and a
        // zero-row user the 404 before the body is parsed.
        let mut preference = load(client, user).await?;

        // `UserNotificationPreferenceSerializer(user_notification_preference,
        // data=request.data, partial=True)` (`base.py:309`).
        let mut serializer = Serializer::new(ctx, true)?;
        // The generated field table, in `self.fields` order (DRF's validation
        // order): `property_change`, `state_change`, `comment`, `mention`,
        // `issue_completed`.
        crate::write::generated::user_notification_preference_serializer::validate(
            &mut serializer,
            client,
        )
        .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `ModelSerializer.update` assigns every validated key, then
        // `instance.save()`.
        if let Some(value) = values.bool("property_change") {
            preference.property_change = value;
        }
        if let Some(value) = values.bool("state_change") {
            preference.state_change = value;
        }
        if let Some(value) = values.bool("comment") {
            preference.comment = value;
        }
        if let Some(value) = values.bool("mention") {
            preference.mention = value;
        }
        if let Some(value) = values.bool("issue_completed") {
            preference.issue_completed = value;
        }

        // H01 on an update, then `auto_now`.
        preference.updated_at = write::now();
        preference.updated_by_id = Audit::on_update(Some(user));

        let statement = client
            .prepare_cached(write_bulk_queries::USER_NOTIFICATION_PREFERENCE_UPDATE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &preference.created_at,
                    &preference.updated_at,
                    &preference.created_by_id,
                    &preference.updated_by_id,
                    &preference.deleted_at,
                    &preference.user_id,
                    &preference.workspace_id,
                    &preference.project_id,
                    &preference.property_change,
                    &preference.state_change,
                    &preference.comment,
                    &preference.mention,
                    &preference.issue_completed,
                    &preference.id,
                ],
            )
            .await
            .map_err(ViewError::from)?;

        render(ctx, &preference)
    })
}
