// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.draft` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.draft.DraftIssueDetailSerializer` (model: `db.DraftIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct DraftIssueDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: Option<&'a str>,
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
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: Option<uuid::Uuid>,
    /// `parent_id`: ReadOnlyField (source `parent_id`).
    pub parent_id: Option<uuid::Uuid>,
    /// `cycle_id`: PrimaryKeyRelatedField (source `cycle_id`).
    pub cycle_id: Option<crate::fields::DrfPk<'a>>,
    /// `module_ids`: ListField (source `module_ids`).
    pub module_ids: Option<&'a [uuid::Uuid]>,
    /// `label_ids`: ListField (source `label_ids`).
    pub label_ids: Option<&'a [uuid::Uuid]>,
    /// `assignee_ids`: ListField (source `assignee_ids`).
    pub assignee_ids: Option<&'a [uuid::Uuid]>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `type_id`: ReadOnlyField (source `type_id`).
    pub type_id: Option<uuid::Uuid>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
}

impl<'a> DraftIssueDetailSerializer<'a> {
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
        "project_id",
        "parent_id",
        "cycle_id",
        "module_ids",
        "label_ids",
        "assignee_ids",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
        "type_id",
        "description_html",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for DraftIssueDetailSerializer<'a> {
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
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn(
                "project_id",
                ctx,
                &self.project_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("parent_id", Self::MODE) {
            obj.field_dyn(
                "parent_id",
                ctx,
                &self.parent_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("cycle_id", Self::MODE)
            && let Some(value) = &self.cycle_id
        {
            obj.field_dyn("cycle_id", ctx, value)?;
        }
        if ctx.emits("module_ids", Self::MODE)
            && let Some(value) = &self.module_ids
        {
            obj.field_dyn("module_ids", ctx, &crate::fields::DrfUuidList(value))?;
        }
        if ctx.emits("label_ids", Self::MODE)
            && let Some(value) = &self.label_ids
        {
            obj.field_dyn("label_ids", ctx, &crate::fields::DrfUuidList(value))?;
        }
        if ctx.emits("assignee_ids", Self::MODE)
            && let Some(value) = &self.assignee_ids
        {
            obj.field_dyn("assignee_ids", ctx, &crate::fields::DrfUuidList(value))?;
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
        if ctx.emits("type_id", Self::MODE) {
            obj.field_dyn("type_id", ctx, &self.type_id.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.draft.DraftIssueSerializer` (model: `db.DraftIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct DraftIssueSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: Option<&'a str>,
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
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: Option<uuid::Uuid>,
    /// `parent_id`: ReadOnlyField (source `parent_id`).
    pub parent_id: Option<uuid::Uuid>,
    /// `cycle_id`: PrimaryKeyRelatedField (source `cycle_id`).
    pub cycle_id: Option<crate::fields::DrfPk<'a>>,
    /// `module_ids`: ListField (source `module_ids`).
    pub module_ids: Option<&'a [uuid::Uuid]>,
    /// `label_ids`: ListField (source `label_ids`).
    pub label_ids: Option<&'a [uuid::Uuid]>,
    /// `assignee_ids`: ListField (source `assignee_ids`).
    pub assignee_ids: Option<&'a [uuid::Uuid]>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `type_id`: ReadOnlyField (source `type_id`).
    pub type_id: Option<uuid::Uuid>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
}

impl<'a> DraftIssueSerializer<'a> {
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
        "project_id",
        "parent_id",
        "cycle_id",
        "module_ids",
        "label_ids",
        "assignee_ids",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
        "type_id",
        "description_html",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for DraftIssueSerializer<'a> {
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
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn(
                "project_id",
                ctx,
                &self.project_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("parent_id", Self::MODE) {
            obj.field_dyn(
                "parent_id",
                ctx,
                &self.parent_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("cycle_id", Self::MODE)
            && let Some(value) = &self.cycle_id
        {
            obj.field_dyn("cycle_id", ctx, value)?;
        }
        if ctx.emits("module_ids", Self::MODE)
            && let Some(value) = &self.module_ids
        {
            obj.field_dyn("module_ids", ctx, &crate::fields::DrfUuidList(value))?;
        }
        if ctx.emits("label_ids", Self::MODE)
            && let Some(value) = &self.label_ids
        {
            obj.field_dyn("label_ids", ctx, &crate::fields::DrfUuidList(value))?;
        }
        if ctx.emits("assignee_ids", Self::MODE)
            && let Some(value) = &self.assignee_ids
        {
            obj.field_dyn("assignee_ids", ctx, &crate::fields::DrfUuidList(value))?;
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
        if ctx.emits("type_id", Self::MODE) {
            obj.field_dyn("type_id", ctx, &self.type_id.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
