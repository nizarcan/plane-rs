// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.api.serializers.user` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.api.serializers.user.UserLiteSerializer` (model: `db.User`).
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
    /// `email`: CharField (source `email`).
    pub email: Option<&'a str>,
    /// `avatar`: CharField (source `avatar`).
    pub avatar: &'a str,
    /// `avatar_url`: CharField (source `avatar_url`).
    pub avatar_url: Option<&'a str>,
    /// `display_name`: CharField (source `display_name`).
    pub display_name: &'a str,
}

impl<'a> UserLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "first_name",
        "last_name",
        "email",
        "avatar",
        "avatar_url",
        "display_name",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
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
        if ctx.emits("email", Self::MODE) {
            obj.field_dyn("email", ctx, &self.email)?;
        }
        if ctx.emits("avatar", Self::MODE) {
            obj.field_dyn("avatar", ctx, &self.avatar)?;
        }
        if ctx.emits("avatar_url", Self::MODE) {
            obj.field_dyn("avatar_url", ctx, &self.avatar_url)?;
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
