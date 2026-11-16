// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.space.serializer.user` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.space.serializer.user.UserLiteSerializer` (model: `db.User`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct UserLiteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `first_name`: CharField (source `first_name`).
    pub first_name: &'a str,
    /// `last_name`: CharField (source `last_name`).
    pub last_name: &'a str,
    /// `avatar`: CharField (source `avatar`).
    pub avatar: &'a str,
    /// `avatar_url`: ReadOnlyField (source `avatar_url`).
    pub avatar_url: Option<&'a str>,
    /// `is_bot`: BooleanField (source `is_bot`).
    pub is_bot: bool,
    /// `display_name`: CharField (source `display_name`).
    pub display_name: &'a str,
}

impl<'a> UserLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "first_name",
        "last_name",
        "avatar",
        "avatar_url",
        "is_bot",
        "display_name",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for UserLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("first_name", Self::MODE) {
            obj.field_dyn("first_name", ctx, &self.first_name)?;
        }
        if ctx.emits("last_name", Self::MODE) {
            obj.field_dyn("last_name", ctx, &self.last_name)?;
        }
        if ctx.emits("avatar", Self::MODE) {
            obj.field_dyn("avatar", ctx, &self.avatar)?;
        }
        if ctx.emits("avatar_url", Self::MODE) {
            obj.field_dyn("avatar_url", ctx, &self.avatar_url)?;
        }
        if ctx.emits("is_bot", Self::MODE) {
            obj.field_dyn("is_bot", ctx, &self.is_bot)?;
        }
        if ctx.emits("display_name", Self::MODE) {
            obj.field_dyn("display_name", ctx, &self.display_name)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
