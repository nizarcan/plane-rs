// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Workspace and project members on `/api/v1/` (routes 292–298).
//!
//! Python: `plane/api/views/member.py:39-337`.
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | `pr/members/`, `pr/project-members/` | `GET` | `ProjectMemberListCreateAPIEndpoint.get` (`:129-149`) |
//! | same | `POST` | `.post` (`:160-164`) |
//! | `pr/members/{pk}/`, `pr/project-members/{pk}/` | `GET`/`PATCH`/`DELETE` | `ProjectMemberDetailAPIEndpoint` (`:183-235`) |
//! | `pr/project-members-lite/` | `GET` | `ProjectMemberLiteAPIEndpoint.get` (`:309-337`) |
//! | `ws/members/` | `GET` | `WorkspaceMemberAPIEndpoint.get` (`:77-99`) |
//! | `ws/members-lite/` | `GET` | `WorkspaceMemberLiteAPIEndpoint.get` (`:262-282`) |
//!
//! The two plain lists are not paginated and append `role` to the serialized
//! user (`member.py:93-98`), which makes the dict's key order the user fields
//! followed by `role`. The two lite lists paginate through `BasePaginator`.
//!
//! ## Permissions (the runtime's job, recorded here because it is behaviour)
//!
//! `ProjectMemberListCreateAPIEndpoint.get_permissions` is
//! `ProjectMemberPermission` for `GET` and `ProjectAdminPermission` for every
//! other verb, `HEAD`/`OPTIONS` included (`member.py:106-109`), resolved by
//! `plane-permissions`' `dynamic` module. `WorkspaceMember*` is
//! `WorkSpaceAdminPermission`, and the lite project list is
//! `ProjectMemberPermission` (`member.py:288`).
//!
//! ## Bugs ported as they are
//!
//! * `WorkspaceMemberAPIEndpoint` includes **inactive** workspace members
//!   (`member.py:90`) and answers 400, not 404, for a missing workspace
//!   (`:84-88`).
//! * the project-member list includes inactive users and members: neither
//!   `is_active` nor a project scope beyond `project_id` is filtered
//!   (`:143-148`).
//! * `ProjectMemberDetailAPIEndpoint.delete` only sets `is_active = False`
//!   (`:232-234`), it does not soft-delete the row.

use chrono::{DateTime, Utc};

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::api_member::{
    ProjectMemberLiteAPISerializer, WorkspaceMemberLiteAPISerializer,
};
use plane_serializers::generated::api_user::UserLiteSerializer;
use plane_serializers::{DrfError, DrfSerialize, DrfUuid, DrfWriter, RenderCtx};
use uuid::Uuid;

use crate::members_queries as queries;
use crate::projects::{error_body, fetch_page, page_envelope};

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.member.ProjectMemberListCreateAPIEndpoint")?
        .method(http::Method::GET, project_members)?
        .method(http::Method::POST, project_member_create)?;
    registry
        .api_view("plane.api.views.member.ProjectMemberDetailAPIEndpoint")?
        .method(http::Method::GET, project_member_get)?
        .method(http::Method::PATCH, project_member_patch)?
        .method(http::Method::DELETE, project_member_delete)?;
    registry
        .api_view("plane.api.views.member.ProjectMemberLiteAPIEndpoint")?
        .method(http::Method::GET, project_members_lite)?;
    registry
        .api_view("plane.api.views.member.WorkspaceMemberAPIEndpoint")?
        .method(http::Method::GET, workspace_members)?;
    registry
        .api_view("plane.api.views.member.WorkspaceMemberLiteAPIEndpoint")?
        .method(http::Method::GET, workspace_members_lite)?;
    Ok(())
}

/// One `users` row joined into a membership row.
#[derive(Debug)]
struct UserRow {
    id: Uuid,
    first_name: String,
    last_name: String,
    email: Option<String>,
    avatar: String,
    avatar_asset_id: Option<Uuid>,
    display_name: String,
    is_bot: bool,
    role: i16,
    is_active: bool,
}

impl UserRow {
    /// A membership row: `id`, `member_id`, `role`, `is_active`, then the
    /// joined user's 8 columns (`id`, names, email, avatar, asset, display
    /// name, `is_bot`).
    fn decode_membership(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.get(4),
            first_name: row.get(5),
            last_name: row.get(6),
            email: row.get(7),
            avatar: row.get(8),
            avatar_asset_id: row.get(9),
            display_name: row.get(10),
            is_bot: row.get(11),
            role: row.get(2),
            is_active: row.get(3),
        })
    }

    fn lite<'b>(&'b self, avatar_url: Option<&'b str>) -> UserLiteSerializer<'b> {
        UserLiteSerializer {
            id: self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            email: self.email.as_deref(),
            avatar: &self.avatar,
            avatar_url,
            display_name: &self.display_name,
        }
    }
}

/// `UserLiteSerializer(user).data` plus `user_data["role"] = role`
/// (`plane/api/views/member.py:95-97`).
#[derive(Debug)]
struct UserWithRole<'a> {
    user: &'a UserRow,
    avatar_url: Option<&'a str>,
}

impl DrfSerialize for UserWithRole<'_> {
    fn drf_write(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field("id", &DrfUuid(self.user.id))?;
        obj.field("first_name", &self.user.first_name)?;
        obj.field("last_name", &self.user.last_name)?;
        obj.field("email", &self.user.email)?;
        obj.field("avatar", &self.user.avatar)?;
        obj.field("avatar_url", &self.avatar_url)?;
        obj.field("display_name", &self.user.display_name)?;
        obj.field("role", &i64::from(self.user.role))?;
        obj.end();
        Ok(())
    }
}

/// Resolve every row's `avatar_url` property in one asset query
/// (`User.avatar_url`, `plane/db/models/user.py:142-151`).
async fn avatar_urls(
    client: &deadpool_postgres::Client,
    rows: &[UserRow],
) -> Result<Vec<Option<String>>, ViewError> {
    let ids: std::collections::HashSet<Uuid> =
        rows.iter().filter_map(|row| row.avatar_asset_id).collect();
    let urls = plane_http_app::projects::lite::load_asset_urls(client, &ids).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(plane_http_app::projects::lite::asset_or_text(
            row.avatar_asset_id,
            row.avatar.clone(),
            &urls,
        )?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 297 — workspace members
// ---------------------------------------------------------------------------

/// `WorkspaceMemberAPIEndpoint.get` (`plane/api/views/member.py:77-99`).
pub fn workspace_members<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let client = plane_http_app::common::client(ctx)?;

        let present: bool = client
            .query_one(queries::WORKSPACE_EXISTS.sql, &[&slug])
            .await
            .map_err(ViewError::from)?
            .get(0);
        if !present {
            // 400, not 404 (`member.py:85-88`).
            return Ok(error_body(400, "Provided workspace does not exist"));
        }

        let rows = client
            .query(queries::WORKSPACE_MEMBERS.sql, &[&slug])
            .await
            .map_err(ViewError::from)?;
        let mut members = Vec::with_capacity(rows.len());
        for row in &rows {
            members.push(UserRow::decode_membership(row)?);
        }
        let urls = avatar_urls(client, &members).await?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let render_ctx = RenderCtx::full(&timezone);
        let serializers: Vec<UserWithRole<'_>> = members
            .iter()
            .enumerate()
            .map(|(index, user)| UserWithRole {
                user,
                avatar_url: urls[index].as_deref(),
            })
            .collect();
        let body = plane_http_app::common::render_list(&serializers, &render_ctx)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// 298 — workspace members (lite)
// ---------------------------------------------------------------------------

/// `WorkspaceMemberLiteAPIEndpoint.get` (`plane/api/views/member.py:262-282`).
pub fn workspace_members_lite<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let client = plane_http_app::common::client(ctx)?;

        let present: bool = client
            .query_one(queries::WORKSPACE_EXISTS.sql, &[&slug])
            .await
            .map_err(ViewError::from)?
            .get(0);
        if !present {
            return Ok(error_body(404, "Provided workspace does not exist"));
        }

        // `WorkspaceMember.objects.filter(...).order_by("-created_at")`; the
        // paginator slices it (`member.py:275-282`).
        let (rows, total, window, next, prev) = fetch_page(
            ctx,
            client,
            queries::WORKSPACE_MEMBERS_PAGE.sql,
            &[&slug],
            queries::WORKSPACE_MEMBERS_COUNT.sql,
            &[&slug],
            1000,
            UserRow::decode_membership,
        )
        .await?;
        let urls = avatar_urls(client, &rows).await?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let mut serializers = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            serializers.push(WorkspaceMemberLiteAPISerializer {
                id: row.id,
                first_name: &row.first_name,
                last_name: &row.last_name,
                email: row.email.as_deref(),
                avatar: &row.avatar,
                avatar_url: urls[index].as_deref(),
                display_name: &row.display_name,
                role: i64::from(row.role),
                is_active: row.is_active,
                is_bot: row.is_bot,
            });
        }
        let results =
            plane_http_app::common::render_list(&serializers, &RenderCtx::full(&timezone))?;
        Ok(page_envelope(
            total,
            &window,
            &next,
            &prev,
            serializers.len(),
            results,
        ))
    })
}

// ---------------------------------------------------------------------------
// 292/294 — project members
// ---------------------------------------------------------------------------

/// `ProjectMemberListCreateAPIEndpoint.get` (`plane/api/views/member.py:129-149`).
pub fn project_members<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = plane_http_app::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let client = plane_http_app::common::client(ctx)?;

        let present: bool = client
            .query_one(queries::WORKSPACE_EXISTS.sql, &[&slug])
            .await
            .map_err(ViewError::from)?
            .get(0);
        if !present {
            return Ok(error_body(400, "Provided workspace does not exist"));
        }

        let rows = client
            .query(queries::PROJECT_MEMBER_USERS.sql, &[&project_id, &slug])
            .await
            .map_err(ViewError::from)?;
        let mut users = Vec::with_capacity(rows.len());
        for row in &rows {
            users.push(UserRow {
                id: row.get(0),
                first_name: row.get(1),
                last_name: row.get(2),
                email: row.get(3),
                avatar: row.get(4),
                avatar_asset_id: row.get(5),
                display_name: row.get(6),
                is_bot: false,
                role: 0,
                is_active: true,
            });
        }
        let urls = avatar_urls(client, &users).await?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let render_ctx = RenderCtx::full(&timezone);
        let serializers: Vec<UserLiteSerializer<'_>> = users
            .iter()
            .enumerate()
            .map(|(index, user)| user.lite(urls[index].as_deref()))
            .collect();
        let body = plane_http_app::common::render_list(&serializers, &render_ctx)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `ProjectMemberDetailAPIEndpoint.get` (`plane/api/views/member.py:183-200`).
pub fn project_member_get<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = plane_http_app::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let client = plane_http_app::common::client(ctx)?;

        let present: bool = client
            .query_one(queries::WORKSPACE_EXISTS.sql, &[&slug])
            .await
            .map_err(ViewError::from)?
            .get(0);
        if !present {
            return Ok(error_body(400, "Provided workspace does not exist"));
        }

        let row = client
            .query_opt(
                queries::PROJECT_MEMBER_DETAIL_USER.sql,
                &[&project_id, &slug, &pk],
            )
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            // `ProjectMember.objects.get(...)` or `User.objects.get(...)`.
            return Err(ViewError::ObjectDoesNotExist);
        };
        let user = UserRow {
            id: row.get(0),
            first_name: row.get(1),
            last_name: row.get(2),
            email: row.get(3),
            avatar: row.get(4),
            avatar_asset_id: row.get(5),
            display_name: row.get(6),
            is_bot: false,
            role: 0,
            is_active: true,
        };
        let urls = avatar_urls(client, std::slice::from_ref(&user)).await?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body =
            plane_serializers::render(&user.lite(urls[0].as_deref()), &RenderCtx::full(&timezone))
                .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// 295 — project members (lite)
// ---------------------------------------------------------------------------

/// `ProjectMemberLiteAPIEndpoint.get` (`plane/api/views/member.py:309-337`).
pub fn project_members_lite<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = plane_http_app::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let client = plane_http_app::common::client(ctx)?;

        let present: bool = client
            .query_one(queries::WORKSPACE_EXISTS.sql, &[&slug])
            .await
            .map_err(ViewError::from)?
            .get(0);
        if !present {
            return Ok(error_body(404, "Provided workspace does not exist"));
        }
        let present: bool = client
            .query_one(queries::PROJECT_EXISTS.sql, &[&project_id, &slug])
            .await
            .map_err(ViewError::from)?
            .get(0);
        if !present {
            return Ok(error_body(404, "Provided project does not exist"));
        }

        let (rows, total, window, next, prev) = fetch_page(
            ctx,
            client,
            queries::PROJECT_MEMBERS_PAGE.sql,
            &[&project_id, &slug],
            queries::PROJECT_MEMBERS_COUNT.sql,
            &[&project_id, &slug],
            1000,
            UserRow::decode_membership,
        )
        .await?;
        let urls = avatar_urls(client, &rows).await?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let mut serializers = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            serializers.push(ProjectMemberLiteAPISerializer {
                id: row.id,
                first_name: &row.first_name,
                last_name: &row.last_name,
                email: row.email.as_deref(),
                avatar: &row.avatar,
                avatar_url: urls[index].as_deref(),
                display_name: &row.display_name,
                role: i64::from(row.role),
                is_active: row.is_active,
                is_bot: row.is_bot,
            });
        }
        let results =
            plane_http_app::common::render_list(&serializers, &RenderCtx::full(&timezone))?;
        Ok(page_envelope(
            total,
            &window,
            &next,
            &prev,
            serializers.len(),
            results,
        ))
    })
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// `ProjectMemberSerializer`'s `validate_member` (`api/serializers/member.py:27-36`):
/// the target must be an **active** workspace member.
async fn member_is_in_workspace(
    client: &deadpool_postgres::Client,
    slug: &str,
    member: Uuid,
) -> Result<bool, ViewError> {
    Ok(client
        .query_one(queries::WORKSPACE_MEMBER_ACTIVE.sql, &[&slug, &member])
        .await
        .map_err(ViewError::from)?
        .get(0))
}

/// The role of an active workspace membership, or `None`
/// (`api/serializers/member.py:53-57`).
async fn workspace_role(
    client: &deadpool_postgres::Client,
    slug: &str,
    member: Uuid,
) -> Result<Option<i16>, ViewError> {
    let row = client
        .query_opt(queries::WORKSPACE_MEMBER_ROLE.sql, &[&slug, &member])
        .await
        .map_err(ViewError::from)?;
    Ok(row.map(|row| row.get(0)))
}

/// `ROLE.GUEST` / `ROLE.MEMBER` / `ROLE.ADMIN` (`plane/utils/permissions/base.py`).
const ROLE_GUEST: i16 = 5;
const ROLE_MEMBER: i16 = 15;
const ROLE_ADMIN: i16 = 20;

/// A full `project_members` row, as `get()` loads it.
struct MemberModel {
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
    member_id: Option<Uuid>,
    comment: Option<String>,
    role: i16,
    view_props: JsonText,
    default_props: JsonText,
    preferences: JsonText,
    sort_order: f64,
    is_active: bool,
}

impl MemberModel {
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            created_at: row.get(0),
            updated_at: row.get(1),
            created_by: row.get(2),
            updated_by: row.get(3),
            deleted_at: row.get(4),
            id: row.get(5),
            project_id: row.get(6),
            workspace_id: row.get(7),
            member_id: row.get(8),
            comment: row.get(9),
            role: row.get(10),
            view_props: row.get(11),
            default_props: row.get(12),
            preferences: row.get(13),
            sort_order: row.get(14),
            is_active: row.get(15),
        })
    }
}

async fn get_member(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    pk: Uuid,
) -> Result<MemberModel, ViewError> {
    let row = client
        .query_opt(
            queries::PROJECT_MEMBER_BY_PK.sql,
            &[&project_id, &slug, &pk],
        )
        .await
        .map_err(ViewError::from)?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    MemberModel::decode(&row)
}

/// `ProjectMemberSerializer(instance).data` — `id`, `member`, `role`
/// (`api/serializers/member.py:71-75`).
fn render_member(member: &MemberModel, status: u16) -> ViewResponse {
    let body = format!(
        "{{\"id\":\"{}\",\"member\":{},\"role\":{}}}",
        member.id,
        match member.member_id {
            Some(id) => format!("\"{id}\""),
            None => "null".to_string(),
        },
        member.role,
    );
    ViewResponse::raw(status, APPLICATION_JSON, body.into_bytes())
}

/// `ProjectMemberListCreateAPIEndpoint.post` (`plane/api/views/member.py:160-164`).
pub fn project_member_create<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = plane_http_app::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let actor = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        // `serializer = ProjectMemberSerializer(data=request.data,
        // context={"slug": slug})`; `is_valid(raise_exception=True)`.
        let mut serializer = plane_http_app::write::Serializer::new(ctx, false)?;
        plane_http_app::write::generated::api_project_member_serializer::validate(
            &mut serializer,
            client,
        )
        .await?;
        if let Some(plane_http_app::write::Value::Uuid(member)) = serializer.value("member")
            && !member_is_in_workspace(client, slug, *member).await?
        {
            // `validate_member` (`api/serializers/member.py:27-36`).
            serializer.push_error(
                "member",
                plane_http_app::write::Detail::new("Member not found in workspace", "invalid"),
            );
        }
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };
        let member = values
            .uuid("member")
            .ok_or_else(|| ViewError::Unknown("KeyError: 'member'".to_string()))?;
        // The model field's default (`plane/db/models/project.py:265`).
        let role = values
            .integer("role")
            .map(|role| role as i16)
            .unwrap_or(ROLE_GUEST);

        // `ProjectMemberSerializer.validate` (`api/serializers/member.py:41-63`):
        // a workspace guest cannot be made a member or admin.
        match workspace_role(client, slug, member).await? {
            Some(workspace_role) => {
                if workspace_role == ROLE_GUEST && (role == ROLE_ADMIN || role == ROLE_MEMBER) {
                    return Ok(plane_http_app::write::field_string_error(
                        "non_field_errors",
                        "You cannot assign a role higher than the member's workspace role",
                    ));
                }
            }
            None => {
                return Ok(plane_http_app::write::field_string_error(
                    "member",
                    "Member not found in workspace",
                ));
            }
        }

        // `serializer.save(project_id=…)` → `ProjectMember.objects.create` →
        // `ProjectMember.save` (`plane/db/models/project.py:276-292`).
        let workspace_id: Uuid = client
            .query_one(queries::PROJECT_WORKSPACE_ID_ANY.sql, &[&project_id])
            .await
            .map_err(ViewError::from)?
            .get(0);
        let now = plane_http_app::write::now();
        let id = Uuid::new_v4();
        let view_props = default_props();

        // H32: the membership's `ProjectUserProperty`, with the workspace-wide
        // minimum sort order minus 10000 (or the model default 65535).
        let min: Option<f64> = client
            .query_one(
                queries::PROJECT_USER_PROPERTY_MIN.sql,
                &[&workspace_id, &member],
            )
            .await
            .map_err(ViewError::from)?
            .get(0);
        let sort_order = min.map_or(65535.0, |min| min - 10000.0);
        client
            .execute(
                queries::PROJECT_USER_PROPERTY_INSERT.sql,
                &[
                    &now,
                    &now,
                    &Some(actor),
                    &None::<Uuid>,
                    &None::<DateTime<Utc>>,
                    &Uuid::new_v4(),
                    &project_id,
                    &workspace_id,
                    &member,
                    &JsonText::new(DEFAULT_FILTERS),
                    &JsonText::new(DEFAULT_DISPLAY_FILTERS),
                    &JsonText::new(DEFAULT_DISPLAY_PROPERTIES),
                    &JsonText::new("{}"),
                    &default_preferences(),
                    &sort_order,
                ],
            )
            .await
            .map_err(db_error)?;

        client
            .execute(
                queries::PROJECT_MEMBER_INSERT.sql,
                &[
                    &now,
                    &now,
                    &Some(actor),
                    &None::<Uuid>,
                    &None::<DateTime<Utc>>,
                    &id,
                    &project_id,
                    &workspace_id,
                    &Some(member),
                    &None::<String>,
                    &role,
                    &view_props,
                    &view_props,
                    &default_preferences(),
                    &65535.0_f64,
                    &true,
                ],
            )
            .await
            .map_err(db_error)?;

        Ok(render_member(
            &MemberModel {
                created_at: now,
                updated_at: now,
                created_by: Some(actor),
                updated_by: None,
                deleted_at: None,
                id,
                project_id,
                workspace_id,
                member_id: Some(member),
                comment: None,
                role,
                view_props,
                default_props: default_props(),
                preferences: default_preferences(),
                sort_order: 65535.0,
                is_active: true,
            },
            201,
        ))
    })
}

/// `ProjectMemberDetailAPIEndpoint.patch` (`plane/api/views/member.py:211-221`).
pub fn project_member_patch<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = plane_http_app::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let actor = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        let mut current = get_member(client, slug, project_id, pk).await?;
        // `if not WorkspaceMember.objects.filter(workspace__slug=slug,
        // member_id=project_member.member_id, is_active=True).exists()`
        // (`member.py:214-217`) — 400 with a field-shaped body.
        let target_active = match current.member_id {
            Some(member) => client
                .query_one(queries::WORKSPACE_MEMBER_ACTIVE.sql, &[&slug, &member])
                .await
                .map_err(ViewError::from)?
                .get::<_, bool>(0),
            None => false,
        };
        if !target_active {
            return Ok(error_list_body("member", "Member not found in workspace"));
        }

        let mut serializer = plane_http_app::write::Serializer::new(ctx, true)?;
        plane_http_app::write::generated::api_project_member_serializer::validate(
            &mut serializer,
            client,
        )
        .await?;
        if let Some(plane_http_app::write::Value::Uuid(member)) = serializer.value("member")
            && !member_is_in_workspace(client, slug, *member).await?
        {
            serializer.push_error(
                "member",
                plane_http_app::write::Detail::new("Member not found in workspace", "invalid"),
            );
        }
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `ProjectMemberSerializer.validate` (`api/serializers/member.py:41-63`)
        // runs on the merged body/instance values.
        let role = values
            .integer("role")
            .map(|role| role as i16)
            .unwrap_or(current.role);
        let member = values.uuid("member").or(current.member_id);
        if let Some(member) = member {
            match workspace_role(client, slug, member).await? {
                Some(workspace_role) => {
                    if workspace_role == ROLE_GUEST && (role == ROLE_ADMIN || role == ROLE_MEMBER) {
                        return Ok(error_list_body(
                            "non_field_errors",
                            "You cannot assign a role higher than the member's workspace role",
                        ));
                    }
                }
                None => {
                    return Ok(error_list_body("member", "Member not found in workspace"));
                }
            }
        }

        // `ModelSerializer.update` assigns the validated fields, then
        // `instance.save()` writes every column (`BaseModel.save` stamps
        // `updated_at`/`updated_by`).
        current.member_id = member;
        current.role = role;
        current.updated_at = plane_http_app::write::now();
        current.updated_by = Some(actor);
        client
            .execute(
                queries::PROJECT_MEMBER_UPDATE.sql,
                &[
                    &current.created_at,
                    &current.updated_at,
                    &current.created_by,
                    &current.updated_by,
                    &current.deleted_at,
                    &current.project_id,
                    &current.workspace_id,
                    &current.member_id,
                    &current.comment,
                    &current.role,
                    &current.view_props,
                    &current.default_props,
                    &current.preferences,
                    &current.sort_order,
                    &current.is_active,
                    &current.id,
                ],
            )
            .await
            .map_err(db_error)?;

        Ok(render_member(&current, 200))
    })
}

/// `ProjectMemberDetailAPIEndpoint.delete` (`plane/api/views/member.py:231-235`):
/// `is_active = False; save()`.
pub fn project_member_delete<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = plane_http_app::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let actor = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        let current = get_member(client, slug, project_id, pk).await?;
        let now = plane_http_app::write::now();
        client
            .execute(
                queries::PROJECT_MEMBER_SET_INACTIVE.sql,
                &[&now, &Some(actor), &current.id],
            )
            .await
            .map_err(ViewError::from)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `{"<field>": ["<message>"]}` with DRF's 400, for the views that return the
/// body themselves (`member.py:217`).
fn error_list_body(field: &'static str, message: &str) -> ViewResponse {
    let body = plane_query::Json::object(vec![(
        field,
        plane_query::Json::Array(vec![plane_query::Json::string(message)]),
    )]);
    ViewResponse::raw(400, APPLICATION_JSON, body.to_drf())
}

/// Django's `IntegrityError` family is the family 400; other database errors
/// are the 500 (`plane/api/views/base.py:183-243`).
fn db_error(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// `plane/db/models/project.py:39` (`get_default_props`).
fn default_props() -> JsonText {
    JsonText::new(
        r#"{"filters": {"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}, "display_filters": {"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}}"#,
    )
}

/// `plane/db/models/project.py:64` (`get_default_preferences`).
fn default_preferences() -> JsonText {
    JsonText::new(
        r#"{"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}}"#,
    )
}

/// `plane/db/models/issue.py:47` (`get_default_filters`).
const DEFAULT_FILTERS: &str = r#"{"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}"#;
/// `plane/db/models/issue.py:61` (`get_default_display_filters`).
const DEFAULT_DISPLAY_FILTERS: &str = r#"{"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}"#;
/// `plane/db/models/issue.py:73` (`get_default_display_properties`).
const DEFAULT_DISPLAY_PROPERTIES: &str = r#"{"assignee": true, "attachment_count": true, "created_on": true, "due_date": true, "estimate": true, "key": true, "labels": true, "link": true, "priority": true, "start_date": true, "state": true, "sub_issue_count": true, "updated_on": true}"#;
