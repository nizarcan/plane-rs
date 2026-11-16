// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.api.serializers.intake` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.api.serializers.intake.IntakeIssueCreateSerializer` (model: `db.IntakeIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IntakeIssueCreateSerializer<'a> {
    /// `issue`: IssueForIntakeSerializer (source `issue`).
    pub issue: Option<&'a crate::generated::api_intake::IssueForIntakeSerializer<'a>>,
}

impl<'a> IntakeIssueCreateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["issue"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IntakeIssueCreateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("issue", Self::MODE) {
            obj.field_nested_dyn(
                "issue",
                ctx,
                self.issue
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.intake.IntakeIssueSerializer` (model: `db.IntakeIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IntakeIssueSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `issue_detail`: IssueExpandSerializer (source `issue`).
    pub issue_detail: Option<&'a crate::generated::api_issue::IssueExpandSerializer<'a>>,
    /// `inbox`: UUIDField (source `intake.id`).
    pub inbox: uuid::Uuid,
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
        "inbox",
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
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
        if ctx.emits("inbox", Self::MODE) {
            obj.field_dyn("inbox", ctx, &crate::fields::DrfUuid(self.inbox))?;
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

/// `plane.api.serializers.intake.IntakeIssueUpdateSerializer` (model: `db.IntakeIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IntakeIssueUpdateSerializer<'a> {
    /// `status`: ChoiceField (source `status`).
    pub status: i64,
    /// `snoozed_till`: DateTimeField (source `snoozed_till`).
    pub snoozed_till: Option<chrono::DateTime<chrono::Utc>>,
    /// `duplicate_to`: PrimaryKeyRelatedField (source `duplicate_to`).
    pub duplicate_to: Option<uuid::Uuid>,
    /// `source`: CharField (source `source`).
    pub source: Option<&'a str>,
    /// `source_email`: CharField (source `source_email`).
    pub source_email: Option<&'a str>,
    /// `issue`: IssueForIntakeSerializer (source `issue`).
    pub issue: Option<&'a crate::generated::api_intake::IssueForIntakeSerializer<'a>>,
}

impl<'a> IntakeIssueUpdateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "status",
        "snoozed_till",
        "duplicate_to",
        "source",
        "source_email",
        "issue",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IntakeIssueUpdateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
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
        if ctx.emits("duplicate_to", Self::MODE) {
            obj.field_dyn(
                "duplicate_to",
                ctx,
                &self.duplicate_to.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("source", Self::MODE) {
            obj.field_dyn("source", ctx, &self.source)?;
        }
        if ctx.emits("source_email", Self::MODE) {
            obj.field_dyn("source_email", ctx, &self.source_email)?;
        }
        if ctx.emits("issue", Self::MODE) {
            obj.field_nested_dyn(
                "issue",
                ctx,
                self.issue
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.intake.IssueDataSerializer` (model: `no model`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueDataSerializer<'a> {
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
}

impl<'a> IssueDataSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["name", "description_html", "priority"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueDataSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        if ctx.emits("priority", Self::MODE) {
            obj.field_dyn("priority", ctx, &self.priority)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.intake.IssueForIntakeSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueForIntakeSerializer<'a> {
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `description`: JSONField (source `description_json`).
    pub description: Option<crate::fields::PgJson<'a>>,
    /// `description_json`: JSONField (source `description_json`).
    pub description_json: Option<crate::fields::PgJson<'a>>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
}

impl<'a> IssueForIntakeSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "description_json",
        "description_html",
        "priority",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for IssueForIntakeSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_json_dyn("description", ctx, self.description)?;
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
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
