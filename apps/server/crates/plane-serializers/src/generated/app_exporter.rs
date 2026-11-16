// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.exporter` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.exporter.ExporterHistorySerializer` (model: `db.ExporterHistory`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ExporterHistorySerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `project`: ListField (source `project`).
    pub project: Option<&'a [uuid::Uuid]>,
    /// `provider`: ChoiceField (source `provider`).
    pub provider: &'a str,
    /// `status`: ChoiceField (source `status`).
    pub status: &'a str,
    /// `url`: URLField (source `url`).
    pub url: Option<&'a str>,
    /// `initiated_by`: PrimaryKeyRelatedField (source `initiated_by`).
    pub initiated_by: uuid::Uuid,
    /// `initiated_by_detail`: UserLiteSerializer (source `initiated_by`).
    pub initiated_by_detail: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `token`: CharField (source `token`).
    pub token: &'a str,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> ExporterHistorySerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "project",
        "provider",
        "status",
        "url",
        "initiated_by",
        "initiated_by_detail",
        "token",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ExporterHistorySerializer<'a> {
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
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn(
                "project",
                ctx,
                &self.project.map(crate::fields::DrfUuidList),
            )?;
        }
        if ctx.emits("provider", Self::MODE) {
            obj.field_dyn("provider", ctx, &self.provider)?;
        }
        if ctx.emits("status", Self::MODE) {
            obj.field_dyn("status", ctx, &self.status)?;
        }
        if ctx.emits("url", Self::MODE) {
            obj.field_dyn("url", ctx, &self.url)?;
        }
        if ctx.emits("initiated_by", Self::MODE) {
            obj.field_dyn(
                "initiated_by",
                ctx,
                &crate::fields::DrfUuid(self.initiated_by),
            )?;
        }
        if ctx.emits("initiated_by_detail", Self::MODE) {
            obj.field_nested_dyn(
                "initiated_by_detail",
                ctx,
                self.initiated_by_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("token", Self::MODE) {
            obj.field_dyn("token", ctx, &self.token)?;
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
