// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/member.py`: workspace members, the caller's own
//! membership, and the project-member map.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/members/` | `WorkSpaceMemberViewSet.list` | `member.py:57-75` |
//! | `GET /api/workspaces/{slug}/members/{pk}/` | `WorkSpaceMemberViewSet.retrieve` | `member.py:77-94` |
//! | `GET /api/workspaces/{slug}/workspace-members/me/` | `WorkspaceMemberUserEndpoint.get` | `member.py:234-251` |
//! | `GET /api/workspaces/{slug}/project-members/` | `WorkspaceProjectMemberEndpoint.get` | `member.py:254-283` |
//!
//! `NON_GUEST_FIELDS` (`member.py:47`) is the field mask the view passes to the
//! serializer for a non-guest caller. That mask is the *view's*, not the
//! request's: `?fields=` on the query string is never forwarded, so the request
//! parameter has no effect on these routes
//! (`plane/app/serializers/base.py:93-114`).

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::workspace::WorkspaceMember;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::fields::PgJson;
use plane_serializers::generated::app_project::ProjectMemberRoleSerializer;
use plane_serializers::generated::app_user::{UserAdminLiteSerializer, UserLiteSerializer};
use plane_serializers::generated::app_workspace::{
    WorkSpaceMemberSerializer, WorkspaceMemberAdminSerializer, WorkspaceMemberMeSerializer,
};
use plane_serializers::{FieldEntry, RenderCtx};
use uuid::Uuid;

use crate::common::client;
use crate::workspaces::queries;
use crate::workspaces::rows::{self, UserRow};

/// `NON_GUEST_FIELDS` (`member.py:47`).
const NON_GUEST_FIELDS: [&str; 5] = ["id", "member", "role", "is_active", "created_at"];

/// The three fields a guest sees (`member.py:74,93`).
const GUEST_FIELDS: [&str; 3] = ["id", "member", "role"];

/// `ProjectMemberRoleSerializer` minus `project`, which the view pops out of
/// every member object (`member.py:278`).
const MEMBER_WITHOUT_PROJECT: [&str; 5] = ["id", "role", "member", "original_role", "created_at"];

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.workspace.member.WorkSpaceMemberViewSet")?
        .action("list", list)?
        .action("retrieve", retrieve)?;
    registry
        .api_view("plane.app.views.workspace.member.WorkspaceMemberUserEndpoint")?
        .method(Method::GET, me)?;
    registry
        .api_view("plane.app.views.workspace.member.WorkspaceProjectMemberEndpoint")?
        .method(Method::GET, project_members)?;
    Ok(())
}

/// One decoded membership plus the joined user row.
struct MemberRecord {
    model: WorkspaceMember,
    user: UserRow,
}

impl MemberRecord {
    fn from_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            model: WorkspaceMember::from_row(row, 0)?,
            user: UserRow::from_row(row, 17)?,
        })
    }
}

/// `WorkspaceMemberAdminSerializer` (`member.py:72,91`).
fn admin_serializer<'a>(
    model: &'a WorkspaceMember,
    user: &'a UserAdminLiteSerializer<'a>,
) -> WorkspaceMemberAdminSerializer<'a> {
    WorkspaceMemberAdminSerializer {
        id: model.id,
        member: Some(user),
        created_at: model.created_at,
        updated_at: model.updated_at,
        deleted_at: model.deleted_at,
        role: i64::from(model.role),
        company_role: model.company_role.as_deref(),
        view_props: Some(PgJson::from(&model.view_props)),
        default_props: Some(PgJson::from(&model.default_props)),
        issue_props: Some(PgJson::from(&model.issue_props)),
        is_active: model.is_active,
        getting_started_checklist: Some(PgJson::from(&model.getting_started_checklist)),
        tips: Some(PgJson::from(&model.tips)),
        explored_features: Some(PgJson::from(&model.explored_features)),
        created_by: model.created_by_id,
        updated_by: model.updated_by_id,
        workspace: model.workspace_id,
    }
}

/// `WorkSpaceMemberSerializer` (`member.py:74,93`).
fn guest_serializer<'a>(
    model: &'a WorkspaceMember,
    user: &'a UserLiteSerializer<'a>,
) -> WorkSpaceMemberSerializer<'a> {
    WorkSpaceMemberSerializer {
        id: model.id,
        member: Some(user),
        created_at: model.created_at,
        updated_at: model.updated_at,
        deleted_at: model.deleted_at,
        role: i64::from(model.role),
        company_role: model.company_role.as_deref(),
        view_props: Some(PgJson::from(&model.view_props)),
        default_props: Some(PgJson::from(&model.default_props)),
        issue_props: Some(PgJson::from(&model.issue_props)),
        is_active: model.is_active,
        getting_started_checklist: Some(PgJson::from(&model.getting_started_checklist)),
        tips: Some(PgJson::from(&model.tips)),
        explored_features: Some(PgJson::from(&model.explored_features)),
        created_by: model.created_by_id,
        updated_by: model.updated_by_id,
        workspace: model.workspace_id,
    }
}

/// The `RenderCtx` for a view-supplied field mask: no `expand=` and none of the
/// request's `fields=`, which these views never forward.
fn masked_ctx<'a>(
    timezone: &'a chrono_tz::Tz,
    entries: &'a [FieldEntry<'static>],
) -> RenderCtx<'a> {
    RenderCtx::new(timezone, Some(entries), &[])
}

fn field_entries(names: &[&'static str]) -> Vec<FieldEntry<'static>> {
    names.iter().map(|name| FieldEntry::name(name)).collect()
}

/// `WorkspaceMember.objects.get(member=user, workspace__slug=slug,
/// is_active=True)` (`member.py:67,79`). `.get()` clears the default ordering
/// and caps the result at `MAX_GET_RESULTS` (21).
async fn caller_membership(
    client: &deadpool_postgres::Client,
    slug: &str,
    user: Uuid,
) -> Result<WorkspaceMember, ViewError> {
    let statement = client
        .prepare(
            "SELECT wm.created_at, wm.updated_at, wm.created_by_id, wm.updated_by_id, \
             wm.deleted_at, wm.id, wm.workspace_id, wm.member_id, wm.role, wm.company_role, \
             wm.view_props, wm.default_props, wm.issue_props, wm.is_active, \
             wm.getting_started_checklist, wm.tips, wm.explored_features \
             FROM workspace_members wm \
             INNER JOIN workspaces w ON w.id = wm.workspace_id \
             WHERE w.slug = $2 AND wm.member_id = $1 AND wm.is_active \
               AND wm.deleted_at IS NULL LIMIT 21",
        )
        .await?;
    let rows = client.query(&statement, &[&user, &slug]).await?;
    match rows.len() {
        0 => Err(ViewError::ObjectDoesNotExist),
        1 => Ok(WorkspaceMember::from_row(&rows[0], 0)?),
        _ => Err(rows::multiple_objects("WorkspaceMember")),
    }
}

/// `WorkSpaceMemberViewSet.list` (`member.py:66-75`).
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let client = client(ctx)?;
        let timezone = rows::timezone(ctx);

        let caller = caller_membership(client, slug, user).await?;
        let statement = client
            .prepare_cached(queries::WORKSPACE_MEMBERS.sql)
            .await?;
        let rows = client.query(&statement, &[&slug]).await?;
        let mut decoded = Vec::with_capacity(rows.len());
        for row in &rows {
            decoded.push(MemberRecord::from_row(row)?);
        }

        let body = if caller.role > 5 {
            let entries = field_entries(&NON_GUEST_FIELDS);
            let render_ctx = masked_ctx(&timezone, &entries);
            let users: Vec<_> = decoded
                .iter()
                .map(|record| record.user.admin_lite())
                .collect();
            let serializers: Vec<_> = decoded
                .iter()
                .zip(users.iter())
                .map(|(record, user)| admin_serializer(&record.model, user))
                .collect();
            plane_serializers::render_list(&serializers, &render_ctx)?
        } else {
            let entries = field_entries(&GUEST_FIELDS);
            let render_ctx = masked_ctx(&timezone, &entries);
            let users: Vec<_> = decoded.iter().map(|record| record.user.lite()).collect();
            let serializers: Vec<_> = decoded
                .iter()
                .zip(users.iter())
                .map(|(record, user)| guest_serializer(&record.model, user))
                .collect();
            plane_serializers::render_list(&serializers, &render_ctx)?
        };
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkSpaceMemberViewSet.retrieve` (`member.py:78-94`).
fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(ViewError::Key);
        };
        // `get(pk=…)` coerces through `UUIDField.to_python`; the `try` only
        // catches `WorkspaceMember.DoesNotExist`, so a `ValidationError` is the
        // family's 400 (`member.py:81-88`).
        let Ok(pk) = Uuid::parse_str(pk) else {
            return Err(ViewError::Validation);
        };
        let client = client(ctx)?;
        let timezone = rows::timezone(ctx);

        let caller = caller_membership(client, slug, user).await?;
        let statement = client
            .prepare_cached(queries::WORKSPACE_MEMBER_BY_ID.sql)
            .await?;
        let Some(row) = client.query_opt(&statement, &[&slug, &pk]).await? else {
            return rows::error_body(404, "Workspace member not found");
        };
        let record = MemberRecord::from_row(&row)?;

        let body = if caller.role > 5 {
            let entries = field_entries(&NON_GUEST_FIELDS);
            let user = record.user.admin_lite();
            let serializer = admin_serializer(&record.model, &user);
            plane_serializers::render(&serializer, &masked_ctx(&timezone, &entries))?
        } else {
            let entries = field_entries(&GUEST_FIELDS);
            let user = record.user.lite();
            let serializer = guest_serializer(&record.model, &user);
            plane_serializers::render(&serializer, &masked_ctx(&timezone, &entries))?
        };
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkspaceMemberUserEndpoint.get` (`member.py:237-251`).
fn me<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::WORKSPACE_MEMBER_ME.sql)
            .await?;
        // `.first()`: the permission class already proved a row exists.
        let Some(row) = client.query_opt(&statement, &[&user, &slug]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let model = WorkspaceMember::from_row(&row, 0)?;
        let draft_issue_count: i64 = row.try_get(17)?;
        let serializer = WorkspaceMemberMeSerializer {
            id: model.id,
            draft_issue_count: Some(draft_issue_count),
            created_at: model.created_at,
            updated_at: model.updated_at,
            deleted_at: model.deleted_at,
            role: i64::from(model.role),
            company_role: model.company_role.as_deref(),
            view_props: Some(PgJson::from(&model.view_props)),
            default_props: Some(PgJson::from(&model.default_props)),
            issue_props: Some(PgJson::from(&model.issue_props)),
            is_active: model.is_active,
            getting_started_checklist: Some(PgJson::from(&model.getting_started_checklist)),
            tips: Some(PgJson::from(&model.tips)),
            explored_features: Some(PgJson::from(&model.explored_features)),
            created_by: model.created_by_id,
            updated_by: model.updated_by_id,
            workspace: model.workspace_id,
            member: model.member_id,
        };
        let body = rows::render_plain(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkspaceProjectMemberEndpoint.get` (`member.py:260-283`): one statement;
/// the response is a dict keyed by project id in first-appearance order, with
/// the `project` key popped out of each member object.
fn project_members<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let client = client(ctx)?;
        let timezone = rows::timezone(ctx);
        let statement = client
            .prepare_cached(queries::PROJECT_MEMBERS_FOR_WORKSPACE.sql)
            .await?;
        let rows = client.query(&statement, &[&slug, &user]).await?;

        // `project_members_dict[str(project_id)].append(...)`: insertion order
        // of the project ids is the grouping order.
        let mut groups: Vec<(Uuid, Vec<u8>)> = Vec::new();
        let mut index: std::collections::HashMap<Uuid, usize> = std::collections::HashMap::new();
        for row in &rows {
            let id: Uuid = row.try_get(0)?;
            let role: i16 = row.try_get(1)?;
            let member: Option<Uuid> = row.try_get(2)?;
            let project: Uuid = row.try_get(3)?;
            let created_at: chrono::DateTime<chrono::Utc> = row.try_get(4)?;
            let serializer = ProjectMemberRoleSerializer {
                id,
                role: i64::from(role),
                member,
                project,
                original_role: i64::from(role),
                created_at,
            };
            // `project_member.pop("project")` (`member.py:278`): the key is
            // removed from the object, which is a `fields=` mask without it.
            let entries = field_entries(&MEMBER_WITHOUT_PROJECT);
            let body = plane_serializers::render(&serializer, &masked_ctx(&timezone, &entries))?;
            match index.get(&project) {
                Some(position) => {
                    let group = &mut groups[*position].1;
                    group.push(b',');
                    group.extend_from_slice(&body);
                }
                None => {
                    index.insert(project, groups.len());
                    groups.push((project, body));
                }
            }
        }

        let mut object = Vec::with_capacity(2 + groups.len() * 64);
        object.push(b'{');
        for (position, (project, body)) in groups.iter().enumerate() {
            if position > 0 {
                object.push(b',');
            }
            // `str(project_id)` as a JSON key: hyphenated lowercase, no escaping.
            object.extend_from_slice(format!("\"{project}\":[").as_bytes());
            object.extend_from_slice(body);
            object.push(b']');
        }
        object.push(b'}');
        Ok(ViewResponse::raw(200, APPLICATION_JSON, object))
    })
}
