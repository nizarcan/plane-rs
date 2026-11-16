// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.importer` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.importer.ImporterSerializer` (model: `db.Importer`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ImporterSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `initiated_by_detail`: UserLiteSerializer (source `initiated_by`).
    pub initiated_by_detail: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `workspace_detail`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace_detail: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `service`: ChoiceField (source `service`).
    pub service: &'a str,
    /// `status`: ChoiceField (source `status`).
    pub status: &'a str,
    /// `metadata`: JSONField (source `metadata`).
    pub metadata: Option<crate::fields::PgJson<'a>>,
    /// `config`: JSONField (source `config`).
    pub config: Option<crate::fields::PgJson<'a>>,
    /// `data`: JSONField (source `data`).
    pub data: Option<crate::fields::PgJson<'a>>,
    /// `imported_data`: JSONField (source `imported_data`).
    pub imported_data: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `initiated_by`: PrimaryKeyRelatedField (source `initiated_by`).
    pub initiated_by: uuid::Uuid,
    /// `token`: PrimaryKeyRelatedField (source `token`).
    pub token: uuid::Uuid,
}

impl<'a> ImporterSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "initiated_by_detail",
        "project_detail",
        "workspace_detail",
        "created_at",
        "updated_at",
        "deleted_at",
        "service",
        "status",
        "metadata",
        "config",
        "data",
        "imported_data",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "initiated_by",
        "token",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ImporterSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("initiated_by_detail", Self::MODE) {
            obj.field_nested_dyn(
                "initiated_by_detail",
                ctx,
                self.initiated_by_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("project_detail", Self::MODE) {
            obj.field_nested_dyn(
                "project_detail",
                ctx,
                self.project_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("workspace_detail", Self::MODE) {
            obj.field_nested_dyn(
                "workspace_detail",
                ctx,
                self.workspace_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
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
        if ctx.emits("service", Self::MODE) {
            obj.field_dyn("service", ctx, &self.service)?;
        }
        if ctx.emits("status", Self::MODE) {
            obj.field_dyn("status", ctx, &self.status)?;
        }
        if ctx.emits("metadata", Self::MODE) {
            obj.field_json_dyn("metadata", ctx, self.metadata)?;
        }
        if ctx.emits("config", Self::MODE) {
            obj.field_json_dyn("config", ctx, self.config)?;
        }
        if ctx.emits("data", Self::MODE) {
            obj.field_json_dyn("data", ctx, self.data)?;
        }
        if ctx.emits("imported_data", Self::MODE) {
            obj.field_json_dyn("imported_data", ctx, self.imported_data)?;
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
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("initiated_by", Self::MODE) {
            obj.field_dyn(
                "initiated_by",
                ctx,
                &crate::fields::DrfUuid(self.initiated_by),
            )?;
        }
        if ctx.emits("token", Self::MODE) {
            obj.field_dyn("token", ctx, &crate::fields::DrfUuid(self.token))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
