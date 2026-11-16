// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.webhook` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.webhook.WebhookLogSerializer` (model: `db.WebhookLog`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WebhookLogSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `webhook`: UUIDField (source `webhook`).
    pub webhook: uuid::Uuid,
    /// `event_type`: CharField (source `event_type`).
    pub event_type: Option<&'a str>,
    /// `request_method`: CharField (source `request_method`).
    pub request_method: Option<&'a str>,
    /// `request_headers`: CharField (source `request_headers`).
    pub request_headers: Option<&'a str>,
    /// `request_body`: CharField (source `request_body`).
    pub request_body: Option<&'a str>,
    /// `response_status`: CharField (source `response_status`).
    pub response_status: Option<&'a str>,
    /// `response_headers`: CharField (source `response_headers`).
    pub response_headers: Option<&'a str>,
    /// `response_body`: CharField (source `response_body`).
    pub response_body: Option<&'a str>,
    /// `retry_count`: IntegerField (source `retry_count`).
    pub retry_count: i64,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
}

impl<'a> WebhookLogSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "webhook",
        "event_type",
        "request_method",
        "request_headers",
        "request_body",
        "response_status",
        "response_headers",
        "response_body",
        "retry_count",
        "created_by",
        "updated_by",
        "workspace",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for WebhookLogSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("created_at", Self::MODE) {
            obj.field_dyn(
                "created_at",
                ctx,
                &crate::fields::drf_datetime(self.created_at, ctx.tz()),
            )?;
        }
        if ctx.emits("updated_at", Self::MODE) {
            obj.field_dyn(
                "updated_at",
                ctx,
                &crate::fields::drf_datetime(self.updated_at, ctx.tz()),
            )?;
        }
        if ctx.emits("deleted_at", Self::MODE) {
            obj.field_dyn(
                "deleted_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.deleted_at, ctx.tz()),
            )?;
        }
        if ctx.emits("webhook", Self::MODE) {
            obj.field_dyn("webhook", ctx, &crate::fields::DrfUuid(self.webhook))?;
        }
        if ctx.emits("event_type", Self::MODE) {
            obj.field_dyn("event_type", ctx, &self.event_type)?;
        }
        if ctx.emits("request_method", Self::MODE) {
            obj.field_dyn("request_method", ctx, &self.request_method)?;
        }
        if ctx.emits("request_headers", Self::MODE) {
            obj.field_dyn("request_headers", ctx, &self.request_headers)?;
        }
        if ctx.emits("request_body", Self::MODE) {
            obj.field_dyn("request_body", ctx, &self.request_body)?;
        }
        if ctx.emits("response_status", Self::MODE) {
            obj.field_dyn("response_status", ctx, &self.response_status)?;
        }
        if ctx.emits("response_headers", Self::MODE) {
            obj.field_dyn("response_headers", ctx, &self.response_headers)?;
        }
        if ctx.emits("response_body", Self::MODE) {
            obj.field_dyn("response_body", ctx, &self.response_body)?;
        }
        if ctx.emits("retry_count", Self::MODE) {
            obj.field_dyn("retry_count", ctx, &self.retry_count)?;
        }
        if ctx.emits("created_by", Self::MODE) {
            obj.field_dyn(
                "created_by",
                ctx,
                &self.created_by.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("updated_by", Self::MODE) {
            obj.field_dyn(
                "updated_by",
                ctx,
                &self.updated_by.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
