// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.license.api.serializers.admin` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.license.api.serializers.admin.InstanceAdminMeSerializer` (model: `db.User`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct InstanceAdminMeSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `avatar`: CharField (source `avatar`).
    pub avatar: &'a str,
    /// `avatar_url`: ReadOnlyField (source `avatar_url`).
    pub avatar_url: Option<&'a str>,
    /// `cover_image`: URLField (source `cover_image`).
    pub cover_image: Option<&'a str>,
    /// `date_joined`: DateTimeField (source `date_joined`).
    pub date_joined: chrono::DateTime<chrono::Utc>,
    /// `display_name`: CharField (source `display_name`).
    pub display_name: &'a str,
    /// `email`: CharField (source `email`).
    pub email: Option<&'a str>,
    /// `first_name`: CharField (source `first_name`).
    pub first_name: &'a str,
    /// `last_name`: CharField (source `last_name`).
    pub last_name: &'a str,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `is_bot`: BooleanField (source `is_bot`).
    pub is_bot: bool,
    /// `is_email_verified`: BooleanField (source `is_email_verified`).
    pub is_email_verified: bool,
    /// `user_timezone`: ChoiceField (source `user_timezone`).
    pub user_timezone: &'a str,
    /// `username`: CharField (source `username`).
    pub username: &'a str,
    /// `is_password_autoset`: BooleanField (source `is_password_autoset`).
    pub is_password_autoset: bool,
}

impl<'a> InstanceAdminMeSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "avatar",
        "avatar_url",
        "cover_image",
        "date_joined",
        "display_name",
        "email",
        "first_name",
        "last_name",
        "is_active",
        "is_bot",
        "is_email_verified",
        "user_timezone",
        "username",
        "is_password_autoset",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for InstanceAdminMeSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("avatar", Self::MODE) {
            obj.field_dyn("avatar", ctx, &self.avatar)?;
        }
        if ctx.emits("avatar_url", Self::MODE) {
            obj.field_dyn("avatar_url", ctx, &self.avatar_url)?;
        }
        if ctx.emits("cover_image", Self::MODE) {
            obj.field_dyn("cover_image", ctx, &self.cover_image)?;
        }
        if ctx.emits("date_joined", Self::MODE) {
            obj.field_dyn(
                "date_joined",
                ctx,
                &crate::fields::drf_datetime(self.date_joined, ctx.tz()),
            )?;
        }
        if ctx.emits("display_name", Self::MODE) {
            obj.field_dyn("display_name", ctx, &self.display_name)?;
        }
        if ctx.emits("email", Self::MODE) {
            obj.field_dyn("email", ctx, &self.email)?;
        }
        if ctx.emits("first_name", Self::MODE) {
            obj.field_dyn("first_name", ctx, &self.first_name)?;
        }
        if ctx.emits("last_name", Self::MODE) {
            obj.field_dyn("last_name", ctx, &self.last_name)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
        }
        if ctx.emits("is_bot", Self::MODE) {
            obj.field_dyn("is_bot", ctx, &self.is_bot)?;
        }
        if ctx.emits("is_email_verified", Self::MODE) {
            obj.field_dyn("is_email_verified", ctx, &self.is_email_verified)?;
        }
        if ctx.emits("user_timezone", Self::MODE) {
            obj.field_dyn("user_timezone", ctx, &self.user_timezone)?;
        }
        if ctx.emits("username", Self::MODE) {
            obj.field_dyn("username", ctx, &self.username)?;
        }
        if ctx.emits("is_password_autoset", Self::MODE) {
            obj.field_dyn("is_password_autoset", ctx, &self.is_password_autoset)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.license.api.serializers.admin.InstanceAdminSerializer` (model: `license.InstanceAdmin`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct InstanceAdminSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `user_detail`: UserAdminLiteSerializer (source `user`).
    pub user_detail: Option<&'a crate::generated::app_user::UserAdminLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `is_verified`: BooleanField (source `is_verified`).
    pub is_verified: bool,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: Option<uuid::Uuid>,
    /// `instance`: PrimaryKeyRelatedField (source `instance`).
    pub instance: uuid::Uuid,
}

impl<'a> InstanceAdminSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "user_detail",
        "created_at",
        "updated_at",
        "deleted_at",
        "role",
        "is_verified",
        "created_by",
        "updated_by",
        "user",
        "instance",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for InstanceAdminSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("user_detail", Self::MODE) {
            obj.field_nested_dyn(
                "user_detail",
                ctx,
                self.user_detail
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
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        if ctx.emits("is_verified", Self::MODE) {
            obj.field_dyn("is_verified", ctx, &self.is_verified)?;
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
        if ctx.emits("user", Self::MODE) {
            obj.field_dyn("user", ctx, &self.user.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("instance", Self::MODE) {
            obj.field_dyn("instance", ctx, &crate::fields::DrfUuid(self.instance))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
