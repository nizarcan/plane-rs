// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.api.serializers.issue` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.api.serializers.issue.IssueActivitySerializer` (model: `db.IssueActivity`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueActivitySerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `verb`: CharField (source `verb`).
    pub verb: &'a str,
    /// `field`: CharField (source `field`).
    pub field: Option<&'a str>,
    /// `old_value`: CharField (source `old_value`).
    pub old_value: Option<&'a str>,
    /// `new_value`: CharField (source `new_value`).
    pub new_value: Option<&'a str>,
    /// `comment`: CharField (source `comment`).
    pub comment: &'a str,
    /// `attachments`: ListField (source `attachments`).
    pub attachments: &'a [&'a str],
    /// `old_identifier`: UUIDField (source `old_identifier`).
    pub old_identifier: Option<uuid::Uuid>,
    /// `new_identifier`: UUIDField (source `new_identifier`).
    pub new_identifier: Option<uuid::Uuid>,
    /// `epoch`: FloatField (source `epoch`).
    pub epoch: Option<f64>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: Option<uuid::Uuid>,
    /// `issue_comment`: PrimaryKeyRelatedField (source `issue_comment`).
    pub issue_comment: Option<uuid::Uuid>,
    /// `actor`: PrimaryKeyRelatedField (source `actor`).
    pub actor: Option<uuid::Uuid>,
}

impl<'a> IssueActivitySerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "verb",
        "field",
        "old_value",
        "new_value",
        "comment",
        "attachments",
        "old_identifier",
        "new_identifier",
        "epoch",
        "project",
        "workspace",
        "issue",
        "issue_comment",
        "actor",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueActivitySerializer<'a> {
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
        if ctx.emits("verb", Self::MODE) {
            obj.field_dyn("verb", ctx, &self.verb)?;
        }
        if ctx.emits("field", Self::MODE) {
            obj.field_dyn("field", ctx, &self.field)?;
        }
        if ctx.emits("old_value", Self::MODE) {
            obj.field_dyn("old_value", ctx, &self.old_value)?;
        }
        if ctx.emits("new_value", Self::MODE) {
            obj.field_dyn("new_value", ctx, &self.new_value)?;
        }
        if ctx.emits("comment", Self::MODE) {
            obj.field_dyn("comment", ctx, &self.comment)?;
        }
        if ctx.emits("attachments", Self::MODE) {
            obj.field_dyn(
                "attachments",
                ctx,
                &crate::fields::DrfStrList(self.attachments),
            )?;
        }
        if ctx.emits("old_identifier", Self::MODE) {
            obj.field_dyn(
                "old_identifier",
                ctx,
                &self.old_identifier.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("new_identifier", Self::MODE) {
            obj.field_dyn(
                "new_identifier",
                ctx,
                &self.new_identifier.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("epoch", Self::MODE) {
            obj.field_dyn("epoch", ctx, &self.epoch)?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &self.issue.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("issue_comment", Self::MODE) {
            obj.field_dyn(
                "issue_comment",
                ctx,
                &self.issue_comment.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("actor", Self::MODE) {
            obj.field_dyn("actor", ctx, &self.actor.map(crate::fields::DrfUuid))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueAttachmentSerializer` (model: `db.FileAsset`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueAttachmentSerializer<'a> {
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

impl<'a> IssueAttachmentSerializer<'a> {
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueAttachmentSerializer<'a> {
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

/// `plane.api.serializers.issue.IssueAttachmentUploadSerializer` (model: `no model`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueAttachmentUploadSerializer<'a> {
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `type`: CharField (source `type`).
    pub r#type: &'a str,
    /// `size`: IntegerField (source `size`).
    pub size: i64,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: &'a str,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: &'a str,
}

impl<'a> IssueAttachmentUploadSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] =
        &["name", "type", "size", "external_id", "external_source"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueAttachmentUploadSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("type", Self::MODE) {
            obj.field_dyn("type", ctx, &self.r#type)?;
        }
        if ctx.emits("size", Self::MODE) {
            obj.field_dyn("size", ctx, &self.size)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueCommentCreateSerializer` (model: `db.IssueComment`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueCommentCreateSerializer<'a> {
    /// `comment_json`: JSONField (source `comment_json`).
    pub comment_json: Option<crate::fields::PgJson<'a>>,
    /// `comment_html`: CharField (source `comment_html`).
    pub comment_html: &'a str,
    /// `access`: ChoiceField (source `access`).
    pub access: &'a str,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
}

impl<'a> IssueCommentCreateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "comment_json",
        "comment_html",
        "access",
        "external_source",
        "external_id",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueCommentCreateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("comment_json", Self::MODE) {
            obj.field_json_dyn("comment_json", ctx, self.comment_json)?;
        }
        if ctx.emits("comment_html", Self::MODE) {
            obj.field_dyn("comment_html", ctx, &self.comment_html)?;
        }
        if ctx.emits("access", Self::MODE) {
            obj.field_dyn("access", ctx, &self.access)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueCommentSerializer` (model: `db.IssueComment`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueCommentSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `is_member`: BooleanField (source `is_member`).
    pub is_member: Option<bool>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `comment_html`: CharField (source `comment_html`).
    pub comment_html: &'a str,
    /// `attachments`: ListField (source `attachments`).
    pub attachments: &'a [&'a str],
    /// `access`: ChoiceField (source `access`).
    pub access: &'a str,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `edited_at`: DateTimeField (source `edited_at`).
    pub edited_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `description`: PrimaryKeyRelatedField (source `description`).
    pub description: Option<uuid::Uuid>,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
    /// `actor`: PrimaryKeyRelatedField (source `actor`).
    pub actor: Option<uuid::Uuid>,
    /// `parent`: PrimaryKeyRelatedField (source `parent`).
    pub parent: Option<uuid::Uuid>,
}

impl<'a> IssueCommentSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "is_member",
        "created_at",
        "updated_at",
        "deleted_at",
        "comment_html",
        "attachments",
        "access",
        "external_source",
        "external_id",
        "edited_at",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "description",
        "issue",
        "actor",
        "parent",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueCommentSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("is_member", Self::MODE)
            && let Some(value) = &self.is_member
        {
            obj.field_dyn("is_member", ctx, &value)?;
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
        if ctx.emits("comment_html", Self::MODE) {
            obj.field_dyn("comment_html", ctx, &self.comment_html)?;
        }
        if ctx.emits("attachments", Self::MODE) {
            obj.field_dyn(
                "attachments",
                ctx,
                &crate::fields::DrfStrList(self.attachments),
            )?;
        }
        if ctx.emits("access", Self::MODE) {
            obj.field_dyn("access", ctx, &self.access)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
        }
        if ctx.emits("edited_at", Self::MODE) {
            obj.field_dyn(
                "edited_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.edited_at, ctx.tz()),
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
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_dyn(
                "description",
                ctx,
                &self.description.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        if ctx.emits("actor", Self::MODE) {
            obj.field_dyn("actor", ctx, &self.actor.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("parent", Self::MODE) {
            obj.field_dyn("parent", ctx, &self.parent.map(crate::fields::DrfUuid))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueExpandSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueExpandSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `cycle`: CycleLiteSerializer (source `issue_cycle.cycle`).
    pub cycle: Option<&'a crate::generated::api_cycle::CycleLiteSerializer<'a>>,
    /// `module`: ModuleLiteSerializer (source `issue_module.module`).
    pub module: Option<&'a crate::generated::api_module::ModuleLiteSerializer<'a>>,
    /// `labels`: SerializerMethodField (source `*`).
    pub labels: crate::value::DrfValue<'a>,
    /// `assignees`: SerializerMethodField (source `*`).
    pub assignees: crate::value::DrfValue<'a>,
    /// `state`: StateLiteSerializer (source `state`).
    pub state: Option<&'a crate::generated::api_state::StateLiteSerializer<'a>>,
    /// `description`: JSONField (source `description_json`).
    pub description: Option<crate::fields::PgJson<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `point`: IntegerField (source `point`).
    pub point: Option<i64>,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `description_json`: JSONField (source `description_json`).
    pub description_json: Option<crate::fields::PgJson<'a>>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
    /// `description_stripped`: CharField (source `description_stripped`).
    pub description_stripped: Option<&'a str>,
    /// `description_binary`: ModelField (source `description_binary`).
    pub description_binary: Option<&'a [u8]>,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
    /// `start_date`: DateField (source `start_date`).
    pub start_date: Option<chrono::NaiveDate>,
    /// `target_date`: DateField (source `target_date`).
    pub target_date: Option<chrono::NaiveDate>,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `completed_at`: DateTimeField (source `completed_at`).
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `archived_at`: DateField (source `archived_at`).
    pub archived_at: Option<chrono::NaiveDate>,
    /// `is_draft`: BooleanField (source `is_draft`).
    pub is_draft: bool,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `parent`: PrimaryKeyRelatedField (source `parent`).
    pub parent: Option<uuid::Uuid>,
    /// `estimate_point`: PrimaryKeyRelatedField (source `estimate_point`).
    pub estimate_point: Option<uuid::Uuid>,
    /// `type`: PrimaryKeyRelatedField (source `type`).
    pub r#type: Option<uuid::Uuid>,
}

impl<'a> IssueExpandSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "cycle",
        "module",
        "labels",
        "assignees",
        "state",
        "description",
        "created_at",
        "updated_at",
        "deleted_at",
        "point",
        "name",
        "description_json",
        "description_html",
        "description_stripped",
        "description_binary",
        "priority",
        "start_date",
        "target_date",
        "sequence_id",
        "sort_order",
        "completed_at",
        "archived_at",
        "is_draft",
        "external_source",
        "external_id",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "parent",
        "estimate_point",
        "type",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueExpandSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("cycle", Self::MODE) {
            obj.field_nested_dyn(
                "cycle",
                ctx,
                self.cycle
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("module", Self::MODE) {
            obj.field_nested_dyn(
                "module",
                ctx,
                self.module
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("labels", Self::MODE) {
            obj.field_value_dyn("labels", ctx, &self.labels)?;
        }
        if ctx.emits("assignees", Self::MODE) {
            obj.field_value_dyn("assignees", ctx, &self.assignees)?;
        }
        if ctx.emits("state", Self::MODE) {
            obj.field_nested_dyn(
                "state",
                ctx,
                self.state
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_json_dyn("description", ctx, self.description)?;
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
        if ctx.emits("point", Self::MODE) {
            obj.field_dyn("point", ctx, &self.point)?;
        }
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("description_json", Self::MODE) {
            obj.field_json_dyn("description_json", ctx, self.description_json)?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        if ctx.emits("description_stripped", Self::MODE) {
            obj.field_dyn("description_stripped", ctx, &self.description_stripped)?;
        }
        if ctx.emits("description_binary", Self::MODE) {
            obj.field_dyn(
                "description_binary",
                ctx,
                &self.description_binary.map(crate::fields::PyBase64),
            )?;
        }
        if ctx.emits("priority", Self::MODE) {
            obj.field_dyn("priority", ctx, &self.priority)?;
        }
        if ctx.emits("start_date", Self::MODE) {
            obj.field_dyn(
                "start_date",
                ctx,
                &self.start_date.map(crate::fields::PyDate),
            )?;
        }
        if ctx.emits("target_date", Self::MODE) {
            obj.field_dyn(
                "target_date",
                ctx,
                &self.target_date.map(crate::fields::PyDate),
            )?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("completed_at", Self::MODE) {
            obj.field_dyn(
                "completed_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.completed_at, ctx.tz()),
            )?;
        }
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &self.archived_at.map(crate::fields::PyDate),
            )?;
        }
        if ctx.emits("is_draft", Self::MODE) {
            obj.field_dyn("is_draft", ctx, &self.is_draft)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
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
        if ctx.emits("parent", Self::MODE) {
            obj.field_dyn("parent", ctx, &self.parent.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("estimate_point", Self::MODE) {
            obj.field_dyn(
                "estimate_point",
                ctx,
                &self.estimate_point.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("type", Self::MODE) {
            obj.field_dyn("type", ctx, &self.r#type.map(crate::fields::DrfUuid))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueLinkCreateSerializer` (model: `db.IssueLink`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueLinkCreateSerializer<'a> {
    /// `title`: CharField (source `title`).
    pub title: Option<&'a str>,
    /// `url`: CharField (source `url`).
    pub url: &'a str,
    /// `issue_id`: ReadOnlyField (source `issue_id`).
    pub issue_id: uuid::Uuid,
}

impl<'a> IssueLinkCreateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["title", "url", "issue_id"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueLinkCreateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("title", Self::MODE) {
            obj.field_dyn("title", ctx, &self.title)?;
        }
        if ctx.emits("url", Self::MODE) {
            obj.field_dyn("url", ctx, &self.url)?;
        }
        if ctx.emits("issue_id", Self::MODE) {
            obj.field_dyn("issue_id", ctx, &crate::fields::DrfUuid(self.issue_id))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueLinkSerializer` (model: `db.IssueLink`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueLinkSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `title`: CharField (source `title`).
    pub title: Option<&'a str>,
    /// `url`: CharField (source `url`).
    pub url: &'a str,
    /// `metadata`: JSONField (source `metadata`).
    pub metadata: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
}

impl<'a> IssueLinkSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "title",
        "url",
        "metadata",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "issue",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueLinkSerializer<'a> {
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
        if ctx.emits("title", Self::MODE) {
            obj.field_dyn("title", ctx, &self.title)?;
        }
        if ctx.emits("url", Self::MODE) {
            obj.field_dyn("url", ctx, &self.url)?;
        }
        if ctx.emits("metadata", Self::MODE) {
            obj.field_json_dyn("metadata", ctx, self.metadata)?;
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
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueLinkUpdateSerializer` (model: `db.IssueLink`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueLinkUpdateSerializer<'a> {
    /// `title`: CharField (source `title`).
    pub title: Option<&'a str>,
    /// `url`: CharField (source `url`).
    pub url: &'a str,
    /// `issue_id`: ReadOnlyField (source `issue_id`).
    pub issue_id: uuid::Uuid,
}

impl<'a> IssueLinkUpdateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["title", "url", "issue_id"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueLinkUpdateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("title", Self::MODE) {
            obj.field_dyn("title", ctx, &self.title)?;
        }
        if ctx.emits("url", Self::MODE) {
            obj.field_dyn("url", ctx, &self.url)?;
        }
        if ctx.emits("issue_id", Self::MODE) {
            obj.field_dyn("issue_id", ctx, &crate::fields::DrfUuid(self.issue_id))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueLiteSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueLiteSerializer {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: uuid::Uuid,
}

impl IssueLiteSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["id", "sequence_id", "project_id"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl crate::ser::DrfSerialize for IssueLiteSerializer {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueRelationCreateSerializer` (model: `no model`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueRelationCreateSerializer<'a> {
    /// `relation_type`: ChoiceField (source `relation_type`).
    pub relation_type: &'a str,
    /// `issues`: ListField (source `issues`).
    pub issues: &'a [uuid::Uuid],
}

impl<'a> IssueRelationCreateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["relation_type", "issues"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueRelationCreateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("relation_type", Self::MODE) {
            obj.field_dyn("relation_type", ctx, &self.relation_type)?;
        }
        if ctx.emits("issues", Self::MODE) {
            obj.field_dyn("issues", ctx, &crate::fields::DrfUuidList(self.issues))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueRelationRefSerializer` (model: `no model`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueRelationRefSerializer {
    /// `project_id`: UUIDField (source `project_id`).
    pub project_id: uuid::Uuid,
    /// `issue_id`: UUIDField (source `issue_id`).
    pub issue_id: uuid::Uuid,
}

impl IssueRelationRefSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["project_id", "issue_id"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl crate::ser::DrfSerialize for IssueRelationRefSerializer {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        if ctx.emits("issue_id", Self::MODE) {
            obj.field_dyn("issue_id", ctx, &crate::fields::DrfUuid(self.issue_id))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueRelationRemoveSerializer` (model: `no model`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueRelationRemoveSerializer {
    /// `related_issue`: UUIDField (source `related_issue`).
    pub related_issue: uuid::Uuid,
}

impl IssueRelationRemoveSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["related_issue"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl crate::ser::DrfSerialize for IssueRelationRemoveSerializer {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("related_issue", Self::MODE) {
            obj.field_dyn(
                "related_issue",
                ctx,
                &crate::fields::DrfUuid(self.related_issue),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.IssueRelationSerializer` (model: `db.IssueRelation`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueRelationSerializer<'a> {
    /// `id`: UUIDField (source `related_issue.id`).
    pub id: uuid::Uuid,
    /// `project_id`: UUIDField (source `related_issue.project_id`).
    pub project_id: uuid::Uuid,
    /// `sequence_id`: IntegerField (source `related_issue.sequence_id`).
    pub sequence_id: i64,
    /// `relation_type`: CharField (source `relation_type`).
    pub relation_type: &'a str,
    /// `name`: CharField (source `related_issue.name`).
    pub name: &'a str,
    /// `state_id`: UUIDField (source `related_issue.state.id`).
    pub state_id: uuid::Uuid,
    /// `priority`: CharField (source `related_issue.priority`).
    pub priority: &'a str,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> IssueRelationSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "project_id",
        "sequence_id",
        "relation_type",
        "name",
        "state_id",
        "priority",
        "created_by",
        "created_at",
        "updated_at",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueRelationSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("relation_type", Self::MODE) {
            obj.field_dyn("relation_type", ctx, &self.relation_type)?;
        }
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("state_id", Self::MODE) {
            obj.field_dyn("state_id", ctx, &crate::fields::DrfUuid(self.state_id))?;
        }
        if ctx.emits("priority", Self::MODE) {
            obj.field_dyn("priority", ctx, &self.priority)?;
        }
        if ctx.emits("created_by", Self::MODE) {
            obj.field_dyn(
                "created_by",
                ctx,
                &self.created_by.map(crate::fields::DrfUuid),
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

/// `plane.api.serializers.issue.IssueSearchSerializer` (model: `no model`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueSearchSerializer<'a> {
    /// `id`: CharField (source `id`).
    pub id: &'a str,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `sequence_id`: CharField (source `sequence_id`).
    pub sequence_id: &'a str,
    /// `project__identifier`: CharField (source `project__identifier`).
    pub project_identifier: &'a str,
    /// `project_id`: CharField (source `project_id`).
    pub project_id: &'a str,
    /// `workspace__slug`: CharField (source `workspace__slug`).
    pub workspace_slug: &'a str,
}

impl<'a> IssueSearchSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "sequence_id",
        "project__identifier",
        "project_id",
        "workspace__slug",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueSearchSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &self.id)?;
        }
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("project__identifier", Self::MODE) {
            obj.field_dyn("project__identifier", ctx, &self.project_identifier)?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &self.project_id)?;
        }
        if ctx.emits("workspace__slug", Self::MODE) {
            obj.field_dyn("workspace__slug", ctx, &self.workspace_slug)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.LabelCreateUpdateSerializer` (model: `db.Label`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct LabelCreateUpdateSerializer<'a> {
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `color`: CharField (source `color`).
    pub color: &'a str,
    /// `description`: CharField (source `description`).
    pub description: &'a str,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `parent`: PrimaryKeyRelatedField (source `parent`).
    pub parent: Option<uuid::Uuid>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
}

impl<'a> LabelCreateUpdateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "name",
        "color",
        "description",
        "external_source",
        "external_id",
        "parent",
        "sort_order",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for LabelCreateUpdateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("color", Self::MODE) {
            obj.field_dyn("color", ctx, &self.color)?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_dyn("description", ctx, &self.description)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
        }
        if ctx.emits("parent", Self::MODE) {
            obj.field_dyn("parent", ctx, &self.parent.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.LabelLiteSerializer` (model: `db.Label`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct LabelLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `color`: CharField (source `color`).
    pub color: &'a str,
}

impl<'a> LabelLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["id", "name", "color"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for LabelLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("color", Self::MODE) {
            obj.field_dyn("color", ctx, &self.color)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.LabelSerializer` (model: `db.Label`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct LabelSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `description`: CharField (source `description`).
    pub description: &'a str,
    /// `color`: CharField (source `color`).
    pub color: &'a str,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: Option<uuid::Uuid>,
    /// `parent`: PrimaryKeyRelatedField (source `parent`).
    pub parent: Option<uuid::Uuid>,
}

impl<'a> LabelSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "description",
        "color",
        "sort_order",
        "external_source",
        "external_id",
        "created_by",
        "updated_by",
        "workspace",
        "project",
        "parent",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for LabelSerializer<'a> {
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
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_dyn("description", ctx, &self.description)?;
        }
        if ctx.emits("color", Self::MODE) {
            obj.field_dyn("color", ctx, &self.color)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
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
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &self.project.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("parent", Self::MODE) {
            obj.field_dyn("parent", ctx, &self.parent.map(crate::fields::DrfUuid))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.issue.RelatedIssueSerializer` (model: `db.IssueRelation`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct RelatedIssueSerializer<'a> {
    /// `id`: UUIDField (source `issue.id`).
    pub id: uuid::Uuid,
    /// `project_id`: PrimaryKeyRelatedField (source `issue.project_id`).
    pub project_id: uuid::Uuid,
    /// `sequence_id`: IntegerField (source `issue.sequence_id`).
    pub sequence_id: i64,
    /// `relation_type`: CharField (source `relation_type`).
    pub relation_type: &'a str,
    /// `name`: CharField (source `issue.name`).
    pub name: &'a str,
    /// `type_id`: UUIDField (source `issue.type.id`).
    pub type_id: uuid::Uuid,
    /// `is_epic`: BooleanField (source `issue.type.is_epic`).
    pub is_epic: bool,
    /// `state_id`: UUIDField (source `issue.state.id`).
    pub state_id: uuid::Uuid,
    /// `priority`: CharField (source `issue.priority`).
    pub priority: &'a str,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl<'a> RelatedIssueSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "project_id",
        "sequence_id",
        "relation_type",
        "name",
        "type_id",
        "is_epic",
        "state_id",
        "priority",
        "created_by",
        "created_at",
        "updated_by",
        "updated_at",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for RelatedIssueSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("relation_type", Self::MODE) {
            obj.field_dyn("relation_type", ctx, &self.relation_type)?;
        }
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("type_id", Self::MODE) {
            obj.field_dyn("type_id", ctx, &crate::fields::DrfUuid(self.type_id))?;
        }
        if ctx.emits("is_epic", Self::MODE) {
            obj.field_dyn("is_epic", ctx, &self.is_epic)?;
        }
        if ctx.emits("state_id", Self::MODE) {
            obj.field_dyn("state_id", ctx, &crate::fields::DrfUuid(self.state_id))?;
        }
        if ctx.emits("priority", Self::MODE) {
            obj.field_dyn("priority", ctx, &self.priority)?;
        }
        if ctx.emits("created_by", Self::MODE) {
            obj.field_dyn(
                "created_by",
                ctx,
                &self.created_by.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("created_at", Self::MODE) {
            obj.field_dyn(
                "created_at",
                ctx,
                &crate::fields::drf_datetime(self.created_at, ctx.tz()),
            )?;
        }
        if ctx.emits("updated_by", Self::MODE) {
            obj.field_dyn(
                "updated_by",
                ctx,
                &self.updated_by.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("updated_at", Self::MODE) {
            obj.field_dyn(
                "updated_at",
                ctx,
                &crate::fields::drf_datetime(self.updated_at, ctx.tz()),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
