// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.api.serializers.project` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.api.serializers.project.ProjectCreateSerializer` (model: `db.Project`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectCreateSerializer<'a> {
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `description`: CharField (source `description`).
    pub description: &'a str,
    /// `project_lead`: PrimaryKeyRelatedField (source `project_lead`).
    pub project_lead: Option<uuid::Uuid>,
    /// `default_assignee`: PrimaryKeyRelatedField (source `default_assignee`).
    pub default_assignee: Option<uuid::Uuid>,
    /// `identifier`: CharField (source `identifier`).
    pub identifier: &'a str,
    /// `icon_prop`: JSONField (source `icon_prop`).
    pub icon_prop: Option<crate::fields::PgJson<'a>>,
    /// `emoji`: CharField (source `emoji`).
    pub emoji: Option<&'a str>,
    /// `cover_image`: CharField (source `cover_image`).
    pub cover_image: Option<&'a str>,
    /// `module_view`: BooleanField (source `module_view`).
    pub module_view: bool,
    /// `cycle_view`: BooleanField (source `cycle_view`).
    pub cycle_view: bool,
    /// `issue_views_view`: BooleanField (source `issue_views_view`).
    pub issue_views_view: bool,
    /// `page_view`: BooleanField (source `page_view`).
    pub page_view: bool,
    /// `intake_view`: BooleanField (source `intake_view`).
    pub intake_view: bool,
    /// `guest_view_all_features`: BooleanField (source `guest_view_all_features`).
    pub guest_view_all_features: bool,
    /// `archive_in`: IntegerField (source `archive_in`).
    pub archive_in: i64,
    /// `close_in`: IntegerField (source `close_in`).
    pub close_in: i64,
    /// `timezone`: ChoiceField (source `timezone`).
    pub timezone: &'a str,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `is_issue_type_enabled`: BooleanField (source `is_issue_type_enabled`).
    pub is_issue_type_enabled: bool,
    /// `is_time_tracking_enabled`: BooleanField (source `is_time_tracking_enabled`).
    pub is_time_tracking_enabled: bool,
}

impl<'a> ProjectCreateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "project_lead",
        "default_assignee",
        "identifier",
        "icon_prop",
        "emoji",
        "cover_image",
        "module_view",
        "cycle_view",
        "issue_views_view",
        "page_view",
        "intake_view",
        "guest_view_all_features",
        "archive_in",
        "close_in",
        "timezone",
        "external_source",
        "external_id",
        "is_issue_type_enabled",
        "is_time_tracking_enabled",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for ProjectCreateSerializer<'a> {
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
            obj.field_dyn("description", ctx, &self.description)?;
        }
        if ctx.emits("project_lead", Self::MODE) {
            obj.field_dyn(
                "project_lead",
                ctx,
                &self.project_lead.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("default_assignee", Self::MODE) {
            obj.field_dyn(
                "default_assignee",
                ctx,
                &self.default_assignee.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("identifier", Self::MODE) {
            obj.field_dyn("identifier", ctx, &self.identifier)?;
        }
        if ctx.emits("icon_prop", Self::MODE) {
            obj.field_json_dyn("icon_prop", ctx, self.icon_prop)?;
        }
        if ctx.emits("emoji", Self::MODE) {
            obj.field_dyn("emoji", ctx, &self.emoji)?;
        }
        if ctx.emits("cover_image", Self::MODE) {
            obj.field_dyn("cover_image", ctx, &self.cover_image)?;
        }
        if ctx.emits("module_view", Self::MODE) {
            obj.field_dyn("module_view", ctx, &self.module_view)?;
        }
        if ctx.emits("cycle_view", Self::MODE) {
            obj.field_dyn("cycle_view", ctx, &self.cycle_view)?;
        }
        if ctx.emits("issue_views_view", Self::MODE) {
            obj.field_dyn("issue_views_view", ctx, &self.issue_views_view)?;
        }
        if ctx.emits("page_view", Self::MODE) {
            obj.field_dyn("page_view", ctx, &self.page_view)?;
        }
        if ctx.emits("intake_view", Self::MODE) {
            obj.field_dyn("intake_view", ctx, &self.intake_view)?;
        }
        if ctx.emits("guest_view_all_features", Self::MODE) {
            obj.field_dyn(
                "guest_view_all_features",
                ctx,
                &self.guest_view_all_features,
            )?;
        }
        if ctx.emits("archive_in", Self::MODE) {
            obj.field_dyn("archive_in", ctx, &self.archive_in)?;
        }
        if ctx.emits("close_in", Self::MODE) {
            obj.field_dyn("close_in", ctx, &self.close_in)?;
        }
        if ctx.emits("timezone", Self::MODE) {
            obj.field_dyn("timezone", ctx, &self.timezone)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
        }
        if ctx.emits("is_issue_type_enabled", Self::MODE) {
            obj.field_dyn("is_issue_type_enabled", ctx, &self.is_issue_type_enabled)?;
        }
        if ctx.emits("is_time_tracking_enabled", Self::MODE) {
            obj.field_dyn(
                "is_time_tracking_enabled",
                ctx,
                &self.is_time_tracking_enabled,
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.project.ProjectLiteSerializer` (model: `db.Project`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `identifier`: CharField (source `identifier`).
    pub identifier: &'a str,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `cover_image`: CharField (source `cover_image`).
    pub cover_image: Option<&'a str>,
    /// `icon_prop`: JSONField (source `icon_prop`).
    pub icon_prop: Option<crate::fields::PgJson<'a>>,
    /// `emoji`: CharField (source `emoji`).
    pub emoji: Option<&'a str>,
    /// `description`: CharField (source `description`).
    pub description: &'a str,
    /// `cover_image_url`: CharField (source `cover_image_url`).
    pub cover_image_url: Option<&'a str>,
    /// `archived_at`: DateTimeField (source `archived_at`).
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl<'a> ProjectLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "identifier",
        "name",
        "cover_image",
        "icon_prop",
        "emoji",
        "description",
        "cover_image_url",
        "archived_at",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for ProjectLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("identifier", Self::MODE) {
            obj.field_dyn("identifier", ctx, &self.identifier)?;
        }
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("cover_image", Self::MODE) {
            obj.field_dyn("cover_image", ctx, &self.cover_image)?;
        }
        if ctx.emits("icon_prop", Self::MODE) {
            obj.field_json_dyn("icon_prop", ctx, self.icon_prop)?;
        }
        if ctx.emits("emoji", Self::MODE) {
            obj.field_dyn("emoji", ctx, &self.emoji)?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_dyn("description", ctx, &self.description)?;
        }
        if ctx.emits("cover_image_url", Self::MODE) {
            obj.field_dyn("cover_image_url", ctx, &self.cover_image_url)?;
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

/// `plane.api.serializers.project.ProjectSerializer` (model: `db.Project`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `total_members`: IntegerField (source `total_members`).
    pub total_members: crate::fields::FieldValue<i64>,
    /// `total_cycles`: IntegerField (source `total_cycles`).
    pub total_cycles: crate::fields::FieldValue<i64>,
    /// `total_modules`: IntegerField (source `total_modules`).
    pub total_modules: crate::fields::FieldValue<i64>,
    /// `is_member`: BooleanField (source `is_member`).
    pub is_member: crate::fields::FieldValue<bool>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: crate::fields::FieldValue<f64>,
    /// `member_role`: IntegerField (source `member_role`).
    pub member_role: crate::fields::FieldValue<i64>,
    /// `is_deployed`: BooleanField (source `is_deployed`).
    pub is_deployed: crate::fields::FieldValue<bool>,
    /// `cover_image_url`: CharField (source `cover_image_url`).
    pub cover_image_url: Option<&'a str>,
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
    /// `network`: ChoiceField (source `network`).
    pub network: i64,
    /// `identifier`: CharField (source `identifier`).
    pub identifier: &'a str,
    /// `emoji`: CharField (source `emoji`).
    pub emoji: Option<&'a str>,
    /// `icon_prop`: JSONField (source `icon_prop`).
    pub icon_prop: Option<crate::fields::PgJson<'a>>,
    /// `module_view`: BooleanField (source `module_view`).
    pub module_view: bool,
    /// `cycle_view`: BooleanField (source `cycle_view`).
    pub cycle_view: bool,
    /// `issue_views_view`: BooleanField (source `issue_views_view`).
    pub issue_views_view: bool,
    /// `page_view`: BooleanField (source `page_view`).
    pub page_view: bool,
    /// `intake_view`: BooleanField (source `intake_view`).
    pub intake_view: bool,
    /// `is_time_tracking_enabled`: BooleanField (source `is_time_tracking_enabled`).
    pub is_time_tracking_enabled: bool,
    /// `is_issue_type_enabled`: BooleanField (source `is_issue_type_enabled`).
    pub is_issue_type_enabled: bool,
    /// `guest_view_all_features`: BooleanField (source `guest_view_all_features`).
    pub guest_view_all_features: bool,
    /// `cover_image`: CharField (source `cover_image`).
    pub cover_image: Option<&'a str>,
    /// `archive_in`: IntegerField (source `archive_in`).
    pub archive_in: i64,
    /// `close_in`: IntegerField (source `close_in`).
    pub close_in: i64,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `archived_at`: DateTimeField (source `archived_at`).
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `timezone`: ChoiceField (source `timezone`).
    pub timezone: &'a str,
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
    /// `default_assignee`: PrimaryKeyRelatedField (source `default_assignee`).
    pub default_assignee: Option<uuid::Uuid>,
    /// `project_lead`: PrimaryKeyRelatedField (source `project_lead`).
    pub project_lead: Option<uuid::Uuid>,
    /// `cover_image_asset`: PrimaryKeyRelatedField (source `cover_image_asset`).
    pub cover_image_asset: Option<uuid::Uuid>,
    /// `estimate`: PrimaryKeyRelatedField (source `estimate`).
    pub estimate: Option<uuid::Uuid>,
    /// `default_state`: PrimaryKeyRelatedField (source `default_state`).
    pub default_state: Option<uuid::Uuid>,
}

impl<'a> ProjectSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "total_members",
        "total_cycles",
        "total_modules",
        "is_member",
        "sort_order",
        "member_role",
        "is_deployed",
        "cover_image_url",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "description",
        "description_text",
        "description_html",
        "network",
        "identifier",
        "emoji",
        "icon_prop",
        "module_view",
        "cycle_view",
        "issue_views_view",
        "page_view",
        "intake_view",
        "is_time_tracking_enabled",
        "is_issue_type_enabled",
        "guest_view_all_features",
        "cover_image",
        "archive_in",
        "close_in",
        "logo_props",
        "archived_at",
        "timezone",
        "external_source",
        "external_id",
        "created_by",
        "updated_by",
        "workspace",
        "default_assignee",
        "project_lead",
        "cover_image_asset",
        "estimate",
        "default_state",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for ProjectSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("total_members", Self::MODE) {
            obj.field_optional_dyn("total_members", ctx, &self.total_members)?;
        }
        if ctx.emits("total_cycles", Self::MODE) {
            obj.field_optional_dyn("total_cycles", ctx, &self.total_cycles)?;
        }
        if ctx.emits("total_modules", Self::MODE) {
            obj.field_optional_dyn("total_modules", ctx, &self.total_modules)?;
        }
        if ctx.emits("is_member", Self::MODE) {
            obj.field_optional_dyn("is_member", ctx, &self.is_member)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_optional_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("member_role", Self::MODE) {
            obj.field_optional_dyn("member_role", ctx, &self.member_role)?;
        }
        if ctx.emits("is_deployed", Self::MODE) {
            obj.field_optional_dyn("is_deployed", ctx, &self.is_deployed)?;
        }
        if ctx.emits("cover_image_url", Self::MODE) {
            obj.field_dyn("cover_image_url", ctx, &self.cover_image_url)?;
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
        if ctx.emits("network", Self::MODE) {
            obj.field_dyn("network", ctx, &self.network)?;
        }
        if ctx.emits("identifier", Self::MODE) {
            obj.field_dyn("identifier", ctx, &self.identifier)?;
        }
        if ctx.emits("emoji", Self::MODE) {
            obj.field_dyn("emoji", ctx, &self.emoji)?;
        }
        if ctx.emits("icon_prop", Self::MODE) {
            obj.field_json_dyn("icon_prop", ctx, self.icon_prop)?;
        }
        if ctx.emits("module_view", Self::MODE) {
            obj.field_dyn("module_view", ctx, &self.module_view)?;
        }
        if ctx.emits("cycle_view", Self::MODE) {
            obj.field_dyn("cycle_view", ctx, &self.cycle_view)?;
        }
        if ctx.emits("issue_views_view", Self::MODE) {
            obj.field_dyn("issue_views_view", ctx, &self.issue_views_view)?;
        }
        if ctx.emits("page_view", Self::MODE) {
            obj.field_dyn("page_view", ctx, &self.page_view)?;
        }
        if ctx.emits("intake_view", Self::MODE) {
            obj.field_dyn("intake_view", ctx, &self.intake_view)?;
        }
        if ctx.emits("is_time_tracking_enabled", Self::MODE) {
            obj.field_dyn(
                "is_time_tracking_enabled",
                ctx,
                &self.is_time_tracking_enabled,
            )?;
        }
        if ctx.emits("is_issue_type_enabled", Self::MODE) {
            obj.field_dyn("is_issue_type_enabled", ctx, &self.is_issue_type_enabled)?;
        }
        if ctx.emits("guest_view_all_features", Self::MODE) {
            obj.field_dyn(
                "guest_view_all_features",
                ctx,
                &self.guest_view_all_features,
            )?;
        }
        if ctx.emits("cover_image", Self::MODE) {
            obj.field_dyn("cover_image", ctx, &self.cover_image)?;
        }
        if ctx.emits("archive_in", Self::MODE) {
            obj.field_dyn("archive_in", ctx, &self.archive_in)?;
        }
        if ctx.emits("close_in", Self::MODE) {
            obj.field_dyn("close_in", ctx, &self.close_in)?;
        }
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.archived_at, ctx.tz()),
            )?;
        }
        if ctx.emits("timezone", Self::MODE) {
            obj.field_dyn("timezone", ctx, &self.timezone)?;
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
        if ctx.emits("default_assignee", Self::MODE) {
            obj.field_dyn(
                "default_assignee",
                ctx,
                &self.default_assignee.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("project_lead", Self::MODE) {
            obj.field_dyn(
                "project_lead",
                ctx,
                &self.project_lead.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("cover_image_asset", Self::MODE) {
            obj.field_dyn(
                "cover_image_asset",
                ctx,
                &self.cover_image_asset.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("estimate", Self::MODE) {
            obj.field_dyn("estimate", ctx, &self.estimate.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("default_state", Self::MODE) {
            obj.field_dyn(
                "default_state",
                ctx,
                &self.default_state.map(crate::fields::DrfUuid),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.project.ProjectUpdateSerializer` (model: `db.Project`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectUpdateSerializer<'a> {
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `description`: CharField (source `description`).
    pub description: &'a str,
    /// `project_lead`: PrimaryKeyRelatedField (source `project_lead`).
    pub project_lead: Option<uuid::Uuid>,
    /// `default_assignee`: PrimaryKeyRelatedField (source `default_assignee`).
    pub default_assignee: Option<uuid::Uuid>,
    /// `identifier`: CharField (source `identifier`).
    pub identifier: &'a str,
    /// `icon_prop`: JSONField (source `icon_prop`).
    pub icon_prop: Option<crate::fields::PgJson<'a>>,
    /// `emoji`: CharField (source `emoji`).
    pub emoji: Option<&'a str>,
    /// `cover_image`: CharField (source `cover_image`).
    pub cover_image: Option<&'a str>,
    /// `module_view`: BooleanField (source `module_view`).
    pub module_view: bool,
    /// `cycle_view`: BooleanField (source `cycle_view`).
    pub cycle_view: bool,
    /// `issue_views_view`: BooleanField (source `issue_views_view`).
    pub issue_views_view: bool,
    /// `page_view`: BooleanField (source `page_view`).
    pub page_view: bool,
    /// `intake_view`: BooleanField (source `intake_view`).
    pub intake_view: bool,
    /// `guest_view_all_features`: BooleanField (source `guest_view_all_features`).
    pub guest_view_all_features: bool,
    /// `archive_in`: IntegerField (source `archive_in`).
    pub archive_in: i64,
    /// `close_in`: IntegerField (source `close_in`).
    pub close_in: i64,
    /// `timezone`: ChoiceField (source `timezone`).
    pub timezone: &'a str,
    /// `external_source`: CharField (source `external_source`).
    pub external_source: Option<&'a str>,
    /// `external_id`: CharField (source `external_id`).
    pub external_id: Option<&'a str>,
    /// `is_issue_type_enabled`: BooleanField (source `is_issue_type_enabled`).
    pub is_issue_type_enabled: bool,
    /// `is_time_tracking_enabled`: BooleanField (source `is_time_tracking_enabled`).
    pub is_time_tracking_enabled: bool,
    /// `default_state`: PrimaryKeyRelatedField (source `default_state`).
    pub default_state: Option<uuid::Uuid>,
    /// `estimate`: PrimaryKeyRelatedField (source `estimate`).
    pub estimate: Option<uuid::Uuid>,
}

impl<'a> ProjectUpdateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "name",
        "description",
        "project_lead",
        "default_assignee",
        "identifier",
        "icon_prop",
        "emoji",
        "cover_image",
        "module_view",
        "cycle_view",
        "issue_views_view",
        "page_view",
        "intake_view",
        "guest_view_all_features",
        "archive_in",
        "close_in",
        "timezone",
        "external_source",
        "external_id",
        "is_issue_type_enabled",
        "is_time_tracking_enabled",
        "default_state",
        "estimate",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for ProjectUpdateSerializer<'a> {
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
            obj.field_dyn("description", ctx, &self.description)?;
        }
        if ctx.emits("project_lead", Self::MODE) {
            obj.field_dyn(
                "project_lead",
                ctx,
                &self.project_lead.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("default_assignee", Self::MODE) {
            obj.field_dyn(
                "default_assignee",
                ctx,
                &self.default_assignee.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("identifier", Self::MODE) {
            obj.field_dyn("identifier", ctx, &self.identifier)?;
        }
        if ctx.emits("icon_prop", Self::MODE) {
            obj.field_json_dyn("icon_prop", ctx, self.icon_prop)?;
        }
        if ctx.emits("emoji", Self::MODE) {
            obj.field_dyn("emoji", ctx, &self.emoji)?;
        }
        if ctx.emits("cover_image", Self::MODE) {
            obj.field_dyn("cover_image", ctx, &self.cover_image)?;
        }
        if ctx.emits("module_view", Self::MODE) {
            obj.field_dyn("module_view", ctx, &self.module_view)?;
        }
        if ctx.emits("cycle_view", Self::MODE) {
            obj.field_dyn("cycle_view", ctx, &self.cycle_view)?;
        }
        if ctx.emits("issue_views_view", Self::MODE) {
            obj.field_dyn("issue_views_view", ctx, &self.issue_views_view)?;
        }
        if ctx.emits("page_view", Self::MODE) {
            obj.field_dyn("page_view", ctx, &self.page_view)?;
        }
        if ctx.emits("intake_view", Self::MODE) {
            obj.field_dyn("intake_view", ctx, &self.intake_view)?;
        }
        if ctx.emits("guest_view_all_features", Self::MODE) {
            obj.field_dyn(
                "guest_view_all_features",
                ctx,
                &self.guest_view_all_features,
            )?;
        }
        if ctx.emits("archive_in", Self::MODE) {
            obj.field_dyn("archive_in", ctx, &self.archive_in)?;
        }
        if ctx.emits("close_in", Self::MODE) {
            obj.field_dyn("close_in", ctx, &self.close_in)?;
        }
        if ctx.emits("timezone", Self::MODE) {
            obj.field_dyn("timezone", ctx, &self.timezone)?;
        }
        if ctx.emits("external_source", Self::MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("external_id", Self::MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
        }
        if ctx.emits("is_issue_type_enabled", Self::MODE) {
            obj.field_dyn("is_issue_type_enabled", ctx, &self.is_issue_type_enabled)?;
        }
        if ctx.emits("is_time_tracking_enabled", Self::MODE) {
            obj.field_dyn(
                "is_time_tracking_enabled",
                ctx,
                &self.is_time_tracking_enabled,
            )?;
        }
        if ctx.emits("default_state", Self::MODE) {
            obj.field_dyn(
                "default_state",
                ctx,
                &self.default_state.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("estimate", Self::MODE) {
            obj.field_dyn("estimate", ctx, &self.estimate.map(crate::fields::DrfUuid))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
