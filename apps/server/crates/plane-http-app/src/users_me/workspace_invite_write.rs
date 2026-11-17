// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `UserWorkspaceInvitationsViewSet.create` —
//! `POST /api/users/me/workspaces/invitations/` (route 190,
//! `plane/app/views/workspace/invite.py:275-313`).
//!
//! The signed-in user accepts a list of invitations addressed to their own
//! address. `get_queryset`'s `email=request.user.email` filter is repeated in
//! `create`, so an id of somebody else's invitation is simply not selected.
//!
//! Per invitation, in `-created_at` order: one cache invalidation for that
//! workspace's member list, then `WorkspaceMember.objects.filter(workspace_id=…,
//! member=…)`.update(is_active=True, role=invitation.role)` — a queryset update
//! that writes **only** those two columns, so `updated_at`/`updated_by` keep
//! their values (`invite.py:284-294`).
//!
//! Then one `bulk_create(..., ignore_conflicts=True)` of a membership per
//! invitation (a workspace the user already belongs to conflicts and is
//! skipped), and a queryset `delete()` of the consumed invitations, which is H04
//! — `deleted_at=now` and nothing else (`invite.py:296-311`).
//!
//! `invitations: null` is `pk__in=None`, an iteration `TypeError` (the family
//! 500); a non-UUID element is Django's `ValidationError` (the family 400). Both
//! happen when `.filter()` compiles, before any write.

use plane_core::error::ApiError;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use crate::common::{cache, client};
use crate::views::write_support::{Body, body_get};
use crate::workspaces::write_b_queries as queries_b;
use crate::write;

/// `plane.app.views.workspace.invite.UserWorkspaceInvitationsViewSet`.
const VIEW: &str = "plane.app.views.workspace.invite.UserWorkspaceInvitationsViewSet";

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.viewset(VIEW)?.action("create", create)?;
    Ok(())
}

/// `request.data.get("invitations", [])` through Django's `pk__in` coercion
/// (`django/db/models/fields/__init__.py`, `UUIDField.to_python`).
///
/// * absent → an empty list (`EmptyResultSet`, no statement);
/// * `null` → `TypeError: 'NoneType' object is not iterable` (the family 500);
/// * a number/bool → the same `TypeError` for a non-iterable;
/// * a string/dict → its keys/characters go through `UUID(hex=…)` and fail
///   (`ValidationError`, the family 400) unless it is empty;
/// * a list → one coercion per element, `null` sinking the comparison.
fn invitation_ids(value: Option<Body<'_>>) -> Result<Vec<Uuid>, ViewError> {
    let not_iterable =
        |name: &str| ViewError::Unknown(format!("TypeError: '{name}' object is not iterable"));
    match value {
        None | Some(Body::Missing) => Ok(Vec::new()),
        Some(Body::Form(text)) => {
            if text.is_empty() {
                Ok(Vec::new())
            } else {
                Err(ViewError::Validation)
            }
        }
        Some(Body::Json(Json::Null)) => Err(not_iterable("NoneType")),
        Some(Body::Json(Json::Bool(_))) => Err(not_iterable("bool")),
        Some(Body::Json(Json::Int(_))) => Err(not_iterable("int")),
        Some(Body::Json(Json::Float(_))) => Err(not_iterable("float")),
        Some(Body::Json(Json::Str(text))) => {
            if text.as_str().is_some_and(str::is_empty) {
                Ok(Vec::new())
            } else {
                Err(ViewError::Validation)
            }
        }
        Some(Body::Json(Json::Dict(entries))) => {
            if entries.is_empty() {
                Ok(Vec::new())
            } else {
                Err(ViewError::Validation)
            }
        }
        Some(Body::Json(Json::List(items))) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Json::Null => {}
                    Json::Str(text) => {
                        let text = text.as_str().ok_or(ViewError::Validation)?;
                        ids.push(Uuid::parse_str(text).map_err(|_| ViewError::Validation)?);
                    }
                    Json::Bool(flag) => ids.push(Uuid::from_u128(u128::from(*flag))),
                    Json::Int(number) => {
                        let text = number.to_string();
                        let value: u128 = text.parse().map_err(|_| ViewError::Validation)?;
                        ids.push(Uuid::from_u128(value));
                    }
                    Json::Float(_) | Json::List(_) | Json::Dict(_) => {
                        return Err(ViewError::Validation);
                    }
                }
            }
            Ok(ids)
        }
    }
}

/// `UserWorkspaceInvitationsViewSet.create` (`invite.py:275-313`).
fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `@invalidate_cache(path="/api/workspaces/", user=False)` then
        // `@invalidate_cache(path="/api/users/me/workspaces/", multiple=True)`
        // (`invite.py:275-276`), outermost first.
        cache::invalidate_cache(ctx, Some("/api/workspaces/"), false, false, false).await?;
        cache::invalidate_cache(ctx, Some("/api/users/me/workspaces/"), false, true, true).await?;

        let user = ctx
            .user
            .user()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let user_id = user.id;
        let email = user.email.clone();
        let client = client(ctx)?;
        let data = ctx.data()?;

        let ids = invitation_ids(Some(body_get(data, "invitations")?))?;

        // `WorkspaceMemberInvite.objects.filter(pk__in=invitations,
        // email=request.user.email).order_by("-created_at")`
        // (`invite.py:279-281`).
        let invites = if ids.is_empty() {
            Vec::new()
        } else {
            let statement = queries_b::INVITES_FOR_USER_BY_PKS.prepare(client).await?;
            client.query(&statement, &[&email, &ids]).await?
        };

        let mut rows = Vec::with_capacity(invites.len());
        for row in &invites {
            let workspace_id: Uuid = row.get(6);
            let role: i16 = row.get(12);
            let slug: String = row.get(13);
            rows.push((workspace_id, role, slug));
        }

        // The per-invitation invalidation and reactivation (`invite.py:284-294`).
        for (workspace_id, role, slug) in &rows {
            let path = format!("/api/workspaces/{slug}/members/");
            cache::invalidate_cache(ctx, Some(&path), false, false, true).await?;
            let statement = queries_b::MEMBER_SET_ACTIVE_ROLE.prepare(client).await?;
            client
                .execute(&statement, &[role, &user_id, workspace_id])
                .await
                .map_err(db_error)?;
        }

        // `WorkspaceMember.objects.bulk_create([…], ignore_conflicts=True)`
        // (`invite.py:296-308`), one atomic like Django's `bulk_create`.
        let statement = queries_b::MEMBER_INSERT_IGNORE.prepare(client).await?;
        client.batch_execute("BEGIN").await?;
        let mut inserted = Ok(());
        for (workspace_id, role, _) in &rows {
            let now = write::now();
            if let Err(error) = client
                .execute(
                    &statement,
                    &[
                        &now,
                        &now,
                        &Uuid::new_v4(),
                        role,
                        &Some(user_id),
                        &user_id,
                        workspace_id,
                        &plane_db::JsonText::new(
                            crate::projects::invite_write::WORKSPACE_DEFAULT_PROPS,
                        ),
                        &plane_db::JsonText::new(
                            crate::projects::invite_write::WORKSPACE_DEFAULT_PROPS,
                        ),
                        &plane_db::JsonText::new(
                            crate::projects::invite_write::WORKSPACE_ISSUE_PROPS,
                        ),
                    ],
                )
                .await
            {
                inserted = Err(db_error(error));
                break;
            }
        }
        match inserted {
            Ok(()) => client.batch_execute("COMMIT").await?,
            Err(error) => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(error);
            }
        }

        // `workspace_invitations.delete()` (`invite.py:311`) — H04.
        if !ids.is_empty() {
            plane_db::hooks::soft_delete::delete_where(
                client,
                &queries_b::INVITES_DELETE_FOR_USER,
                &[&email, &ids],
                write::now(),
            )
            .await
            .map_err(ViewError::from)?;
        }

        Ok(ViewResponse::empty(204))
    })
}

/// Django's `IntegrityError` family is the family 400.
fn db_error(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}
