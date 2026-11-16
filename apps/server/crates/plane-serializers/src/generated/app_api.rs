// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.api` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.api.APIActivityLogSerializer` (model: `db.APIActivityLog`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct APIActivityLogSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `token_identifier`: CharField (source `token_identifier`).
    pub token_identifier: &'a str,
    /// `path`: CharField (source `path`).
    pub path: &'a str,
    /// `method`: CharField (source `method`).
    pub method: &'a str,
    /// `query_params`: CharField (source `query_params`).
    pub query_params: Option<&'a str>,
    /// `headers`: CharField (source `headers`).
    pub headers: Option<&'a str>,
    /// `body`: CharField (source `body`).
    pub body: Option<&'a str>,
    /// `response_code`: IntegerField (source `response_code`).
    pub response_code: i64,
    /// `response_body`: CharField (source `response_body`).
    pub response_body: Option<&'a str>,
    /// `ip_address`: IPAddressField (source `ip_address`).
    pub ip_address: Option<&'a str>,
    /// `user_agent`: CharField (source `user_agent`).
    pub user_agent: Option<&'a str>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> APIActivityLogSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "token_identifier",
        "path",
        "method",
        "query_params",
        "headers",
        "body",
        "response_code",
        "response_body",
        "ip_address",
        "user_agent",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for APIActivityLogSerializer<'a> {
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
        if ctx.emits("token_identifier", Self::MODE) {
            obj.field_dyn("token_identifier", ctx, &self.token_identifier)?;
        }
        if ctx.emits("path", Self::MODE) {
            obj.field_dyn("path", ctx, &self.path)?;
        }
        if ctx.emits("method", Self::MODE) {
            obj.field_dyn("method", ctx, &self.method)?;
        }
        if ctx.emits("query_params", Self::MODE) {
            obj.field_dyn("query_params", ctx, &self.query_params)?;
        }
        if ctx.emits("headers", Self::MODE) {
            obj.field_dyn("headers", ctx, &self.headers)?;
        }
        if ctx.emits("body", Self::MODE) {
            obj.field_dyn("body", ctx, &self.body)?;
        }
        if ctx.emits("response_code", Self::MODE) {
            obj.field_dyn("response_code", ctx, &self.response_code)?;
        }
        if ctx.emits("response_body", Self::MODE) {
            obj.field_dyn("response_body", ctx, &self.response_body)?;
        }
        if ctx.emits("ip_address", Self::MODE) {
            obj.field_dyn("ip_address", ctx, &self.ip_address)?;
        }
        if ctx.emits("user_agent", Self::MODE) {
            obj.field_dyn("user_agent", ctx, &self.user_agent)?;
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
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.api.APITokenReadSerializer` (model: `db.APIToken`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct APITokenReadSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `is_active`: SerializerMethodField (source `*`).
    pub is_active: crate::value::DrfValue<'a>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `label`: CharField (source `label`).
    pub label: &'a str,
    /// `description`: CharField (source `description`).
    pub description: &'a str,
    /// `last_used`: DateTimeField (source `last_used`).
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    /// `user_type`: ChoiceField (source `user_type`).
    pub user_type: i64,
    /// `expired_at`: DateTimeField (source `expired_at`).
    pub expired_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: Option<uuid::Uuid>,
}

impl<'a> APITokenReadSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "is_active",
        "created_at",
        "updated_at",
        "deleted_at",
        "label",
        "description",
        "last_used",
        "user_type",
        "expired_at",
        "created_by",
        "updated_by",
        "user",
        "workspace",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for APITokenReadSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_value_dyn("is_active", ctx, &self.is_active)?;
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
        if ctx.emits("label", Self::MODE) {
            obj.field_dyn("label", ctx, &self.label)?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_dyn("description", ctx, &self.description)?;
        }
        if ctx.emits("last_used", Self::MODE) {
            obj.field_dyn(
                "last_used",
                ctx,
                &crate::fields::drf_datetime_opt(self.last_used, ctx.tz()),
            )?;
        }
        if ctx.emits("user_type", Self::MODE) {
            obj.field_dyn("user_type", ctx, &self.user_type)?;
        }
        if ctx.emits("expired_at", Self::MODE) {
            obj.field_dyn(
                "expired_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.expired_at, ctx.tz()),
            )?;
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
        if ctx.emits("user", Self::MODE) {
            obj.field_dyn("user", ctx, &crate::fields::DrfUuid(self.user))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn(
                "workspace",
                ctx,
                &self.workspace.map(crate::fields::DrfUuid),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.api.APITokenSerializer` (model: `db.APIToken`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct APITokenSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `label`: CharField (source `label`).
    pub label: &'a str,
    /// `description`: CharField (source `description`).
    pub description: &'a str,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `last_used`: DateTimeField (source `last_used`).
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    /// `token`: CharField (source `token`).
    pub token: &'a str,
    /// `user_type`: ChoiceField (source `user_type`).
    pub user_type: i64,
    /// `expired_at`: DateTimeField (source `expired_at`).
    pub expired_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: Option<uuid::Uuid>,
}

impl<'a> APITokenSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "label",
        "description",
        "is_active",
        "last_used",
        "token",
        "user_type",
        "expired_at",
        "created_by",
        "updated_by",
        "user",
        "workspace",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for APITokenSerializer<'a> {
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
        if ctx.emits("label", Self::MODE) {
            obj.field_dyn("label", ctx, &self.label)?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_dyn("description", ctx, &self.description)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
        }
        if ctx.emits("last_used", Self::MODE) {
            obj.field_dyn(
                "last_used",
                ctx,
                &crate::fields::drf_datetime_opt(self.last_used, ctx.tz()),
            )?;
        }
        if ctx.emits("token", Self::MODE) {
            obj.field_dyn("token", ctx, &self.token)?;
        }
        if ctx.emits("user_type", Self::MODE) {
            obj.field_dyn("user_type", ctx, &self.user_type)?;
        }
        if ctx.emits("expired_at", Self::MODE) {
            obj.field_dyn(
                "expired_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.expired_at, ctx.tz()),
            )?;
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
        if ctx.emits("user", Self::MODE) {
            obj.field_dyn("user", ctx, &crate::fields::DrfUuid(self.user))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn(
                "workspace",
                ctx,
                &self.workspace.map(crate::fields::DrfUuid),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
