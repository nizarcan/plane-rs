// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.issue` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.issue.CommentReactionSerializer` (model: `db.CommentReaction`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct CommentReactionSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `actor`: PrimaryKeyRelatedField (source `actor`).
    pub actor: uuid::Uuid,
    /// `comment`: PrimaryKeyRelatedField (source `comment`).
    pub comment: uuid::Uuid,
    /// `reaction`: CharField (source `reaction`).
    pub reaction: &'a str,
    /// `display_name`: CharField (source `actor.display_name`).
    pub display_name: &'a str,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> CommentReactionSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "actor",
        "comment",
        "reaction",
        "display_name",
        "deleted_at",
        "workspace",
        "project",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
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
        if ctx.emits("actor", Self::MODE) {
            obj.field_dyn("actor", ctx, &crate::fields::DrfUuid(self.actor))?;
        }
        if ctx.emits("comment", Self::MODE) {
            obj.field_dyn("comment", ctx, &crate::fields::DrfUuid(self.comment))?;
        }
        if ctx.emits("reaction", Self::MODE) {
            obj.field_dyn("reaction", ctx, &self.reaction)?;
        }
        if ctx.emits("display_name", Self::MODE) {
            obj.field_dyn("display_name", ctx, &self.display_name)?;
        }
        if ctx.emits("deleted_at", Self::MODE) {
            obj.field_dyn(
                "deleted_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.deleted_at, ctx.tz()),
            )?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
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

/// `plane.app.serializers.issue.CycleBaseSerializer` (model: `db.Cycle`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct CycleBaseSerializer<'a> {
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
    /// `start_date`: DateTimeField (source `start_date`).
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    /// `end_date`: DateTimeField (source `end_date`).
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `progress_snapshot`: JSONField (source `progress_snapshot`).
    pub progress_snapshot: Option<crate::fields::PgJson<'a>>,
    /// `archived_at`: DateTimeField (source `archived_at`).
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `timezone`: ChoiceField (source `timezone`).
    pub timezone: &'a str,
    /// `version`: IntegerField (source `version`).
    pub version: i64,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `owned_by`: PrimaryKeyRelatedField (source `owned_by`).
    pub owned_by: uuid::Uuid,
}

impl<'a> CycleBaseSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "description",
        "start_date",
        "end_date",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "progress_snapshot",
        "archived_at",
        "logo_props",
        "timezone",
        "version",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "owned_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for CycleBaseSerializer<'a> {
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
        if ctx.emits("start_date", Self::MODE) {
            obj.field_dyn(
                "start_date",
                ctx,
                &crate::fields::drf_datetime_opt(self.start_date, ctx.tz()),
            )?;
        }
        if ctx.emits("end_date", Self::MODE) {
            obj.field_dyn(
                "end_date",
                ctx,
                &crate::fields::drf_datetime_opt(self.end_date, ctx.tz()),
            )?;
        }
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
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
        if ctx.emits("progress_snapshot", Self::MODE) {
            obj.field_json_dyn("progress_snapshot", ctx, self.progress_snapshot)?;
        }
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.archived_at, ctx.tz()),
            )?;
        }
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("timezone", Self::MODE) {
            obj.field_dyn("timezone", ctx, &self.timezone)?;
        }
        if ctx.emits("version", Self::MODE) {
            obj.field_dyn("version", ctx, &self.version)?;
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
        if ctx.emits("owned_by", Self::MODE) {
            obj.field_dyn("owned_by", ctx, &crate::fields::DrfUuid(self.owned_by))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueActivitySerializer` (model: `db.IssueActivity`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueActivitySerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `actor_detail`: UserLiteSerializer (source `actor`).
    pub actor_detail: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `issue_detail`: IssueFlatSerializer (source `issue`).
    pub issue_detail: Option<&'a crate::generated::app_issue::IssueFlatSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `workspace_detail`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace_detail: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `source_data`: SerializerMethodField (source `*`).
    pub source_data: crate::value::DrfValue<'a>,
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
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
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
        "actor_detail",
        "issue_detail",
        "project_detail",
        "workspace_detail",
        "source_data",
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
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "issue",
        "issue_comment",
        "actor",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
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
        if ctx.emits("source_data", Self::MODE) {
            obj.field_value_dyn("source_data", ctx, &self.source_data)?;
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

/// `plane.app.serializers.issue.IssueAssigneeSerializer` (model: `db.IssueAssignee`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueAssigneeSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `assignee_details`: UserLiteSerializer (source `assignee`).
    pub assignee_details: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
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
    /// `assignee`: PrimaryKeyRelatedField (source `assignee`).
    pub assignee: uuid::Uuid,
}

impl<'a> IssueAssigneeSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "assignee_details",
        "created_at",
        "updated_at",
        "deleted_at",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "issue",
        "assignee",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueAssigneeSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("assignee_details", Self::MODE) {
            obj.field_nested_dyn(
                "assignee_details",
                ctx,
                self.assignee_details
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
        if ctx.emits("assignee", Self::MODE) {
            obj.field_dyn("assignee", ctx, &crate::fields::DrfUuid(self.assignee))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueAttachmentLiteSerializer` (model: `db.FileAsset`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueAttachmentLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `asset`: FileField (source `asset`).
    pub asset: &'a str,
    /// `attributes`: JSONField (source `attributes`).
    pub attributes: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `asset_url`: ReadOnlyField (source `asset_url`).
    pub asset_url: Option<&'a str>,
}

impl<'a> IssueAttachmentLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "asset",
        "attributes",
        "created_by",
        "updated_at",
        "updated_by",
        "asset_url",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for IssueAttachmentLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("asset", Self::MODE) {
            obj.field_dyn("asset", ctx, &self.asset)?;
        }
        if ctx.emits("attributes", Self::MODE) {
            obj.field_json_dyn("attributes", ctx, self.attributes)?;
        }
        if ctx.emits("created_by", Self::MODE) {
            obj.field_dyn(
                "created_by",
                ctx,
                &self.created_by.map(crate::fields::DrfUuid),
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
        if ctx.emits("asset_url", Self::MODE) {
            obj.field_dyn("asset_url", ctx, &self.asset_url)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueAttachmentSerializer` (model: `db.FileAsset`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueAttachmentSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `asset_url`: CharField (source `asset_url`).
    pub asset_url: Option<&'a str>,
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
        "asset_url",
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
        if ctx.emits("asset_url", Self::MODE) {
            obj.field_dyn("asset_url", ctx, &self.asset_url)?;
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

/// `plane.app.serializers.issue.IssueCommentSerializer` (model: `db.IssueComment`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueCommentSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `actor_detail`: UserLiteSerializer (source `actor`).
    pub actor_detail: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `issue_detail`: IssueFlatSerializer (source `issue`).
    pub issue_detail: Option<&'a crate::generated::app_issue::IssueFlatSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `workspace_detail`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace_detail: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `comment_reactions`: ListSerializer (source `comment_reactions`).
    pub comment_reactions: &'a [crate::generated::app_issue::CommentReactionSerializer<'a>],
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

/// `plane.app.serializers.issue.IssueCycleDetailSerializer` (model: `db.CycleIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueCycleDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `cycle_detail`: CycleBaseSerializer (source `cycle`).
    pub cycle_detail: Option<&'a crate::generated::app_issue::CycleBaseSerializer<'a>>,
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

/// `plane.app.serializers.issue.IssueDescriptionVersionDetailSerializer` (model: `db.IssueDescriptionVersion`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueDescriptionVersionDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
    /// `description_binary`: ModelField (source `description_binary`).
    pub description_binary: Option<&'a [u8]>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
    /// `description_stripped`: CharField (source `description_stripped`).
    pub description_stripped: Option<&'a str>,
    /// `description_json`: JSONField (source `description_json`).
    pub description_json: Option<crate::fields::PgJson<'a>>,
    /// `last_saved_at`: DateTimeField (source `last_saved_at`).
    pub last_saved_at: chrono::DateTime<chrono::Utc>,
    /// `owned_by`: PrimaryKeyRelatedField (source `owned_by`).
    pub owned_by: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> IssueDescriptionVersionDetailSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace",
        "project",
        "issue",
        "description_binary",
        "description_html",
        "description_stripped",
        "description_json",
        "last_saved_at",
        "owned_by",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueDescriptionVersionDetailSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        if ctx.emits("description_binary", Self::MODE) {
            obj.field_dyn(
                "description_binary",
                ctx,
                &self.description_binary.map(crate::fields::PyBase64),
            )?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        if ctx.emits("description_stripped", Self::MODE) {
            obj.field_dyn("description_stripped", ctx, &self.description_stripped)?;
        }
        if ctx.emits("description_json", Self::MODE) {
            obj.field_json_dyn("description_json", ctx, self.description_json)?;
        }
        if ctx.emits("last_saved_at", Self::MODE) {
            obj.field_dyn(
                "last_saved_at",
                ctx,
                &crate::fields::drf_datetime(self.last_saved_at, ctx.tz()),
            )?;
        }
        if ctx.emits("owned_by", Self::MODE) {
            obj.field_dyn("owned_by", ctx, &crate::fields::DrfUuid(self.owned_by))?;
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

/// `plane.app.serializers.issue.IssueDetailSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `state_id`: ReadOnlyField (source `state_id`).
    pub state_id: Option<uuid::Uuid>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `completed_at`: DateTimeField (source `completed_at`).
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `estimate_point`: PrimaryKeyRelatedField (source `estimate_point`).
    pub estimate_point: Option<uuid::Uuid>,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
    /// `start_date`: DateField (source `start_date`).
    pub start_date: Option<chrono::NaiveDate>,
    /// `target_date`: DateField (source `target_date`).
    pub target_date: Option<chrono::NaiveDate>,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: uuid::Uuid,
    /// `parent_id`: ReadOnlyField (source `parent_id`).
    pub parent_id: Option<uuid::Uuid>,
    /// `cycle_id`: PrimaryKeyRelatedField (source `cycle_id`).
    pub cycle_id: crate::fields::FieldValue<crate::fields::DrfPk<'a>>,
    /// `module_ids`: ListField (source `module_ids`).
    pub module_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `label_ids`: ListField (source `label_ids`).
    pub label_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `assignee_ids`: ListField (source `assignee_ids`).
    pub assignee_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `sub_issues_count`: IntegerField (source `sub_issues_count`).
    pub sub_issues_count: crate::fields::FieldValue<i64>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `attachment_count`: IntegerField (source `attachment_count`).
    pub attachment_count: crate::fields::FieldValue<i64>,
    /// `link_count`: IntegerField (source `link_count`).
    pub link_count: crate::fields::FieldValue<i64>,
    /// `is_draft`: BooleanField (source `is_draft`).
    pub is_draft: bool,
    /// `archived_at`: DateField (source `archived_at`).
    pub archived_at: Option<chrono::NaiveDate>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
    /// `is_subscribed`: BooleanField (source `is_subscribed`).
    pub is_subscribed: crate::fields::FieldValue<bool>,
    /// `is_intake`: BooleanField (source `is_intake`).
    pub is_intake: crate::fields::FieldValue<bool>,
}

impl<'a> IssueDetailSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "state_id",
        "sort_order",
        "completed_at",
        "estimate_point",
        "priority",
        "start_date",
        "target_date",
        "sequence_id",
        "project_id",
        "parent_id",
        "cycle_id",
        "module_ids",
        "label_ids",
        "assignee_ids",
        "sub_issues_count",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
        "attachment_count",
        "link_count",
        "is_draft",
        "archived_at",
        "description_html",
        "is_subscribed",
        "is_intake",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for IssueDetailSerializer<'a> {
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
        if ctx.emits("state_id", Self::MODE) {
            obj.field_dyn("state_id", ctx, &self.state_id.map(crate::fields::DrfUuid))?;
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
        if ctx.emits("estimate_point", Self::MODE) {
            obj.field_dyn(
                "estimate_point",
                ctx,
                &self.estimate_point.map(crate::fields::DrfUuid),
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
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        if ctx.emits("parent_id", Self::MODE) {
            obj.field_dyn(
                "parent_id",
                ctx,
                &self.parent_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("cycle_id", Self::MODE) {
            obj.field_optional_dyn("cycle_id", ctx, &self.cycle_id)?;
        }
        if ctx.emits("module_ids", Self::MODE) {
            obj.field_optional_dyn("module_ids", ctx, &self.module_ids)?;
        }
        if ctx.emits("label_ids", Self::MODE) {
            obj.field_optional_dyn("label_ids", ctx, &self.label_ids)?;
        }
        if ctx.emits("assignee_ids", Self::MODE) {
            obj.field_optional_dyn("assignee_ids", ctx, &self.assignee_ids)?;
        }
        if ctx.emits("sub_issues_count", Self::MODE) {
            obj.field_optional_dyn("sub_issues_count", ctx, &self.sub_issues_count)?;
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
        if ctx.emits("attachment_count", Self::MODE) {
            obj.field_optional_dyn("attachment_count", ctx, &self.attachment_count)?;
        }
        if ctx.emits("link_count", Self::MODE) {
            obj.field_optional_dyn("link_count", ctx, &self.link_count)?;
        }
        if ctx.emits("is_draft", Self::MODE) {
            obj.field_dyn("is_draft", ctx, &self.is_draft)?;
        }
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &self.archived_at.map(crate::fields::PyDate),
            )?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        if ctx.emits("is_subscribed", Self::MODE) {
            obj.field_optional_dyn("is_subscribed", ctx, &self.is_subscribed)?;
        }
        if ctx.emits("is_intake", Self::MODE) {
            obj.field_optional_dyn("is_intake", ctx, &self.is_intake)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueFlatSerializer` (model: `db.Issue`).
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

/// `plane.app.serializers.issue.IssueIntakeSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueIntakeSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `label_ids`: ListField (source `label_ids`).
    pub label_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
}

impl<'a> IssueIntakeSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "priority",
        "sequence_id",
        "project_id",
        "created_at",
        "label_ids",
        "created_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for IssueIntakeSerializer<'a> {
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
        if ctx.emits("priority", Self::MODE) {
            obj.field_dyn("priority", ctx, &self.priority)?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        if ctx.emits("created_at", Self::MODE) {
            obj.field_dyn(
                "created_at",
                ctx,
                &crate::fields::drf_datetime(self.created_at, ctx.tz()),
            )?;
        }
        if ctx.emits("label_ids", Self::MODE) {
            obj.field_optional_dyn("label_ids", ctx, &self.label_ids)?;
        }
        if ctx.emits("created_by", Self::MODE) {
            obj.field_dyn(
                "created_by",
                ctx,
                &self.created_by.map(crate::fields::DrfUuid),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueLabelSerializer` (model: `db.IssueLabel`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueLabelSerializer {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
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
    /// `label`: PrimaryKeyRelatedField (source `label`).
    pub label: uuid::Uuid,
}

impl IssueLabelSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "issue",
        "label",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl crate::ser::DrfSerialize for IssueLabelSerializer {
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
        if ctx.emits("label", Self::MODE) {
            obj.field_dyn("label", ctx, &crate::fields::DrfUuid(self.label))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueLinkLiteSerializer` (model: `db.IssueLink`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueLinkLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `issue_id`: ReadOnlyField (source `issue_id`).
    pub issue_id: uuid::Uuid,
    /// `title`: CharField (source `title`).
    pub title: Option<&'a str>,
    /// `url`: CharField (source `url`).
    pub url: &'a str,
    /// `metadata`: JSONField (source `metadata`).
    pub metadata: Option<crate::fields::PgJson<'a>>,
    /// `created_by_id`: ReadOnlyField (source `created_by_id`).
    pub created_by_id: Option<uuid::Uuid>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl<'a> IssueLinkLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "issue_id",
        "title",
        "url",
        "metadata",
        "created_by_id",
        "created_at",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueLinkLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("issue_id", Self::MODE) {
            obj.field_dyn("issue_id", ctx, &crate::fields::DrfUuid(self.issue_id))?;
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
        if ctx.emits("created_by_id", Self::MODE) {
            obj.field_dyn(
                "created_by_id",
                ctx,
                &self.created_by_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("created_at", Self::MODE) {
            obj.field_dyn(
                "created_at",
                ctx,
                &crate::fields::drf_datetime(self.created_at, ctx.tz()),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueLinkSerializer` (model: `db.IssueLink`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueLinkSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_by_detail`: UserLiteSerializer (source `created_by`).
    pub created_by_detail: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
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

/// `plane.app.serializers.issue.IssueLiteSerializer` (model: `db.Issue`).
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
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

/// `plane.app.serializers.issue.IssueModuleDetailSerializer` (model: `db.ModuleIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueModuleDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `module_detail`: ModuleBaseSerializer (source `module`).
    pub module_detail: Option<&'a crate::generated::app_issue::ModuleBaseSerializer<'a>>,
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

/// `plane.app.serializers.issue.IssueProjectLiteSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueProjectLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
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

/// `plane.app.serializers.issue.IssuePublicSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssuePublicSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `state`: PrimaryKeyRelatedField (source `state`).
    pub state: Option<uuid::Uuid>,
    /// `state_detail`: StateLiteSerializer (source `state`).
    pub state_detail: Option<&'a crate::generated::app_state::StateLiteSerializer<'a>>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
    /// `target_date`: DateField (source `target_date`).
    pub target_date: Option<chrono::NaiveDate>,
    /// `reactions`: ListSerializer (source `issue_reactions`).
    pub reactions: &'a [crate::generated::app_issue::IssueReactionSerializer<'a>],
    /// `votes`: ListSerializer (source `votes`).
    pub votes: &'a [crate::generated::app_issue::IssueVoteSerializer<'a>],
}

impl<'a> IssuePublicSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "description_html",
        "sequence_id",
        "state",
        "state_detail",
        "project",
        "project_detail",
        "workspace",
        "priority",
        "target_date",
        "reactions",
        "votes",
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
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("state", Self::MODE) {
            obj.field_dyn("state", ctx, &self.state.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("state_detail", Self::MODE) {
            obj.field_nested_dyn(
                "state_detail",
                ctx,
                self.state_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("project_detail", Self::MODE) {
            obj.field_nested_dyn(
                "project_detail",
                ctx,
                self.project_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
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
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueReactionLiteSerializer` (model: `db.IssueReaction`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueReactionLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `actor`: PrimaryKeyRelatedField (source `actor`).
    pub actor: uuid::Uuid,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
    /// `reaction`: CharField (source `reaction`).
    pub reaction: &'a str,
    /// `display_name`: CharField (source `actor.display_name`).
    pub display_name: &'a str,
}

impl<'a> IssueReactionLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] =
        &["id", "actor", "issue", "reaction", "display_name"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for IssueReactionLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("actor", Self::MODE) {
            obj.field_dyn("actor", ctx, &crate::fields::DrfUuid(self.actor))?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        if ctx.emits("reaction", Self::MODE) {
            obj.field_dyn("reaction", ctx, &self.reaction)?;
        }
        if ctx.emits("display_name", Self::MODE) {
            obj.field_dyn("display_name", ctx, &self.display_name)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueReactionSerializer` (model: `db.IssueReaction`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueReactionSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `actor_detail`: UserLiteSerializer (source `actor`).
    pub actor_detail: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
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
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
}

impl<'a> IssueReactionSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "actor_detail",
        "created_at",
        "updated_at",
        "deleted_at",
        "reaction",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "actor",
        "issue",
    ];
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
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueRelationSerializer` (model: `db.IssueRelation`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueRelationSerializer<'a> {
    /// `id`: UUIDField (source `related_issue.id`).
    pub id: uuid::Uuid,
    /// `project_id`: PrimaryKeyRelatedField (source `related_issue.project_id`).
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
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

/// `plane.app.serializers.issue.IssueSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `state_id`: ReadOnlyField (source `state_id`).
    pub state_id: Option<uuid::Uuid>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `completed_at`: DateTimeField (source `completed_at`).
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `estimate_point`: PrimaryKeyRelatedField (source `estimate_point`).
    pub estimate_point: Option<uuid::Uuid>,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
    /// `start_date`: DateField (source `start_date`).
    pub start_date: Option<chrono::NaiveDate>,
    /// `target_date`: DateField (source `target_date`).
    pub target_date: Option<chrono::NaiveDate>,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: uuid::Uuid,
    /// `parent_id`: ReadOnlyField (source `parent_id`).
    pub parent_id: Option<uuid::Uuid>,
    /// `cycle_id`: PrimaryKeyRelatedField (source `cycle_id`).
    pub cycle_id: crate::fields::FieldValue<crate::fields::DrfPk<'a>>,
    /// `module_ids`: ListField (source `module_ids`).
    pub module_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `label_ids`: ListField (source `label_ids`).
    pub label_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `assignee_ids`: ListField (source `assignee_ids`).
    pub assignee_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `sub_issues_count`: IntegerField (source `sub_issues_count`).
    pub sub_issues_count: crate::fields::FieldValue<i64>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `attachment_count`: IntegerField (source `attachment_count`).
    pub attachment_count: crate::fields::FieldValue<i64>,
    /// `link_count`: IntegerField (source `link_count`).
    pub link_count: crate::fields::FieldValue<i64>,
    /// `is_draft`: BooleanField (source `is_draft`).
    pub is_draft: bool,
    /// `archived_at`: DateField (source `archived_at`).
    pub archived_at: Option<chrono::NaiveDate>,
}

impl<'a> IssueSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "state_id",
        "sort_order",
        "completed_at",
        "estimate_point",
        "priority",
        "start_date",
        "target_date",
        "sequence_id",
        "project_id",
        "parent_id",
        "cycle_id",
        "module_ids",
        "label_ids",
        "assignee_ids",
        "sub_issues_count",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
        "attachment_count",
        "link_count",
        "is_draft",
        "archived_at",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
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
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("state_id", Self::MODE) {
            obj.field_dyn("state_id", ctx, &self.state_id.map(crate::fields::DrfUuid))?;
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
        if ctx.emits("estimate_point", Self::MODE) {
            obj.field_dyn(
                "estimate_point",
                ctx,
                &self.estimate_point.map(crate::fields::DrfUuid),
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
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        if ctx.emits("parent_id", Self::MODE) {
            obj.field_dyn(
                "parent_id",
                ctx,
                &self.parent_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("cycle_id", Self::MODE) {
            obj.field_optional_dyn("cycle_id", ctx, &self.cycle_id)?;
        }
        if ctx.emits("module_ids", Self::MODE) {
            obj.field_optional_dyn("module_ids", ctx, &self.module_ids)?;
        }
        if ctx.emits("label_ids", Self::MODE) {
            obj.field_optional_dyn("label_ids", ctx, &self.label_ids)?;
        }
        if ctx.emits("assignee_ids", Self::MODE) {
            obj.field_optional_dyn("assignee_ids", ctx, &self.assignee_ids)?;
        }
        if ctx.emits("sub_issues_count", Self::MODE) {
            obj.field_optional_dyn("sub_issues_count", ctx, &self.sub_issues_count)?;
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
        if ctx.emits("attachment_count", Self::MODE) {
            obj.field_optional_dyn("attachment_count", ctx, &self.attachment_count)?;
        }
        if ctx.emits("link_count", Self::MODE) {
            obj.field_optional_dyn("link_count", ctx, &self.link_count)?;
        }
        if ctx.emits("is_draft", Self::MODE) {
            obj.field_dyn("is_draft", ctx, &self.is_draft)?;
        }
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &self.archived_at.map(crate::fields::PyDate),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueStateFlatSerializer` (model: `db.Issue`).
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
    pub state_detail: Option<&'a crate::generated::app_state::StateLiteSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
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

/// `plane.app.serializers.issue.IssueStateSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueStateSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `label_details`: ListSerializer (source `labels`).
    pub label_details: &'a [crate::generated::app_issue::LabelLiteSerializer<'a>],
    /// `state_detail`: StateLiteSerializer (source `state`).
    pub state_detail: Option<&'a crate::generated::app_state::StateLiteSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `assignee_details`: ListSerializer (source `assignees`).
    pub assignee_details: &'a [crate::generated::app_user::UserLiteSerializer<'a>],
    /// `sub_issues_count`: IntegerField (source `sub_issues_count`).
    pub sub_issues_count: crate::fields::FieldValue<i64>,
    /// `attachment_count`: IntegerField (source `attachment_count`).
    pub attachment_count: crate::fields::FieldValue<i64>,
    /// `link_count`: IntegerField (source `link_count`).
    pub link_count: crate::fields::FieldValue<i64>,
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

impl<'a> IssueStateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "label_details",
        "state_detail",
        "project_detail",
        "assignee_details",
        "sub_issues_count",
        "attachment_count",
        "link_count",
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for IssueStateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("label_details", Self::MODE) {
            obj.field_nested_list_dyn("label_details", ctx, self.label_details)?;
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
        if ctx.emits("assignee_details", Self::MODE) {
            obj.field_nested_list_dyn("assignee_details", ctx, self.assignee_details)?;
        }
        if ctx.emits("sub_issues_count", Self::MODE) {
            obj.field_optional_dyn("sub_issues_count", ctx, &self.sub_issues_count)?;
        }
        if ctx.emits("attachment_count", Self::MODE) {
            obj.field_optional_dyn("attachment_count", ctx, &self.attachment_count)?;
        }
        if ctx.emits("link_count", Self::MODE) {
            obj.field_optional_dyn("link_count", ctx, &self.link_count)?;
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

/// `plane.app.serializers.issue.IssueSubscriberSerializer` (model: `db.IssueSubscriber`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueSubscriberSerializer {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
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
    /// `subscriber`: PrimaryKeyRelatedField (source `subscriber`).
    pub subscriber: uuid::Uuid,
}

impl IssueSubscriberSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "issue",
        "subscriber",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl crate::ser::DrfSerialize for IssueSubscriberSerializer {
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
        if ctx.emits("subscriber", Self::MODE) {
            obj.field_dyn("subscriber", ctx, &crate::fields::DrfUuid(self.subscriber))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.IssueVersionDetailSerializer` (model: `db.IssueVersion`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueVersionDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
    /// `parent`: UUIDField (source `parent`).
    pub parent: Option<uuid::Uuid>,
    /// `state`: UUIDField (source `state`).
    pub state: Option<uuid::Uuid>,
    /// `estimate_point`: UUIDField (source `estimate_point`).
    pub estimate_point: Option<uuid::Uuid>,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
    /// `start_date`: DateField (source `start_date`).
    pub start_date: Option<chrono::NaiveDate>,
    /// `target_date`: DateField (source `target_date`).
    pub target_date: Option<chrono::NaiveDate>,
    /// `assignees`: ListField (source `assignees`).
    pub assignees: &'a [uuid::Uuid],
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `labels`: ListField (source `labels`).
    pub labels: &'a [uuid::Uuid],
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
    /// `type`: UUIDField (source `type`).
    pub r#type: Option<uuid::Uuid>,
    /// `cycle`: UUIDField (source `cycle`).
    pub cycle: Option<uuid::Uuid>,
    /// `modules`: ListField (source `modules`).
    pub modules: &'a [uuid::Uuid],
    /// `meta`: JSONField (source `meta`).
    pub meta: Option<crate::fields::PgJson<'a>>,
    /// `last_saved_at`: DateTimeField (source `last_saved_at`).
    pub last_saved_at: chrono::DateTime<chrono::Utc>,
    /// `owned_by`: PrimaryKeyRelatedField (source `owned_by`).
    pub owned_by: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> IssueVersionDetailSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace",
        "project",
        "issue",
        "parent",
        "state",
        "estimate_point",
        "name",
        "priority",
        "start_date",
        "target_date",
        "assignees",
        "sequence_id",
        "labels",
        "sort_order",
        "completed_at",
        "archived_at",
        "is_draft",
        "external_source",
        "external_id",
        "type",
        "cycle",
        "modules",
        "meta",
        "last_saved_at",
        "owned_by",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueVersionDetailSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
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
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
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
        if ctx.emits("assignees", Self::MODE) {
            obj.field_dyn(
                "assignees",
                ctx,
                &crate::fields::DrfUuidList(self.assignees),
            )?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("labels", Self::MODE) {
            obj.field_dyn("labels", ctx, &crate::fields::DrfUuidList(self.labels))?;
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
        if ctx.emits("type", Self::MODE) {
            obj.field_dyn("type", ctx, &self.r#type.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("cycle", Self::MODE) {
            obj.field_dyn("cycle", ctx, &self.cycle.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("modules", Self::MODE) {
            obj.field_dyn("modules", ctx, &crate::fields::DrfUuidList(self.modules))?;
        }
        if ctx.emits("meta", Self::MODE) {
            obj.field_json_dyn("meta", ctx, self.meta)?;
        }
        if ctx.emits("last_saved_at", Self::MODE) {
            obj.field_dyn(
                "last_saved_at",
                ctx,
                &crate::fields::drf_datetime(self.last_saved_at, ctx.tz()),
            )?;
        }
        if ctx.emits("owned_by", Self::MODE) {
            obj.field_dyn("owned_by", ctx, &crate::fields::DrfUuid(self.owned_by))?;
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

/// `plane.app.serializers.issue.IssueVoteSerializer` (model: `db.IssueVote`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueVoteSerializer<'a> {
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
    /// `actor_detail`: UserLiteSerializer (source `actor`).
    pub actor_detail: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
}

impl<'a> IssueVoteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "issue",
        "vote",
        "workspace",
        "project",
        "actor",
        "actor_detail",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueVoteSerializer<'a> {
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

/// `plane.app.serializers.issue.LabelLiteSerializer` (model: `db.Label`).
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

/// `plane.app.serializers.issue.LabelSerializer` (model: `db.Label`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct LabelSerializer<'a> {
    /// `parent`: PrimaryKeyRelatedField (source `parent`).
    pub parent: Option<uuid::Uuid>,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `color`: CharField (source `color`).
    pub color: &'a str,
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: Option<uuid::Uuid>,
    /// `workspace_id`: ReadOnlyField (source `workspace_id`).
    pub workspace_id: uuid::Uuid,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
}

impl<'a> LabelSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "parent",
        "name",
        "color",
        "id",
        "project_id",
        "workspace_id",
        "sort_order",
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
        if ctx.emits("parent", Self::MODE) {
            obj.field_dyn("parent", ctx, &self.parent.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("color", Self::MODE) {
            obj.field_dyn("color", ctx, &self.color)?;
        }
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn(
                "project_id",
                ctx,
                &self.project_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("workspace_id", Self::MODE) {
            obj.field_dyn(
                "workspace_id",
                ctx,
                &crate::fields::DrfUuid(self.workspace_id),
            )?;
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

/// `plane.app.serializers.issue.ModuleBaseSerializer` (model: `db.Module`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ModuleBaseSerializer<'a> {
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
    /// `description_text`: JSONField (source `description_text`).
    pub description_text: Option<crate::fields::PgJson<'a>>,
    /// `description_html`: JSONField (source `description_html`).
    pub description_html: Option<crate::fields::PgJson<'a>>,
    /// `start_date`: DateField (source `start_date`).
    pub start_date: Option<chrono::NaiveDate>,
    /// `target_date`: DateField (source `target_date`).
    pub target_date: Option<chrono::NaiveDate>,
    /// `status`: ChoiceField (source `status`).
    pub status: &'a str,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `archived_at`: DateTimeField (source `archived_at`).
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `lead`: PrimaryKeyRelatedField (source `lead`).
    pub lead: Option<uuid::Uuid>,
    /// `members`: ManyRelatedField (source `members`).
    pub members: &'a [uuid::Uuid],
}

impl<'a> ModuleBaseSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "description",
        "description_text",
        "description_html",
        "start_date",
        "target_date",
        "status",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "archived_at",
        "logo_props",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "lead",
        "members",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ModuleBaseSerializer<'a> {
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
        if ctx.emits("description_text", Self::MODE) {
            obj.field_json_dyn("description_text", ctx, self.description_text)?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_json_dyn("description_html", ctx, self.description_html)?;
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
        if ctx.emits("status", Self::MODE) {
            obj.field_dyn("status", ctx, &self.status)?;
        }
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
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
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.archived_at, ctx.tz()),
            )?;
        }
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
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
        if ctx.emits("lead", Self::MODE) {
            obj.field_dyn("lead", ctx, &self.lead.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("members", Self::MODE) {
            obj.field_dyn("members", ctx, &crate::fields::DrfUuidList(self.members))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.ProjectUserPropertySerializer` (model: `db.ProjectUserProperty`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectUserPropertySerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `filters`: JSONField (source `filters`).
    pub filters: Option<crate::fields::PgJson<'a>>,
    /// `display_filters`: JSONField (source `display_filters`).
    pub display_filters: Option<crate::fields::PgJson<'a>>,
    /// `display_properties`: JSONField (source `display_properties`).
    pub display_properties: Option<crate::fields::PgJson<'a>>,
    /// `rich_filters`: JSONField (source `rich_filters`).
    pub rich_filters: Option<crate::fields::PgJson<'a>>,
    /// `preferences`: JSONField (source `preferences`).
    pub preferences: Option<crate::fields::PgJson<'a>>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: uuid::Uuid,
}

impl<'a> ProjectUserPropertySerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
        "preferences",
        "sort_order",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "user",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectUserPropertySerializer<'a> {
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
        if ctx.emits("filters", Self::MODE) {
            obj.field_json_dyn("filters", ctx, self.filters)?;
        }
        if ctx.emits("display_filters", Self::MODE) {
            obj.field_json_dyn("display_filters", ctx, self.display_filters)?;
        }
        if ctx.emits("display_properties", Self::MODE) {
            obj.field_json_dyn("display_properties", ctx, self.display_properties)?;
        }
        if ctx.emits("rich_filters", Self::MODE) {
            obj.field_json_dyn("rich_filters", ctx, self.rich_filters)?;
        }
        if ctx.emits("preferences", Self::MODE) {
            obj.field_json_dyn("preferences", ctx, self.preferences)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
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
        if ctx.emits("user", Self::MODE) {
            obj.field_dyn("user", ctx, &crate::fields::DrfUuid(self.user))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.issue.RelatedIssueSerializer` (model: `db.IssueRelation`).
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
        "state_id",
        "priority",
        "created_by",
        "created_at",
        "updated_by",
        "updated_at",
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
