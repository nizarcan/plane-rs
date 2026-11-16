// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.project` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.project.DeployBoardSerializer` (model: `db.DeployBoard`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct DeployBoardSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `project_details`: ProjectLiteSerializer (source `project`).
    pub project_details: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `workspace_detail`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace_detail: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `entity_identifier`: UUIDField (source `entity_identifier`).
    pub entity_identifier: Option<uuid::Uuid>,
    /// `entity_name`: CharField (source `entity_name`).
    pub entity_name: Option<&'a str>,
    /// `anchor`: CharField (source `anchor`).
    pub anchor: &'a str,
    /// `is_comments_enabled`: BooleanField (source `is_comments_enabled`).
    pub is_comments_enabled: bool,
    /// `is_reactions_enabled`: BooleanField (source `is_reactions_enabled`).
    pub is_reactions_enabled: bool,
    /// `is_votes_enabled`: BooleanField (source `is_votes_enabled`).
    pub is_votes_enabled: bool,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `is_activity_enabled`: BooleanField (source `is_activity_enabled`).
    pub is_activity_enabled: bool,
    /// `is_disabled`: BooleanField (source `is_disabled`).
    pub is_disabled: bool,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: Option<uuid::Uuid>,
    /// `intake`: PrimaryKeyRelatedField (source `intake`).
    pub intake: Option<uuid::Uuid>,
}

impl<'a> DeployBoardSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "project_details",
        "workspace_detail",
        "created_at",
        "updated_at",
        "deleted_at",
        "entity_identifier",
        "entity_name",
        "anchor",
        "is_comments_enabled",
        "is_reactions_enabled",
        "is_votes_enabled",
        "view_props",
        "is_activity_enabled",
        "is_disabled",
        "created_by",
        "updated_by",
        "workspace",
        "project",
        "intake",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for DeployBoardSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("project_details", Self::MODE) {
            obj.field_nested_dyn(
                "project_details",
                ctx,
                self.project_details
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
        if ctx.emits("entity_identifier", Self::MODE) {
            obj.field_dyn(
                "entity_identifier",
                ctx,
                &self.entity_identifier.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("entity_name", Self::MODE) {
            obj.field_dyn("entity_name", ctx, &self.entity_name)?;
        }
        if ctx.emits("anchor", Self::MODE) {
            obj.field_dyn("anchor", ctx, &self.anchor)?;
        }
        if ctx.emits("is_comments_enabled", Self::MODE) {
            obj.field_dyn("is_comments_enabled", ctx, &self.is_comments_enabled)?;
        }
        if ctx.emits("is_reactions_enabled", Self::MODE) {
            obj.field_dyn("is_reactions_enabled", ctx, &self.is_reactions_enabled)?;
        }
        if ctx.emits("is_votes_enabled", Self::MODE) {
            obj.field_dyn("is_votes_enabled", ctx, &self.is_votes_enabled)?;
        }
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
        }
        if ctx.emits("is_activity_enabled", Self::MODE) {
            obj.field_dyn("is_activity_enabled", ctx, &self.is_activity_enabled)?;
        }
        if ctx.emits("is_disabled", Self::MODE) {
            obj.field_dyn("is_disabled", ctx, &self.is_disabled)?;
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
        if ctx.emits("intake", Self::MODE) {
            obj.field_dyn("intake", ctx, &self.intake.map(crate::fields::DrfUuid))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.project.ProjectDetailSerializer` (model: `db.Project`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `default_assignee`: UserLiteSerializer (source `default_assignee`).
    pub default_assignee: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `project_lead`: UserLiteSerializer (source `project_lead`).
    pub project_lead: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `is_favorite`: BooleanField (source `is_favorite`).
    pub is_favorite: crate::fields::FieldValue<bool>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: crate::fields::FieldValue<f64>,
    /// `member_role`: IntegerField (source `member_role`).
    pub member_role: crate::fields::FieldValue<i64>,
    /// `anchor`: CharField (source `anchor`).
    pub anchor: crate::fields::FieldValue<&'a str>,
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
    /// `cover_image_asset`: PrimaryKeyRelatedField (source `cover_image_asset`).
    pub cover_image_asset: Option<uuid::Uuid>,
    /// `estimate`: PrimaryKeyRelatedField (source `estimate`).
    pub estimate: Option<uuid::Uuid>,
    /// `default_state`: PrimaryKeyRelatedField (source `default_state`).
    pub default_state: Option<uuid::Uuid>,
}

impl<'a> ProjectDetailSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "default_assignee",
        "project_lead",
        "is_favorite",
        "sort_order",
        "member_role",
        "anchor",
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
        "cover_image_asset",
        "estimate",
        "default_state",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectDetailSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("default_assignee", Self::MODE) {
            obj.field_nested_dyn(
                "default_assignee",
                ctx,
                self.default_assignee
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("project_lead", Self::MODE) {
            obj.field_nested_dyn(
                "project_lead",
                ctx,
                self.project_lead
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("is_favorite", Self::MODE) {
            obj.field_optional_dyn("is_favorite", ctx, &self.is_favorite)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_optional_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("member_role", Self::MODE) {
            obj.field_optional_dyn("member_role", ctx, &self.member_role)?;
        }
        if ctx.emits("anchor", Self::MODE) {
            obj.field_optional_dyn("anchor", ctx, &self.anchor)?;
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

/// `plane.app.serializers.project.ProjectIdentifierSerializer` (model: `db.ProjectIdentifier`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectIdentifierSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: i64,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
}

impl<'a> ProjectIdentifierSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "created_by",
        "updated_by",
        "workspace",
        "project",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectIdentifierSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &self.id)?;
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
            obj.field_dyn(
                "workspace",
                ctx,
                &self.workspace.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.project.ProjectListSerializer` (model: `db.Project`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectListSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `is_favorite`: BooleanField (source `is_favorite`).
    pub is_favorite: crate::fields::FieldValue<bool>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: crate::fields::FieldValue<f64>,
    /// `member_role`: IntegerField (source `member_role`).
    pub member_role: crate::fields::FieldValue<i64>,
    /// `anchor`: CharField (source `anchor`).
    pub anchor: crate::fields::FieldValue<&'a str>,
    /// `members`: SerializerMethodField (source `*`).
    pub members: crate::value::DrfValue<'a>,
    /// `cover_image_url`: CharField (source `cover_image_url`).
    pub cover_image_url: Option<&'a str>,
    /// `inbox_view`: BooleanField (source `intake_view`).
    pub inbox_view: bool,
    /// `next_work_item_sequence`: SerializerMethodField (source `*`).
    pub next_work_item_sequence: crate::value::DrfValue<'a>,
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

impl<'a> ProjectListSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "is_favorite",
        "sort_order",
        "member_role",
        "anchor",
        "members",
        "cover_image_url",
        "inbox_view",
        "next_work_item_sequence",
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for ProjectListSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("is_favorite", Self::MODE) {
            obj.field_optional_dyn("is_favorite", ctx, &self.is_favorite)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_optional_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("member_role", Self::MODE) {
            obj.field_optional_dyn("member_role", ctx, &self.member_role)?;
        }
        if ctx.emits("anchor", Self::MODE) {
            obj.field_optional_dyn("anchor", ctx, &self.anchor)?;
        }
        if ctx.emits("members", Self::MODE) {
            obj.field_value_dyn("members", ctx, &self.members)?;
        }
        if ctx.emits("cover_image_url", Self::MODE) {
            obj.field_dyn("cover_image_url", ctx, &self.cover_image_url)?;
        }
        if ctx.emits("inbox_view", Self::MODE) {
            obj.field_dyn("inbox_view", ctx, &self.inbox_view)?;
        }
        if ctx.emits("next_work_item_sequence", Self::MODE) {
            obj.field_value_dyn(
                "next_work_item_sequence",
                ctx,
                &self.next_work_item_sequence,
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

/// `plane.app.serializers.project.ProjectLiteSerializer` (model: `db.Project`).
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
    /// `cover_image_url`: ReadOnlyField (source `cover_image_url`).
    pub cover_image_url: Option<&'a str>,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `description`: CharField (source `description`).
    pub description: &'a str,
}

impl<'a> ProjectLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "identifier",
        "name",
        "cover_image",
        "cover_image_url",
        "logo_props",
        "description",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
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
        if ctx.emits("cover_image_url", Self::MODE) {
            obj.field_dyn("cover_image_url", ctx, &self.cover_image_url)?;
        }
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_dyn("description", ctx, &self.description)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.project.ProjectMemberAdminSerializer` (model: `db.ProjectMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectMemberAdminSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `project`: ProjectLiteSerializer (source `project`).
    pub project: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `member`: UserAdminLiteSerializer (source `member`).
    pub member: Option<&'a crate::generated::app_user::UserAdminLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `comment`: CharField (source `comment`).
    pub comment: Option<&'a str>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `default_props`: JSONField (source `default_props`).
    pub default_props: Option<crate::fields::PgJson<'a>>,
    /// `preferences`: JSONField (source `preferences`).
    pub preferences: Option<crate::fields::PgJson<'a>>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> ProjectMemberAdminSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace",
        "project",
        "member",
        "created_at",
        "updated_at",
        "deleted_at",
        "comment",
        "role",
        "view_props",
        "default_props",
        "preferences",
        "sort_order",
        "is_active",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectMemberAdminSerializer<'a> {
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
            obj.field_nested_dyn(
                "workspace",
                ctx,
                self.workspace
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_nested_dyn(
                "project",
                ctx,
                self.project
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("member", Self::MODE) {
            obj.field_nested_dyn(
                "member",
                ctx,
                self.member
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
        if ctx.emits("comment", Self::MODE) {
            obj.field_dyn("comment", ctx, &self.comment)?;
        }
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
        }
        if ctx.emits("default_props", Self::MODE) {
            obj.field_json_dyn("default_props", ctx, self.default_props)?;
        }
        if ctx.emits("preferences", Self::MODE) {
            obj.field_json_dyn("preferences", ctx, self.preferences)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
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

/// `plane.app.serializers.project.ProjectMemberInvitePublicSerializer` (model: `db.ProjectMemberInvite`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectMemberInvitePublicSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `project`: ProjectLiteSerializer (source `project`).
    pub project: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `workspace`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `message`: CharField (source `message`).
    pub message: Option<&'a str>,
    /// `accepted`: BooleanField (source `accepted`).
    pub accepted: bool,
    /// `responded_at`: DateTimeField (source `responded_at`).
    pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl<'a> ProjectMemberInvitePublicSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "project",
        "workspace",
        "role",
        "message",
        "accepted",
        "responded_at",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectMemberInvitePublicSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_nested_dyn(
                "project",
                ctx,
                self.project
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_nested_dyn(
                "workspace",
                ctx,
                self.workspace
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        if ctx.emits("message", Self::MODE) {
            obj.field_dyn("message", ctx, &self.message)?;
        }
        if ctx.emits("accepted", Self::MODE) {
            obj.field_dyn("accepted", ctx, &self.accepted)?;
        }
        if ctx.emits("responded_at", Self::MODE) {
            obj.field_dyn(
                "responded_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.responded_at, ctx.tz()),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.project.ProjectMemberInviteSerializer` (model: `db.ProjectMemberInvite`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectMemberInviteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `project`: ProjectLiteSerializer (source `project`).
    pub project: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `workspace`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `email`: CharField (source `email`).
    pub email: &'a str,
    /// `accepted`: BooleanField (source `accepted`).
    pub accepted: bool,
    /// `token`: CharField (source `token`).
    pub token: &'a str,
    /// `message`: CharField (source `message`).
    pub message: Option<&'a str>,
    /// `responded_at`: DateTimeField (source `responded_at`).
    pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> ProjectMemberInviteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "project",
        "workspace",
        "created_at",
        "updated_at",
        "deleted_at",
        "email",
        "accepted",
        "token",
        "message",
        "responded_at",
        "role",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectMemberInviteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_nested_dyn(
                "project",
                ctx,
                self.project
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_nested_dyn(
                "workspace",
                ctx,
                self.workspace
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
        if ctx.emits("email", Self::MODE) {
            obj.field_dyn("email", ctx, &self.email)?;
        }
        if ctx.emits("accepted", Self::MODE) {
            obj.field_dyn("accepted", ctx, &self.accepted)?;
        }
        if ctx.emits("token", Self::MODE) {
            obj.field_dyn("token", ctx, &self.token)?;
        }
        if ctx.emits("message", Self::MODE) {
            obj.field_dyn("message", ctx, &self.message)?;
        }
        if ctx.emits("responded_at", Self::MODE) {
            obj.field_dyn(
                "responded_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.responded_at, ctx.tz()),
            )?;
        }
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
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

/// `plane.app.serializers.project.ProjectMemberLiteSerializer` (model: `db.ProjectMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectMemberLiteSerializer<'a> {
    /// `member`: UserLiteSerializer (source `member`).
    pub member: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `is_subscribed`: BooleanField (source `is_subscribed`).
    pub is_subscribed: crate::fields::FieldValue<bool>,
}

impl<'a> ProjectMemberLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["member", "id", "is_subscribed"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectMemberLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("member", Self::MODE) {
            obj.field_nested_dyn(
                "member",
                ctx,
                self.member
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("is_subscribed", Self::MODE) {
            obj.field_optional_dyn("is_subscribed", ctx, &self.is_subscribed)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.project.ProjectMemberPreferenceSerializer` (model: `db.ProjectMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectMemberPreferenceSerializer<'a> {
    /// `preferences`: JSONField (source `preferences`).
    pub preferences: Option<crate::fields::PgJson<'a>>,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: uuid::Uuid,
    /// `member_id`: ReadOnlyField (source `member_id`).
    pub member_id: Option<uuid::Uuid>,
    /// `workspace_id`: ReadOnlyField (source `workspace_id`).
    pub workspace_id: uuid::Uuid,
}

impl<'a> ProjectMemberPreferenceSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] =
        &["preferences", "project_id", "member_id", "workspace_id"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectMemberPreferenceSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("preferences", Self::MODE) {
            obj.field_json_dyn("preferences", ctx, self.preferences)?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        if ctx.emits("member_id", Self::MODE) {
            obj.field_dyn(
                "member_id",
                ctx,
                &self.member_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("workspace_id", Self::MODE) {
            obj.field_dyn(
                "workspace_id",
                ctx,
                &crate::fields::DrfUuid(self.workspace_id),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.project.ProjectMemberRoleSerializer` (model: `db.ProjectMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectMemberRoleSerializer {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `member`: PrimaryKeyRelatedField (source `member`).
    pub member: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: uuid::Uuid,
    /// `original_role`: IntegerField (source `role`).
    pub original_role: i64,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ProjectMemberRoleSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "role",
        "member",
        "project",
        "original_role",
        "created_at",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl crate::ser::DrfSerialize for ProjectMemberRoleSerializer {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        if ctx.emits("member", Self::MODE) {
            obj.field_dyn("member", ctx, &self.member.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &crate::fields::DrfUuid(self.project))?;
        }
        if ctx.emits("original_role", Self::MODE) {
            obj.field_dyn("original_role", ctx, &self.original_role)?;
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

/// `plane.app.serializers.project.ProjectMemberSerializer` (model: `db.ProjectMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectMemberSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `project`: ProjectLiteSerializer (source `project`).
    pub project: Option<&'a crate::generated::app_project::ProjectLiteSerializer<'a>>,
    /// `member`: UserLiteSerializer (source `member`).
    pub member: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `comment`: CharField (source `comment`).
    pub comment: Option<&'a str>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `default_props`: JSONField (source `default_props`).
    pub default_props: Option<crate::fields::PgJson<'a>>,
    /// `preferences`: JSONField (source `preferences`).
    pub preferences: Option<crate::fields::PgJson<'a>>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> ProjectMemberSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace",
        "project",
        "member",
        "created_at",
        "updated_at",
        "deleted_at",
        "comment",
        "role",
        "view_props",
        "default_props",
        "preferences",
        "sort_order",
        "is_active",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectMemberSerializer<'a> {
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
            obj.field_nested_dyn(
                "workspace",
                ctx,
                self.workspace
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_nested_dyn(
                "project",
                ctx,
                self.project
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("member", Self::MODE) {
            obj.field_nested_dyn(
                "member",
                ctx,
                self.member
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
        if ctx.emits("comment", Self::MODE) {
            obj.field_dyn("comment", ctx, &self.comment)?;
        }
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
        }
        if ctx.emits("default_props", Self::MODE) {
            obj.field_json_dyn("default_props", ctx, self.default_props)?;
        }
        if ctx.emits("preferences", Self::MODE) {
            obj.field_json_dyn("preferences", ctx, self.preferences)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
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

/// `plane.app.serializers.project.ProjectPublicMemberSerializer` (model: `db.ProjectPublicMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectPublicMemberSerializer {
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
    /// `member`: PrimaryKeyRelatedField (source `member`).
    pub member: uuid::Uuid,
}

impl ProjectPublicMemberSerializer {
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
        "member",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl crate::ser::DrfSerialize for ProjectPublicMemberSerializer {
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
        if ctx.emits("member", Self::MODE) {
            obj.field_dyn("member", ctx, &crate::fields::DrfUuid(self.member))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.project.ProjectSerializer` (model: `db.Project`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace_detail`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace_detail: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `inbox_view`: BooleanField (source `intake_view`).
    pub inbox_view: bool,
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
        "workspace_detail",
        "inbox_view",
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
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
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
        if ctx.emits("workspace_detail", Self::MODE) {
            obj.field_nested_dyn(
                "workspace_detail",
                ctx,
                self.workspace_detail
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("inbox_view", Self::MODE) {
            obj.field_dyn("inbox_view", ctx, &self.inbox_view)?;
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
