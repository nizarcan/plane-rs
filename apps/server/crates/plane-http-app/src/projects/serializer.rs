// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Two hand-written serializers.
//!
//! Both exist because of a gap between `plane-serializers`' generated code and
//! DRF's actual output rules; both are reported to the orchestrator.
//!
//! 1. **`ProjectListSerializer`'s annotation fields.** DRF's
//!    `Serializer.to_representation` writes `null` for any attribute that *is*
//!    `None` (`rest_framework/serializers.py:541-546`) and only raises
//!    `SkipField` when the attribute is *missing*. `anchor`
//!    (`serializers.CharField(read_only=True)`), `is_favorite`, `member_role`
//!    and `sort_order` are annotations: the attribute always exists, so a
//!    project with no board, no membership or no sort order renders
//!    `"anchor": null`, not an absent key. The generated struct types them
//!    `Option<…>` and skips a `None`, which drops the key — so this module
//!    writes them unconditionally, in the same order and with the same types.
//!    (A model column such as `emoji` is generated correctly; the difference is
//!    that an annotation field's `None` is a value, not a missing attribute.)
//!
//! 2. **`_without_personal_fields`.** `ProjectMemberViewSet.retrieve` and
//!    `ProjectMemberAdminSerializer` (a plain `BaseSerializer`, `FilterMode::None`)
//!    pops `preferences`, `view_props` and `default_props` from the serialized
//!    dict when the row is not the caller's own
//!    (`plane/app/views/project/member.py:30-34,217-219`). A `fields=` mask
//!    cannot express a removal for `FilterMode::None`, so the reduced object is
//!    written here: the same keys in the same order, minus those three.

use chrono::{DateTime, Utc};
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_user::UserAdminLiteSerializer;
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use plane_serializers::{
    DrfError, DrfSerialize, DrfValue, DrfWriter, FilterMode, PgJson, RenderCtx, drf_datetime,
    drf_datetime_opt,
};
use uuid::Uuid;

/// `ProjectListSerializer.project_details` — the URL a project cover resolves
/// to, already resolved by [`super::lite`].
#[derive(Debug)]
pub struct ProjectList<'a> {
    pub id: Uuid,
    pub is_favorite: bool,
    pub sort_order: Option<f64>,
    pub member_role: Option<i64>,
    pub anchor: Option<&'a str>,
    pub members: DrfValue<'a>,
    pub cover_image_url: Option<&'a str>,
    pub inbox_view: bool,
    pub next_work_item_sequence: DrfValue<'a>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub name: &'a str,
    pub description: &'a str,
    pub description_text: Option<PgJson<'a>>,
    pub description_html: Option<PgJson<'a>>,
    pub network: i64,
    pub identifier: &'a str,
    pub emoji: Option<&'a str>,
    pub icon_prop: Option<PgJson<'a>>,
    pub module_view: bool,
    pub cycle_view: bool,
    pub issue_views_view: bool,
    pub page_view: bool,
    pub intake_view: bool,
    pub is_time_tracking_enabled: bool,
    pub is_issue_type_enabled: bool,
    pub guest_view_all_features: bool,
    pub cover_image: Option<&'a str>,
    pub archive_in: i64,
    pub close_in: i64,
    pub logo_props: Option<PgJson<'a>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub timezone: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub workspace: Uuid,
    pub default_assignee: Option<Uuid>,
    pub project_lead: Option<Uuid>,
    pub cover_image_asset: Option<Uuid>,
    pub estimate: Option<Uuid>,
    pub default_state: Option<Uuid>,
}

impl<'a> ProjectList<'a> {
    /// `_readable_fields` order (`ProjectListSerializer.FIELDS`).
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
}

impl DrfSerialize for ProjectList<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        const MODE: FilterMode = FilterMode::OutputOnly;
        let mut obj = w.begin_object();
        if ctx.emits("id", MODE) {
            obj.field_dyn("id", ctx, &plane_serializers::DrfUuid(self.id))?;
        }
        if ctx.emits("is_favorite", MODE) {
            obj.field_dyn("is_favorite", ctx, &self.is_favorite)?;
        }
        if ctx.emits("sort_order", MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("member_role", MODE) {
            obj.field_dyn("member_role", ctx, &self.member_role)?;
        }
        if ctx.emits("anchor", MODE) {
            obj.field_dyn("anchor", ctx, &self.anchor)?;
        }
        if ctx.emits("members", MODE) {
            obj.field_value_dyn("members", ctx, &self.members)?;
        }
        if ctx.emits("cover_image_url", MODE) {
            obj.field_dyn("cover_image_url", ctx, &self.cover_image_url)?;
        }
        if ctx.emits("inbox_view", MODE) {
            obj.field_dyn("inbox_view", ctx, &self.inbox_view)?;
        }
        if ctx.emits("next_work_item_sequence", MODE) {
            obj.field_value_dyn(
                "next_work_item_sequence",
                ctx,
                &self.next_work_item_sequence,
            )?;
        }
        if ctx.emits("created_at", MODE) {
            obj.field_dyn("created_at", ctx, &drf_datetime(self.created_at, ctx.tz()))?;
        }
        if ctx.emits("updated_at", MODE) {
            obj.field_dyn("updated_at", ctx, &drf_datetime(self.updated_at, ctx.tz()))?;
        }
        if ctx.emits("deleted_at", MODE) {
            obj.field_dyn(
                "deleted_at",
                ctx,
                &drf_datetime_opt(self.deleted_at, ctx.tz()),
            )?;
        }
        if ctx.emits("name", MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("description", MODE) {
            obj.field_dyn("description", ctx, &self.description)?;
        }
        if ctx.emits("description_text", MODE) {
            obj.field_json_dyn("description_text", ctx, self.description_text)?;
        }
        if ctx.emits("description_html", MODE) {
            obj.field_json_dyn("description_html", ctx, self.description_html)?;
        }
        if ctx.emits("network", MODE) {
            obj.field_dyn("network", ctx, &self.network)?;
        }
        if ctx.emits("identifier", MODE) {
            obj.field_dyn("identifier", ctx, &self.identifier)?;
        }
        if ctx.emits("emoji", MODE) {
            obj.field_dyn("emoji", ctx, &self.emoji)?;
        }
        if ctx.emits("icon_prop", MODE) {
            obj.field_json_dyn("icon_prop", ctx, self.icon_prop)?;
        }
        if ctx.emits("module_view", MODE) {
            obj.field_dyn("module_view", ctx, &self.module_view)?;
        }
        if ctx.emits("cycle_view", MODE) {
            obj.field_dyn("cycle_view", ctx, &self.cycle_view)?;
        }
        if ctx.emits("issue_views_view", MODE) {
            obj.field_dyn("issue_views_view", ctx, &self.issue_views_view)?;
        }
        if ctx.emits("page_view", MODE) {
            obj.field_dyn("page_view", ctx, &self.page_view)?;
        }
        if ctx.emits("intake_view", MODE) {
            obj.field_dyn("intake_view", ctx, &self.intake_view)?;
        }
        if ctx.emits("is_time_tracking_enabled", MODE) {
            obj.field_dyn(
                "is_time_tracking_enabled",
                ctx,
                &self.is_time_tracking_enabled,
            )?;
        }
        if ctx.emits("is_issue_type_enabled", MODE) {
            obj.field_dyn("is_issue_type_enabled", ctx, &self.is_issue_type_enabled)?;
        }
        if ctx.emits("guest_view_all_features", MODE) {
            obj.field_dyn(
                "guest_view_all_features",
                ctx,
                &self.guest_view_all_features,
            )?;
        }
        if ctx.emits("cover_image", MODE) {
            obj.field_dyn("cover_image", ctx, &self.cover_image)?;
        }
        if ctx.emits("archive_in", MODE) {
            obj.field_dyn("archive_in", ctx, &self.archive_in)?;
        }
        if ctx.emits("close_in", MODE) {
            obj.field_dyn("close_in", ctx, &self.close_in)?;
        }
        if ctx.emits("logo_props", MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("archived_at", MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &drf_datetime_opt(self.archived_at, ctx.tz()),
            )?;
        }
        if ctx.emits("timezone", MODE) {
            obj.field_dyn("timezone", ctx, &self.timezone)?;
        }
        if ctx.emits("external_source", MODE) {
            obj.field_dyn("external_source", ctx, &self.external_source)?;
        }
        if ctx.emits("external_id", MODE) {
            obj.field_dyn("external_id", ctx, &self.external_id)?;
        }
        if ctx.emits("created_by", MODE) {
            obj.field_dyn(
                "created_by",
                ctx,
                &self.created_by.map(plane_serializers::DrfUuid),
            )?;
        }
        if ctx.emits("updated_by", MODE) {
            obj.field_dyn(
                "updated_by",
                ctx,
                &self.updated_by.map(plane_serializers::DrfUuid),
            )?;
        }
        if ctx.emits("workspace", MODE) {
            obj.field_dyn(
                "workspace",
                ctx,
                &plane_serializers::DrfUuid(self.workspace),
            )?;
        }
        if ctx.emits("default_assignee", MODE) {
            obj.field_dyn(
                "default_assignee",
                ctx,
                &self.default_assignee.map(plane_serializers::DrfUuid),
            )?;
        }
        if ctx.emits("project_lead", MODE) {
            obj.field_dyn(
                "project_lead",
                ctx,
                &self.project_lead.map(plane_serializers::DrfUuid),
            )?;
        }
        if ctx.emits("cover_image_asset", MODE) {
            obj.field_dyn(
                "cover_image_asset",
                ctx,
                &self.cover_image_asset.map(plane_serializers::DrfUuid),
            )?;
        }
        if ctx.emits("estimate", MODE) {
            obj.field_dyn(
                "estimate",
                ctx,
                &self.estimate.map(plane_serializers::DrfUuid),
            )?;
        }
        if ctx.emits("default_state", MODE) {
            obj.field_dyn(
                "default_state",
                ctx,
                &self.default_state.map(plane_serializers::DrfUuid),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, MODE)?;
        obj.end();
        Ok(())
    }
}

/// `ProjectMemberAdminSerializer(project_member)` with
/// `_without_personal_fields` already applied.
///
/// The keys are the serializer's declaration order minus `preferences`,
/// `view_props` and `default_props` (`plane/app/serializers/project.py:192-200`,
/// `member.py:27-34`).
#[derive(Debug)]
pub struct ProjectMemberAdminPublic<'a> {
    pub id: Uuid,
    pub workspace: Option<&'a WorkspaceLiteSerializer<'a>>,
    pub project: Option<&'a ProjectLiteSerializer<'a>>,
    pub member: Option<&'a UserAdminLiteSerializer<'a>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub comment: Option<&'a str>,
    pub role: i64,
    pub sort_order: f64,
    pub is_active: bool,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
}

impl DrfSerialize for ProjectMemberAdminPublic<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &plane_serializers::DrfUuid(self.id))?;
        obj.field_nested_dyn(
            "workspace",
            ctx,
            self.workspace
                .map(|value| value as &(dyn DrfSerialize + '_)),
        )?;
        obj.field_nested_dyn(
            "project",
            ctx,
            self.project.map(|value| value as &(dyn DrfSerialize + '_)),
        )?;
        obj.field_nested_dyn(
            "member",
            ctx,
            self.member.map(|value| value as &(dyn DrfSerialize + '_)),
        )?;
        obj.field_dyn("created_at", ctx, &drf_datetime(self.created_at, ctx.tz()))?;
        obj.field_dyn("updated_at", ctx, &drf_datetime(self.updated_at, ctx.tz()))?;
        obj.field_dyn(
            "deleted_at",
            ctx,
            &drf_datetime_opt(self.deleted_at, ctx.tz()),
        )?;
        obj.field_dyn("comment", ctx, &self.comment)?;
        obj.field_dyn("role", ctx, &self.role)?;
        obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        obj.field_dyn("is_active", ctx, &self.is_active)?;
        obj.field_dyn(
            "created_by",
            ctx,
            &self.created_by.map(plane_serializers::DrfUuid),
        )?;
        obj.field_dyn(
            "updated_by",
            ctx,
            &self.updated_by.map(plane_serializers::DrfUuid),
        )?;
        obj.end();
        Ok(())
    }
}
