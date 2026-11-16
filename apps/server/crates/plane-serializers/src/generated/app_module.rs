// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.module` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.module.ModuleDetailSerializer` (model: `db.Module`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ModuleDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace_id`: ReadOnlyField (source `workspace_id`).
    pub workspace_id: uuid::Uuid,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: uuid::Uuid,
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
    /// `lead_id`: ReadOnlyField (source `lead_id`).
    pub lead_id: Option<uuid::Uuid>,
    /// `member_ids`: ListField (source `member_ids`).
    pub member_ids: Option<&'a [uuid::Uuid]>,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `total_estimate_points`: FloatField (source `total_estimate_points`).
    pub total_estimate_points: Option<f64>,
    /// `completed_estimate_points`: FloatField (source `completed_estimate_points`).
    pub completed_estimate_points: Option<f64>,
    /// `is_favorite`: BooleanField (source `is_favorite`).
    pub is_favorite: Option<bool>,
    /// `total_issues`: IntegerField (source `total_issues`).
    pub total_issues: Option<i64>,
    /// `cancelled_issues`: IntegerField (source `cancelled_issues`).
    pub cancelled_issues: Option<i64>,
    /// `completed_issues`: IntegerField (source `completed_issues`).
    pub completed_issues: Option<i64>,
    /// `started_issues`: IntegerField (source `started_issues`).
    pub started_issues: Option<i64>,
    /// `unstarted_issues`: IntegerField (source `unstarted_issues`).
    pub unstarted_issues: Option<i64>,
    /// `backlog_issues`: IntegerField (source `backlog_issues`).
    pub backlog_issues: Option<i64>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `archived_at`: DateTimeField (source `archived_at`).
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `link_module`: ListSerializer (source `link_module`).
    pub link_module: &'a [crate::generated::app_module::ModuleLinkSerializer<'a>],
    /// `sub_issues`: IntegerField (source `sub_issues`).
    pub sub_issues: Option<i64>,
    /// `backlog_estimate_points`: FloatField (source `backlog_estimate_points`).
    pub backlog_estimate_points: Option<f64>,
    /// `unstarted_estimate_points`: FloatField (source `unstarted_estimate_points`).
    pub unstarted_estimate_points: Option<f64>,
    /// `started_estimate_points`: FloatField (source `started_estimate_points`).
    pub started_estimate_points: Option<f64>,
    /// `cancelled_estimate_points`: FloatField (source `cancelled_estimate_points`).
    pub cancelled_estimate_points: Option<f64>,
}

impl<'a> ModuleDetailSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace_id",
        "project_id",
        "name",
        "description",
        "description_text",
        "description_html",
        "start_date",
        "target_date",
        "status",
        "lead_id",
        "member_ids",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "logo_props",
        "total_estimate_points",
        "completed_estimate_points",
        "is_favorite",
        "total_issues",
        "cancelled_issues",
        "completed_issues",
        "started_issues",
        "unstarted_issues",
        "backlog_issues",
        "created_at",
        "updated_at",
        "archived_at",
        "link_module",
        "sub_issues",
        "backlog_estimate_points",
        "unstarted_estimate_points",
        "started_estimate_points",
        "cancelled_estimate_points",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for ModuleDetailSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("workspace_id", Self::MODE) {
            obj.field_dyn(
                "workspace_id",
                ctx,
                &crate::fields::DrfUuid(self.workspace_id),
            )?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
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
        if ctx.emits("lead_id", Self::MODE) {
            obj.field_dyn("lead_id", ctx, &self.lead_id.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("member_ids", Self::MODE) {
            obj.field_dyn(
                "member_ids",
                ctx,
                &self.member_ids.map(crate::fields::DrfUuidList),
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
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("total_estimate_points", Self::MODE)
            && let Some(value) = &self.total_estimate_points
        {
            obj.field_dyn("total_estimate_points", ctx, &value)?;
        }
        if ctx.emits("completed_estimate_points", Self::MODE)
            && let Some(value) = &self.completed_estimate_points
        {
            obj.field_dyn("completed_estimate_points", ctx, &value)?;
        }
        if ctx.emits("is_favorite", Self::MODE)
            && let Some(value) = &self.is_favorite
        {
            obj.field_dyn("is_favorite", ctx, &value)?;
        }
        if ctx.emits("total_issues", Self::MODE)
            && let Some(value) = &self.total_issues
        {
            obj.field_dyn("total_issues", ctx, &value)?;
        }
        if ctx.emits("cancelled_issues", Self::MODE)
            && let Some(value) = &self.cancelled_issues
        {
            obj.field_dyn("cancelled_issues", ctx, &value)?;
        }
        if ctx.emits("completed_issues", Self::MODE)
            && let Some(value) = &self.completed_issues
        {
            obj.field_dyn("completed_issues", ctx, &value)?;
        }
        if ctx.emits("started_issues", Self::MODE)
            && let Some(value) = &self.started_issues
        {
            obj.field_dyn("started_issues", ctx, &value)?;
        }
        if ctx.emits("unstarted_issues", Self::MODE)
            && let Some(value) = &self.unstarted_issues
        {
            obj.field_dyn("unstarted_issues", ctx, &value)?;
        }
        if ctx.emits("backlog_issues", Self::MODE)
            && let Some(value) = &self.backlog_issues
        {
            obj.field_dyn("backlog_issues", ctx, &value)?;
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
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.archived_at, ctx.tz()),
            )?;
        }
        if ctx.emits("link_module", Self::MODE) {
            obj.field_nested_list_dyn("link_module", ctx, self.link_module)?;
        }
        if ctx.emits("sub_issues", Self::MODE)
            && let Some(value) = &self.sub_issues
        {
            obj.field_dyn("sub_issues", ctx, &value)?;
        }
        if ctx.emits("backlog_estimate_points", Self::MODE)
            && let Some(value) = &self.backlog_estimate_points
        {
            obj.field_dyn("backlog_estimate_points", ctx, &value)?;
        }
        if ctx.emits("unstarted_estimate_points", Self::MODE)
            && let Some(value) = &self.unstarted_estimate_points
        {
            obj.field_dyn("unstarted_estimate_points", ctx, &value)?;
        }
        if ctx.emits("started_estimate_points", Self::MODE)
            && let Some(value) = &self.started_estimate_points
        {
            obj.field_dyn("started_estimate_points", ctx, &value)?;
        }
        if ctx.emits("cancelled_estimate_points", Self::MODE)
            && let Some(value) = &self.cancelled_estimate_points
        {
            obj.field_dyn("cancelled_estimate_points", ctx, &value)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.module.ModuleFlatSerializer` (model: `db.Module`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ModuleFlatSerializer<'a> {
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

impl<'a> ModuleFlatSerializer<'a> {
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

impl<'a> crate::ser::DrfSerialize for ModuleFlatSerializer<'a> {
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

/// `plane.app.serializers.module.ModuleIssueSerializer` (model: `db.ModuleIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ModuleIssueSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `module_detail`: ModuleFlatSerializer (source `module`).
    pub module_detail: Option<&'a crate::generated::app_module::ModuleFlatSerializer<'a>>,
    /// `issue_detail`: ProjectLiteSerializer (source `issue`).
    pub issue_detail: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `sub_issues_count`: IntegerField (source `sub_issues_count`).
    pub sub_issues_count: Option<i64>,
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

impl<'a> ModuleIssueSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "module_detail",
        "issue_detail",
        "sub_issues_count",
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

impl<'a> crate::ser::DrfSerialize for ModuleIssueSerializer<'a> {
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
        if ctx.emits("issue_detail", Self::MODE) {
            obj.field_nested_dyn(
                "issue_detail",
                ctx,
                self.issue_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("sub_issues_count", Self::MODE)
            && let Some(value) = &self.sub_issues_count
        {
            obj.field_dyn("sub_issues_count", ctx, &value)?;
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

/// `plane.app.serializers.module.ModuleLinkSerializer` (model: `db.ModuleLink`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ModuleLinkSerializer<'a> {
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
    /// `url`: URLField (source `url`).
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
    /// `module`: PrimaryKeyRelatedField (source `module`).
    pub module: uuid::Uuid,
}

impl<'a> ModuleLinkSerializer<'a> {
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
        "module",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ModuleLinkSerializer<'a> {
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
        if ctx.emits("module", Self::MODE) {
            obj.field_dyn("module", ctx, &crate::fields::DrfUuid(self.module))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.module.ModuleSerializer` (model: `db.Module`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ModuleSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace_id`: ReadOnlyField (source `workspace_id`).
    pub workspace_id: uuid::Uuid,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: uuid::Uuid,
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
    /// `lead_id`: ReadOnlyField (source `lead_id`).
    pub lead_id: Option<uuid::Uuid>,
    /// `member_ids`: ListField (source `member_ids`).
    pub member_ids: Option<&'a [uuid::Uuid]>,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `total_estimate_points`: FloatField (source `total_estimate_points`).
    pub total_estimate_points: Option<f64>,
    /// `completed_estimate_points`: FloatField (source `completed_estimate_points`).
    pub completed_estimate_points: Option<f64>,
    /// `is_favorite`: BooleanField (source `is_favorite`).
    pub is_favorite: Option<bool>,
    /// `total_issues`: IntegerField (source `total_issues`).
    pub total_issues: Option<i64>,
    /// `cancelled_issues`: IntegerField (source `cancelled_issues`).
    pub cancelled_issues: Option<i64>,
    /// `completed_issues`: IntegerField (source `completed_issues`).
    pub completed_issues: Option<i64>,
    /// `started_issues`: IntegerField (source `started_issues`).
    pub started_issues: Option<i64>,
    /// `unstarted_issues`: IntegerField (source `unstarted_issues`).
    pub unstarted_issues: Option<i64>,
    /// `backlog_issues`: IntegerField (source `backlog_issues`).
    pub backlog_issues: Option<i64>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `archived_at`: DateTimeField (source `archived_at`).
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl<'a> ModuleSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace_id",
        "project_id",
        "name",
        "description",
        "description_text",
        "description_html",
        "start_date",
        "target_date",
        "status",
        "lead_id",
        "member_ids",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "logo_props",
        "total_estimate_points",
        "completed_estimate_points",
        "is_favorite",
        "total_issues",
        "cancelled_issues",
        "completed_issues",
        "started_issues",
        "unstarted_issues",
        "backlog_issues",
        "created_at",
        "updated_at",
        "archived_at",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for ModuleSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("workspace_id", Self::MODE) {
            obj.field_dyn(
                "workspace_id",
                ctx,
                &crate::fields::DrfUuid(self.workspace_id),
            )?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
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
        if ctx.emits("lead_id", Self::MODE) {
            obj.field_dyn("lead_id", ctx, &self.lead_id.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("member_ids", Self::MODE) {
            obj.field_dyn(
                "member_ids",
                ctx,
                &self.member_ids.map(crate::fields::DrfUuidList),
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
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("total_estimate_points", Self::MODE)
            && let Some(value) = &self.total_estimate_points
        {
            obj.field_dyn("total_estimate_points", ctx, &value)?;
        }
        if ctx.emits("completed_estimate_points", Self::MODE)
            && let Some(value) = &self.completed_estimate_points
        {
            obj.field_dyn("completed_estimate_points", ctx, &value)?;
        }
        if ctx.emits("is_favorite", Self::MODE)
            && let Some(value) = &self.is_favorite
        {
            obj.field_dyn("is_favorite", ctx, &value)?;
        }
        if ctx.emits("total_issues", Self::MODE)
            && let Some(value) = &self.total_issues
        {
            obj.field_dyn("total_issues", ctx, &value)?;
        }
        if ctx.emits("cancelled_issues", Self::MODE)
            && let Some(value) = &self.cancelled_issues
        {
            obj.field_dyn("cancelled_issues", ctx, &value)?;
        }
        if ctx.emits("completed_issues", Self::MODE)
            && let Some(value) = &self.completed_issues
        {
            obj.field_dyn("completed_issues", ctx, &value)?;
        }
        if ctx.emits("started_issues", Self::MODE)
            && let Some(value) = &self.started_issues
        {
            obj.field_dyn("started_issues", ctx, &value)?;
        }
        if ctx.emits("unstarted_issues", Self::MODE)
            && let Some(value) = &self.unstarted_issues
        {
            obj.field_dyn("unstarted_issues", ctx, &value)?;
        }
        if ctx.emits("backlog_issues", Self::MODE)
            && let Some(value) = &self.backlog_issues
        {
            obj.field_dyn("backlog_issues", ctx, &value)?;
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
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.archived_at, ctx.tz()),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.module.ModuleUserPropertiesSerializer` (model: `db.ModuleUserProperties`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ModuleUserPropertiesSerializer<'a> {
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
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: uuid::Uuid,
}

impl<'a> ModuleUserPropertiesSerializer<'a> {
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
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "module",
        "user",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ModuleUserPropertiesSerializer<'a> {
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
        if ctx.emits("user", Self::MODE) {
            obj.field_dyn("user", ctx, &crate::fields::DrfUuid(self.user))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
