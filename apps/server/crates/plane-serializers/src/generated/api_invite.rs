// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.api.serializers.invite` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.api.serializers.invite.WorkspaceInviteSerializer` (model: `db.WorkspaceMemberInvite`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceInviteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `email`: CharField (source `email`).
    pub email: &'a str,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `responded_at`: DateTimeField (source `responded_at`).
    pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `accepted`: BooleanField (source `accepted`).
    pub accepted: bool,
}

impl<'a> WorkspaceInviteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "email",
        "role",
        "created_at",
        "updated_at",
        "responded_at",
        "accepted",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::RemoveFields;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceInviteSerializer<'a> {
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
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
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
        if ctx.emits("responded_at", Self::MODE) {
            obj.field_dyn(
                "responded_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.responded_at, ctx.tz()),
            )?;
        }
        if ctx.emits("accepted", Self::MODE) {
            obj.field_dyn("accepted", ctx, &self.accepted)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
