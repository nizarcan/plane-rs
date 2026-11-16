// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `plane.app.serializers.workspace` by `tools/inventory/serializers.py --rust`.
//!
//! Do not edit. Re-run the generator after changing `apps/api`.

/// `plane.app.serializers.workspace.IssueRecentVisitSerializer` (model: `db.Issue`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct IssueRecentVisitSerializer<'a> {
    /// `id`: UUIDField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `state`: PrimaryKeyRelatedField (source `state`).
    pub state: Option<uuid::Uuid>,
    /// `priority`: ChoiceField (source `priority`).
    pub priority: &'a str,
    /// `assignees`: SerializerMethodField (source `*`).
    pub assignees: crate::value::DrfValue<'a>,
    /// `type`: PrimaryKeyRelatedField (source `type`).
    pub r#type: Option<uuid::Uuid>,
    /// `sequence_id`: IntegerField (source `sequence_id`).
    pub sequence_id: i64,
    /// `project_id`: ReadOnlyField (source `project_id`).
    pub project_id: uuid::Uuid,
    /// `project_identifier`: SerializerMethodField (source `*`).
    pub project_identifier: crate::value::DrfValue<'a>,
}

impl<'a> IssueRecentVisitSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "state",
        "priority",
        "assignees",
        "type",
        "sequence_id",
        "project_id",
        "project_identifier",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for IssueRecentVisitSerializer<'a> {
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
        if ctx.emits("state", Self::MODE) {
            obj.field_dyn("state", ctx, &self.state.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("priority", Self::MODE) {
            obj.field_dyn("priority", ctx, &self.priority)?;
        }
        if ctx.emits("assignees", Self::MODE) {
            obj.field_value_dyn("assignees", ctx, &self.assignees)?;
        }
        if ctx.emits("type", Self::MODE) {
            obj.field_dyn("type", ctx, &self.r#type.map(crate::fields::DrfUuid))?;
        }
        if ctx.emits("sequence_id", Self::MODE) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_dyn("project_id", ctx, &crate::fields::DrfUuid(self.project_id))?;
        }
        if ctx.emits("project_identifier", Self::MODE) {
            obj.field_value_dyn("project_identifier", ctx, &self.project_identifier)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.PageRecentVisitSerializer` (model: `db.Page`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct PageRecentVisitSerializer<'a> {
    /// `id`: UUIDField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `project_id`: SerializerMethodField (source `*`).
    pub project_id: crate::value::DrfValue<'a>,
    /// `owned_by`: PrimaryKeyRelatedField (source `owned_by`).
    pub owned_by: uuid::Uuid,
    /// `project_identifier`: SerializerMethodField (source `*`).
    pub project_identifier: crate::value::DrfValue<'a>,
}

impl<'a> PageRecentVisitSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "name",
        "logo_props",
        "project_id",
        "owned_by",
        "project_identifier",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for PageRecentVisitSerializer<'a> {
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
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("project_id", Self::MODE) {
            obj.field_value_dyn("project_id", ctx, &self.project_id)?;
        }
        if ctx.emits("owned_by", Self::MODE) {
            obj.field_dyn("owned_by", ctx, &crate::fields::DrfUuid(self.owned_by))?;
        }
        if ctx.emits("project_identifier", Self::MODE) {
            obj.field_value_dyn("project_identifier", ctx, &self.project_identifier)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.ProjectRecentVisitSerializer` (model: `db.Project`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct ProjectRecentVisitSerializer<'a> {
    /// `id`: UUIDField (source `id`).
    pub id: uuid::Uuid,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `project_members`: SerializerMethodField (source `*`).
    pub project_members: crate::value::DrfValue<'a>,
    /// `identifier`: CharField (source `identifier`).
    pub identifier: &'a str,
}

impl<'a> ProjectRecentVisitSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] =
        &["id", "name", "logo_props", "project_members", "identifier"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for ProjectRecentVisitSerializer<'a> {
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
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("project_members", Self::MODE) {
            obj.field_value_dyn("project_members", ctx, &self.project_members)?;
        }
        if ctx.emits("identifier", Self::MODE) {
            obj.field_dyn("identifier", ctx, &self.identifier)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.StickySerializer` (model: `db.Sticky`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct StickySerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `name`: CharField (source `name`).
    pub name: Option<&'a str>,
    /// `description`: JSONField (source `description`).
    pub description: Option<crate::fields::PgJson<'a>>,
    /// `description_html`: CharField (source `description_html`).
    pub description_html: &'a str,
    /// `description_stripped`: CharField (source `description_stripped`).
    pub description_stripped: Option<&'a str>,
    /// `description_binary`: ModelField (source `description_binary`).
    pub description_binary: Option<&'a [u8]>,
    /// `logo_props`: JSONField (source `logo_props`).
    pub logo_props: Option<crate::fields::PgJson<'a>>,
    /// `color`: CharField (source `color`).
    pub color: Option<&'a str>,
    /// `background_color`: CharField (source `background_color`).
    pub background_color: Option<&'a str>,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `owner`: PrimaryKeyRelatedField (source `owner`).
    pub owner: uuid::Uuid,
}

impl<'a> StickySerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "description",
        "description_html",
        "description_stripped",
        "description_binary",
        "logo_props",
        "color",
        "background_color",
        "sort_order",
        "created_by",
        "updated_by",
        "workspace",
        "owner",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for StickySerializer<'a> {
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
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("description", Self::MODE) {
            obj.field_json_dyn("description", ctx, self.description)?;
        }
        if ctx.emits("description_html", Self::MODE) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        if ctx.emits("description_stripped", Self::MODE) {
            obj.field_dyn("description_stripped", ctx, &self.description_stripped)?;
        }
        if ctx.emits("description_binary", Self::MODE) {
            obj.field_dyn(
                "description_binary",
                ctx,
                &self.description_binary.map(crate::fields::PyBase64),
            )?;
        }
        if ctx.emits("logo_props", Self::MODE) {
            obj.field_json_dyn("logo_props", ctx, self.logo_props)?;
        }
        if ctx.emits("color", Self::MODE) {
            obj.field_dyn("color", ctx, &self.color)?;
        }
        if ctx.emits("background_color", Self::MODE) {
            obj.field_dyn("background_color", ctx, &self.background_color)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
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
        if ctx.emits("owner", Self::MODE) {
            obj.field_dyn("owner", ctx, &crate::fields::DrfUuid(self.owner))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkSpaceMemberInvitePublicSerializer` (model: `db.WorkspaceMemberInvite`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkSpaceMemberInvitePublicSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `email`: CharField (source `email`).
    pub email: &'a str,
    /// `workspace`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `message`: CharField (source `message`).
    pub message: Option<&'a str>,
    /// `accepted`: BooleanField (source `accepted`).
    pub accepted: bool,
    /// `responded_at`: DateTimeField (source `responded_at`).
    pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
}

impl<'a> WorkSpaceMemberInvitePublicSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "email",
        "workspace",
        "role",
        "message",
        "accepted",
        "responded_at",
        "created_at",
        "updated_at",
        "created_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkSpaceMemberInvitePublicSerializer<'a> {
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
            obj.field_nested_dyn(
                "workspace",
                ctx,
                self.workspace
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
        }
        if ctx.emits("message", Self::MODE) {
            obj.field_dyn("message", ctx, &self.message)?;
        }
        if ctx.emits("accepted", Self::MODE) {
            obj.field_dyn("accepted", ctx, &self.accepted)?;
        }
        if ctx.emits("responded_at", Self::MODE) {
            obj.field_dyn(
                "responded_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.responded_at, ctx.tz()),
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
        if ctx.emits("created_by", Self::MODE) {
            obj.field_dyn(
                "created_by",
                ctx,
                &self.created_by.map(crate::fields::DrfUuid),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkSpaceMemberInviteSerializer` (model: `db.WorkspaceMemberInvite`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkSpaceMemberInviteSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `workspace`: WorkspaceLiteSerializer (source `workspace`).
    pub workspace: Option<&'a crate::generated::app_workspace::WorkspaceLiteSerializer<'a>>,
    /// `invite_link`: SerializerMethodField (source `*`).
    pub invite_link: crate::value::DrfValue<'a>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `email`: CharField (source `email`).
    pub email: &'a str,
    /// `accepted`: BooleanField (source `accepted`).
    pub accepted: bool,
    /// `token`: CharField (source `token`).
    pub token: &'a str,
    /// `message`: CharField (source `message`).
    pub message: Option<&'a str>,
    /// `responded_at`: DateTimeField (source `responded_at`).
    pub responded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
}

impl<'a> WorkSpaceMemberInviteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "workspace",
        "invite_link",
        "created_at",
        "updated_at",
        "deleted_at",
        "email",
        "accepted",
        "token",
        "message",
        "responded_at",
        "role",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkSpaceMemberInviteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("workspace", Self::MODE) {
            obj.field_nested_dyn(
                "workspace",
                ctx,
                self.workspace
                    .map(|value| value as &(dyn crate::ser::DrfSerialize + '_)),
            )?;
        }
        if ctx.emits("invite_link", Self::MODE) {
            obj.field_value_dyn("invite_link", ctx, &self.invite_link)?;
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
        if ctx.emits("email", Self::MODE) {
            obj.field_dyn("email", ctx, &self.email)?;
        }
        if ctx.emits("accepted", Self::MODE) {
            obj.field_dyn("accepted", ctx, &self.accepted)?;
        }
        if ctx.emits("token", Self::MODE) {
            obj.field_dyn("token", ctx, &self.token)?;
        }
        if ctx.emits("message", Self::MODE) {
            obj.field_dyn("message", ctx, &self.message)?;
        }
        if ctx.emits("responded_at", Self::MODE) {
            obj.field_dyn(
                "responded_at",
                ctx,
                &crate::fields::drf_datetime_opt(self.responded_at, ctx.tz()),
            )?;
        }
        if ctx.emits("role", Self::MODE) {
            obj.field_dyn("role", ctx, &self.role)?;
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

/// `plane.app.serializers.workspace.WorkSpaceMemberSerializer` (model: `db.WorkspaceMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkSpaceMemberSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `member`: UserLiteSerializer (source `member`).
    pub member: Option<&'a crate::generated::app_user::UserLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `company_role`: CharField (source `company_role`).
    pub company_role: Option<&'a str>,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `default_props`: JSONField (source `default_props`).
    pub default_props: Option<crate::fields::PgJson<'a>>,
    /// `issue_props`: JSONField (source `issue_props`).
    pub issue_props: Option<crate::fields::PgJson<'a>>,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `getting_started_checklist`: JSONField (source `getting_started_checklist`).
    pub getting_started_checklist: Option<crate::fields::PgJson<'a>>,
    /// `tips`: JSONField (source `tips`).
    pub tips: Option<crate::fields::PgJson<'a>>,
    /// `explored_features`: JSONField (source `explored_features`).
    pub explored_features: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
}

impl<'a> WorkSpaceMemberSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "member",
        "created_at",
        "updated_at",
        "deleted_at",
        "role",
        "company_role",
        "view_props",
        "default_props",
        "issue_props",
        "is_active",
        "getting_started_checklist",
        "tips",
        "explored_features",
        "created_by",
        "updated_by",
        "workspace",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for WorkSpaceMemberSerializer<'a> {
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
            obj.field_nested_dyn(
                "member",
                ctx,
                self.member
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
        if ctx.emits("company_role", Self::MODE) {
            obj.field_dyn("company_role", ctx, &self.company_role)?;
        }
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
        }
        if ctx.emits("default_props", Self::MODE) {
            obj.field_json_dyn("default_props", ctx, self.default_props)?;
        }
        if ctx.emits("issue_props", Self::MODE) {
            obj.field_json_dyn("issue_props", ctx, self.issue_props)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
        }
        if ctx.emits("getting_started_checklist", Self::MODE) {
            obj.field_json_dyn(
                "getting_started_checklist",
                ctx,
                self.getting_started_checklist,
            )?;
        }
        if ctx.emits("tips", Self::MODE) {
            obj.field_json_dyn("tips", ctx, self.tips)?;
        }
        if ctx.emits("explored_features", Self::MODE) {
            obj.field_json_dyn("explored_features", ctx, self.explored_features)?;
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
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkSpaceSerializer` (model: `db.Workspace`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkSpaceSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `total_members`: IntegerField (source `total_members`).
    pub total_members: Option<i64>,
    /// `logo_url`: CharField (source `logo_url`).
    pub logo_url: Option<&'a str>,
    /// `role`: IntegerField (source `role`).
    pub role: Option<i64>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `logo`: CharField (source `logo`).
    pub logo: Option<&'a str>,
    /// `slug`: SlugField (source `slug`).
    pub slug: &'a str,
    /// `organization_size`: CharField (source `organization_size`).
    pub organization_size: Option<&'a str>,
    /// `timezone`: ChoiceField (source `timezone`).
    pub timezone: &'a str,
    /// `background_color`: CharField (source `background_color`).
    pub background_color: &'a str,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `logo_asset`: PrimaryKeyRelatedField (source `logo_asset`).
    pub logo_asset: Option<uuid::Uuid>,
    /// `owner`: PrimaryKeyRelatedField (source `owner`).
    pub owner: uuid::Uuid,
}

impl<'a> WorkSpaceSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "total_members",
        "logo_url",
        "role",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "logo",
        "slug",
        "organization_size",
        "timezone",
        "background_color",
        "created_by",
        "updated_by",
        "logo_asset",
        "owner",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for WorkSpaceSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("total_members", Self::MODE)
            && let Some(value) = &self.total_members
        {
            obj.field_dyn("total_members", ctx, &value)?;
        }
        if ctx.emits("logo_url", Self::MODE) {
            obj.field_dyn("logo_url", ctx, &self.logo_url)?;
        }
        if ctx.emits("role", Self::MODE)
            && let Some(value) = &self.role
        {
            obj.field_dyn("role", ctx, &value)?;
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
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("logo", Self::MODE) {
            obj.field_dyn("logo", ctx, &self.logo)?;
        }
        if ctx.emits("slug", Self::MODE) {
            obj.field_dyn("slug", ctx, &self.slug)?;
        }
        if ctx.emits("organization_size", Self::MODE) {
            obj.field_dyn("organization_size", ctx, &self.organization_size)?;
        }
        if ctx.emits("timezone", Self::MODE) {
            obj.field_dyn("timezone", ctx, &self.timezone)?;
        }
        if ctx.emits("background_color", Self::MODE) {
            obj.field_dyn("background_color", ctx, &self.background_color)?;
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
        if ctx.emits("logo_asset", Self::MODE) {
            obj.field_dyn(
                "logo_asset",
                ctx,
                &self.logo_asset.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("owner", Self::MODE) {
            obj.field_dyn("owner", ctx, &crate::fields::DrfUuid(self.owner))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkspaceHomePreferenceSerializer` (model: `db.WorkspaceHomePreference`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceHomePreferenceSerializer<'a> {
    /// `key`: CharField (source `key`).
    pub key: &'a str,
    /// `is_enabled`: BooleanField (source `is_enabled`).
    pub is_enabled: bool,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
}

impl<'a> WorkspaceHomePreferenceSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["key", "is_enabled", "sort_order"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceHomePreferenceSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("key", Self::MODE) {
            obj.field_dyn("key", ctx, &self.key)?;
        }
        if ctx.emits("is_enabled", Self::MODE) {
            obj.field_dyn("is_enabled", ctx, &self.is_enabled)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkspaceLiteSerializer` (model: `db.Workspace`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceLiteSerializer<'a> {
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `slug`: SlugField (source `slug`).
    pub slug: &'a str,
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `logo_url`: ReadOnlyField (source `logo_url`).
    pub logo_url: Option<&'a str>,
}

impl<'a> WorkspaceLiteSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["name", "slug", "id", "logo_url"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceLiteSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("slug", Self::MODE) {
            obj.field_dyn("slug", ctx, &self.slug)?;
        }
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("logo_url", Self::MODE) {
            obj.field_dyn("logo_url", ctx, &self.logo_url)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkspaceMemberAdminSerializer` (model: `db.WorkspaceMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceMemberAdminSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `member`: UserAdminLiteSerializer (source `member`).
    pub member: Option<&'a crate::generated::app_user::UserAdminLiteSerializer<'a>>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `company_role`: CharField (source `company_role`).
    pub company_role: Option<&'a str>,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `default_props`: JSONField (source `default_props`).
    pub default_props: Option<crate::fields::PgJson<'a>>,
    /// `issue_props`: JSONField (source `issue_props`).
    pub issue_props: Option<crate::fields::PgJson<'a>>,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `getting_started_checklist`: JSONField (source `getting_started_checklist`).
    pub getting_started_checklist: Option<crate::fields::PgJson<'a>>,
    /// `tips`: JSONField (source `tips`).
    pub tips: Option<crate::fields::PgJson<'a>>,
    /// `explored_features`: JSONField (source `explored_features`).
    pub explored_features: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
}

impl<'a> WorkspaceMemberAdminSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "member",
        "created_at",
        "updated_at",
        "deleted_at",
        "role",
        "company_role",
        "view_props",
        "default_props",
        "issue_props",
        "is_active",
        "getting_started_checklist",
        "tips",
        "explored_features",
        "created_by",
        "updated_by",
        "workspace",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::OutputOnly;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceMemberAdminSerializer<'a> {
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
            obj.field_nested_dyn(
                "member",
                ctx,
                self.member
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
        if ctx.emits("company_role", Self::MODE) {
            obj.field_dyn("company_role", ctx, &self.company_role)?;
        }
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
        }
        if ctx.emits("default_props", Self::MODE) {
            obj.field_json_dyn("default_props", ctx, self.default_props)?;
        }
        if ctx.emits("issue_props", Self::MODE) {
            obj.field_json_dyn("issue_props", ctx, self.issue_props)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
        }
        if ctx.emits("getting_started_checklist", Self::MODE) {
            obj.field_json_dyn(
                "getting_started_checklist",
                ctx,
                self.getting_started_checklist,
            )?;
        }
        if ctx.emits("tips", Self::MODE) {
            obj.field_json_dyn("tips", ctx, self.tips)?;
        }
        if ctx.emits("explored_features", Self::MODE) {
            obj.field_json_dyn("explored_features", ctx, self.explored_features)?;
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
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkspaceMemberMeSerializer` (model: `db.WorkspaceMember`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceMemberMeSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `draft_issue_count`: IntegerField (source `draft_issue_count`).
    pub draft_issue_count: Option<i64>,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `role`: ChoiceField (source `role`).
    pub role: i64,
    /// `company_role`: CharField (source `company_role`).
    pub company_role: Option<&'a str>,
    /// `view_props`: JSONField (source `view_props`).
    pub view_props: Option<crate::fields::PgJson<'a>>,
    /// `default_props`: JSONField (source `default_props`).
    pub default_props: Option<crate::fields::PgJson<'a>>,
    /// `issue_props`: JSONField (source `issue_props`).
    pub issue_props: Option<crate::fields::PgJson<'a>>,
    /// `is_active`: BooleanField (source `is_active`).
    pub is_active: bool,
    /// `getting_started_checklist`: JSONField (source `getting_started_checklist`).
    pub getting_started_checklist: Option<crate::fields::PgJson<'a>>,
    /// `tips`: JSONField (source `tips`).
    pub tips: Option<crate::fields::PgJson<'a>>,
    /// `explored_features`: JSONField (source `explored_features`).
    pub explored_features: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `member`: PrimaryKeyRelatedField (source `member`).
    pub member: uuid::Uuid,
}

impl<'a> WorkspaceMemberMeSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "draft_issue_count",
        "created_at",
        "updated_at",
        "deleted_at",
        "role",
        "company_role",
        "view_props",
        "default_props",
        "issue_props",
        "is_active",
        "getting_started_checklist",
        "tips",
        "explored_features",
        "created_by",
        "updated_by",
        "workspace",
        "member",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceMemberMeSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("draft_issue_count", Self::MODE)
            && let Some(value) = &self.draft_issue_count
        {
            obj.field_dyn("draft_issue_count", ctx, &value)?;
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
        if ctx.emits("company_role", Self::MODE) {
            obj.field_dyn("company_role", ctx, &self.company_role)?;
        }
        if ctx.emits("view_props", Self::MODE) {
            obj.field_json_dyn("view_props", ctx, self.view_props)?;
        }
        if ctx.emits("default_props", Self::MODE) {
            obj.field_json_dyn("default_props", ctx, self.default_props)?;
        }
        if ctx.emits("issue_props", Self::MODE) {
            obj.field_json_dyn("issue_props", ctx, self.issue_props)?;
        }
        if ctx.emits("is_active", Self::MODE) {
            obj.field_dyn("is_active", ctx, &self.is_active)?;
        }
        if ctx.emits("getting_started_checklist", Self::MODE) {
            obj.field_json_dyn(
                "getting_started_checklist",
                ctx,
                self.getting_started_checklist,
            )?;
        }
        if ctx.emits("tips", Self::MODE) {
            obj.field_json_dyn("tips", ctx, self.tips)?;
        }
        if ctx.emits("explored_features", Self::MODE) {
            obj.field_json_dyn("explored_features", ctx, self.explored_features)?;
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
        if ctx.emits("member", Self::MODE) {
            obj.field_dyn("member", ctx, &crate::fields::DrfUuid(self.member))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkspaceRecentVisitSerializer` (model: `db.UserRecentVisit`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceRecentVisitSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `entity_name`: CharField (source `entity_name`).
    pub entity_name: &'a str,
    /// `entity_identifier`: UUIDField (source `entity_identifier`).
    pub entity_identifier: Option<uuid::Uuid>,
    /// `entity_data`: SerializerMethodField (source `*`).
    pub entity_data: crate::value::DrfValue<'a>,
    /// `visited_at`: DateTimeField (source `visited_at`).
    pub visited_at: chrono::DateTime<chrono::Utc>,
}

impl<'a> WorkspaceRecentVisitSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "entity_name",
        "entity_identifier",
        "entity_data",
        "visited_at",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceRecentVisitSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", Self::MODE) {
            obj.field_dyn("id", ctx, &crate::fields::DrfUuid(self.id))?;
        }
        if ctx.emits("entity_name", Self::MODE) {
            obj.field_dyn("entity_name", ctx, &self.entity_name)?;
        }
        if ctx.emits("entity_identifier", Self::MODE) {
            obj.field_dyn(
                "entity_identifier",
                ctx,
                &self.entity_identifier.map(crate::fields::DrfUuid),
            )?;
        }
        if ctx.emits("entity_data", Self::MODE) {
            obj.field_value_dyn("entity_data", ctx, &self.entity_data)?;
        }
        if ctx.emits("visited_at", Self::MODE) {
            obj.field_dyn(
                "visited_at",
                ctx,
                &crate::fields::drf_datetime(self.visited_at, ctx.tz()),
            )?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkspaceThemeSerializer` (model: `db.WorkspaceTheme`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceThemeSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `name`: CharField (source `name`).
    pub name: &'a str,
    /// `colors`: JSONField (source `colors`).
    pub colors: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `actor`: PrimaryKeyRelatedField (source `actor`).
    pub actor: uuid::Uuid,
}

impl<'a> WorkspaceThemeSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "colors",
        "created_by",
        "updated_by",
        "workspace",
        "actor",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceThemeSerializer<'a> {
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
        if ctx.emits("name", Self::MODE) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("colors", Self::MODE) {
            obj.field_json_dyn("colors", ctx, self.colors)?;
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
        if ctx.emits("actor", Self::MODE) {
            obj.field_dyn("actor", ctx, &crate::fields::DrfUuid(self.actor))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkspaceUserLinkSerializer` (model: `db.WorkspaceUserLink`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceUserLinkSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `title`: CharField (source `title`).
    pub title: Option<&'a str>,
    /// `url`: CharField (source `url`).
    pub url: &'a str,
    /// `metadata`: JSONField (source `metadata`).
    pub metadata: Option<crate::fields::PgJson<'a>>,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `project`: PrimaryKeyRelatedField (source `project`).
    pub project: Option<uuid::Uuid>,
    /// `owner`: PrimaryKeyRelatedField (source `owner`).
    pub owner: uuid::Uuid,
}

impl<'a> WorkspaceUserLinkSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "title",
        "url",
        "metadata",
        "created_by",
        "updated_by",
        "workspace",
        "project",
        "owner",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceUserLinkSerializer<'a> {
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
        if ctx.emits("title", Self::MODE) {
            obj.field_dyn("title", ctx, &self.title)?;
        }
        if ctx.emits("url", Self::MODE) {
            obj.field_dyn("url", ctx, &self.url)?;
        }
        if ctx.emits("metadata", Self::MODE) {
            obj.field_json_dyn("metadata", ctx, self.metadata)?;
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
        if ctx.emits("owner", Self::MODE) {
            obj.field_dyn("owner", ctx, &crate::fields::DrfUuid(self.owner))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkspaceUserPreferenceSerializer` (model: `db.WorkspaceUserPreference`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceUserPreferenceSerializer<'a> {
    /// `key`: CharField (source `key`).
    pub key: &'a str,
    /// `is_pinned`: BooleanField (source `is_pinned`).
    pub is_pinned: bool,
    /// `sort_order`: FloatField (source `sort_order`).
    pub sort_order: f64,
}

impl<'a> WorkspaceUserPreferenceSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &["key", "is_pinned", "sort_order"];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceUserPreferenceSerializer<'a> {
    fn drf_write(
        &self,
        w: &mut crate::writer::DrfWriter,
        ctx: &crate::ser::RenderCtx<'_>,
    ) -> Result<(), crate::DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("key", Self::MODE) {
            obj.field_dyn("key", ctx, &self.key)?;
        }
        if ctx.emits("is_pinned", Self::MODE) {
            obj.field_dyn("is_pinned", ctx, &self.is_pinned)?;
        }
        if ctx.emits("sort_order", Self::MODE) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.workspace.WorkspaceUserPropertiesSerializer` (model: `db.WorkspaceUserProperties`).
///
/// Fields are in DRF `_readable_fields` order, which is the
/// `to_representation` key order.
#[derive(Clone, Debug)]
pub struct WorkspaceUserPropertiesSerializer<'a> {
    /// `id`: PrimaryKeyRelatedField (source `id`).
    pub id: uuid::Uuid,
    /// `created_at`: DateTimeField (source `created_at`).
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `updated_at`: DateTimeField (source `updated_at`).
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// `deleted_at`: DateTimeField (source `deleted_at`).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `filters`: JSONField (source `filters`).
    pub filters: Option<crate::fields::PgJson<'a>>,
    /// `display_filters`: JSONField (source `display_filters`).
    pub display_filters: Option<crate::fields::PgJson<'a>>,
    /// `display_properties`: JSONField (source `display_properties`).
    pub display_properties: Option<crate::fields::PgJson<'a>>,
    /// `rich_filters`: JSONField (source `rich_filters`).
    pub rich_filters: Option<crate::fields::PgJson<'a>>,
    /// `navigation_project_limit`: IntegerField (source `navigation_project_limit`).
    pub navigation_project_limit: i64,
    /// `navigation_control_preference`: ChoiceField (source `navigation_control_preference`).
    pub navigation_control_preference: &'a str,
    /// `created_by`: PrimaryKeyRelatedField (source `created_by`).
    pub created_by: Option<uuid::Uuid>,
    /// `updated_by`: PrimaryKeyRelatedField (source `updated_by`).
    pub updated_by: Option<uuid::Uuid>,
    /// `workspace`: PrimaryKeyRelatedField (source `workspace`).
    pub workspace: uuid::Uuid,
    /// `user`: PrimaryKeyRelatedField (source `user`).
    pub user: uuid::Uuid,
}

impl<'a> WorkspaceUserPropertiesSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
        "navigation_project_limit",
        "navigation_control_preference",
        "created_by",
        "updated_by",
        "workspace",
        "user",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::None;
}

impl<'a> crate::ser::DrfSerialize for WorkspaceUserPropertiesSerializer<'a> {
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
        if ctx.emits("filters", Self::MODE) {
            obj.field_json_dyn("filters", ctx, self.filters)?;
        }
        if ctx.emits("display_filters", Self::MODE) {
            obj.field_json_dyn("display_filters", ctx, self.display_filters)?;
        }
        if ctx.emits("display_properties", Self::MODE) {
            obj.field_json_dyn("display_properties", ctx, self.display_properties)?;
        }
        if ctx.emits("rich_filters", Self::MODE) {
            obj.field_json_dyn("rich_filters", ctx, self.rich_filters)?;
        }
        if ctx.emits("navigation_project_limit", Self::MODE) {
            obj.field_dyn(
                "navigation_project_limit",
                ctx,
                &self.navigation_project_limit,
            )?;
        }
        if ctx.emits("navigation_control_preference", Self::MODE) {
            obj.field_dyn(
                "navigation_control_preference",
                ctx,
                &self.navigation_control_preference,
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
        if ctx.emits("user", Self::MODE) {
            obj.field_dyn("user", ctx, &crate::fields::DrfUuid(self.user))?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;
        obj.end();
        Ok(())
    }
}
