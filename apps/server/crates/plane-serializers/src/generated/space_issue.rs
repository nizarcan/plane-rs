// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.space.serializer.issue` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.space.serializer.issue.CommentReactionLiteSerializer` (model: `db.CommentReaction`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct CommentReactionLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `reaction`: CharField (source `reaction`).
    pub reaction: &'a str,
    /// `comment`: PrimaryKeyRelatedField (source `comment`).
    pub comment: uuid::Uuid,
    /// `actor_detail`: UserLiteSerializer (source `actor`).
    pub actor_detail: Option<&'a crate::generated::space_user::UserLiteSerializer<'a>>,
}

impl<'a> CommentReactionLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["id", "reaction", "comment", "actor_detail"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for CommentReactionLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("reaction", Self::MODE) {
            obj.field_dyn("reaction", ctx, &self.reaction)?;
        }
        if ctx.emits("comment", Self::MODE) {
            obj.field_dyn("comment", ctx, &crate::fields::DrfUuid(self.comment))?;
        }
        if ctx.emits("actor_detail", Self::MODE) {
            obj.field_nested_dyn(
                "actor_detail",
                ctx,
                self.actor_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.CommentReactionSerializer` (model: `db.CommentReaction`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct CommentReactionSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `reaction`: CharField (source `reaction`).
    pub reaction: &'a str,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `actor`: PrimaryKeyRelatedField (source `actor`).
    pub actor: uuid::Uuid,
    /// `comment`: PrimaryKeyRelatedField (source `comment`).
    pub comment: uuid::Uuid,
}

impl<'a> CommentReactionSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "reaction",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "actor",
        "comment",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for CommentReactionSerializer<'a> {
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
        if ctx.emits("reaction", Self::MODE) {
            obj.field_dyn("reaction", ctx, &self.reaction)?;
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
        if ctx.emits("actor", Self::MODE) {
            obj.field_dyn("actor", ctx, &crate::fields::DrfUuid(self.actor))?;
        }
        if ctx.emits("comment", Self::MODE) {
            obj.field_dyn("comment", ctx, &crate::fields::DrfUuid(self.comment))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.IssueAttachmentSerializer` (model: `db.FileAsset`).
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
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

/// `plane.space.serializer.issue.IssueCommentSerializer` (model: `db.IssueComment`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueCommentSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `actor_detail`: UserLiteSerializer (source `actor`).
    pub actor_detail: Option<&'a crate::generated::space_user::UserLiteSerializer<'a>>,
    /// `issue_detail`: IssueFlatSerializer (source `issue`).
    pub issue_detail: Option<&'a crate::generated::space_issue::IssueFlatSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::space_project::ProjectLiteSerializer<'a>>,
    /// `workspace_detail`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace_detail:
        Option<&'a crate::generated::space_workspace::WorkspaceLiteSerializer<'a>>,
    /// `comment_reactions`: ListSerializer (source `comment_reactions`).
    pub comment_reactions: &'a [crate::generated::space_issue::CommentReactionLiteSerializer<'a>],
    /// `is_member`: BooleanField (source `is_member`).
    pub is_member: crate::fields::FieldValue<bool>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `comment_stripped`: CharField (source `comment_stripped`).
    pub comment_stripped: &'a str,
    /// `comment_json`: JSONField (source `comment_json`).
    pub comment_json: Option<crate::fields::PgJson<'a>>,
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
        "actor_detail",
        "issue_detail",
        "project_detail",
        "workspace_detail",
        "comment_reactions",
        "is_member",
        "created_at",
        "updated_at",
        "deleted_at",
        "comment_stripped",
        "comment_json",
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
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
        if ctx.emits("actor_detail", Self::MODE) {
            obj.field_nested_dyn(
                "actor_detail",
                ctx,
                self.actor_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("issue_detail", Self::MODE) {
            obj.field_nested_dyn(
                "issue_detail",
                ctx,
                self.issue_detail
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
        if ctx.emits("comment_reactions", Self::MODE) {
            obj.field_nested_list_dyn("comment_reactions", ctx, self.comment_reactions)?;
        }
        if ctx.emits("is_member", Self::MODE) {
            obj.field_optional_dyn("is_member", ctx, &self.is_member)?;
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
        if ctx.emits("comment_stripped", Self::MODE) {
            obj.field_dyn("comment_stripped", ctx, &self.comment_stripped)?;
        }
        if ctx.emits("comment_json", Self::MODE) {
            obj.field_json_dyn("comment_json", ctx, self.comment_json)?;
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

/// `plane.space.serializer.issue.IssueCycleDetailSerializer` (model: `db.CycleIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueCycleDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `cycle_detail`: CycleBaseSerializer (source `cycle`).
    pub cycle_detail: Option<&'a crate::generated::space_cycle::CycleBaseSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
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
    /// `cycle`: PrimaryKeyRelatedField (source `cycle`).
    pub cycle: uuid::Uuid,
}

impl<'a> IssueCycleDetailSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "cycle_detail",
        "created_at",
        "updated_at",
        "deleted_at",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "issue",
        "cycle",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueCycleDetailSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("cycle_detail", Self::MODE) {
            obj.field_nested_dyn(
                "cycle_detail",
                ctx,
                self.cycle_detail
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
        if ctx.emits("cycle", Self::MODE) {
            obj.field_dyn("cycle", ctx, &crate::fields::DrfUuid(self.cycle))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.IssueFlatSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueFlatSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `description_json`: JSONField (source `description_json`).
    pub description_json: Option<crate::fields::PgJson<'a>>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
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
    /// `is_draft`: BooleanField (source `is_draft`).
    pub is_draft: bool,
}

impl<'a> IssueFlatSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "description_json",
        "description_html",
        "priority",
        "start_date",
        "target_date",
        "sequence_id",
        "sort_order",
        "is_draft",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueFlatSerializer<'a> {
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
        if ctx.emits("description_json", Self::MODE) {
            obj.field_json_dyn("description_json", ctx, self.description_json)?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
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
        if ctx.emits("is_draft", Self::MODE) {
            obj.field_dyn("is_draft", ctx, &self.is_draft)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.IssueLinkSerializer` (model: `db.IssueLink`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueLinkSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_by_detail`: UserLiteSerializer (source `created_by`).
    pub created_by_detail: Option<&'a crate::generated::space_user::UserLiteSerializer<'a>>,
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
        "created_by_detail",
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
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
        if ctx.emits("created_by_detail", Self::MODE) {
            obj.field_nested_dyn(
                "created_by_detail",
                ctx,
                self.created_by_detail
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

/// `plane.space.serializer.issue.IssueModuleDetailSerializer` (model: `db.ModuleIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueModuleDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `module_detail`: ModuleBaseSerializer (source `module`).
    pub module_detail: Option<&'a crate::generated::space_module::ModuleBaseSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `module`: PrimaryKeyRelatedField (source `module`).
    pub module: uuid::Uuid,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
}

impl<'a> IssueModuleDetailSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "module_detail",
        "created_at",
        "updated_at",
        "deleted_at",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "module",
        "issue",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueModuleDetailSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("module_detail", Self::MODE) {
            obj.field_nested_dyn(
                "module_detail",
                ctx,
                self.module_detail
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
        if ctx.emits("module", Self::MODE) {
            obj.field_dyn("module", ctx, &crate::fields::DrfUuid(self.module))?;
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

/// `plane.space.serializer.issue.IssueProjectLiteSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueProjectLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::space_project::ProjectLiteSerializer<'a>>,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
}

impl<'a> IssueProjectLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["id", "project_detail", "name", "sequence_id"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueProjectLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("project_detail", Self::MODE) {
            obj.field_nested_dyn(
                "project_detail",
                ctx,
                self.project_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.IssuePublicSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssuePublicSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `state`: PrimaryKeyRelatedField (source `state`).
    pub state: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
    /// `target_date`: DateField (source `target_date`).
    pub target_date: Option<chrono::NaiveDate>,
    /// `reactions`: ListSerializer (source `issue_reactions`).
    pub reactions: &'a [crate::generated::space_issue::IssueReactionSerializer<'a>],
    /// `votes`: ListSerializer (source `votes`).
    pub votes: &'a [crate::generated::space_issue::IssueVoteSerializer],
    /// `module_ids`: ListField (source `module_ids`).
    pub module_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `label_ids`: ListField (source `label_ids`).
    pub label_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `assignee_ids`: ListField (source `assignee_ids`).
    pub assignee_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
}

impl<'a> IssuePublicSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "sequence_id",
        "state",
        "project",
        "workspace",
        "priority",
        "target_date",
        "reactions",
        "votes",
        "module_ids",
        "created_by",
        "label_ids",
        "assignee_ids",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssuePublicSerializer<'a> {
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
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("state", Self::MODE) {
            obj.field_dyn("state", ctx, &self.state.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("priority", Self::MODE) {
            obj.field_dyn("priority", ctx, &self.priority)?;
        }
        if ctx.emits("target_date", Self::MODE) {
            obj.field_dyn(
                "target_date",
                ctx,
                &self.target_date.map(crate::fields::PyDate),
            )?;
        }
        if ctx.emits("reactions", Self::MODE) {
            obj.field_nested_list_dyn("reactions", ctx, self.reactions)?;
        }
        if ctx.emits("votes", Self::MODE) {
            obj.field_nested_list_dyn("votes", ctx, self.votes)?;
        }
        if ctx.emits("module_ids", Self::MODE) {
            obj.field_optional_dyn("module_ids", ctx, &self.module_ids)?;
        }
        if ctx.emits("created_by", Self::MODE) {
            obj.field_dyn(
                "created_by",
                ctx,
                &self.created_by.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("label_ids", Self::MODE) {
            obj.field_optional_dyn("label_ids", ctx, &self.label_ids)?;
        }
        if ctx.emits("assignee_ids", Self::MODE) {
            obj.field_optional_dyn("assignee_ids", ctx, &self.assignee_ids)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.IssueReactionSerializer` (model: `db.IssueReaction`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueReactionSerializer<'a> {
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
    /// `reaction`: CharField (source `reaction`).
    pub reaction: &'a str,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `actor`: PrimaryKeyRelatedField (source `actor`).
    pub actor: uuid::Uuid,
}

impl<'a> IssueReactionSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] =
        &["issue", "reaction", "workspace", "project", "actor"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueReactionSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        if ctx.emits("reaction", Self::MODE) {
            obj.field_dyn("reaction", ctx, &self.reaction)?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("actor", Self::MODE) {
            obj.field_dyn("actor", ctx, &crate::fields::DrfUuid(self.actor))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.IssueRelationSerializer` (model: `db.IssueRelation`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueRelationSerializer<'a> {
    /// `issue_detail`: IssueProjectLiteSerializer (source `related_issue`).
    pub issue_detail: Option<&'a crate::generated::space_issue::IssueProjectLiteSerializer<'a>>,
    /// `relation_type`: CharField (source `relation_type`).
    pub relation_type: &'a str,
    /// `related_issue`: PrimaryKeyRelatedField (source `related_issue`).
    pub related_issue: uuid::Uuid,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
}

impl<'a> IssueRelationSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "issue_detail",
        "relation_type",
        "related_issue",
        "issue",
        "id",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueRelationSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("issue_detail", Self::MODE) {
            obj.field_nested_dyn(
                "issue_detail",
                ctx,
                self.issue_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("relation_type", Self::MODE) {
            obj.field_dyn("relation_type", ctx, &self.relation_type)?;
        }
        if ctx.emits("related_issue", Self::MODE) {
            obj.field_dyn(
                "related_issue",
                ctx,
                &crate::fields::DrfUuid(self.related_issue),
            )?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.IssueSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::space_project::ProjectLiteSerializer<'a>>,
    /// `state_detail`: StateSerializer (source `state`).
    pub state_detail: Option<&'a crate::generated::space_state::StateSerializer<'a>>,
    /// `parent_detail`: IssueStateFlatSerializer (source `parent`).
    pub parent_detail: Option<&'a crate::generated::space_issue::IssueStateFlatSerializer<'a>>,
    /// `label_details`: ListSerializer (source `labels`).
    pub label_details: &'a [crate::generated::space_issue::LabelSerializer<'a>],
    /// `assignee_details`: ListSerializer (source `assignees`).
    pub assignee_details: &'a [crate::generated::space_user::UserLiteSerializer<'a>],
    /// `related_issues`: ListSerializer (source `issue_relation`).
    pub related_issues: &'a [crate::generated::space_issue::IssueRelationSerializer<'a>],
    /// `issue_relations`: ListSerializer (source `issue_related`).
    pub issue_relations: &'a [crate::generated::space_issue::RelatedIssueSerializer<'a>],
    /// `issue_cycle`: IssueCycleDetailSerializer (source `issue_cycle`).
    pub issue_cycle: Option<&'a crate::generated::space_issue::IssueCycleDetailSerializer<'a>>,
    /// `issue_module`: IssueModuleDetailSerializer (source `issue_module`).
    pub issue_module: Option<&'a crate::generated::space_issue::IssueModuleDetailSerializer<'a>>,
    /// `issue_link`: ListSerializer (source `issue_link`).
    pub issue_link: &'a [crate::generated::space_issue::IssueLinkSerializer<'a>],
    /// `issue_attachment`: ListSerializer (source `issue_attachment`).
    pub issue_attachment: &'a [crate::generated::space_issue::IssueAttachmentSerializer<'a>],
    /// `sub_issues_count`: IntegerField (source `sub_issues_count`).
    pub sub_issues_count: crate::fields::FieldValue<i64>,
    /// `issue_reactions`: ListSerializer (source `issue_reactions`).
    pub issue_reactions: &'a [crate::generated::space_issue::IssueReactionSerializer<'a>],
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
    /// `state`: PrimaryKeyRelatedField (source `state`).
    pub state: Option<uuid::Uuid>,
    /// `estimate_point`: PrimaryKeyRelatedField (source `estimate_point`).
    pub estimate_point: Option<uuid::Uuid>,
    /// `type`: PrimaryKeyRelatedField (source `type`).
    pub r#type: Option<uuid::Uuid>,
    /// `assignees`: ManyRelatedField (source `assignees`).
    pub assignees: &'a [uuid::Uuid],
    /// `labels`: ManyRelatedField (source `labels`).
    pub labels: &'a [uuid::Uuid],
}

impl<'a> IssueSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "project_detail",
        "state_detail",
        "parent_detail",
        "label_details",
        "assignee_details",
        "related_issues",
        "issue_relations",
        "issue_cycle",
        "issue_module",
        "issue_link",
        "issue_attachment",
        "sub_issues_count",
        "issue_reactions",
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
        "state",
        "estimate_point",
        "type",
        "assignees",
        "labels",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("project_detail", Self::MODE) {
            obj.field_nested_dyn(
                "project_detail",
                ctx,
                self.project_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("state_detail", Self::MODE) {
            obj.field_nested_dyn(
                "state_detail",
                ctx,
                self.state_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("parent_detail", Self::MODE) {
            obj.field_nested_dyn(
                "parent_detail",
                ctx,
                self.parent_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("label_details", Self::MODE) {
            obj.field_nested_list_dyn("label_details", ctx, self.label_details)?;
        }
        if ctx.emits("assignee_details", Self::MODE) {
            obj.field_nested_list_dyn("assignee_details", ctx, self.assignee_details)?;
        }
        if ctx.emits("related_issues", Self::MODE) {
            obj.field_nested_list_dyn("related_issues", ctx, self.related_issues)?;
        }
        if ctx.emits("issue_relations", Self::MODE) {
            obj.field_nested_list_dyn("issue_relations", ctx, self.issue_relations)?;
        }
        if ctx.emits("issue_cycle", Self::MODE) {
            obj.field_nested_dyn(
                "issue_cycle",
                ctx,
                self.issue_cycle
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("issue_module", Self::MODE) {
            obj.field_nested_dyn(
                "issue_module",
                ctx,
                self.issue_module
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("issue_link", Self::MODE) {
            obj.field_nested_list_dyn("issue_link", ctx, self.issue_link)?;
        }
        if ctx.emits("issue_attachment", Self::MODE) {
            obj.field_nested_list_dyn("issue_attachment", ctx, self.issue_attachment)?;
        }
        if ctx.emits("sub_issues_count", Self::MODE) {
            obj.field_optional_dyn("sub_issues_count", ctx, &self.sub_issues_count)?;
        }
        if ctx.emits("issue_reactions", Self::MODE) {
            obj.field_nested_list_dyn("issue_reactions", ctx, self.issue_reactions)?;
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
        if ctx.emits("state", Self::MODE) {
            obj.field_dyn("state", ctx, &self.state.map(crate::fields::DrfUuid))?;
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
        if ctx.emits("assignees", Self::MODE) {
            obj.field_dyn(
                "assignees",
                ctx,
                &crate::fields::DrfUuidList(self.assignees),
            )?;
        }
        if ctx.emits("labels", Self::MODE) {
            obj.field_dyn("labels", ctx, &crate::fields::DrfUuidList(self.labels))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.IssueStateFlatSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueStateFlatSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `state_detail`: StateLiteSerializer (source `state`).
    pub state_detail: Option<&'a crate::generated::space_state::StateLiteSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::space_project::ProjectLiteSerializer<'a>>,
}

impl<'a> IssueStateFlatSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "sequence_id",
        "name",
        "state_detail",
        "project_detail",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueStateFlatSerializer<'a> {
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
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("state_detail", Self::MODE) {
            obj.field_nested_dyn(
                "state_detail",
                ctx,
                self.state_detail
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
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.IssueVoteSerializer` (model: `db.IssueVote`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueVoteSerializer {
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
    /// `vote`: ChoiceField (source `vote`).
    pub vote: i64,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `actor`: PrimaryKeyRelatedField (source `actor`).
    pub actor: uuid::Uuid,
}

impl IssueVoteSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["issue", "vote", "workspace", "project", "actor"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl crate::ser::DrfSerialize for IssueVoteSerializer {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        if ctx.emits("vote", Self::MODE) {
            obj.field_dyn("vote", ctx, &self.vote)?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("actor", Self::MODE) {
            obj.field_dyn("actor", ctx, &crate::fields::DrfUuid(self.actor))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.issue.LabelLiteSerializer` (model: `db.Label`).
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
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

/// `plane.space.serializer.issue.LabelSerializer` (model: `db.Label`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct LabelSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace_detail`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace_detail:
        Option<&'a crate::generated::space_workspace::WorkspaceLiteSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::space_project::ProjectLiteSerializer<'a>>,
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
        "workspace_detail",
        "project_detail",
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
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
        if ctx.emits("workspace_detail", Self::MODE) {
            obj.field_nested_dyn(
                "workspace_detail",
                ctx,
                self.workspace_detail
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

/// `plane.space.serializer.issue.RelatedIssueSerializer` (model: `db.IssueRelation`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct RelatedIssueSerializer<'a> {
    /// `issue_detail`: IssueProjectLiteSerializer (source `issue`).
    pub issue_detail: Option<&'a crate::generated::space_issue::IssueProjectLiteSerializer<'a>>,
    /// `relation_type`: CharField (source `relation_type`).
    pub relation_type: &'a str,
    /// `related_issue`: PrimaryKeyRelatedField (source `related_issue`).
    pub related_issue: uuid::Uuid,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
}

impl<'a> RelatedIssueSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "issue_detail",
        "relation_type",
        "related_issue",
        "issue",
        "id",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for RelatedIssueSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("issue_detail", Self::MODE) {
            obj.field_nested_dyn(
                "issue_detail",
                ctx,
                self.issue_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("relation_type", Self::MODE) {
            obj.field_dyn("relation_type", ctx, &self.relation_type)?;
        }
        if ctx.emits("related_issue", Self::MODE) {
            obj.field_dyn(
                "related_issue",
                ctx,
                &crate::fields::DrfUuid(self.related_issue),
            )?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
