// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Rendering and the shared refusal used by the `notifications` handlers.
//!
//! `NotificationSerializer` and `UserNotificationPreferenceSerializer` are
//! plain `ModelSerializer`s, so DRF never passes `fields=`/`expand=` to them
//! (`rest_framework/serializers.py:465-485`); the context below has no mask for
//! that reason, and the nested `UserLiteSerializer` inherits the same one.
//!
//! ## One row, two shapes
//!
//! The list view annotates `is_inbox_issue`, `is_intake_issue` and
//! `is_mentioned_notification` (`base.py:64-78`), so its rows carry all three
//! values. The action views (`base.py:156-198`) answer from a plain
//! `Notification.objects.get(...)`, where the three declared read-only fields
//! have no attribute and DRF raises `SkipField`
//! (`rest_framework/fields.py:426-458`): the body has **22 keys, not 25**.
//! [`render_row`] is the annotated shape (GET bytes unchanged); [`render_plain`]
//! is the action shape, built with `FieldValue::absent()` for the three
//! (precedent `states/write.rs:611-643`).
//!
//! `triggered_by_details` is the nested `UserLiteSerializer` over
//! `source="triggered_by"`. The forward FK uses Django's `_base_manager`, so
//! neither the `users` nor the `file_assets` join carries a `deleted_at`
//! predicate (`base.rs:291-304`).

use chrono_tz::Tz;
use plane_db::models::notification::Notification;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;
use plane_serializers::generated::app_notification::NotificationSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;
use uuid::Uuid;

use crate::intake::asset;

/// The active timezone, or `settings.TIME_ZONE` (UTC).
pub fn timezone(ctx: &ViewCtx<'_>) -> Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

/// The context a plain serializer renders under.
pub fn plain_context<'a>(ctx: &'a ViewCtx<'a>) -> RenderCtx<'a> {
    let timezone: &'a Tz = match ctx.timezone {
        Some(timezone) => timezone,
        None => &chrono_tz::UTC,
    };
    RenderCtx::new(timezone, None, &[])
}

/// A handler for an action the runtime's `BaseViewSet` guard always refuses
/// (`plane/app/views/base.py:103-198`). Registering the action is what makes
/// the guard reachable — an unregistered route answers the pipeline's 501 —
/// and the handler can therefore never execute.
pub fn guard_only<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        Err(ViewError::Unknown(format!(
            "{} reached a handler its BaseViewSet guard always refuses",
            ctx.view
        )))
    })
}

/// `render_rows`: decode the notification and `triggered_by` columns, resolve
/// the avatar assets for the whole page, then render `NotificationSerializer`
/// with all three annotations present.
pub(super) fn render_rows(
    rows: &[tokio_postgres::Row],
    timezone: &Tz,
) -> Result<Vec<Vec<u8>>, ViewError> {
    let context = RenderCtx::new(timezone, None, &[]);

    let mut rendered = Vec::with_capacity(rows.len());
    for row in rows {
        let notification = Notification::from_row(row, 0)?;
        let triggered_by = decode_triggered_by(row, TRIGGERED_BY_START)?;
        let triggered_by_serializer = triggered_by.as_ref().map(|user| user.lite());
        let serializer = NotificationSerializer {
            id: notification.id,
            triggered_by_details: triggered_by_serializer.as_ref(),
            // All three annotations exist: the two `EXISTS`s and the `CASE`.
            is_inbox_issue: plane_serializers::FieldValue::value(row.get(21)),
            is_intake_issue: plane_serializers::FieldValue::value(row.get(22)),
            is_mentioned_notification: plane_serializers::FieldValue::value(row.get(23)),
            created_at: notification.created_at,
            updated_at: notification.updated_at,
            deleted_at: notification.deleted_at,
            data: notification
                .data
                .as_ref()
                .map(plane_serializers::PgJson::from),
            entity_identifier: notification.entity_identifier,
            entity_name: &notification.entity_name,
            title: &notification.title,
            message: notification
                .message
                .as_ref()
                .map(plane_serializers::PgJson::from),
            message_html: &notification.message_html,
            message_stripped: notification.message_stripped.as_deref(),
            sender: &notification.sender,
            read_at: notification.read_at,
            snoozed_till: notification.snoozed_till,
            archived_at: notification.archived_at,
            created_by: notification.created_by_id,
            updated_by: notification.updated_by_id,
            workspace: notification.workspace_id,
            project: notification.project_id,
            triggered_by: notification.triggered_by_id,
            receiver: notification.receiver_id,
        };
        rendered.push(plane_serializers::render(&serializer, &context)?);
    }
    Ok(rendered)
}

/// `NotificationSerializer(instance).data` for one **plain** instance
/// (`base.py:164,173,181,189,197`): the three annotation fields have no
/// attribute, so DRF's `SkipField` drops them and the body is 22 keys.
pub(super) fn render_plain(
    notification: &Notification,
    triggered_by: Option<&TriggeredBy>,
    context: &RenderCtx<'_>,
) -> Result<Vec<u8>, ViewError> {
    let triggered_by_serializer = triggered_by.map(TriggeredBy::lite);
    let serializer = NotificationSerializer {
        id: notification.id,
        triggered_by_details: triggered_by_serializer.as_ref(),
        is_inbox_issue: plane_serializers::FieldValue::absent(),
        is_intake_issue: plane_serializers::FieldValue::absent(),
        is_mentioned_notification: plane_serializers::FieldValue::absent(),
        created_at: notification.created_at,
        updated_at: notification.updated_at,
        deleted_at: notification.deleted_at,
        data: notification
            .data
            .as_ref()
            .map(plane_serializers::PgJson::from),
        entity_identifier: notification.entity_identifier,
        entity_name: &notification.entity_name,
        title: &notification.title,
        message: notification
            .message
            .as_ref()
            .map(plane_serializers::PgJson::from),
        message_html: &notification.message_html,
        message_stripped: notification.message_stripped.as_deref(),
        sender: &notification.sender,
        read_at: notification.read_at,
        snoozed_till: notification.snoozed_till,
        archived_at: notification.archived_at,
        created_by: notification.created_by_id,
        updated_by: notification.updated_by_id,
        workspace: notification.workspace_id,
        project: notification.project_id,
        triggered_by: notification.triggered_by_id,
        receiver: notification.receiver_id,
    };
    plane_serializers::render(&serializer, context).map_err(ViewError::from)
}

/// The first column of the `triggered_by` block when the statement projects
/// the 21 model columns and the three list annotations before it.
pub(super) const TRIGGERED_BY_START: usize = 24;

/// The owned form of `UserLiteSerializer`'s input.
pub(super) struct TriggeredBy {
    id: Uuid,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
}

impl TriggeredBy {
    fn lite(&self) -> UserLiteSerializer<'_> {
        UserLiteSerializer {
            id: self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.is_bot,
            display_name: &self.display_name,
        }
    }
}

/// Decode the `UserLiteSerializer` block starting at column `start`: `id`,
/// `first_name`, `last_name`, `avatar`, `avatar_asset_id`, `is_bot`,
/// `display_name`, then the seven `file_assets`/`workspaces` columns.
pub(super) fn decode_triggered_by(
    row: &tokio_postgres::Row,
    start: usize,
) -> Result<Option<TriggeredBy>, ViewError> {
    let Some(id) = row.get::<_, Option<Uuid>>(start) else {
        // `triggered_by_details` is a nested serializer whose source is null:
        // DRF writes `null` (`rest_framework/serializers.py:499-521`).
        return Ok(None);
    };
    let avatar: String = row.get(start + 3);
    let avatar_asset_id: Option<Uuid> = row.get(start + 4);
    let avatar_url = match avatar_asset_id {
        Some(asset_id) => {
            let row_id: Option<Uuid> = row.get(start + 7);
            if row_id.is_none() {
                // The FK points at a row the base manager cannot see: Django
                // raises `RelatedObjectDoesNotExist` (a 500).
                return Err(ViewError::Unknown(format!(
                    "the file asset {asset_id} referenced by this row does not exist"
                )));
            }
            asset::asset_url(
                asset_id,
                row.get(start + 8),
                row.get(start + 9),
                row.get(start + 10),
                row.get::<_, Option<String>>(start + 11).as_deref(),
            )
        }
        None if !avatar.is_empty() => Some(avatar.clone()),
        None => None,
    };
    Ok(Some(TriggeredBy {
        id,
        first_name: row.get(start + 1),
        last_name: row.get(start + 2),
        avatar,
        avatar_url,
        is_bot: row.get(start + 5),
        display_name: row.get(start + 6),
    }))
}
