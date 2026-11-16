// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.api.serializers.member` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.api.serializers.member.ProjectMemberLiteAPISerializer` (model: `db.ProjectMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectMemberLiteAPISerializer<'a> {
    /// `id`: UUIDField (source `member.id`).
    pub id: uuid::Uuid,
    /// `first_name`: CharField (source `member.first_name`).
    pub first_name: &'a str,
    /// `last_name`: CharField (source `member.last_name`).
    pub last_name: &'a str,
    /// `email`: EmailField (source `member.email`).
    pub email: Option<&'a str>,
    /// `avatar`: CharField (source `member.avatar`).
    pub avatar: &'a str,
    /// `avatar_url`: CharField (source `member.avatar_url`).
    pub avatar_url: Option<&'a str>,
    /// `display_name`: CharField (source `member.display_name`).
    pub display_name: &'a str,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `is_bot`: BooleanField (source `member.is_bot`).
    pub is_bot: bool,
}

impl<'a> ProjectMemberLiteAPISerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "first_name",
        "last_name",
        "email",
        "avatar",
        "avatar_url",
        "display_name",
        "role",
        "is_active",
        "is_bot",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for ProjectMemberLiteAPISerializer<'a> {
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
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
        }
        if ctx.emits("is_bot", Self::MODE) {
            obj.field_dyn("is_bot", ctx, &self.is_bot)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.member.ProjectMemberSerializer` (model: `db.ProjectMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectMemberSerializer {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `member`: PrimaryKeyRelatedField (source `member`).
    pub member: Option<uuid::Uuid>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
}

impl ProjectMemberSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["id", "member", "role"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl crate::ser::DrfSerialize for ProjectMemberSerializer {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("member", Self::MODE) {
            obj.field_dyn("member", ctx, &self.member.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.member.WorkspaceMemberLiteAPISerializer` (model: `db.WorkspaceMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceMemberLiteAPISerializer<'a> {
    /// `id`: UUIDField (source `member.id`).
    pub id: uuid::Uuid,
    /// `first_name`: CharField (source `member.first_name`).
    pub first_name: &'a str,
    /// `last_name`: CharField (source `member.last_name`).
    pub last_name: &'a str,
    /// `email`: EmailField (source `member.email`).
    pub email: Option<&'a str>,
    /// `avatar`: CharField (source `member.avatar`).
    pub avatar: &'a str,
    /// `avatar_url`: CharField (source `member.avatar_url`).
    pub avatar_url: Option<&'a str>,
    /// `display_name`: CharField (source `member.display_name`).
    pub display_name: &'a str,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `is_bot`: BooleanField (source `member.is_bot`).
    pub is_bot: bool,
}

impl<'a> WorkspaceMemberLiteAPISerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "first_name",
        "last_name",
        "email",
        "avatar",
        "avatar_url",
        "display_name",
        "role",
        "is_active",
        "is_bot",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceMemberLiteAPISerializer<'a> {
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
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
        }
        if ctx.emits("is_bot", Self::MODE) {
            obj.field_dyn("is_bot", ctx, &self.is_bot)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
