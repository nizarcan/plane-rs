// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.webhook.base`
//! (`plane/app/views/webhook/base.py`), the workspace webhook reads.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/webhooks/` | `WebhookEndpoint.get` (no `pk`) | `base.py:43-62` |
//! | `GET /api/workspaces/{slug}/webhooks/{pk}/` | `WebhookEndpoint.get` (with `pk`) | `base.py:63-79` |
//! | `GET /api/workspaces/{slug}/webhook-logs/{webhook_id}/` | `WebhookLogsEndpoint.get` | `base.py:125-129` |
//!
//! One query per request. Both `WebhookEndpoint` branches serialize with a
//! **fixed** `fields=` tuple (`base.py:45-57,64-75`), so `?fields=` and
//! `?expand=` in the query string are ignored — the view never passes them to
//! the constructor — and the response keys are the serializer's declaration
//! order filtered to that tuple: `id`, `url`, `created_at`, `updated_at`,
//! `is_active`, `project`, `issue`, `module`, `cycle`, `issue_comment`
//! (`module` before `cycle`, which is the `Meta.model` order, not the tuple's).
//! `secret_key` is excluded by the tuple and would also be popped by
//! `WebhookSerializer.to_representation` without `show_secret_key`
//! (`plane/app/serializers/webhook.py:44-52`).
//!
//! `WebhookLogsEndpoint` serializes with `WebhookLogSerializer(…)` and no
//! arguments, so it emits every field in declaration order and ignores
//! `?fields=`/`?expand=` too.

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_webhook::WebhookLogSerializer;
use plane_serializers::{DrfError, DrfSerialize, DrfUuid, DrfWriter, RenderCtx};
use uuid::Uuid;

use crate::common::lookup;
use crate::common::{client, render_list};
use crate::webhooks::queries;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.webhook.base.WebhookEndpoint")?
        .method(Method::GET, get)?;
    registry
        .api_view("plane.app.views.webhook.base.WebhookLogsEndpoint")?
        .method(Method::GET, get_logs)?;
    Ok(())
}

/// One row of either `WebhookEndpoint` branch.
#[derive(Debug)]
struct Webhook {
    id: Uuid,
    url: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    is_active: bool,
    project: bool,
    issue: bool,
    module: bool,
    cycle: bool,
    issue_comment: bool,
}

impl Webhook {
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            url: row.try_get(1).map_err(ViewError::from)?,
            created_at: row.try_get(2).map_err(ViewError::from)?,
            updated_at: row.try_get(3).map_err(ViewError::from)?,
            is_active: row.try_get(4).map_err(ViewError::from)?,
            project: row.try_get(5).map_err(ViewError::from)?,
            issue: row.try_get(6).map_err(ViewError::from)?,
            module: row.try_get(7).map_err(ViewError::from)?,
            cycle: row.try_get(8).map_err(ViewError::from)?,
            issue_comment: row.try_get(9).map_err(ViewError::from)?,
        })
    }
}

/// `WebhookSerializer(webhook, fields=(…))`'s `.data`, in declaration order.
#[derive(Debug)]
struct WebhookValue<'a>(&'a Webhook);

impl DrfSerialize for WebhookValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let webhook = self.0;
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(webhook.id))?;
        obj.field_dyn("url", ctx, &webhook.url)?;
        obj.field_dyn(
            "created_at",
            ctx,
            &plane_serializers::fields::drf_datetime(webhook.created_at, ctx.tz()),
        )?;
        obj.field_dyn(
            "updated_at",
            ctx,
            &plane_serializers::fields::drf_datetime(webhook.updated_at, ctx.tz()),
        )?;
        obj.field_dyn("is_active", ctx, &webhook.is_active)?;
        obj.field_dyn("project", ctx, &webhook.project)?;
        obj.field_dyn("issue", ctx, &webhook.issue)?;
        obj.field_dyn("module", ctx, &webhook.module)?;
        obj.field_dyn("cycle", ctx, &webhook.cycle)?;
        obj.field_dyn("issue_comment", ctx, &webhook.issue_comment)?;
        obj.end();
        Ok(())
    }
}

/// `WebhookEndpoint.get` (`plane/app/views/webhook/base.py:43-79`). One
/// registered method serves both URL shapes; `pk` decides which branch runs.
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let client = client(ctx)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);

        let Some(pk) = ctx.kwarg("pk") else {
            // `Webhook.objects.filter(workspace__slug=slug)` (`:44-62`).
            let statement = client
                .prepare_cached(queries::WEBHOOKS_BY_WORKSPACE.sql)
                .await
                .map_err(ViewError::from)?;
            let rows = client
                .query(&statement, &[&slug])
                .await
                .map_err(ViewError::from)?;
            let mut webhooks = Vec::with_capacity(rows.len());
            for row in &rows {
                webhooks.push(Webhook::decode(row)?);
            }
            let values: Vec<WebhookValue<'_>> = webhooks.iter().map(WebhookValue).collect();
            let body = render_list(&values, &RenderCtx::full(&timezone))?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        };

        // `Webhook.objects.get(workspace__slug=slug, pk=pk)` (`:63-79`).
        let pk = lookup::uuid_kwarg("pk", pk).map_err(ViewError::from)?;
        let statement = client
            .prepare_cached(queries::WEBHOOK_BY_PK.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&statement, &[&slug, &pk])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let webhook = Webhook::decode(&row)?;
        let body = plane_serializers::render(&WebhookValue(&webhook), &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// One `webhook_logs` row, owned so the borrowing serializer can be built.
#[derive(Debug)]
struct WebhookLog {
    id: Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    webhook: Uuid,
    event_type: Option<String>,
    request_method: Option<String>,
    request_headers: Option<String>,
    request_body: Option<String>,
    response_status: Option<String>,
    response_headers: Option<String>,
    response_body: Option<String>,
    retry_count: i64,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    workspace: Uuid,
}

impl WebhookLog {
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            created_at: row.try_get(1).map_err(ViewError::from)?,
            updated_at: row.try_get(2).map_err(ViewError::from)?,
            deleted_at: row.try_get(3).map_err(ViewError::from)?,
            webhook: row.try_get(4).map_err(ViewError::from)?,
            event_type: row.try_get(5).map_err(ViewError::from)?,
            request_method: row.try_get(6).map_err(ViewError::from)?,
            request_headers: row.try_get(7).map_err(ViewError::from)?,
            request_body: row.try_get(8).map_err(ViewError::from)?,
            response_status: row.try_get(9).map_err(ViewError::from)?,
            response_headers: row.try_get(10).map_err(ViewError::from)?,
            response_body: row.try_get(11).map_err(ViewError::from)?,
            retry_count: i64::from(row.try_get::<_, i16>(12).map_err(ViewError::from)?),
            created_by: row.try_get(13).map_err(ViewError::from)?,
            updated_by: row.try_get(14).map_err(ViewError::from)?,
            workspace: row.try_get(15).map_err(ViewError::from)?,
        })
    }

    fn value(&self) -> WebhookLogSerializer<'_> {
        WebhookLogSerializer {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            webhook: self.webhook,
            event_type: self.event_type.as_deref(),
            request_method: self.request_method.as_deref(),
            request_headers: self.request_headers.as_deref(),
            request_body: self.request_body.as_deref(),
            response_status: self.response_status.as_deref(),
            response_headers: self.response_headers.as_deref(),
            response_body: self.response_body.as_deref(),
            retry_count: self.retry_count,
            created_by: self.created_by,
            updated_by: self.updated_by,
            workspace: self.workspace,
        }
    }
}

/// `WebhookLogsEndpoint.get` (`plane/app/views/webhook/base.py:125-129`).
pub fn get_logs<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let webhook_id =
            lookup::uuid_kwarg("webhook_id", ctx.kwarg("webhook_id").ok_or(ViewError::Key)?)
                .map_err(ViewError::from)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);

        let statement = client(ctx)?
            .prepare_cached(queries::WEBHOOK_LOGS.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client(ctx)?
            .query(&statement, &[&slug, &webhook_id])
            .await
            .map_err(ViewError::from)?;

        let mut decoded = Vec::with_capacity(rows.len());
        for row in &rows {
            decoded.push(WebhookLog::decode(row)?);
        }
        let logs: Vec<WebhookLogSerializer<'_>> = decoded.iter().map(WebhookLog::value).collect();

        let body = render_list(&logs, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
