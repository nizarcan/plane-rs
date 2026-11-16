// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.page` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.page.PageBinaryUpdateSerializer` (model: `no model`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct PageBinaryUpdateSerializer<'a> {
    /// `description_binary`: CharField (source `description_binary`).
    pub description_binary: &'a str,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
    /// `description_json`: JSONField (source `description_json`).
    pub description_json: Option<crate::fields::PgJson<'a>>,
}

impl<'a> PageBinaryUpdateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] =
        &["description_binary", "description_html", "description_json"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for PageBinaryUpdateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("description_binary", Self::MODE) {
            obj.field_dyn("description_binary", ctx, &self.description_binary)?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        if ctx.emits("description_json", Self::MODE) {
            obj.field_json_dyn("description_json", ctx, self.description_json)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.page.PageDetailSerializer` (model: `db.Page`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct PageDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `owned_by`: PrimaryKeyRelatedField (source `owned_by`).
    pub owned_by: uuid::Uuid,
    /// `access`: ChoiceField (source `access`).
    pub access: i64,
    /// `color`: CharField (source `color`).
    pub color: &'a str,
    /// `parent`: PrimaryKeyRelatedField (source `parent`).
    pub parent: Option<uuid::Uuid>,
    /// `is_favorite`: BooleanField (source `is_favorite`).
    pub is_favorite: crate::fields::FieldValue<bool>,
    /// `is_locked`: BooleanField (source `is_locked`).
    pub is_locked: bool,
    /// `archived_at`: DateField (source `archived_at`).
    pub archived_at: Option<chrono::NaiveDate>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `label_ids`: ListField (source `label_ids`).
    pub label_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `project_ids`: ListField (source `project_ids`).
    pub project_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
}

impl<'a> PageDetailSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "owned_by",
        "access",
        "color",
        "parent",
        "is_favorite",
        "is_locked",
        "archived_at",
        "workspace",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
        "view_props",
        "logo_props",
        "label_ids",
        "project_ids",
        "description_html",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for PageDetailSerializer<'a> {
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
        if ctx.emits("owned_by", Self::MODE) {
            obj.field_dyn("owned_by", ctx, &crate::fields::DrfUuid(self.owned_by))?;
        }
        if ctx.emits("access", Self::MODE) {
            obj.field_dyn("access", ctx, &self.access)?;
        }
        if ctx.emits("color", Self::MODE) {
            obj.field_dyn("color", ctx, &self.color)?;
        }
        if ctx.emits("parent", Self::MODE) {
            obj.field_dyn("parent", ctx, &self.parent.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("is_favorite", Self::MODE) {
            obj.field_optional_dyn("is_favorite", ctx, &self.is_favorite)?;
        }
        if ctx.emits("is_locked", Self::MODE) {
            obj.field_dyn("is_locked", ctx, &self.is_locked)?;
        }
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &self.archived_at.map(crate::fields::PyDate),
            )?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
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
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
        }
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("label_ids", Self::MODE) {
            obj.field_optional_dyn("label_ids", ctx, &self.label_ids)?;
        }
        if ctx.emits("project_ids", Self::MODE) {
            obj.field_optional_dyn("project_ids", ctx, &self.project_ids)?;
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

/// `plane.app.serializers.page.PageSerializer` (model: `db.Page`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct PageSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `owned_by`: PrimaryKeyRelatedField (source `owned_by`).
    pub owned_by: uuid::Uuid,
    /// `access`: ChoiceField (source `access`).
    pub access: i64,
    /// `color`: CharField (source `color`).
    pub color: &'a str,
    /// `parent`: PrimaryKeyRelatedField (source `parent`).
    pub parent: Option<uuid::Uuid>,
    /// `is_favorite`: BooleanField (source `is_favorite`).
    pub is_favorite: crate::fields::FieldValue<bool>,
    /// `is_locked`: BooleanField (source `is_locked`).
    pub is_locked: bool,
    /// `archived_at`: DateField (source `archived_at`).
    pub archived_at: Option<chrono::NaiveDate>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `label_ids`: ListField (source `label_ids`).
    pub label_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
    /// `project_ids`: ListField (source `project_ids`).
    pub project_ids: crate::fields::FieldValue<&'a [uuid::Uuid]>,
}

impl<'a> PageSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "owned_by",
        "access",
        "color",
        "parent",
        "is_favorite",
        "is_locked",
        "archived_at",
        "workspace",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
        "view_props",
        "logo_props",
        "label_ids",
        "project_ids",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for PageSerializer<'a> {
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
        if ctx.emits("owned_by", Self::MODE) {
            obj.field_dyn("owned_by", ctx, &crate::fields::DrfUuid(self.owned_by))?;
        }
        if ctx.emits("access", Self::MODE) {
            obj.field_dyn("access", ctx, &self.access)?;
        }
        if ctx.emits("color", Self::MODE) {
            obj.field_dyn("color", ctx, &self.color)?;
        }
        if ctx.emits("parent", Self::MODE) {
            obj.field_dyn("parent", ctx, &self.parent.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("is_favorite", Self::MODE) {
            obj.field_optional_dyn("is_favorite", ctx, &self.is_favorite)?;
        }
        if ctx.emits("is_locked", Self::MODE) {
            obj.field_dyn("is_locked", ctx, &self.is_locked)?;
        }
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &self.archived_at.map(crate::fields::PyDate),
            )?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
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
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
        }
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("label_ids", Self::MODE) {
            obj.field_optional_dyn("label_ids", ctx, &self.label_ids)?;
        }
        if ctx.emits("project_ids", Self::MODE) {
            obj.field_optional_dyn("project_ids", ctx, &self.project_ids)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.page.PageVersionDetailSerializer` (model: `db.PageVersion`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct PageVersionDetailSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `page`: PrimaryKeyRelatedField (source `page`).
    pub page: uuid::Uuid,
    /// `last_saved_at`: DateTimeField (source `last_saved_at`).
    pub last_saved_at: chrono::DateTime<chrono::Utc>,
    /// `description_binary`: ModelField (source `description_binary`).
    pub description_binary: Option<&'a [u8]>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
    /// `description_json`: JSONField (source `description_json`).
    pub description_json: Option<crate::fields::PgJson<'a>>,
    /// `owned_by`: PrimaryKeyRelatedField (source `owned_by`).
    pub owned_by: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> PageVersionDetailSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace",
        "page",
        "last_saved_at",
        "description_binary",
        "description_html",
        "description_json",
        "owned_by",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for PageVersionDetailSerializer<'a> {
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
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("page", Self::MODE) {
            obj.field_dyn("page", ctx, &crate::fields::DrfUuid(self.page))?;
        }
        if ctx.emits("last_saved_at", Self::MODE) {
            obj.field_dyn(
                "last_saved_at",
                ctx,
                &crate::fields::drf_datetime(self.last_saved_at, ctx.tz()),
            )?;
        }
        if ctx.emits("description_binary", Self::MODE) {
            obj.field_dyn(
                "description_binary",
                ctx,
                &self.description_binary.map(crate::fields::PyBase64),
            )?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        if ctx.emits("description_json", Self::MODE) {
            obj.field_json_dyn("description_json", ctx, self.description_json)?;
        }
        if ctx.emits("owned_by", Self::MODE) {
            obj.field_dyn("owned_by", ctx, &crate::fields::DrfUuid(self.owned_by))?;
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
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.page.PageVersionSerializer` (model: `db.PageVersion`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct PageVersionSerializer {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `page`: PrimaryKeyRelatedField (source `page`).
    pub page: uuid::Uuid,
    /// `last_saved_at`: DateTimeField (source `last_saved_at`).
    pub last_saved_at: chrono::DateTime<chrono::Utc>,
    /// `owned_by`: PrimaryKeyRelatedField (source `owned_by`).
    pub owned_by: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl PageVersionSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace",
        "page",
        "last_saved_at",
        "owned_by",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl crate::ser::DrfSerialize for PageVersionSerializer {
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
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("page", Self::MODE) {
            obj.field_dyn("page", ctx, &crate::fields::DrfUuid(self.page))?;
        }
        if ctx.emits("last_saved_at", Self::MODE) {
            obj.field_dyn(
                "last_saved_at",
                ctx,
                &crate::fields::drf_datetime(self.last_saved_at, ctx.tz()),
            )?;
        }
        if ctx.emits("owned_by", Self::MODE) {
            obj.field_dyn("owned_by", ctx, &crate::fields::DrfUuid(self.owned_by))?;
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
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
