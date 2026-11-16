// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.notification` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.notification.NotificationSerializer` (model: `db.Notification`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct NotificationSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `triggered_by_details`: UserLiteSerializer (source `triggered_by`).
    pub triggered_by_details: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `is_inbox_issue`: BooleanField (source `is_inbox_issue`).
    pub is_inbox_issue: Option<bool>,
    /// `is_intake_issue`: BooleanField (source `is_intake_issue`).
    pub is_intake_issue: Option<bool>,
    /// `is_mentioned_notification`: BooleanField (source `is_mentioned_notification`).
    pub is_mentioned_notification: Option<bool>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `data`: JSONField (source `data`).
    pub data: Option<crate::fields::PgJson<'a>>,
    /// `entity_identifier`: UUIDField (source `entity_identifier`).
    pub entity_identifier: Option<uuid::Uuid>,
    /// `entity_name`: CharField (source `entity_name`).
    pub entity_name: &'a str,
    /// `title`: CharField (source `title`).
    pub title: &'a str,
    /// `message`: JSONField (source `message`).
    pub message: Option<crate::fields::PgJson<'a>>,
    /// `message_html`: CharField (source `message_html`).
    pub message_html: &'a str,
    /// `message_stripped`: CharField (source `message_stripped`).
    pub message_stripped: Option<&'a str>,
    /// `sender`: CharField (source `sender`).
    pub sender: &'a str,
    /// `read_at`: DateTimeField (source `read_at`).
    pub read_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `snoozed_till`: DateTimeField (source `snoozed_till`).
    pub snoozed_till: Option<chrono::DateTime<chrono::Utc>>,
    /// `archived_at`: DateTimeField (source `archived_at`).
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: Option<uuid::Uuid>,
    /// `triggered_by`: PrimaryKeyRelatedField (source `triggered_by`).
    pub triggered_by: Option<uuid::Uuid>,
    /// `receiver`: PrimaryKeyRelatedField (source `receiver`).
    pub receiver: uuid::Uuid,
}

impl<'a> NotificationSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "triggered_by_details",
        "is_inbox_issue",
        "is_intake_issue",
        "is_mentioned_notification",
        "created_at",
        "updated_at",
        "deleted_at",
        "data",
        "entity_identifier",
        "entity_name",
        "title",
        "message",
        "message_html",
        "message_stripped",
        "sender",
        "read_at",
        "snoozed_till",
        "archived_at",
        "created_by",
        "updated_by",
        "workspace",
        "project",
        "triggered_by",
        "receiver",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for NotificationSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("triggered_by_details", Self::MODE) {
            obj.field_nested_dyn(
                "triggered_by_details",
                ctx,
                self.triggered_by_details
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("is_inbox_issue", Self::MODE)
            && let Some(value) = &self.is_inbox_issue
        {
            obj.field_dyn("is_inbox_issue", ctx, &value)?;
        }
        if ctx.emits("is_intake_issue", Self::MODE)
            && let Some(value) = &self.is_intake_issue
        {
            obj.field_dyn("is_intake_issue", ctx, &value)?;
        }
        if ctx.emits("is_mentioned_notification", Self::MODE)
            && let Some(value) = &self.is_mentioned_notification
        {
            obj.field_dyn("is_mentioned_notification", ctx, &value)?;
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
        if ctx.emits("data", Self::MODE) {
            obj.field_json_dyn("data", ctx, self.data)?;
        }
        if ctx.emits("entity_identifier", Self::MODE) {
            obj.field_dyn(
                "entity_identifier",
                ctx,
                &self.entity_identifier.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("entity_name", Self::MODE) {
            obj.field_dyn("entity_name", ctx, &self.entity_name)?;
        }
        if ctx.emits("title", Self::MODE) {
            obj.field_dyn("title", ctx, &self.title)?;
        }
        if ctx.emits("message", Self::MODE) {
            obj.field_json_dyn("message", ctx, self.message)?;
        }
        if ctx.emits("message_html", Self::MODE) {
            obj.field_dyn("message_html", ctx, &self.message_html)?;
        }
        if ctx.emits("message_stripped", Self::MODE) {
            obj.field_dyn("message_stripped", ctx, &self.message_stripped)?;
        }
        if ctx.emits("sender", Self::MODE) {
            obj.field_dyn("sender", ctx, &self.sender)?;
        }
        if ctx.emits("read_at", Self::MODE) {
            obj.field_dyn(
                "read_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.read_at, ctx.tz()),
            )?;
        }
        if ctx.emits("snoozed_till", Self::MODE) {
            obj.field_dyn(
                "snoozed_till",
                ctx,
                &crate::fields::drf_datetime_opt(self.snoozed_till, ctx.tz()),
            )?;
        }
        if ctx.emits("archived_at", Self::MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.archived_at, ctx.tz()),
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
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn("workspace", ctx, &crate::fields::DrfUuid(self.workspace))?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &self.project.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("triggered_by", Self::MODE) {
            obj.field_dyn(
                "triggered_by",
                ctx,
                &self.triggered_by.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("receiver", Self::MODE) {
            obj.field_dyn("receiver", ctx, &crate::fields::DrfUuid(self.receiver))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.notification.UserNotificationPreferenceSerializer` (model: `db.UserNotificationPreference`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct UserNotificationPreferenceSerializer {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `property_change`: BooleanField (source `property_change`).
    pub property_change: bool,
    /// `state_change`: BooleanField (source `state_change`).
    pub state_change: bool,
    /// `comment`: BooleanField (source `comment`).
    pub comment: bool,
    /// `mention`: BooleanField (source `mention`).
    pub mention: bool,
    /// `issue_completed`: BooleanField (source `issue_completed`).
    pub issue_completed: bool,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: uuid::Uuid,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: Option<uuid::Uuid>,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: Option<uuid::Uuid>,
}

impl UserNotificationPreferenceSerializer {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "property_change",
        "state_change",
        "comment",
        "mention",
        "issue_completed",
        "created_by",
        "updated_by",
        "user",
        "workspace",
        "project",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl crate::ser::DrfSerialize for UserNotificationPreferenceSerializer {
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
        if ctx.emits("deleted_at", Self::MODE) {
            obj.field_dyn(
                "deleted_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.deleted_at, ctx.tz()),
            )?;
        }
        if ctx.emits("property_change", Self::MODE) {
            obj.field_dyn("property_change", ctx, &self.property_change)?;
        }
        if ctx.emits("state_change", Self::MODE) {
            obj.field_dyn("state_change", ctx, &self.state_change)?;
        }
        if ctx.emits("comment", Self::MODE) {
            obj.field_dyn("comment", ctx, &self.comment)?;
        }
        if ctx.emits("mention", Self::MODE) {
            obj.field_dyn("mention", ctx, &self.mention)?;
        }
        if ctx.emits("issue_completed", Self::MODE) {
            obj.field_dyn("issue_completed", ctx, &self.issue_completed)?;
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
            obj.field_dyn("user", ctx, &crate::fields::DrfUuid(self.user))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_dyn(
                "workspace",
                ctx,
                &self.workspace.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("project", Self::MODE) {
            obj.field_dyn("project", ctx, &self.project.map(crate::fields::DrfUuid))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
