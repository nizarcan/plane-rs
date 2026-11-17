// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `NotificationViewSet`'s write actions, routes 117–119
//! (`plane/app/views/notification/base.py:156-198`).
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | `…/users/notifications/{pk}/` (117) | `PATCH` | `partial_update` (`base.py:156-166`) | [`partial_update`] |
//! | the same route (117) | `DELETE` | `DestroyModelMixin.destroy` — the guard's 405 | [`render::guard_only`] |
//! | `…/{pk}/read/` (118) | `POST` | `mark_read` (`base.py:168-174`) | [`mark_read`] |
//! | `…/{pk}/read/` (118) | `DELETE` | `mark_unread` (`base.py:176-182`) | [`mark_unread`] |
//! | `…/{pk}/archive/` (119) | `POST` | `archive` (`base.py:184-190`) | [`archive`] |
//! | `…/{pk}/archive/` (119) | `DELETE` | `unarchive` (`base.py:192-198`) | [`unarchive`] |
//!
//! `allow_permission(allowed_roles=[ADMIN, MEMBER, GUEST], level="WORKSPACE")`
//! is a method decorator (`plane/app/permissions/base.py:19-103`, WORKSPACE at
//! `:59-66`) and the runtime already enforced it from `routes.json`: a
//! non-member or an inactive member is the permission 403 before this handler,
//! for a missing pk too.
//!
//! ## Order, and the one body
//!
//! Every action is `Notification.objects.get(...)` → mutate → `save()` →
//! `NotificationSerializer(instance).data`, in that order and with no
//! transaction (`HANDLERS.md` §4). 117 reads the body **after** the lookup
//! (`base.py:158` then `:160`), so `{not json` on a missing pk is the 404, not
//! the parser's 400.
//!
//! 117 rebuilds the body rather than passing it to DRF:
//! `{"snoozed_till": request.data.get("snoozed_till", None)}` (`base.py:160-161`).
//! A plain dict has no html-input semantics (`rest_framework/utils/html.py:9-12`),
//! so a form `""` is an invalid datetime instead of DRF's "empty → None", and
//! every other key — `read_at`, `archived_at`, `deleted_at` included — is
//! ignored. That is why the serializer is built with
//! `Serializer::from_value` over the one-key dict.
//!
//! ## The response is 22 keys
//!
//! These views answer from a **plain** instance, not the list's annotated
//! queryset, so the three declared read-only fields have no attribute and DRF
//! raises `SkipField`: `notifications::render::render_plain` renders that shape
//! (`base.py:164`).

use plane_db::hooks::audit::Audit;
use plane_db::models::notification::Notification;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use uuid::Uuid;

use super::render::{self, TriggeredBy, decode_triggered_by, plain_context, render_plain};
use super::write_queries as queries;
use crate::issues::lists::list_common::{client, kwarg_text, kwarg_uuid, user_id};
use crate::views::write_support::{Body, body_get};
use crate::write::{self, Serializer};

/// Register the three routes' actions. `destroy` is registered only so the
/// runtime's `BaseViewSet` guard is reachable (it answers Django's 405);
/// without it the route would fall through to the pipeline's 501.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.notification.base.NotificationViewSet")?
        .action("partial_update", partial_update)?
        .action("mark_read", mark_read)?
        .action("mark_unread", mark_unread)?
        .action("archive", archive)?
        .action("unarchive", unarchive)?
        .action("destroy", render::guard_only)?;
    Ok(())
}

/// One row of [`queries::NOTIFICATION_BY_PK`] and its nested
/// `UserLiteSerializer` input.
struct Found {
    notification: Notification,
    triggered_by: Option<TriggeredBy>,
}

/// `Notification.objects.get(workspace__slug=slug, pk=pk,
/// receiver=request.user)` (`base.py:158,170,178,186,194`).
///
/// A miss is `DoesNotExist`, which Plane's family handler renders as
/// `404 {"error": "The required object does not exist."}`
/// (`plane/app/views/base.py:226-233`).
async fn load(
    client: &deadpool_postgres::Client,
    slug: &str,
    user: Uuid,
    pk: Uuid,
) -> Result<Found, ViewError> {
    let statement = client
        .prepare_cached(queries::NOTIFICATION_BY_PK.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&pk, &slug, &user])
        .await
        .map_err(ViewError::from)?;
    match rows.as_slice() {
        [row] => Ok(Found {
            notification: Notification::from_row(row, 0)?,
            triggered_by: decode_triggered_by(row, 21)?,
        }),
        // `MultipleObjectsReturned` is an `ObjectDoesNotExist` subclass and a
        // pk lookup cannot produce it; both branches are the same 404.
        _ => Err(ViewError::ObjectDoesNotExist),
    }
}

/// `instance.save()` on an existing row (`django/db/models/base.py:928-936`):
/// H01's `updated_by`, `auto_now`'s `updated_at`, then one full-row `UPDATE`.
async fn save(
    client: &deadpool_postgres::Client,
    notification: &mut Notification,
    user: Uuid,
) -> Result<(), ViewError> {
    notification.updated_at = write::now();
    notification.updated_by_id = Audit::on_update(Some(user));

    let statement = client
        .prepare_cached(queries::NOTIFICATION_FULL_SAVE.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &notification.created_at,
                &notification.updated_at,
                &notification.created_by_id,
                &notification.updated_by_id,
                &notification.deleted_at,
                &notification.workspace_id,
                &notification.project_id,
                &notification.data,
                &notification.entity_identifier,
                &notification.entity_name,
                &notification.title,
                &notification.message,
                &notification.message_html,
                &notification.message_stripped,
                &notification.sender,
                &notification.triggered_by_id,
                &notification.receiver_id,
                &notification.read_at,
                &notification.snoozed_till,
                &notification.archived_at,
                &notification.id,
            ],
        )
        .await
        .map_err(ViewError::from)?;
    Ok(())
}

/// `NotificationSerializer(instance).data`, 200.
fn render(ctx: &ViewCtx<'_>, found: &Found) -> Result<ViewResponse, ViewError> {
    let context = plain_context(ctx);
    let body = render_plain(&found.notification, found.triggered_by.as_ref(), &context)?;
    Ok(ViewResponse::raw(
        200,
        plane_core::error::APPLICATION_JSON,
        body,
    ))
}

/// `NotificationViewSet.partial_update` (`base.py:156-166`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let client = client(ctx)?;

        let mut found = load(client, slug, user, pk).await?;

        // `{"snoozed_till": request.data.get("snoozed_till", None)}`
        // (`base.py:160-161`). `request.data.get` on a body that is not a
        // mapping is the `AttributeError` 500 (`views::write_support`).
        let data = ctx.data()?;
        let mut rebuilt = PyDict::new();
        match body_get(data, "snoozed_till")? {
            Body::Missing => rebuilt.insert(PyStr::new("snoozed_till"), Json::Null),
            Body::Json(value) => rebuilt.insert(PyStr::new("snoozed_till"), value.clone()),
            Body::Form(text) => {
                rebuilt.insert(PyStr::new("snoozed_till"), Json::Str(PyStr::new(text)))
            }
        };
        let value = Json::Dict(rebuilt);

        let mut serializer = Serializer::from_value(ctx, &value, true);
        // `NotificationSerializer`'s writable fields are every model column but
        // the read-only ones; the rebuilt dict has one key, so this is the only
        // call.
        let _ = serializer.datetime_field("snoozed_till", true);
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `ModelSerializer.update`: assign the validated value, then `save()`.
        found.notification.snoozed_till = values.datetime("snoozed_till");
        save(client, &mut found.notification, user).await?;
        render(ctx, &found)
    })
}

/// `NotificationViewSet.mark_read` (`base.py:168-174`): `read_at = now()`.
pub fn mark_read<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let client = client(ctx)?;

        let mut found = load(client, slug, user, pk).await?;
        found.notification.read_at = Some(write::now());
        save(client, &mut found.notification, user).await?;
        render(ctx, &found)
    })
}

/// `NotificationViewSet.mark_unread` (`base.py:176-182`): `read_at = None`.
pub fn mark_unread<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let client = client(ctx)?;

        let mut found = load(client, slug, user, pk).await?;
        found.notification.read_at = None;
        save(client, &mut found.notification, user).await?;
        render(ctx, &found)
    })
}

/// `NotificationViewSet.archive` (`base.py:184-190`): `archived_at = now()`.
pub fn archive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let client = client(ctx)?;

        let mut found = load(client, slug, user, pk).await?;
        found.notification.archived_at = Some(write::now());
        save(client, &mut found.notification, user).await?;
        render(ctx, &found)
    })
}

/// `NotificationViewSet.unarchive` (`base.py:192-198`): `archived_at = None`.
pub fn unarchive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let client = client(ctx)?;

        let mut found = load(client, slug, user, pk).await?;
        found.notification.archived_at = None;
        save(client, &mut found.notification, user).await?;
        render(ctx, &found)
    })
}
