// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WebhookEndpoint.post` and `WebhookSecretRegenerateEndpoint.post`
//! (`plane/app/views/webhook/base.py:20-39`, `:114-121`) — set A of
//! `plans/WEBHOOKS.md`.
//!
//! | Route | Python | Handler |
//! | --- | --- | --- |
//! | `POST /api/workspaces/{slug}/webhooks/` | `WebhookEndpoint.post` (`:21-39`) | [`create`] |
//! | `POST /api/workspaces/{slug}/webhooks/{pk}/` | the same method, with a `pk` the signature does not take | [`create`]'s 500 branch |
//! | `POST /api/workspaces/{slug}/webhooks/{pk}/regenerate/` | `WebhookSecretRegenerateEndpoint.post` (`:115-121`) | [`regenerate`] |
//!
//! ## create (`base.py:21-39`)
//!
//! 1. `allow_permission([ADMIN], WORKSPACE)` (`:21`) — the runtime's
//!    `plane-permissions` already enforced it, and it runs **before** the
//!    method body, so a MEMBER/GUEST gets the 403 even on the `pk` route that
//!    then raises (`permissions/base.py:22-24`).
//! 2. `Workspace.objects.get(slug=slug)` (`:23`), outside the `try`: a miss is
//!    the family 404 and it runs before the body is parsed.
//! 3. `WebhookSerializer(data=request.data, context={request, show_secret_key})`;
//!    `is_valid()` validates the fields in `self.fields` order and a failure is
//!    the 400 with `serializer.errors` (`:29-32`).
//! 4. `serializer.save(workspace_id=workspace.id)` (`:30`) → `create()`
//!    (`serializers/webhook.py:69-72`) runs the save-time SSRF/domain check
//!    ([`super::validate::check`]) and then `Webhook.objects.create`, which is
//!    H01 create (`created_by` = caller, `updated_by` = NULL —
//!    `plane/db/models/base.py:36-39`, so a body `created_by` is overwritten)
//!    plus the `auto_now_add`/`auto_now` timestamps.
//! 5. The INSERT's `IntegrityError` handler (`:33-39`): `"already exists" in
//!    str(e)` is the partial unique index
//!    `webhook_url_unique_url_when_deleted_at_null`
//!    (`plane/db/models/webhook.py:56-62`) and answers **409**
//!    `{"error": "URL already exists for the workspace"}`; anything else
//!    re-raises and the family handler maps it — `IntegrityError` is the 400
//!    `{"error": "The payload is not valid"}` (`views/base.py:306-310`), but a
//!    varchar(1024) overflow is psycopg's `DataError`, a **500**
//!    (`plane/app/views/base.py:330-334`).
//! 6. `Response(serializer.data, 201)` from the in-memory instance
//!    (`:31`), which is the 17-key body of [`super::render::WebhookFull`].
//!
//! The `webhooks/{pk}/` route shares the view, and `post` takes no `pk`
//! (`:22`), so DRF's `handler(request, **kwargs)` raises
//! `TypeError: … got an unexpected keyword argument 'pk'` — the family 500
//! body. Ported as a branch, not as a 405 (`plans/WEBHOOKS.md` §1), and the
//! permission check above has already passed by then.
//!
//! ## regenerate (`base.py:115-121`)
//!
//! `Webhook.objects.get(workspace__slug=slug, pk=pk)` (404 via the
//! soft-delete manager, `plane/db/mixins.py:56-58`), a fresh
//! `generate_token()` secret (`plane/db/models/webhook.py:17-18`), then
//! `webhook.save()` with no `update_fields`: a full row rewrite with H01
//! update (`updated_by` = caller, `created_by` untouched) and `auto_now`'s
//! `updated_at`. 200 with the same 17 keys.
//!
//! No transaction: the view has no `atomic()` and `ATOMIC_REQUESTS` is unset
//! (`HANDLERS.md` "Writes" §4).

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks::audit::Audit;
use plane_db::models::Webhook;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::projects::write_support;
use crate::webhooks::render::WebhookFull;
use crate::webhooks::{validate, write_queries};
use crate::write::{self, Serializer};

/// `WebhookEndpoint.post` (`plane/app/views/webhook/base.py:21-39`).
///
/// One registered method serves both URL shapes; `pk` selects the TypeError
/// branch, exactly as DRF's keyword expansion does.
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        // `post(self, request, slug)` (`:22`) has no `pk` parameter, and the
        // `webhooks/{pk}/` route (order 229) passes one. DRF calls
        // `handler(request, *args, **kwargs)` (`rest_framework/views.py:517-523`),
        // so the call itself raises and nothing in the body runs.
        if ctx.kwarg("pk").is_some() {
            return Err(ViewError::Unknown(
                "TypeError: WebhookEndpoint.post() got an unexpected keyword argument 'pk'"
                    .to_owned(),
            ));
        }
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`:23`). Outside the `try`, and
        // before `request.data` is read by the serializer.
        let workspace = lookup::workspace_by_slug(client, slug)
            .await
            .map_err(ViewError::from)?;

        let mut serializer = Serializer::new(ctx, false)?;
        write::generated::webhook_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `url` is the one required field, so `finish()` above already
        // answered 400 when it is missing.
        let Some(url) = values.text("url").map(str::to_owned) else {
            return Err(ViewError::Unknown(
                "WebhookSerializer.url is missing after validation".to_owned(),
            ));
        };

        // `_validate_webhook_url(url)` inside `serializer.save()`
        // (`serializers/webhook.py:69-72`): after field validation, before the
        // INSERT. The body is `{"url": "<message>"}` — a bare string.
        if let Err(response) = validate::check(ctx, &url).await {
            return Ok(response);
        }

        // `auto_now_add` and `auto_now` are two separate `timezone.now()`
        // calls in `DateTimeField.pre_save`
        // (`django/db/models/fields/__init__.py:864-877`), one per field, in
        // model order: `created_at` then `updated_at`.
        let created_at = write::now();
        let updated_at = write::now();
        // H01 create (`plane/db/models/base.py:36-39`): the caller becomes
        // `created_by` and `updated_by` is NULL, whatever the body said.
        let audit = Audit::on_create(Some(user));
        let webhook = Webhook {
            created_at,
            updated_at,
            created_by_id: audit.created_by_id,
            updated_by_id: audit.updated_by_id,
            // `deleted_at` is read-only, so the body cannot set it.
            deleted_at: None,
            id: Uuid::new_v4(),
            workspace_id: workspace.id,
            url,
            // Model field defaults (`plane/db/models/webhook.py:37-45`); an
            // absent serializer field leaves `validated_data` without the key.
            is_active: values.bool("is_active").unwrap_or(true),
            secret_key: plane_core::secrets::webhook_secret_key(),
            project: values.bool("project").unwrap_or(false),
            issue: values.bool("issue").unwrap_or(false),
            module: values.bool("module").unwrap_or(false),
            cycle: values.bool("cycle").unwrap_or(false),
            issue_comment: values.bool("issue_comment").unwrap_or(false),
            is_internal: values.bool("is_internal").unwrap_or(false),
            version: values.text("version").unwrap_or("v1").to_owned(),
        };

        let statement = client
            .prepare_cached(write_queries::WEBHOOK_INSERT.sql)
            .await
            .map_err(ViewError::from)?;
        let inserted = client
            .execute(
                &statement,
                &[
                    &webhook.created_at,
                    &webhook.updated_at,
                    &webhook.created_by_id,
                    &webhook.updated_by_id,
                    &webhook.deleted_at,
                    &webhook.id,
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
                ],
            )
            .await;
        if let Err(error) = inserted {
            // `if "already exists" in str(e)` (`:34-38`). psycopg's message is
            // libpq's, whose `DETAIL:` line carries the key and the words
            // "already exists" (`psycopg/errors.py:553-556`); tokio-postgres
            // exposes the same text through `DbError::detail()`.
            if is_duplicate_url(&error) {
                return Ok(write::error_body(
                    409,
                    "URL already exists for the workspace",
                ));
            }
            // Everything else is re-raised: class 23 stays the family's 400,
            // a varchar overflow is `DataError` and therefore a 500
            // (`plane/app/views/base.py:306-310`, `:330-334`).
            return Err(write_support::integrity(error));
        }

        render(ctx, &webhook, 201)
    })
}

/// `WebhookSecretRegenerateEndpoint.post`
/// (`plane/app/views/webhook/base.py:115-121`).
pub fn regenerate<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)
            .map_err(ViewError::from)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `Webhook.objects.get(workspace__slug=slug, pk=pk)` (`:117`).
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
        let mut webhook = Webhook::from_row(&row, 0)
            .map_err(|error| ViewError::Unknown(format!("decoding the webhook row: {error}")))?;

        // `webhook.secret_key = generate_token()` (`:118`).
        webhook.secret_key = plane_core::secrets::webhook_secret_key();
        // `webhook.save()` (`:119`): no `update_fields`, so `auto_now` rewrites
        // `updated_at` and H01 update only replaces `updated_by`
        // (`plane/db/models/base.py:40-42`); every other column keeps its
        // stored value and is rewritten verbatim.
        webhook.updated_at = write::now();
        webhook.updated_by_id = Audit::on_update(Some(user));

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
            .map_err(ViewError::from)?;

        render(ctx, &webhook, 200)
    })
}

/// `"already exists" in str(e)` for the partial unique index.
///
/// The SQLSTATE guard is tokio-postgres' equivalent of psycopg raising
/// `IntegrityError`; the substring is the one the view tests, taken from the
/// `DETAIL` line (precedent: `states/write.rs::is_duplicate_name`).
fn is_duplicate_url(error: &tokio_postgres::Error) -> bool {
    if error.code() != Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION) {
        return false;
    }
    error
        .as_db_error()
        .and_then(|error| error.detail())
        .is_some_and(|detail| detail.contains("already exists"))
}

/// `Response(WebhookSerializer(instance).data)` — the 17 keys, in the active
/// timezone, from the instance the write left in memory
/// (`plane/app/views/webhook/base.py:31`, `:120-121`).
fn render(ctx: &ViewCtx<'_>, webhook: &Webhook, status: u16) -> Result<ViewResponse, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let body = plane_serializers::render(&WebhookFull { webhook }, &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A duplicate-URL error is the 409, and only when the `DETAIL` line says
    /// "already exists"; the SQLSTATE alone is not enough to keep the 400 for
    /// a same-shape unique violation on another index.
    #[test]
    fn the_conflict_message_matches_the_view() {
        let response = write::error_body(409, "URL already exists for the workspace");
        let rendered = plane_drf::view::DrfResponse::render(response).expect("render");
        assert_eq!(
            String::from_utf8(rendered.body).expect("utf8"),
            r#"{"error":"URL already exists for the workspace"}"#
        );
        assert_eq!(rendered.status, 409);
    }
}
