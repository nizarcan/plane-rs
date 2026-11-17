// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WebhookEndpoint.patch` and `WebhookEndpoint.delete`
//! (`plane/app/views/webhook/base.py:81-111`), set B of the webhooks writes.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | `…/webhooks/{pk}/` (229) | `PATCH` | `patch` (`base.py:81-105`) | [`patch`] |
//! | `…/webhooks/{pk}/` (229) | `DELETE` | `delete` (`base.py:107-111`) | [`delete`] |
//! | `…/webhooks/` (228) | `PATCH`/`DELETE` | the kwarg `TypeError` | [`patch`]/[`delete`] |
//!
//! ## The 228 branch
//!
//! `routes.json` maps all four verbs onto both URL shapes, but `patch` and
//! `delete` take `pk` (`base.py:82`, `:108`) while the `webhooks/` route has no
//! `pk` kwarg. DRF calls `handler(request, *args, **kwargs)`
//! (`rest_framework/views.py:517-523`), so Python raises
//! `TypeError: WebhookEndpoint.patch() missing 1 required positional argument:
//! 'pk'`, which is uncaught and becomes the family 500
//! (`plane/app/views/base.py:330-334`). `allow_permission` wraps the method and
//! runs first (`permissions/base.py:22-24`), so a MEMBER gets the 403 there and
//! only an ADMIN reaches the 500. Both handlers reproduce the branch instead of
//! leaving the method unregistered, which would answer a 405.
//!
//! ## patch (`base.py:81-105`)
//!
//! The statement order is the behaviour (the view has no `transaction.atomic`):
//!
//! 1. `Webhook.objects.get(workspace__slug=slug, pk=pk)` (`:83`) — the
//!    manager's `deleted_at IS NULL` (`plane/db/mixins.py:56-58`), so a
//!    soft-deleted row, a row of another workspace and a missing pk are all the
//!    family 404 `{"error": "The required object does not exist."}`. The body
//!    is parsed **after** this lookup.
//! 2. `WebhookSerializer(instance, data=request.data, partial=True)` →
//!    `serializer.errors` is the 400 with each field's message list
//!    (`rest_framework/serializers.py:493-529`). `id`, `created_at`,
//!    `updated_at`, `deleted_at`, `secret_key` and `workspace` are read-only, so
//!    a body value for them is ignored without being validated
//!    (`rest_framework/serializers.py:496-498`).
//! 3. `serializer.save()` → `update()` (`serializers/webhook.py:74-78`) runs
//!    `_validate_webhook_url` only when `validated_data.get("url")` is truthy;
//!    its `ValidationError({"url": …})` is raised outside `is_valid()`, so the
//!    body is `{"url": "<message>"}` — a bare string, not a list
//!    (`webhooks::validate`).
//! 4. `ModelSerializer.update` assigns every validated key, then
//!    `instance.save()` with no `update_fields`
//!    (`django/db/models/base.py:928-936`): H01 sets `updated_by =
//!    request.user` (`plane/db/models/base.py:40-42`), `auto_now` sets
//!    `updated_at`, and every column is rewritten. A body `created_by_id` is
//!    written **verbatim** — the FK is writable and H01 only touches
//!    `updated_by` on an update.
//! 5. The live-row partial unique index
//!    `webhook_url_unique_url_when_deleted_at_null`
//!    (`plane/db/models/webhook.py:56-62`) is the only uniqueness check; PATCH
//!    has no `try/except IntegrityError`, so the conflict reaches the family
//!    handler as the 400 `{"error": "The payload is not valid"}`
//!    (`plane/app/views/base.py:306-310`) — unlike POST's 409.
//! 6. `Response(serializer.data, 200)` renders the in-memory instance with the
//!    fixed ten-name `fields=` tuple, so the body is exactly
//!    [`WebhookTen`] (the GET shape, not the 17-key create/regen shape).
//!
//! ## delete (`base.py:107-111`)
//!
//! `Webhook.objects.get(pk=pk, workspace__slug=slug)` (404) then
//! `webhook.delete()`: H03 (`plane/db/mixins.py:72-78`) sets
//! `deleted_at = now` and **full-saves** (H01's `updated_by`, `auto_now`'s
//! `updated_at`), then publishes `soft_delete_related_objects("db", "webhook",
//! pk, using=None)`. The response is an empty 204; the body is never parsed, so
//! malformed JSON still deletes.

use plane_core::error::ApiError;
use plane_db::hooks::audit::Audit;
use plane_db::models::Webhook;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::webhooks::render::WebhookTen;
use crate::webhooks::{validate, write_queries};
use crate::write::generated::webhook_serializer;
use crate::write::{self, Serializer, Validated};

/// `WebhookEndpoint.patch` (`plane/app/views/webhook/base.py:81-105`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // The `webhooks/` route (228) has no `pk` kwarg: Python raises the
        // `TypeError` above and Plane answers the family 500. Only an admin
        // reaches this line; the permission decorator ran first.
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(missing_kwarg("patch"));
        };
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;
        let pk = lookup::uuid_kwarg("pk", pk).map_err(ViewError::from)?;

        // `Webhook.objects.get(workspace__slug=slug, pk=pk)` (`:83`), before
        // the body is read.
        let mut webhook = load(client, slug, pk).await?;

        let mut serializer = Serializer::new(ctx, true)?;
        webhook_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `update()`'s `if url:` guard (`serializers/webhook.py:74-78`): the
        // save-time SSRF/disallowed-domain check runs only for a body that
        // carries a non-empty url. A field-validator failure returned above, so
        // the value here is always the accepted one.
        if let Some(url) = values.text("url")
            && !url.is_empty()
            && let Err(response) = validate::check(ctx, url).await
        {
            return Ok(response);
        }

        apply(&mut webhook, &values);
        webhook.updated_at = write::now();
        // H01: `self._state.adding` is false, so only `updated_by` is stamped —
        // a body `updated_by` is overwritten by this.
        webhook.updated_by_id = Audit::on_update(Some(user));
        save(client, &webhook).await?;

        render(ctx, &webhook)
    })
}

/// `WebhookEndpoint.delete` (`plane/app/views/webhook/base.py:107-111`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // Same 228 branch as `patch`, with the `delete` method name.
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(missing_kwarg("delete"));
        };
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;
        let pk = lookup::uuid_kwarg("pk", pk).map_err(ViewError::from)?;

        // `Webhook.objects.get(pk=pk, workspace__slug=slug)` (`:109`).
        let mut webhook = load(client, slug, pk).await?;

        // H03 (`plane/db/mixins.py:72-78`): `deleted_at = timezone.now()` then
        // a full save, so `updated_at` (a second, in-window `timezone.now()` in
        // Python) and H01's `updated_by` move with it and every other column is
        // rewritten verbatim.
        let now = write::now();
        webhook.deleted_at = Some(now);
        webhook.updated_at = now;
        webhook.updated_by_id = Audit::on_update(Some(user));
        save(client, &webhook).await?;
        // `soft_delete_related_objects.delay("db", "webhook", pk, using=None)`;
        // the cascade itself runs in the worker.
        write::publish_soft_delete_cascade(ctx, "db", "webhook", webhook.id).await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `ModelSerializer.update`'s assignments
/// (`rest_framework/serializers.py:1260-1290`) over the writable keys of
/// `WebhookSerializer`, in its field order.
///
/// Only keys present in `validated_data` are assigned, so a `partial=True` body
/// that omits a field keeps the stored value. `updated_by` is deliberately not
/// applied: H01 overwrites it on the save that follows.
fn apply(webhook: &mut Webhook, values: &Validated) {
    if let Some(url) = values.text("url") {
        webhook.url = url.to_owned();
    }
    if let Some(value) = values.bool("is_active") {
        webhook.is_active = value;
    }
    if let Some(value) = values.bool("project") {
        webhook.project = value;
    }
    if let Some(value) = values.bool("issue") {
        webhook.issue = value;
    }
    if let Some(value) = values.bool("module") {
        webhook.module = value;
    }
    if let Some(value) = values.bool("cycle") {
        webhook.cycle = value;
    }
    if let Some(value) = values.bool("issue_comment") {
        webhook.issue_comment = value;
    }
    if let Some(value) = values.bool("is_internal") {
        webhook.is_internal = value;
    }
    if let Some(value) = values.text("version") {
        webhook.version = value.to_owned();
    }
    // `created_by` is a writable FK (`allow_null=True`): an explicit `null`
    // clears it, a pk replaces it. `BaseModel.save` only sets `created_by` on a
    // create, so the body value survives an update
    // (`plane/db/models/base.py:36-42`).
    if values.is_null("created_by") {
        webhook.created_by_id = None;
    } else if let Some(value) = values.uuid("created_by") {
        webhook.created_by_id = Some(value);
    }
}

/// `Webhook.objects.get(workspace__slug=slug, pk=pk)` — the lookups of both
/// handlers (`plane/app/views/webhook/base.py:83`, `:109`).
///
/// The manager's `deleted_at IS NULL` comes from
/// `SoftDeletionManager.get_queryset` (`plane/db/mixins.py:56-58`) and the
/// `workspaces` join is unfiltered, exactly like the read route: a member of a
/// soft-deleted workspace passes the permission check and 404s here.
async fn load(
    client: &deadpool_postgres::Client,
    slug: &str,
    pk: Uuid,
) -> Result<Webhook, ViewError> {
    let statement = client
        .prepare_cached(write_queries::WEBHOOK_BY_PK_FULL.sql)
        .await
        .map_err(ViewError::from)?;
    let Some(row) = client
        .query_opt(&statement, &[&slug, &pk])
        .await
        .map_err(ViewError::from)?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Webhook::from_row(&row, 0)
        .map_err(|error| ViewError::Unknown(format!("decoding the webhook row: {error}")))
}

/// `instance.save()` with no `update_fields`
/// (`django/db/models/base.py:928-936`): one `UPDATE` of every column, keyed on
/// the primary key.
async fn save(client: &deadpool_postgres::Client, webhook: &Webhook) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(write_queries::WEBHOOK_UPDATE.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &webhook.created_at,
                &webhook.updated_at,
                &webhook.created_by_id,
                &webhook.updated_by_id,
                &webhook.deleted_at,
                &webhook.workspace_id,
                &webhook.url,
                &webhook.is_active,
                &webhook.secret_key,
                &webhook.project,
                &webhook.issue,
                &webhook.module,
                &webhook.cycle,
                &webhook.issue_comment,
                &webhook.is_internal,
                &webhook.version,
                &webhook.id,
            ],
        )
        .await
        .map(|_| ())
        .map_err(map_write_error)
}

/// `IntegrityError` is the family 400 `{"error": "The payload is not valid"}`
/// (`plane/app/views/base.py:306-310`); every other database failure (a
/// 1025–2048-character url overflowing `varchar(1024)` is SQLSTATE 22001) is
/// uncaught and a 500.
fn map_write_error(error: tokio_postgres::Error) -> ViewError {
    match error.code() {
        Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION)
        | Some(&tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION) => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// `WebhookSerializer(instance).data` with the PATCH `fields=` tuple: the ten
/// keys of [`WebhookTen`], 200.
fn render(ctx: &ViewCtx<'_>, webhook: &Webhook) -> Result<ViewResponse, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let body = plane_serializers::render(
        &WebhookTen::from_model(webhook),
        &RenderCtx::full(&timezone),
    )?;
    Ok(ViewResponse::raw(
        200,
        plane_core::error::APPLICATION_JSON,
        body,
    ))
}

/// `TypeError: WebhookEndpoint.<method>() missing 1 required positional
/// argument: 'pk'` — the 228 branch (`rest_framework/views.py:517-523`), which
/// Plane's family handler renders as its 500 (`plane/app/views/base.py:330-334`).
fn missing_kwarg(method: &str) -> ViewError {
    ViewError::Unknown(format!(
        "TypeError: WebhookEndpoint.{method}() missing 1 required positional argument: 'pk'"
    ))
}
