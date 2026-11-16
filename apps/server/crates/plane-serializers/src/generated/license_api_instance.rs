// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.license.api.serializers.instance` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.license.api.serializers.instance.InstanceSerializer` (model: `license.Instance`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct InstanceSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `primary_owner_details`: UserAdminLiteSerializer (source `primary_owner`).
    pub primary_owner_details: Option<&'a crate::generated::app_user::UserAdminLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `instance_name`: CharField (source `instance_name`).
    pub instance_name: &'a str,
    /// `whitelist_emails`: CharField (source `whitelist_emails`).
    pub whitelist_emails: Option<&'a str>,
    /// `instance_id`: CharField (source `instance_id`).
    pub instance_id: &'a str,
    /// `current_version`: CharField (source `current_version`).
    pub current_version: &'a str,
    /// `latest_version`: CharField (source `latest_version`).
    pub latest_version: Option<&'a str>,
    /// `edition`: CharField (source `edition`).
    pub edition: &'a str,
    /// `domain`: CharField (source `domain`).
    pub domain: &'a str,
    /// `last_checked_at`: DateTimeField (source `last_checked_at`).
    pub last_checked_at: chrono::DateTime<chrono::Utc>,
    /// `namespace`: CharField (source `namespace`).
    pub namespace: Option<&'a str>,
    /// `is_telemetry_enabled`: BooleanField (source `is_telemetry_enabled`).
    pub is_telemetry_enabled: bool,
    /// `is_support_required`: BooleanField (source `is_support_required`).
    pub is_support_required: bool,
    /// `is_setup_done`: BooleanField (source `is_setup_done`).
    pub is_setup_done: bool,
    /// `is_signup_screen_visited`: BooleanField (source `is_signup_screen_visited`).
    pub is_signup_screen_visited: bool,
    /// `is_verified`: BooleanField (source `is_verified`).
    pub is_verified: bool,
    /// `is_test`: BooleanField (source `is_test`).
    pub is_test: bool,
    /// `is_current_version_deprecated`: BooleanField (source `is_current_version_deprecated`).
    pub is_current_version_deprecated: bool,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> InstanceSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "primary_owner_details",
        "created_at",
        "updated_at",
        "deleted_at",
        "instance_name",
        "whitelist_emails",
        "instance_id",
        "current_version",
        "latest_version",
        "edition",
        "domain",
        "last_checked_at",
        "namespace",
        "is_telemetry_enabled",
        "is_support_required",
        "is_setup_done",
        "is_signup_screen_visited",
        "is_verified",
        "is_test",
        "is_current_version_deprecated",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for InstanceSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("primary_owner_details", Self::MODE) {
            obj.field_nested_dyn(
                "primary_owner_details",
                ctx,
                self.primary_owner_details
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
        if ctx.emits("instance_name", Self::MODE) {
            obj.field_dyn("instance_name", ctx, &self.instance_name)?;
        }
        if ctx.emits("whitelist_emails", Self::MODE) {
            obj.field_dyn("whitelist_emails", ctx, &self.whitelist_emails)?;
        }
        if ctx.emits("instance_id", Self::MODE) {
            obj.field_dyn("instance_id", ctx, &self.instance_id)?;
        }
        if ctx.emits("current_version", Self::MODE) {
            obj.field_dyn("current_version", ctx, &self.current_version)?;
        }
        if ctx.emits("latest_version", Self::MODE) {
            obj.field_dyn("latest_version", ctx, &self.latest_version)?;
        }
        if ctx.emits("edition", Self::MODE) {
            obj.field_dyn("edition", ctx, &self.edition)?;
        }
        if ctx.emits("domain", Self::MODE) {
            obj.field_dyn("domain", ctx, &self.domain)?;
        }
        if ctx.emits("last_checked_at", Self::MODE) {
            obj.field_dyn(
                "last_checked_at",
                ctx,
                &crate::fields::drf_datetime(self.last_checked_at, ctx.tz()),
            )?;
        }
        if ctx.emits("namespace", Self::MODE) {
            obj.field_dyn("namespace", ctx, &self.namespace)?;
        }
        if ctx.emits("is_telemetry_enabled", Self::MODE) {
            obj.field_dyn("is_telemetry_enabled", ctx, &self.is_telemetry_enabled)?;
        }
        if ctx.emits("is_support_required", Self::MODE) {
            obj.field_dyn("is_support_required", ctx, &self.is_support_required)?;
        }
        if ctx.emits("is_setup_done", Self::MODE) {
            obj.field_dyn("is_setup_done", ctx, &self.is_setup_done)?;
        }
        if ctx.emits("is_signup_screen_visited", Self::MODE) {
            obj.field_dyn(
                "is_signup_screen_visited",
                ctx,
                &self.is_signup_screen_visited,
            )?;
        }
        if ctx.emits("is_verified", Self::MODE) {
            obj.field_dyn("is_verified", ctx, &self.is_verified)?;
        }
        if ctx.emits("is_test", Self::MODE) {
            obj.field_dyn("is_test", ctx, &self.is_test)?;
        }
        if ctx.emits("is_current_version_deprecated", Self::MODE) {
            obj.field_dyn(
                "is_current_version_deprecated",
                ctx,
                &self.is_current_version_deprecated,
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
