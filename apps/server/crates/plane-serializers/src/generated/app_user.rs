// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.user` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.user.AccountSerializer` (model: `db.Account`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct AccountSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `provider_account_id`: CharField (source `provider_account_id`).
    pub provider_account_id: &'a str,
    /// `provider`: ChoiceField (source `provider`).
    pub provider: &'a str,
    /// `access_token`: CharField (source `access_token`).
    pub access_token: &'a str,
    /// `access_token_expired_at`: DateTimeField (source `access_token_expired_at`).
    pub access_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `refresh_token`: CharField (source `refresh_token`).
    pub refresh_token: Option<&'a str>,
    /// `refresh_token_expired_at`: DateTimeField (source `refresh_token_expired_at`).
    pub refresh_token_expired_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `last_connected_at`: DateTimeField (source `last_connected_at`).
    pub last_connected_at: chrono::DateTime<chrono::Utc>,
    /// `id_token`: CharField (source `id_token`).
    pub id_token: &'a str,
    /// `metadata`: JSONField (source `metadata`).
    pub metadata: Option<crate::fields::PgJson<'a>>,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: uuid::Uuid,
}

impl<'a> AccountSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "provider_account_id",
        "provider",
        "access_token",
        "access_token_expired_at",
        "refresh_token",
        "refresh_token_expired_at",
        "last_connected_at",
        "id_token",
        "metadata",
        "user",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for AccountSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
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
        if ctx.emits("provider_account_id", Self::MODE) {
            obj.field_dyn("provider_account_id", ctx, &self.provider_account_id)?;
        }
        if ctx.emits("provider", Self::MODE) {
            obj.field_dyn("provider", ctx, &self.provider)?;
        }
        if ctx.emits("access_token", Self::MODE) {
            obj.field_dyn("access_token", ctx, &self.access_token)?;
        }
        if ctx.emits("access_token_expired_at", Self::MODE) {
            obj.field_dyn(
                "access_token_expired_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.access_token_expired_at, ctx.tz()),
            )?;
        }
        if ctx.emits("refresh_token", Self::MODE) {
            obj.field_dyn("refresh_token", ctx, &self.refresh_token)?;
        }
        if ctx.emits("refresh_token_expired_at", Self::MODE) {
            obj.field_dyn(
                "refresh_token_expired_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.refresh_token_expired_at, ctx.tz()),
            )?;
        }
        if ctx.emits("last_connected_at", Self::MODE) {
            obj.field_dyn(
                "last_connected_at",
                ctx,
                &crate::fields::drf_datetime(self.last_connected_at, ctx.tz()),
            )?;
        }
        if ctx.emits("id_token", Self::MODE) {
            obj.field_dyn("id_token", ctx, &self.id_token)?;
        }
        if ctx.emits("metadata", Self::MODE) {
            obj.field_json_dyn("metadata", ctx, self.metadata)?;
        }
        if ctx.emits("user", Self::MODE) {
            obj.field_dyn("user", ctx, &crate::fields::DrfUuid(self.user))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.user.ChangePasswordSerializer` (model: `no model`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ChangePasswordSerializer<'a> {
    /// `old_password`: CharField (source `old_password`).
    pub old_password: &'a str,
    /// `new_password`: CharField (source `new_password`).
    pub new_password: &'a str,
    /// `confirm_password`: CharField (source `confirm_password`).
    pub confirm_password: &'a str,
}

impl<'a> ChangePasswordSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] =
        &["old_password", "new_password", "confirm_password"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ChangePasswordSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("old_password", Self::MODE) {
            obj.field_dyn("old_password", ctx, &self.old_password)?;
        }
        if ctx.emits("new_password", Self::MODE) {
            obj.field_dyn("new_password", ctx, &self.new_password)?;
        }
        if ctx.emits("confirm_password", Self::MODE) {
            obj.field_dyn("confirm_password", ctx, &self.confirm_password)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.user.ProfileSerializer` (model: `db.Profile`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProfileSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `theme`: JSONField (source `theme`).
    pub theme: Option<crate::fields::PgJson<'a>>,
    /// `is_app_rail_docked`: BooleanField (source `is_app_rail_docked`).
    pub is_app_rail_docked: bool,
    /// `is_tour_completed`: BooleanField (source `is_tour_completed`).
    pub is_tour_completed: bool,
    /// `onboarding_step`: JSONField (source `onboarding_step`).
    pub onboarding_step: Option<crate::fields::PgJson<'a>>,
    /// `use_case`: CharField (source `use_case`).
    pub use_case: Option<&'a str>,
    /// `role`: CharField (source `role`).
    pub role: Option<&'a str>,
    /// `is_onboarded`: BooleanField (source `is_onboarded`).
    pub is_onboarded: bool,
    /// `last_workspace_id`: UUIDField (source `last_workspace_id`).
    pub last_workspace_id: Option<uuid::Uuid>,
    /// `billing_address_country`: CharField (source `billing_address_country`).
    pub billing_address_country: &'a str,
    /// `billing_address`: JSONField (source `billing_address`).
    pub billing_address: Option<crate::fields::PgJson<'a>>,
    /// `has_billing_address`: BooleanField (source `has_billing_address`).
    pub has_billing_address: bool,
    /// `company_name`: CharField (source `company_name`).
    pub company_name: &'a str,
    /// `notification_view_mode`: ChoiceField (source `notification_view_mode`).
    pub notification_view_mode: &'a str,
    /// `is_smooth_cursor_enabled`: BooleanField (source `is_smooth_cursor_enabled`).
    pub is_smooth_cursor_enabled: bool,
    /// `is_mobile_onboarded`: BooleanField (source `is_mobile_onboarded`).
    pub is_mobile_onboarded: bool,
    /// `mobile_onboarding_step`: JSONField (source `mobile_onboarding_step`).
    pub mobile_onboarding_step: Option<crate::fields::PgJson<'a>>,
    /// `mobile_timezone_auto_set`: BooleanField (source `mobile_timezone_auto_set`).
    pub mobile_timezone_auto_set: bool,
    /// `language`: CharField (source `language`).
    pub language: &'a str,
    /// `start_of_the_week`: ChoiceField (source `start_of_the_week`).
    pub start_of_the_week: i64,
    /// `goals`: JSONField (source `goals`).
    pub goals: Option<crate::fields::PgJson<'a>>,
    /// `background_color`: CharField (source `background_color`).
    pub background_color: &'a str,
    /// `is_navigation_tour_completed`: BooleanField (source `is_navigation_tour_completed`).
    pub is_navigation_tour_completed: bool,
    /// `has_marketing_email_consent`: BooleanField (source `has_marketing_email_consent`).
    pub has_marketing_email_consent: bool,
    /// `is_subscribed_to_changelog`: BooleanField (source `is_subscribed_to_changelog`).
    pub is_subscribed_to_changelog: bool,
    /// `product_tour`: JSONField (source `product_tour`).
    pub product_tour: Option<crate::fields::PgJson<'a>>,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: uuid::Uuid,
}

impl<'a> ProfileSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "theme",
        "is_app_rail_docked",
        "is_tour_completed",
        "onboarding_step",
        "use_case",
        "role",
        "is_onboarded",
        "last_workspace_id",
        "billing_address_country",
        "billing_address",
        "has_billing_address",
        "company_name",
        "notification_view_mode",
        "is_smooth_cursor_enabled",
        "is_mobile_onboarded",
        "mobile_onboarding_step",
        "mobile_timezone_auto_set",
        "language",
        "start_of_the_week",
        "goals",
        "background_color",
        "is_navigation_tour_completed",
        "has_marketing_email_consent",
        "is_subscribed_to_changelog",
        "product_tour",
        "user",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProfileSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
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
        if ctx.emits("theme", Self::MODE) {
            obj.field_json_dyn("theme", ctx, self.theme)?;
        }
        if ctx.emits("is_app_rail_docked", Self::MODE) {
            obj.field_dyn("is_app_rail_docked", ctx, &self.is_app_rail_docked)?;
        }
        if ctx.emits("is_tour_completed", Self::MODE) {
            obj.field_dyn("is_tour_completed", ctx, &self.is_tour_completed)?;
        }
        if ctx.emits("onboarding_step", Self::MODE) {
            obj.field_json_dyn("onboarding_step", ctx, self.onboarding_step)?;
        }
        if ctx.emits("use_case", Self::MODE) {
            obj.field_dyn("use_case", ctx, &self.use_case)?;
        }
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        if ctx.emits("is_onboarded", Self::MODE) {
            obj.field_dyn("is_onboarded", ctx, &self.is_onboarded)?;
        }
        if ctx.emits("last_workspace_id", Self::MODE) {
            obj.field_dyn(
                "last_workspace_id",
                ctx,
                &self.last_workspace_id.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("billing_address_country", Self::MODE) {
            obj.field_dyn(
                "billing_address_country",
                ctx,
                &self.billing_address_country,
            )?;
        }
        if ctx.emits("billing_address", Self::MODE) {
            obj.field_json_dyn("billing_address", ctx, self.billing_address)?;
        }
        if ctx.emits("has_billing_address", Self::MODE) {
            obj.field_dyn("has_billing_address", ctx, &self.has_billing_address)?;
        }
        if ctx.emits("company_name", Self::MODE) {
            obj.field_dyn("company_name", ctx, &self.company_name)?;
        }
        if ctx.emits("notification_view_mode", Self::MODE) {
            obj.field_dyn("notification_view_mode", ctx, &self.notification_view_mode)?;
        }
        if ctx.emits("is_smooth_cursor_enabled", Self::MODE) {
            obj.field_dyn(
                "is_smooth_cursor_enabled",
                ctx,
                &self.is_smooth_cursor_enabled,
            )?;
        }
        if ctx.emits("is_mobile_onboarded", Self::MODE) {
            obj.field_dyn("is_mobile_onboarded", ctx, &self.is_mobile_onboarded)?;
        }
        if ctx.emits("mobile_onboarding_step", Self::MODE) {
            obj.field_json_dyn("mobile_onboarding_step", ctx, self.mobile_onboarding_step)?;
        }
        if ctx.emits("mobile_timezone_auto_set", Self::MODE) {
            obj.field_dyn(
                "mobile_timezone_auto_set",
                ctx,
                &self.mobile_timezone_auto_set,
            )?;
        }
        if ctx.emits("language", Self::MODE) {
            obj.field_dyn("language", ctx, &self.language)?;
        }
        if ctx.emits("start_of_the_week", Self::MODE) {
            obj.field_dyn("start_of_the_week", ctx, &self.start_of_the_week)?;
        }
        if ctx.emits("goals", Self::MODE) {
            obj.field_json_dyn("goals", ctx, self.goals)?;
        }
        if ctx.emits("background_color", Self::MODE) {
            obj.field_dyn("background_color", ctx, &self.background_color)?;
        }
        if ctx.emits("is_navigation_tour_completed", Self::MODE) {
            obj.field_dyn(
                "is_navigation_tour_completed",
                ctx,
                &self.is_navigation_tour_completed,
            )?;
        }
        if ctx.emits("has_marketing_email_consent", Self::MODE) {
            obj.field_dyn(
                "has_marketing_email_consent",
                ctx,
                &self.has_marketing_email_consent,
            )?;
        }
        if ctx.emits("is_subscribed_to_changelog", Self::MODE) {
            obj.field_dyn(
                "is_subscribed_to_changelog",
                ctx,
                &self.is_subscribed_to_changelog,
            )?;
        }
        if ctx.emits("product_tour", Self::MODE) {
            obj.field_json_dyn("product_tour", ctx, self.product_tour)?;
        }
        if ctx.emits("user", Self::MODE) {
            obj.field_dyn("user", ctx, &crate::fields::DrfUuid(self.user))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.user.ResetPasswordSerializer` (model: `no model`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ResetPasswordSerializer<'a> {
    /// `new_password`: CharField (source `new_password`).
    pub new_password: &'a str,
}

impl<'a> ResetPasswordSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["new_password"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ResetPasswordSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("new_password", Self::MODE) {
            obj.field_dyn("new_password", ctx, &self.new_password)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.user.UserAdminLiteSerializer` (model: `db.User`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct UserAdminLiteSerializer<'a> {
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
    /// `email`: CharField (source `email`).
    pub email: Option<&'a str>,
    /// `last_login_medium`: CharField (source `last_login_medium`).
    pub last_login_medium: &'a str,
}

impl<'a> UserAdminLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "first_name",
        "last_name",
        "avatar",
        "avatar_url",
        "is_bot",
        "display_name",
        "email",
        "last_login_medium",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for UserAdminLiteSerializer<'a> {
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
        if ctx.emits("email", Self::MODE) {
            obj.field_dyn("email", ctx, &self.email)?;
        }
        if ctx.emits("last_login_medium", Self::MODE) {
            obj.field_dyn("last_login_medium", ctx, &self.last_login_medium)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.user.UserLiteSerializer` (model: `db.User`).
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

/// `plane.app.serializers.user.UserMeSerializer` (model: `db.User`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct UserMeSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `avatar`: CharField (source `avatar`).
    pub avatar: &'a str,
    /// `cover_image`: URLField (source `cover_image`).
    pub cover_image: Option<&'a str>,
    /// `avatar_url`: ReadOnlyField (source `avatar_url`).
    pub avatar_url: Option<&'a str>,
    /// `cover_image_url`: ReadOnlyField (source `cover_image_url`).
    pub cover_image_url: Option<&'a str>,
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
    /// `last_login_medium`: CharField (source `last_login_medium`).
    pub last_login_medium: &'a str,
    /// `last_login_time`: DateTimeField (source `last_login_time`).
    pub last_login_time: Option<chrono::DateTime<chrono::Utc>>,
}

impl<'a> UserMeSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "avatar",
        "cover_image",
        "avatar_url",
        "cover_image_url",
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
        "last_login_medium",
        "last_login_time",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for UserMeSerializer<'a> {
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
        if ctx.emits("cover_image", Self::MODE) {
            obj.field_dyn("cover_image", ctx, &self.cover_image)?;
        }
        if ctx.emits("avatar_url", Self::MODE) {
            obj.field_dyn("avatar_url", ctx, &self.avatar_url)?;
        }
        if ctx.emits("cover_image_url", Self::MODE) {
            obj.field_dyn("cover_image_url", ctx, &self.cover_image_url)?;
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
        if ctx.emits("last_login_medium", Self::MODE) {
            obj.field_dyn("last_login_medium", ctx, &self.last_login_medium)?;
        }
        if ctx.emits("last_login_time", Self::MODE) {
            obj.field_dyn(
                "last_login_time",
                ctx,
                &crate::fields::drf_datetime_opt(self.last_login_time, ctx.tz()),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.user.UserMeSettingsSerializer` (model: `db.User`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct UserMeSettingsSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `email`: CharField (source `email`).
    pub email: Option<&'a str>,
    /// `workspace`: SerializerMethodField (source `*`).
    pub workspace: crate::value::DrfValue<'a>,
}

impl<'a> UserMeSettingsSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["id", "email", "workspace"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for UserMeSettingsSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("email", Self::MODE) {
            obj.field_dyn("email", ctx, &self.email)?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_value_dyn("workspace", ctx, &self.workspace)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.user.UserSerializer` (model: `db.User`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct UserSerializer<'a> {
    /// `last_login`: DateTimeField (source `last_login`).
    pub last_login: Option<chrono::DateTime<chrono::Utc>>,
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `username`: CharField (source `username`).
    pub username: &'a str,
    /// `mobile_number`: CharField (source `mobile_number`).
    pub mobile_number: Option<&'a str>,
    /// `email`: CharField (source `email`).
    pub email: Option<&'a str>,
    /// `display_name`: CharField (source `display_name`).
    pub display_name: &'a str,
    /// `first_name`: CharField (source `first_name`).
    pub first_name: &'a str,
    /// `last_name`: CharField (source `last_name`).
    pub last_name: &'a str,
    /// `avatar`: CharField (source `avatar`).
    pub avatar: &'a str,
    /// `avatar_asset`: PrimaryKeyRelatedField (source `avatar_asset`).
    pub avatar_asset: Option<uuid::Uuid>,
    /// `cover_image`: URLField (source `cover_image`).
    pub cover_image: Option<&'a str>,
    /// `cover_image_asset`: PrimaryKeyRelatedField (source `cover_image_asset`).
    pub cover_image_asset: Option<uuid::Uuid>,
    /// `date_joined`: DateTimeField (source `date_joined`).
    pub date_joined: chrono::DateTime<chrono::Utc>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `last_location`: CharField (source `last_location`).
    pub last_location: &'a str,
    /// `created_location`: CharField (source `created_location`).
    pub created_location: &'a str,
    /// `is_superuser`: BooleanField (source `is_superuser`).
    pub is_superuser: bool,
    /// `is_managed`: BooleanField (source `is_managed`).
    pub is_managed: bool,
    /// `is_password_expired`: BooleanField (source `is_password_expired`).
    pub is_password_expired: bool,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `is_staff`: BooleanField (source `is_staff`).
    pub is_staff: bool,
    /// `is_email_verified`: BooleanField (source `is_email_verified`).
    pub is_email_verified: bool,
    /// `is_password_autoset`: BooleanField (source `is_password_autoset`).
    pub is_password_autoset: bool,
    /// `is_password_reset_required`: BooleanField (source `is_password_reset_required`).
    pub is_password_reset_required: bool,
    /// `token`: CharField (source `token`).
    pub token: &'a str,
    /// `last_active`: DateTimeField (source `last_active`).
    pub last_active: Option<chrono::DateTime<chrono::Utc>>,
    /// `last_login_time`: DateTimeField (source `last_login_time`).
    pub last_login_time: Option<chrono::DateTime<chrono::Utc>>,
    /// `last_logout_time`: DateTimeField (source `last_logout_time`).
    pub last_logout_time: Option<chrono::DateTime<chrono::Utc>>,
    /// `last_login_ip`: CharField (source `last_login_ip`).
    pub last_login_ip: &'a str,
    /// `last_logout_ip`: CharField (source `last_logout_ip`).
    pub last_logout_ip: &'a str,
    /// `last_login_medium`: CharField (source `last_login_medium`).
    pub last_login_medium: &'a str,
    /// `last_login_uagent`: CharField (source `last_login_uagent`).
    pub last_login_uagent: &'a str,
    /// `token_updated_at`: DateTimeField (source `token_updated_at`).
    pub token_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `is_bot`: BooleanField (source `is_bot`).
    pub is_bot: bool,
    /// `bot_type`: CharField (source `bot_type`).
    pub bot_type: Option<&'a str>,
    /// `user_timezone`: ChoiceField (source `user_timezone`).
    pub user_timezone: &'a str,
    /// `is_email_valid`: BooleanField (source `is_email_valid`).
    pub is_email_valid: bool,
    /// `masked_at`: DateTimeField (source `masked_at`).
    pub masked_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl<'a> UserSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "last_login",
        "id",
        "username",
        "mobile_number",
        "email",
        "display_name",
        "first_name",
        "last_name",
        "avatar",
        "avatar_asset",
        "cover_image",
        "cover_image_asset",
        "date_joined",
        "created_at",
        "updated_at",
        "last_location",
        "created_location",
        "is_superuser",
        "is_managed",
        "is_password_expired",
        "is_active",
        "is_staff",
        "is_email_verified",
        "is_password_autoset",
        "is_password_reset_required",
        "token",
        "last_active",
        "last_login_time",
        "last_logout_time",
        "last_login_ip",
        "last_logout_ip",
        "last_login_medium",
        "last_login_uagent",
        "token_updated_at",
        "is_bot",
        "bot_type",
        "user_timezone",
        "is_email_valid",
        "masked_at",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for UserSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("last_login", Self::MODE) {
            obj.field_dyn(
                "last_login",
                ctx,
                &crate::fields::drf_datetime_opt(self.last_login, ctx.tz()),
            )?;
        }
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("username", Self::MODE) {
            obj.field_dyn("username", ctx, &self.username)?;
        }
        if ctx.emits("mobile_number", Self::MODE) {
            obj.field_dyn("mobile_number", ctx, &self.mobile_number)?;
        }
        if ctx.emits("email", Self::MODE) {
            obj.field_dyn("email", ctx, &self.email)?;
        }
        if ctx.emits("display_name", Self::MODE) {
            obj.field_dyn("display_name", ctx, &self.display_name)?;
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
        if ctx.emits("avatar_asset", Self::MODE) {
            obj.field_dyn(
                "avatar_asset",
                ctx,
                &self.avatar_asset.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("cover_image", Self::MODE) {
            obj.field_dyn("cover_image", ctx, &self.cover_image)?;
        }
        if ctx.emits("cover_image_asset", Self::MODE) {
            obj.field_dyn(
                "cover_image_asset",
                ctx,
                &self.cover_image_asset.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("date_joined", Self::MODE) {
            obj.field_dyn(
                "date_joined",
                ctx,
                &crate::fields::drf_datetime(self.date_joined, ctx.tz()),
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
        if ctx.emits("last_location", Self::MODE) {
            obj.field_dyn("last_location", ctx, &self.last_location)?;
        }
        if ctx.emits("created_location", Self::MODE) {
            obj.field_dyn("created_location", ctx, &self.created_location)?;
        }
        if ctx.emits("is_superuser", Self::MODE) {
            obj.field_dyn("is_superuser", ctx, &self.is_superuser)?;
        }
        if ctx.emits("is_managed", Self::MODE) {
            obj.field_dyn("is_managed", ctx, &self.is_managed)?;
        }
        if ctx.emits("is_password_expired", Self::MODE) {
            obj.field_dyn("is_password_expired", ctx, &self.is_password_expired)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
        }
        if ctx.emits("is_staff", Self::MODE) {
            obj.field_dyn("is_staff", ctx, &self.is_staff)?;
        }
        if ctx.emits("is_email_verified", Self::MODE) {
            obj.field_dyn("is_email_verified", ctx, &self.is_email_verified)?;
        }
        if ctx.emits("is_password_autoset", Self::MODE) {
            obj.field_dyn("is_password_autoset", ctx, &self.is_password_autoset)?;
        }
        if ctx.emits("is_password_reset_required", Self::MODE) {
            obj.field_dyn(
                "is_password_reset_required",
                ctx,
                &self.is_password_reset_required,
            )?;
        }
        if ctx.emits("token", Self::MODE) {
            obj.field_dyn("token", ctx, &self.token)?;
        }
        if ctx.emits("last_active", Self::MODE) {
            obj.field_dyn(
                "last_active",
                ctx,
                &crate::fields::drf_datetime_opt(self.last_active, ctx.tz()),
            )?;
        }
        if ctx.emits("last_login_time", Self::MODE) {
            obj.field_dyn(
                "last_login_time",
                ctx,
                &crate::fields::drf_datetime_opt(self.last_login_time, ctx.tz()),
            )?;
        }
        if ctx.emits("last_logout_time", Self::MODE) {
            obj.field_dyn(
                "last_logout_time",
                ctx,
                &crate::fields::drf_datetime_opt(self.last_logout_time, ctx.tz()),
            )?;
        }
        if ctx.emits("last_login_ip", Self::MODE) {
            obj.field_dyn("last_login_ip", ctx, &self.last_login_ip)?;
        }
        if ctx.emits("last_logout_ip", Self::MODE) {
            obj.field_dyn("last_logout_ip", ctx, &self.last_logout_ip)?;
        }
        if ctx.emits("last_login_medium", Self::MODE) {
            obj.field_dyn("last_login_medium", ctx, &self.last_login_medium)?;
        }
        if ctx.emits("last_login_uagent", Self::MODE) {
            obj.field_dyn("last_login_uagent", ctx, &self.last_login_uagent)?;
        }
        if ctx.emits("token_updated_at", Self::MODE) {
            obj.field_dyn(
                "token_updated_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.token_updated_at, ctx.tz()),
            )?;
        }
        if ctx.emits("is_bot", Self::MODE) {
            obj.field_dyn("is_bot", ctx, &self.is_bot)?;
        }
        if ctx.emits("bot_type", Self::MODE) {
            obj.field_dyn("bot_type", ctx, &self.bot_type)?;
        }
        if ctx.emits("user_timezone", Self::MODE) {
            obj.field_dyn("user_timezone", ctx, &self.user_timezone)?;
        }
        if ctx.emits("is_email_valid", Self::MODE) {
            obj.field_dyn("is_email_valid", ctx, &self.is_email_valid)?;
        }
        if ctx.emits("masked_at", Self::MODE) {
            obj.field_dyn(
                "masked_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.masked_at, ctx.tz()),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
