// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.view` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.view.IssueViewSerializer` (model: `db.IssueView`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueViewSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `is_favorite`: BooleanField (source `is_favorite`).
    pub is_favorite: crate::fields::FieldValue<bool>,
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
    /// `query`: JSONField (source `query`).
    pub query: Option<crate::fields::PgJson<'a>>,
    /// `filters`: JSONField (source `filters`).
    pub filters: Option<crate::fields::PgJson<'a>>,
    /// `display_filters`: JSONField (source `display_filters`).
    pub display_filters: Option<crate::fields::PgJson<'a>>,
    /// `display_properties`: JSONField (source `display_properties`).
    pub display_properties: Option<crate::fields::PgJson<'a>>,
    /// `rich_filters`: JSONField (source `rich_filters`).
    pub rich_filters: Option<crate::fields::PgJson<'a>>,
    /// `access`: ChoiceField (source `access`).
    pub access: i64,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `is_locked`: BooleanField (source `is_locked`).
    pub is_locked: bool,
    /// `archived_at`: DateTimeField (source `archived_at`).
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: Option<uuid::Uuid>,
    /// `owned_by`: PrimaryKeyRelatedField (source `owned_by`).
    pub owned_by: uuid::Uuid,
}

impl<'a> IssueViewSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "is_favorite",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "description",
        "query",
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
        "access",
        "sort_order",
        "logo_props",
        "is_locked",
        "archived_at",
        "created_by",
        "updated_by",
        "workspace",
        "project",
        "owned_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for IssueViewSerializer<'a> {
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
        if ctx.emits("query", Self::MODE) {
            obj.field_json_dyn("query", ctx, self.query)?;
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
        if ctx.emits("access", Self::MODE) {
            obj.field_dyn("access", ctx, &self.access)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("is_locked", Self::MODE) {
            obj.field_dyn("is_locked", ctx, &self.is_locked)?;
        }
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.archived_at, ctx.tz()),
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
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &self.project.map(crate::fields::DrfUuid))?;
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
