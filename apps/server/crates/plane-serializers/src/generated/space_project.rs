// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.space.serializer.project` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.space.serializer.project.ProjectLiteSerializer` (model: `db.Project`).
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
        if ctx.emits("icon_prop", Self::MODE) {
            obj.field_json_dyn("icon_prop", ctx, self.icon_prop)?;
        }
        if ctx.emits("emoji", Self::MODE) {
            obj.field_dyn("emoji", ctx, &self.emoji)?;
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
