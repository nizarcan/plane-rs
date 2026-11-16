// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.asset` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.asset.FileAssetSerializer` (model: `db.FileAsset`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct FileAssetSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `attributes`: JSONField (source `attributes`).
    pub attributes: Option<crate::fields::PgJson<'a>>,
    /// `asset`: FileField (source `asset`).
    pub asset: &'a str,
    /// `entity_type`: CharField (source `entity_type`).
    pub entity_type: Option<&'a str>,
    /// `entity_identifier`: CharField (source `entity_identifier`).
    pub entity_identifier: Option<&'a str>,
    /// `is_deleted`: BooleanField (source `is_deleted`).
    pub is_deleted: bool,
    /// `is_archived`: BooleanField (source `is_archived`).
    pub is_archived: bool,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `size`: FloatField (source `size`).
    pub size: f64,
    /// `is_uploaded`: BooleanField (source `is_uploaded`).
    pub is_uploaded: bool,
    /// `storage_metadata`: JSONField (source `storage_metadata`).
    pub storage_metadata: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: Option<uuid::Uuid>,
    /// `draft_issue`: PrimaryKeyRelatedField (source `draft_issue`).
    pub draft_issue: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: Option<uuid::Uuid>,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: Option<uuid::Uuid>,
    /// `comment`: PrimaryKeyRelatedField (source `comment`).
    pub comment: Option<uuid::Uuid>,
    /// `page`: PrimaryKeyRelatedField (source `page`).
    pub page: Option<uuid::Uuid>,
}

impl<'a> FileAssetSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "attributes",
        "asset",
        "entity_type",
        "entity_identifier",
        "is_deleted",
        "is_archived",
        "external_id",
        "external_source",
        "size",
        "is_uploaded",
        "storage_metadata",
        "created_by",
        "updated_by",
        "user",
        "workspace",
        "draft_issue",
        "project",
        "issue",
        "comment",
        "page",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for FileAssetSerializer<'a> {
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
        if ctx.emits("attributes", Self::MODE) {
            obj.field_json_dyn("attributes", ctx, self.attributes)?;
        }
        if ctx.emits("asset", Self::MODE) {
            obj.field_dyn("asset", ctx, &self.asset)?;
        }
        if ctx.emits("entity_type", Self::MODE) {
            obj.field_dyn("entity_type", ctx, &self.entity_type)?;
        }
        if ctx.emits("entity_identifier", Self::MODE) {
            obj.field_dyn("entity_identifier", ctx, &self.entity_identifier)?;
        }
        if ctx.emits("is_deleted", Self::MODE) {
            obj.field_dyn("is_deleted", ctx, &self.is_deleted)?;
        }
        if ctx.emits("is_archived", Self::MODE) {
            obj.field_dyn("is_archived", ctx, &self.is_archived)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("size", Self::MODE) {
            obj.field_dyn("size", ctx, &self.size)?;
        }
        if ctx.emits("is_uploaded", Self::MODE) {
            obj.field_dyn("is_uploaded", ctx, &self.is_uploaded)?;
        }
        if ctx.emits("storage_metadata", Self::MODE) {
            obj.field_json_dyn("storage_metadata", ctx, self.storage_metadata)?;
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
            obj.field_dyn("user", ctx, &self.user.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn(
                "workspace",
                ctx,
                &self.workspace.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("draft_issue", Self::MODE) {
            obj.field_dyn(
                "draft_issue",
                ctx,
                &self.draft_issue.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &self.project.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &self.issue.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("comment", Self::MODE) {
            obj.field_dyn("comment", ctx, &self.comment.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("page", Self::MODE) {
            obj.field_dyn("page", ctx, &self.page.map(crate::fields::DrfUuid))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
