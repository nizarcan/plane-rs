// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ProjectInvitationsViewset`'s post/destroy, `UserProjectInvitationsViewset`'s
//! create and `ProjectJoinEndpoint.post` (`plane/app/views/project/invite.py`).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/projects/{project_id}/invitations/` | `create` | `invite.py:65-131` |
//! | `DELETE …/projects/{project_id}/invitations/{pk}/` | `destroy` | `invite.py:61-63` |
//! | `POST /api/users/me/workspaces/{slug}/projects/invitations/` | `create` | `invite.py:74-124` |
//! | `POST …/projects/{project_id}/join/{pk}/` | `post` | `invite.py:158-298` |
//!
//! ## Upstream behaviour, ported as it is
//!
//! **Route 139 never creates an invitation.** Its role guard reads `.role` off
//! a `QuerySet`:
//!
//! ```python
//! workspace_role = WorkspaceMember.objects.filter(...).role
//! ```
//!
//! `QuerySet` has no `role`, so any non-empty `emails` raises `AttributeError`
//! before the `bulk_create`; the family handler answers 500. An empty/missing
//! `emails` is the 400 `{"error": "Emails are required"}`. The loop over
//! `project_invitations` below it would also raise (`list.delay`), but it is
//! unreachable: the first loop always raises first. With it, the
//! `project_invitation` task is never enqueued (it is also unregistered — see
//! `TASKS.md` §8.1).
//!
//! **Route 143 contract.** The token is compared before authentication
//! (`invite.py:167-173`), the accepting user must be the invitee
//! (`:181-185`), `accepted` must be a JSON boolean (`:188-194`), and the
//! accepted branch creates/reactivates a `WorkspaceMember` and a
//! `ProjectMember` — the former has no save hook, the latter goes through H32
//! and therefore also creates the `ProjectUserProperty`.

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::JsonText;
use plane_db::hooks::audit::Audit;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::projects::member_queries as queries;
use crate::projects::member_write::{
    ROLE_ADMIN, ROLE_MEMBER, RawValue, body_value, create_member, db_error, fail,
    insert_project_member, insert_user_property, truthy,
};
use crate::write;

/// `plane/db/models/workspace.py:22` (`get_default_props`), which unlike the
/// project model's copy also carries `display_properties`.
const WORKSPACE_DEFAULT_PROPS: &str = r#"{"filters": {"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}, "display_filters": {"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}, "display_properties": {"assignee": true, "attachment_count": true, "created_on": true, "due_date": true, "estimate": true, "key": true, "labels": true, "link": true, "priority": true, "start_date": true, "state": true, "sub_issue_count": true, "updated_on": true}}"#;
/// `plane/db/models/workspace.py:110` (`get_issue_props`).
const ISSUE_PROPS: &str =
    r#"{"subscribed": true, "assigned": true, "created": true, "all_issues": true}"#;

/// A one-key `{"message": …}` body.
fn message_body(status: u16, message: &str) -> Result<ViewResponse, ViewError> {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        message: &'a str,
    }
    let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &Body { message })?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, rendered))
}

/// `ProjectInvitationsViewset.create` (`plane/app/views/project/invite.py:65-131`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let _slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let _project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;

        let data = ctx.data()?;
        let non_empty = match body_value(data, "emails")? {
            None => false,
            Some(RawValue::Json(value)) => truthy(value),
            Some(RawValue::Text(text)) => !text.is_empty(),
        };
        if !non_empty {
            return fail(400, "Emails are required");
        }
        // `WorkspaceMember.objects.filter(...).role` → `AttributeError`
        // (`invite.py:72-74`).
        Err(ViewError::Unknown(
            "AttributeError: 'QuerySet' object has no attribute 'role'".to_string(),
        ))
    })
}

/// `ProjectInvitationsViewset.destroy` (`plane/app/views/project/invite.py:61-63`
/// → `BaseViewSet.destroy`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
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

        let statement = queries::PROJECT_INVITE_DESTROY_TARGET
            .prepare(client)
            .await?;
        let row = client
            .query_opt(&statement, &[&slug, &project_id, &pk])
            .await?;
        let Some(row) = row else {
            // DRF's `get_object()` miss names the model
            // (`rest_framework/generics.py:87-89`).
            return crate::projects::not_found::no_match("ProjectMemberInvite");
        };
        let id: Uuid = row.get(0);

        // `instance.delete()` → H03, then the cascade task.
        let now = write::now();
        write::soft_delete(
            client,
            "project_member_invites",
            "id",
            id,
            Audit::on_update(Some(user_id)),
            now,
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "projectmemberinvite", id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `UserProjectInvitationsViewset.create` (`plane/app/views/project/invite.py:74-124`)
/// — the caller joins a list of projects of their workspace.
pub fn join_projects<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        let project_ids = project_id_list(body_value(data, "project_ids")?)?;

        // `WorkspaceMember.objects.get(member=request.user,
        // workspace__slug=slug, is_active=True)` (`invite.py:88`).
        let statement = queries::WORKSPACE_MEMBER_SELF.prepare(client).await?;
        let member = client.query_opt(&statement, &[&user_id, &slug]).await?;
        let Some(member) = member else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_role: i16 = member.get(1);
        let workspace_id: Uuid = member.get(2);

        // `Project.objects.filter(...)` (`invite.py:91-103`).
        let statement = queries::PROJECTS_BY_IDS.prepare(client).await?;
        let projects = client.query(&statement, &[&project_ids, &slug]).await?;
        // `validated_project_ids` is the *queryset's* ids, not the requested
        // ones: an id that matches no project is dropped (`invite.py:139`).
        let mut validated_project_ids = Vec::with_capacity(projects.len());
        for row in &projects {
            let id: Uuid = row.get(0);
            let network: i16 = row.get(1);
            if network == 0 && workspace_role != ROLE_ADMIN {
                return fail(403, "Only workspace admins can join private project");
            }
            validated_project_ids.push(id);
        }

        // `ProjectMember.objects.filter(...).update(...)` (`invite.py:146-149`).
        if !validated_project_ids.is_empty() {
            let statement = queries::PROJECT_MEMBER_REACTIVATE.prepare(client).await?;
            client
                .execute(
                    &statement,
                    &[
                        &workspace_role,
                        &user_id,
                        &write::now(),
                        &slug,
                        &validated_project_ids,
                        &user_id,
                    ],
                )
                .await
                .map_err(db_error)?;
        }

        // `ProjectMember.objects.bulk_create(...)` and
        // `ProjectUserProperty.objects.bulk_create(...)` (`invite.py:151-201`).
        for project_id in &validated_project_ids {
            insert_project_member(
                client,
                *project_id,
                workspace_id,
                Some(user_id),
                Some(workspace_role),
                Some(user_id),
                write::now(),
            )
            .await?;
        }
        for project_id in &validated_project_ids {
            insert_user_property(
                client,
                *project_id,
                workspace_id,
                user_id,
                65535.0,
                Some(user_id),
                write::now(),
            )
            .await?;
        }

        message_body(201, "Projects joined successfully")
    })
}

/// `request.data.get("project_ids", [])` as Django's `id__in` walks it.
///
/// A string is iterated character by character (so any non-empty string is an
/// invalid UUID); a non-iterable is a `TypeError` (500); `null` behaves like an
/// empty list here (the list is not iterated for a query that would return
/// nothing).
fn project_id_list(value: Option<RawValue<'_>>) -> Result<Vec<Uuid>, ViewError> {
    match value {
        None => Ok(Vec::new()),
        Some(RawValue::Json(Json::List(items))) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Json::Null => {}
                    Json::Str(value) => {
                        let text = value.as_str().ok_or_else(|| {
                            ViewError::Unknown("a lone-surrogate UUID".to_string())
                        })?;
                        ids.push(Uuid::parse_str(text).map_err(|_| ViewError::Validation)?);
                    }
                    Json::Bool(_) | Json::Int(_) | Json::Float(_) => {
                        return Err(ViewError::Unknown(
                            "TypeError: uuid.UUID() argument has the wrong type".to_string(),
                        ));
                    }
                    Json::List(_) | Json::Dict(_) => {
                        return Err(ViewError::Unknown(
                            "TypeError: unhashable UUID input".to_string(),
                        ));
                    }
                }
            }
            Ok(ids)
        }
        Some(RawValue::Json(Json::Null)) => Ok(Vec::new()),
        Some(RawValue::Json(Json::Str(value))) => {
            let text = value
                .as_str()
                .ok_or_else(|| ViewError::Unknown("a lone-surrogate UUID".to_string()))?;
            if text.is_empty() {
                Ok(Vec::new())
            } else {
                // Iterating a string yields characters, none of which is a
                // UUID.
                Err(ViewError::Validation)
            }
        }
        Some(RawValue::Json(Json::Dict(map))) => {
            if map.is_empty() {
                Ok(Vec::new())
            } else {
                Err(ViewError::Validation)
            }
        }
        Some(RawValue::Json(Json::Bool(_) | Json::Int(_) | Json::Float(_))) => Err(
            ViewError::Unknown("TypeError: 'int' object is not iterable".to_string()),
        ),
        Some(RawValue::Text(text)) => {
            if text.is_empty() {
                Ok(Vec::new())
            } else {
                Err(ViewError::Validation)
            }
        }
    }
}

/// The invite row `ProjectJoinEndpoint.post` reads.
struct Invite {
    id: Uuid,
    email: String,
    token: String,
    role: i16,
    responded_at: Option<chrono::DateTime<chrono::Utc>>,
    project_id: Uuid,
    workspace_id: Uuid,
    message: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    created_by_id: Option<Uuid>,
}

/// `ProjectJoinEndpoint.post` (`plane/app/views/project/invite.py:158-298`).
pub fn join<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        let statement = queries::PROJECT_INVITE_BY_PK.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&pk, &project_id, &slug])
            .await?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let invite = Invite {
            id: row.get(0),
            email: row.get(1),
            token: row.get(2),
            role: row.get(3),
            responded_at: row.get(5),
            project_id: row.get(6),
            workspace_id: row.get(7),
            message: row.get(8),
            created_at: row.get(9),
            created_by_id: row.get(10),
        };

        // The token check precedes authentication (`invite.py:167-173`).
        let token = match body_value(data, "token")? {
            None => "",
            Some(RawValue::Text(text)) => text,
            Some(RawValue::Json(Json::Str(value))) => value
                .as_str()
                .ok_or_else(|| ViewError::Unknown("a lone-surrogate token".to_string()))?,
            Some(RawValue::Json(_)) => {
                return fail(403, "You do not have permission to join the project");
            }
        };
        if token.is_empty() || invite.token != token {
            return fail(403, "You do not have permission to join the project");
        }

        if !ctx.user.is_authenticated() {
            return fail(401, "Authentication required to accept project invitation");
        }
        let Some(user) = ctx.user.user() else {
            return fail(401, "Authentication required to accept project invitation");
        };
        let user_id = user.id;
        if user.email.to_lowercase() != invite.email.to_lowercase() {
            return fail(403, "You do not have permission to accept this invitation");
        }

        if invite.responded_at.is_some() {
            return fail(400, "You have already responded to the invitation request");
        }

        // `accepted = request.data.get("accepted", False)`; `isinstance(...,
        // bool)` (`invite.py:188-194`).
        let accepted = match body_value(data, "accepted")? {
            None => false,
            Some(RawValue::Json(Json::Bool(value))) => *value,
            Some(_) => return fail(400, "`accepted` must be a boolean"),
        };

        let responded_at = write::now();
        let updated_at = write::now();
        let statement = queries::PROJECT_INVITE_SAVE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &invite.created_at,
                    &updated_at,
                    &invite.id,
                    &invite.email,
                    &accepted,
                    &invite.token,
                    &invite.message,
                    &responded_at,
                    &invite.role,
                    &invite.created_by_id,
                    &invite.project_id,
                    &Some(user_id),
                    &invite.workspace_id,
                    &invite.id,
                ],
            )
            .await
            .map_err(db_error)?;

        if !accepted {
            return message_body(200, "Project Invitation was not accepted");
        }

        // `WorkspaceMember.objects.filter(workspace__slug=slug, member=user).first()`
        // (`invite.py:205`).
        let statement = queries::WORKSPACE_MEMBER_BY_MEMBER.prepare(client).await?;
        let workspace_member = client.query_opt(&statement, &[&slug, &user_id]).await?;
        match workspace_member {
            None => {
                // `role=(15 if project_invite.role >= 15 else project_invite.role)`
                // (`invite.py:206-210`).
                let role = if invite.role >= ROLE_MEMBER {
                    ROLE_MEMBER
                } else {
                    invite.role
                };
                let now = write::now();
                let statement = queries::WORKSPACE_MEMBER_INSERT.prepare(client).await?;
                client
                    .execute(
                        &statement,
                        &[
                            &now,
                            &now,
                            &Uuid::new_v4(),
                            &role,
                            &Some(user_id),
                            &user_id,
                            &invite.workspace_id,
                            &JsonText::new(WORKSPACE_DEFAULT_PROPS),
                            &JsonText::new(WORKSPACE_DEFAULT_PROPS),
                            &JsonText::new(ISSUE_PROPS),
                        ],
                    )
                    .await
                    .map_err(db_error)?;
            }
            Some(row) => {
                let id: Uuid = row.get(0);
                let statement = queries::WORKSPACE_MEMBER_ACTIVATE.prepare(client).await?;
                client
                    .execute(&statement, &[&write::now(), &Some(user_id), &id])
                    .await
                    .map_err(db_error)?;
            }
        }

        // `ProjectMember.objects.filter(workspace_id=…, project_id=…,
        // member=user).first()` (`invite.py:236-248`).
        let statement = queries::PROJECT_MEMBER_BY_MEMBER.prepare(client).await?;
        let project_member = client
            .query_opt(&statement, &[&invite.workspace_id, &project_id, &user_id])
            .await?;
        match project_member {
            None => {
                create_member(
                    client,
                    project_id,
                    invite.workspace_id,
                    user_id,
                    invite.role,
                    Some(user_id),
                    write::now(),
                )
                .await?;
            }
            Some(row) => {
                let id: Uuid = row.get(0);
                let statement = queries::PROJECT_MEMBER_SET_ROLE_ACTIVE
                    .prepare(client)
                    .await?;
                client
                    .execute(&statement, &[&invite.role, &id])
                    .await
                    .map_err(db_error)?;
            }
        }

        message_body(200, "Project Invitation Accepted")
    })
}
