// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.cycle` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.cycle.CycleIssueSerializer` (model: `db.CycleIssue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct CycleIssueSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `issue_detail`: IssueStateSerializer (source `issue`).
    pub issue_detail: Option<&'a crate::generated::app_issue::IssueStateSerializer<'a>>,
    /// `sub_issues_count`: IntegerField (source `sub_issues_count`).
    pub sub_issues_count: crate::fields::FieldValue<i64>,
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

impl<'a> CycleIssueSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "issue_detail",
        "sub_issues_count",
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

impl<'a> crate::ser::DrfSerialize for CycleIssueSerializer<'a> {
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

/// `plane.app.serializers.cycle.CycleSerializer` (model: `db.Cycle`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct CycleSerializer<'a> {
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
    /// `start_date`: DateTimeField (source `start_date`).
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    /// `end_date`: DateTimeField (source `end_date`).
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
    /// `owned_by_id`: ReadOnlyField (source `owned_by_id`).
    pub owned_by_id: uuid::Uuid,
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
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `is_favorite`: BooleanField (source `is_favorite`).
    pub is_favorite: crate::fields::FieldValue<bool>,
    /// `total_issues`: IntegerField (source `total_issues`).
    pub total_issues: crate::fields::FieldValue<i64>,
    /// `cancelled_issues`: IntegerField (source `cancelled_issues`).
    pub cancelled_issues: crate::fields::FieldValue<i64>,
    /// `completed_issues`: IntegerField (source `completed_issues`).
    pub completed_issues: crate::fields::FieldValue<i64>,
    /// `started_issues`: IntegerField (source `started_issues`).
    pub started_issues: crate::fields::FieldValue<i64>,
    /// `unstarted_issues`: IntegerField (source `unstarted_issues`).
    pub unstarted_issues: crate::fields::FieldValue<i64>,
    /// `backlog_issues`: IntegerField (source `backlog_issues`).
    pub backlog_issues: crate::fields::FieldValue<i64>,
    /// `status`: CharField (source `status`).
    pub status: crate::fields::FieldValue<&'a str>,
}

impl<'a> CycleSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace_id",
        "project_id",
        "name",
        "description",
        "start_date",
        "end_date",
        "owned_by_id",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "progress_snapshot",
        "logo_props",
        "is_favorite",
        "total_issues",
        "cancelled_issues",
        "completed_issues",
        "started_issues",
        "unstarted_issues",
        "backlog_issues",
        "status",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for CycleSerializer<'a> {
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
        if ctx.emits("owned_by_id", Self::MODE) {
            obj.field_dyn(
                "owned_by_id",
                ctx,
                &crate::fields::DrfUuid(self.owned_by_id),
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
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("is_favorite", Self::MODE) {
            obj.field_optional_dyn("is_favorite", ctx, &self.is_favorite)?;
        }
        if ctx.emits("total_issues", Self::MODE) {
            obj.field_optional_dyn("total_issues", ctx, &self.total_issues)?;
        }
        if ctx.emits("cancelled_issues", Self::MODE) {
            obj.field_optional_dyn("cancelled_issues", ctx, &self.cancelled_issues)?;
        }
        if ctx.emits("completed_issues", Self::MODE) {
            obj.field_optional_dyn("completed_issues", ctx, &self.completed_issues)?;
        }
        if ctx.emits("started_issues", Self::MODE) {
            obj.field_optional_dyn("started_issues", ctx, &self.started_issues)?;
        }
        if ctx.emits("unstarted_issues", Self::MODE) {
            obj.field_optional_dyn("unstarted_issues", ctx, &self.unstarted_issues)?;
        }
        if ctx.emits("backlog_issues", Self::MODE) {
            obj.field_optional_dyn("backlog_issues", ctx, &self.backlog_issues)?;
        }
        if ctx.emits("status", Self::MODE) {
            obj.field_optional_dyn("status", ctx, &self.status)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.cycle.CycleUserPropertiesSerializer` (model: `db.CycleUserProperties`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct CycleUserPropertiesSerializer<'a> {
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
    /// `cycle`: PrimaryKeyRelatedField (source `cycle`).
    pub cycle: uuid::Uuid,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: uuid::Uuid,
}

impl<'a> CycleUserPropertiesSerializer<'a> {
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
        "cycle",
        "user",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for CycleUserPropertiesSerializer<'a> {
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
        if ctx.emits("cycle", Self::MODE) {
            obj.field_dyn("cycle", ctx, &crate::fields::DrfUuid(self.cycle))?;
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

/// `plane.app.serializers.cycle.CycleWriteSerializer` (model: `db.Cycle`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct CycleWriteSerializer<'a> {
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

impl<'a> CycleWriteSerializer<'a> {
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

impl<'a> crate::ser::DrfSerialize for CycleWriteSerializer<'a> {
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
