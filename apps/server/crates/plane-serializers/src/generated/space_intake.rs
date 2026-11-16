// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.space.serializer.intake` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.space.serializer.intake.IntakeIssueLiteSerializer` (model: `db.IntakeIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IntakeIssueLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `status`: ChoiceField (source `status`).
    pub status: i64,
    /// `duplicate_to`: PrimaryKeyRelatedField (source `duplicate_to`).
    pub duplicate_to: Option<uuid::Uuid>,
    /// `snoozed_till`: DateTimeField (source `snoozed_till`).
    pub snoozed_till: Option<chrono::DateTime<chrono::Utc>>,
    /// `source`: CharField (source `source`).
    pub source: Option<&'a str>,
}

impl<'a> IntakeIssueLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] =
        &["id", "status", "duplicate_to", "snoozed_till", "source"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IntakeIssueLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("status", Self::MODE) {
            obj.field_dyn("status", ctx, &self.status)?;
        }
        if ctx.emits("duplicate_to", Self::MODE) {
            obj.field_dyn(
                "duplicate_to",
                ctx,
                &self.duplicate_to.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("snoozed_till", Self::MODE) {
            obj.field_dyn(
                "snoozed_till",
                ctx,
                &crate::fields::drf_datetime_opt(self.snoozed_till, ctx.tz()),
            )?;
        }
        if ctx.emits("source", Self::MODE) {
            obj.field_dyn("source", ctx, &self.source)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.intake.IntakeIssueSerializer` (model: `db.IntakeIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IntakeIssueSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `issue_detail`: IssueFlatSerializer (source `issue`).
    pub issue_detail: Option<&'a crate::generated::space_issue::IssueFlatSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::space_project::ProjectLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `status`: ChoiceField (source `status`).
    pub status: i64,
    /// `snoozed_till`: DateTimeField (source `snoozed_till`).
    pub snoozed_till: Option<chrono::DateTime<chrono::Utc>>,
    /// `source`: CharField (source `source`).
    pub source: Option<&'a str>,
    /// `source_email`: CharField (source `source_email`).
    pub source_email: Option<&'a str>,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `extra`: JSONField (source `extra`).
    pub extra: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `intake`: PrimaryKeyRelatedField (source `intake`).
    pub intake: uuid::Uuid,
    /// `issue`: PrimaryKeyRelatedField (source `issue`).
    pub issue: uuid::Uuid,
    /// `duplicate_to`: PrimaryKeyRelatedField (source `duplicate_to`).
    pub duplicate_to: Option<uuid::Uuid>,
}

impl<'a> IntakeIssueSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "issue_detail",
        "project_detail",
        "created_at",
        "updated_at",
        "deleted_at",
        "status",
        "snoozed_till",
        "source",
        "source_email",
        "external_source",
        "external_id",
        "extra",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "intake",
        "issue",
        "duplicate_to",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IntakeIssueSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
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
        if ctx.emits("status", Self::MODE) {
            obj.field_dyn("status", ctx, &self.status)?;
        }
        if ctx.emits("snoozed_till", Self::MODE) {
            obj.field_dyn(
                "snoozed_till",
                ctx,
                &crate::fields::drf_datetime_opt(self.snoozed_till, ctx.tz()),
            )?;
        }
        if ctx.emits("source", Self::MODE) {
            obj.field_dyn("source", ctx, &self.source)?;
        }
        if ctx.emits("source_email", Self::MODE) {
            obj.field_dyn("source_email", ctx, &self.source_email)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
        }
        if ctx.emits("extra", Self::MODE) {
            obj.field_json_dyn("extra", ctx, self.extra)?;
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
        if ctx.emits("intake", Self::MODE) {
            obj.field_dyn("intake", ctx, &crate::fields::DrfUuid(self.intake))?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_dyn("issue", ctx, &crate::fields::DrfUuid(self.issue))?;
        }
        if ctx.emits("duplicate_to", Self::MODE) {
            obj.field_dyn(
                "duplicate_to",
                ctx,
                &self.duplicate_to.map(crate::fields::DrfUuid),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.space.serializer.intake.IssueStateIntakeSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueStateIntakeSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `state_detail`: StateLiteSerializer (source `state`).
    pub state_detail: Option<&'a crate::generated::space_state::StateLiteSerializer<'a>>,
    /// `project_detail`: ProjectLiteSerializer (source `project`).
    pub project_detail: Option<&'a crate::generated::space_project::ProjectLiteSerializer<'a>>,
    /// `label_details`: ListSerializer (source `labels`).
    pub label_details: &'a [crate::generated::space_issue::LabelLiteSerializer<'a>],
    /// `assignee_details`: ListSerializer (source `assignees`).
    pub assignee_details: &'a [crate::generated::space_user::UserLiteSerializer<'a>],
    /// `sub_issues_count`: IntegerField (source `sub_issues_count`).
    pub sub_issues_count: Option<i64>,
    /// `bridge_id`: UUIDField (source `bridge_id`).
    pub bridge_id: Option<uuid::Uuid>,
    /// `issue_intake`: ListSerializer (source `issue_intake`).
    pub issue_intake: &'a [crate::generated::space_intake::IntakeIssueLiteSerializer<'a>],
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

impl<'a> IssueStateIntakeSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "state_detail",
        "project_detail",
        "label_details",
        "assignee_details",
        "sub_issues_count",
        "bridge_id",
        "issue_intake",
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

impl<'a> crate::ser::DrfSerialize for IssueStateIntakeSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
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
        if ctx.emits("label_details", Self::MODE) {
            obj.field_nested_list_dyn("label_details", ctx, self.label_details)?;
        }
        if ctx.emits("assignee_details", Self::MODE) {
            obj.field_nested_list_dyn("assignee_details", ctx, self.assignee_details)?;
        }
        if ctx.emits("sub_issues_count", Self::MODE)
            && let Some(value) = &self.sub_issues_count
        {
            obj.field_dyn("sub_issues_count", ctx, &value)?;
        }
        if ctx.emits("bridge_id", Self::MODE)
            && let Some(value) = &self.bridge_id
        {
            obj.field_dyn("bridge_id", ctx, &crate::fields::DrfUuid(*value))?;
        }
        if ctx.emits("issue_intake", Self::MODE) {
            obj.field_nested_list_dyn("issue_intake", ctx, self.issue_intake)?;
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
