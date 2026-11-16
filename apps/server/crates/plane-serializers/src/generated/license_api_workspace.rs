// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.license.api.serializers.workspace` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.license.api.serializers.workspace.WorkspaceSerializer` (model: `db.Workspace`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `owner`: UserLiteSerializer (source `owner`).
    pub owner: Option<&'a crate::generated::license_api_user::UserLiteSerializer<'a>>,
    /// `logo_url`: CharField (source `logo_url`).
    pub logo_url: Option<&'a str>,
    /// `total_projects`: IntegerField (source `total_projects`).
    pub total_projects: Option<i64>,
    /// `total_members`: IntegerField (source `total_members`).
    pub total_members: Option<i64>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `logo`: CharField (source `logo`).
    pub logo: Option<&'a str>,
    /// `slug`: SlugField (source `slug`).
    pub slug: &'a str,
    /// `organization_size`: CharField (source `organization_size`).
    pub organization_size: Option<&'a str>,
    /// `timezone`: ChoiceField (source `timezone`).
    pub timezone: &'a str,
    /// `background_color`: CharField (source `background_color`).
    pub background_color: &'a str,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `logo_asset`: PrimaryKeyRelatedField (source `logo_asset`).
    pub logo_asset: Option<uuid::Uuid>,
}

impl<'a> WorkspaceSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "owner",
        "logo_url",
        "total_projects",
        "total_members",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "logo",
        "slug",
        "organization_size",
        "timezone",
        "background_color",
        "created_by",
        "updated_by",
        "logo_asset",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("owner", Self::MODE) {
            obj.field_nested_dyn(
                "owner",
                ctx,
                self.owner
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("logo_url", Self::MODE) {
            obj.field_dyn("logo_url", ctx, &self.logo_url)?;
        }
        if ctx.emits("total_projects", Self::MODE)
            && let Some(value) = &self.total_projects
        {
            obj.field_dyn("total_projects", ctx, &value)?;
        }
        if ctx.emits("total_members", Self::MODE)
            && let Some(value) = &self.total_members
        {
            obj.field_dyn("total_members", ctx, &value)?;
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
        if ctx.emits("logo", Self::MODE) {
            obj.field_dyn("logo", ctx, &self.logo)?;
        }
        if ctx.emits("slug", Self::MODE) {
            obj.field_dyn("slug", ctx, &self.slug)?;
        }
        if ctx.emits("organization_size", Self::MODE) {
            obj.field_dyn("organization_size", ctx, &self.organization_size)?;
        }
        if ctx.emits("timezone", Self::MODE) {
            obj.field_dyn("timezone", ctx, &self.timezone)?;
        }
        if ctx.emits("background_color", Self::MODE) {
            obj.field_dyn("background_color", ctx, &self.background_color)?;
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
        if ctx.emits("logo_asset", Self::MODE) {
            obj.field_dyn(
                "logo_asset",
                ctx,
                &self.logo_asset.map(crate::fields::DrfUuid),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
