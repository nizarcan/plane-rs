// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.project.member.ProjectMemberViewSet` — the two read
//! actions, plus `ProjectMemberUserEndpoint` and
//! `ProjectMemberPreferenceEndpoint`.
//!
//! Python: `apps/api/plane/app/views/project/member.py:37-446`.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `GET …/projects/{project_id}/members/` | `list` | `:166-181` |
//! | `GET …/projects/{project_id}/members/{pk}/` | `retrieve` | `:183-219` |
//! | `GET …/projects/{project_id}/project-members/me/` | `ProjectMemberUserEndpoint.get` | `:378-388` |
//! | `GET …/projects/{project_id}/preferences/member/{member_id}/` | `ProjectMemberPreferenceEndpoint.get` | `:434-446` |
//!
//! Two details are behaviour:
//!
//! * `list` passes `fields=("id","member","role","original_role","created_at")`
//!   to `ProjectMemberRoleSerializer`, so `project` is not in the body even
//!   though the serializer declares it;
//! * `retrieve` applies `_without_personal_fields` (`member.py:30-34`):
//!   `preferences`, `view_props` and `default_props` are **popped** from the
//!   serialized dict when the row is not the caller's own, which removes the
//!   keys. `ProjectMemberUserEndpoint` serialises the caller's own row, so it
//!   always keeps them.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_project::{
    ProjectMemberAdminSerializer, ProjectMemberPreferenceSerializer, ProjectMemberRoleSerializer,
    ProjectMemberSerializer,
};
use plane_serializers::{FieldEntry, PgJson, RenderCtx};
use uuid::Uuid;

use crate::common::{client, lookup, render, render_list};
use crate::projects::lite;
use crate::projects::queries;
use crate::projects::serializer::ProjectMemberAdminPublic;

/// The fixed `fields=` of `list` and of the guest branch of `retrieve`
/// (`member.py:178-180,213-215`).
const ROLE_FIELDS: &[&str] = &["id", "member", "role", "original_role", "created_at"];

/// `ROLE.GUEST`.
const ROLE_GUEST: i16 = 5;

/// `ProjectMemberViewSet.list` (`plane/app/views/project/member.py:166-181`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;

        // `list` builds its own queryset and does **not** call
        // `filter_queryset`, so the class's `search_fields` never applies:
        // `?search=` is ignored on this route (`member.py:166-176`). Ported as
        // it is; `retrieve` is unaffected either way.
        let statement = queries::PROJECT_MEMBERS.prepare(client(ctx)?).await?;
        let rows = client(ctx)?
            .query(&statement, &[&slug, &project_id])
            .await?;
        let mut members = Vec::with_capacity(rows.len());
        for row in &rows {
            members.push(MemberRoleRow {
                id: row.get(0),
                member_id: row.get(1),
                role: row.get(2),
                created_at: row.get(3),
            });
        }

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let mask: Vec<FieldEntry<'_>> = ROLE_FIELDS
            .iter()
            .map(|name| FieldEntry::name(name))
            .collect();
        let render_ctx = RenderCtx::new(&timezone, Some(&mask), &[]);
        let serializers: Vec<ProjectMemberRoleSerializer> =
            members.iter().map(role_serializer).collect();
        let body = render_list(&serializers, &render_ctx)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `ProjectMemberViewSet.retrieve` (`plane/app/views/project/member.py:183-219`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // The caller's own membership decides which serializer the target row
        // gets (`member.py:185-215`).
        let requesting_role: Option<i16> = client
            .query_opt(
                queries::PROJECT_MEMBER_SELF_ROLE.sql,
                &[&project_id, &user_id, &slug],
            )
            .await?
            .map(|row| row.get(0));
        let Some(requesting_role) = requesting_role else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        let statement = queries::PROJECT_MEMBER_ONE.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&pk, &project_id, &slug])
            .await?;
        let Some(row) = row else {
            // The view's own body (`member.py:204-208`), not DRF's `get_object()`.
            return error_body(404, "Project member not found");
        };
        let member = member_row(&row)?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        // `requesting_project_member.role > ROLE.GUEST.value`.
        if requesting_role > ROLE_GUEST {
            // The nested project/workspace/user columns come from the same
            // statement (`queries::PROJECT_MEMBER_ONE`), and the three `*_url`
            // properties from one batched asset query.
            let mut project = member.project.clone();
            let mut workspace = member.workspace.clone();
            let mut user = member.user.clone();
            let mut asset_ids = HashSet::new();
            asset_ids.extend(project.cover_image_asset_id);
            asset_ids.extend(workspace.logo_asset_id);
            asset_ids.extend(user.avatar_asset_id);
            let urls = lite::load_asset_urls(client, &asset_ids).await?;
            project.cover_image_url = lite::asset_or_text(
                project.cover_image_asset_id,
                project.cover_image.clone().unwrap_or_default(),
                &urls,
            )?;
            workspace.logo_url = lite::asset_or_text(
                workspace.logo_asset_id,
                workspace.logo.clone().unwrap_or_default(),
                &urls,
            )?;
            user.avatar_url =
                lite::asset_or_text(user.avatar_asset_id, user.avatar.clone(), &urls)?;
            let project_lite = project.lite();
            let workspace_lite = workspace.lite();
            let user_lite = user.admin_lite();

            // The caller's own row keeps the personal fields; another member's
            // row goes through `_without_personal_fields`, which removes the
            // three keys (`serializer.rs` explains why that is a hand-written
            // object and not a `fields=` mask).
            if member.member_id != Some(user_id) {
                let serializer = ProjectMemberAdminPublic {
                    id: member.id,
                    workspace: Some(&workspace_lite),
                    project: Some(&project_lite),
                    member: Some(&user_lite),
                    created_at: member.created_at,
                    updated_at: member.updated_at,
                    deleted_at: member.deleted_at,
                    comment: member.comment.as_deref(),
                    role: i64::from(member.role),
                    sort_order: member.sort_order,
                    is_active: member.is_active,
                    created_by: member.created_by_id,
                    updated_by: member.updated_by_id,
                };
                let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
                return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
            }
            let serializer = ProjectMemberAdminSerializer {
                id: member.id,
                workspace: Some(&workspace_lite),
                project: Some(&project_lite),
                member: Some(&user_lite),
                created_at: member.created_at,
                updated_at: member.updated_at,
                deleted_at: member.deleted_at,
                comment: member.comment.as_deref(),
                role: i64::from(member.role),
                view_props: member.view_props.as_ref().map(PgJson::from),
                default_props: member.default_props.as_ref().map(PgJson::from),
                preferences: member.preferences.as_ref().map(PgJson::from),
                sort_order: member.sort_order,
                is_active: member.is_active,
                created_by: member.created_by_id,
                updated_by: member.updated_by_id,
            };
            let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        }

        let serializer = role_serializer(&MemberRoleRow {
            id: member.id,
            member_id: member.member_id,
            role: member.role,
            created_at: member.created_at,
        });
        let mask: Vec<FieldEntry<'_>> = ROLE_FIELDS
            .iter()
            .map(|name| FieldEntry::name(name))
            .collect();
        let render_ctx = RenderCtx::new(&timezone, Some(&mask), &[]);
        let body = plane_serializers::render(&serializer, &render_ctx)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `ProjectMemberUserEndpoint.get` (`plane/app/views/project/member.py:378-388`).
pub fn me<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let row = client
            .query_opt(
                queries::PROJECT_MEMBER_SELF.sql,
                &[&project_id, &user_id, &slug],
            )
            .await?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let member = member_self(&row)?;

        // The three relations are joined in `queries::PROJECT_MEMBER_SELF`, and
        // their `*_url` properties come from one batched asset query.
        let mut project = member.project.clone();
        let mut workspace = member.workspace.clone();
        let mut user = member.user.clone();
        let mut asset_ids = HashSet::new();
        asset_ids.extend(project.cover_image_asset_id);
        asset_ids.extend(workspace.logo_asset_id);
        asset_ids.extend(user.avatar_asset_id);
        let urls = lite::load_asset_urls(client, &asset_ids).await?;
        project.cover_image_url = lite::asset_or_text(
            project.cover_image_asset_id,
            project.cover_image.clone().unwrap_or_default(),
            &urls,
        )?;
        workspace.logo_url = lite::asset_or_text(
            workspace.logo_asset_id,
            workspace.logo.clone().unwrap_or_default(),
            &urls,
        )?;
        user.avatar_url = lite::asset_or_text(user.avatar_asset_id, user.avatar.clone(), &urls)?;
        let project_lite = project.lite();
        let workspace_lite = workspace.lite();
        let user_lite = user.lite();

        let serializer = ProjectMemberSerializer {
            id: member.id,
            workspace: Some(&workspace_lite),
            project: Some(&project_lite),
            member: Some(&user_lite),
            created_at: member.created_at,
            updated_at: member.updated_at,
            deleted_at: member.deleted_at,
            comment: member.comment.as_deref(),
            role: i64::from(member.role),
            view_props: member.view_props.as_ref().map(PgJson::from),
            default_props: member.default_props.as_ref().map(PgJson::from),
            preferences: member.preferences.as_ref().map(PgJson::from),
            sort_order: member.sort_order,
            is_active: member.is_active,
            created_by: member.created_by_id,
            updated_by: member.updated_by_id,
        };
        let body = render(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `ProjectMemberPreferenceEndpoint.get` (`plane/app/views/project/member.py:434-446`).
pub fn preference<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let member_id =
            lookup::uuid_kwarg("member_id", ctx.kwarg("member_id").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

        // Preferences are personal: a member may only read their own
        // (`member.py:436-441`).
        if member_id != user_id {
            return error_body(403, "You cannot access another member's preferences.");
        }

        let row = client(ctx)?
            .query_opt(
                queries::PROJECT_MEMBER_PREFERENCE.sql,
                &[&project_id, &member_id, &slug],
            )
            .await?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let preferences: plane_db::JsonText = row.get(0);
        let serializer = ProjectMemberPreferenceSerializer {
            preferences: Some(PgJson::from(&preferences)),
            project_id: row.get(1),
            member_id: row.get(2),
            workspace_id: row.get(3),
        };
        let body = render(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// A one-key `{"error": …}` body, in Django's key order.
fn error_body(status: u16, message: &str) -> Result<ViewResponse, ViewError> {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        error: &'a str,
    }
    let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &Body { error: message })?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, rendered))
}

/// The columns `ProjectMemberRoleSerializer` reads.
struct MemberRoleRow {
    id: Uuid,
    member_id: Option<Uuid>,
    role: i16,
    created_at: DateTime<Utc>,
}

/// `ProjectMemberRoleSerializer(row)`.
///
/// `original_role` is `source="role"` (`member.py`'s serializer is
/// `serializers.IntegerField(source="role", read_only=True)`), and `project` is
/// never rendered because both call sites pass a `fields=` without it; the nil
/// value is a placeholder for a field the mask removes.
fn role_serializer(row: &MemberRoleRow) -> ProjectMemberRoleSerializer {
    ProjectMemberRoleSerializer {
        id: row.id,
        role: i64::from(row.role),
        member: row.member_id,
        project: Uuid::nil(),
        original_role: i64::from(row.role),
        created_at: row.created_at,
    }
}

/// A membership row with its own columns, as
/// [`queries::PROJECT_MEMBER_ONE`] projects them.
struct MemberRow {
    id: Uuid,
    member_id: Option<Uuid>,
    comment: Option<String>,
    role: i16,
    view_props: Option<plane_db::JsonText>,
    default_props: Option<plane_db::JsonText>,
    preferences: Option<plane_db::JsonText>,
    sort_order: f64,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    project: lite::LiteProject,
    workspace: lite::LiteWorkspace,
    user: lite::LiteUser,
}

/// `queries::PROJECT_MEMBER_ONE`'s row (the first 16 columns; the nested
/// relations are loaded separately by the caller).
fn member_row(row: &tokio_postgres::Row) -> Result<MemberRow, ViewError> {
    Ok(MemberRow {
        created_at: row.get(0),
        updated_at: row.get(1),
        created_by_id: row.get(2),
        updated_by_id: row.get(3),
        deleted_at: row.get(4),
        id: row.get(5),
        member_id: row.get(8),
        comment: row.get(9),
        role: row.get(10),
        view_props: row.get(11),
        default_props: row.get(12),
        preferences: row.get(13),
        sort_order: row.get(14),
        is_active: row.get(15),
        project: crate::projects::joined::project_at(row, 16)?,
        user: crate::projects::joined::user_at(row, 23)?,
        workspace: crate::projects::joined::workspace_at(row, 32)?,
    })
}

/// `queries::PROJECT_MEMBER_SELF`'s row.
fn member_self(row: &tokio_postgres::Row) -> Result<MemberRow, ViewError> {
    Ok(MemberRow {
        created_at: row.get(0),
        updated_at: row.get(1),
        created_by_id: row.get(2),
        updated_by_id: row.get(3),
        deleted_at: row.get(4),
        id: row.get(5),
        member_id: row.get(8),
        comment: row.get(9),
        role: row.get(10),
        view_props: row.get(11),
        default_props: row.get(12),
        preferences: row.get(13),
        sort_order: row.get(14),
        is_active: row.get(15),
        project: crate::projects::joined::project_at(row, 16)?,
        user: crate::projects::joined::user_at(row, 23)?,
        workspace: crate::projects::joined::workspace_at(row, 32)?,
    })
}
