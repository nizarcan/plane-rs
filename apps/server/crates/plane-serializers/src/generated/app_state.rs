// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.state` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.state.StateLiteSerializer` (model: `db.State`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct StateLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `color`: CharField (source `color`).
    pub color: &'a str,
    /// `group`: ChoiceField (source `group`).
    pub group: &'a str,
}

impl<'a> StateLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["id", "name", "color", "group"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for StateLiteSerializer<'a> {
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
        if ctx.emits("color", Self::MODE) {
            obj.field_dyn("color", ctx, &self.color)?;
        }
        if ctx.emits("group", Self::MODE) {
            obj.field_dyn("group", ctx, &self.group)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.state.StateSerializer` (model: `db.State`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct StateSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: uuid::Uuid,
    /// `workspace_id`: ReadOnlyField (source `workspace_id`).
    pub workspace_id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `color`: CharField (source `color`).
    pub color: &'a str,
    /// `group`: ChoiceField (source `group`).
    pub group: &'a str,
    /// `default`: BooleanField (source `default`).
    pub default: bool,
    /// `description`: CharField (source `description`).
    pub description: &'a str,
    /// `sequence`: FloatField (source `sequence`).
    pub sequence: f64,
    /// `order`: FloatField (source `order`).
    pub order: crate::fields::FieldValue<f64>,
}

impl<'a> StateSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "project_id",
        "workspace_id",
        "name",
        "color",
        "group",
        "default",
        "description",
        "sequence",
        "order",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for StateSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        if ctx.emits("workspace_id", Self::MODE) {
            obj.field_dyn(
                "workspace_id",
                ctx,
                &crate::fields::DrfUuid(self.workspace_id),
            )?;
        }
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("color", Self::MODE) {
            obj.field_dyn("color", ctx, &self.color)?;
        }
        if ctx.emits("group", Self::MODE) {
            obj.field_dyn("group", ctx, &self.group)?;
        }
        if ctx.emits("default", Self::MODE) {
            obj.field_dyn("default", ctx, &self.default)?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_dyn("description", ctx, &self.description)?;
        }
        if ctx.emits("sequence", Self::MODE) {
            obj.field_dyn("sequence", ctx, &self.sequence)?;
        }
        if ctx.emits("order", Self::MODE) {
            obj.field_optional_dyn("order", ctx, &self.order)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
