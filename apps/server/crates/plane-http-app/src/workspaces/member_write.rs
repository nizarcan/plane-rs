// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkSpaceMemberViewSet`'s write actions (`plane/app/views/workspace/member.py`)
//! — workspace-core set B.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 194 `PATCH …/members/{pk}/` | `PATCH` | `partial_update` (`member.py:96-139`) | [`partial_update`] |
//! | 194 `DELETE …/members/{pk}/` | `DELETE` | `destroy` (`member.py:141-180`) | [`destroy`] |
//! | 195 `POST …/members/leave/` | `POST` | `leave` (`member.py:182-222`) | [`leave`] |
//!
//! ## 194 PATCH is the only handler here with a transaction
//!
//! `partial_update` wraps the project-membership demotion and the member save
//! in `transaction.atomic()` (`member.py:130-137`). Demoting to `ROLE.GUEST`
//! rewrites **every live `project_members` row** of the member in the workspace
//! — inactive rows included, soft-deleted ones excluded by the manager — to
//! `role=5`, then writes the three member columns `role`, `updated_at`,
//! `updated_by` (`update_fields`). A guest who is the only admin of any project
//! is refused **before** the transaction, and the guard counts soft-deleted and
//! inactive project rows (`member.py:31-43`).
//!
//! `destroy` and `leave` have no transaction: they deactivate the active
//! project memberships first, then the workspace membership, with a full save
//! whose moving columns are `is_active`, `updated_at` and `updated_by`. The
//! `workspace_members` row is never deleted and no cascade task is published.
//!
//! ## The three refusals, and their order
//!
//! * PATCH: own row (`member.py:101-105`) → body key set (`:109-113`) →
//!   serializer → the sole-admin guard (`:122-128`).
//! * DELETE: own row (`:153-157`) → a higher-role target (`:159-163`) →
//!   the sole-admin guard (`:165-171`).
//! * leave: the only active workspace admin (`:195-204`, which `.count()`s the
//!   role-20 rows and includes soft-deleted memberships in the count's absence
//!   of a `deleted_at` shift — the manager's own predicate is the only one) →
//!   the sole-admin guard (`:206-212`).

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks::audit::Audit;
use plane_db::models::workspace::WorkspaceMember;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use crate::common::{cache, client, lookup};
use crate::workspaces::invite_write::{member_serializer, only_role};
use crate::workspaces::rows::{self, UserRow};
use crate::workspaces::write_b_queries as queries_b;
use crate::workspaces::write_queries as shared_queries;
use crate::write::{self, Serializer};

/// `plane.app.views.workspace.member.WorkSpaceMemberViewSet`.
const VIEW: &str = "plane.app.views.workspace.member.WorkSpaceMemberViewSet";

/// `ROLE.GUEST` / `ROLE.ADMIN` (`plane/db/models/workspace.py`).
const ROLE_GUEST: i16 = 5;
const ROLE_ADMIN: i16 = 20;

/// The three `ROLE_CHOICES` values, in the serializer's choice order.
const ROLE_CHOICES: &[i16] = &[20, 15, 5];

/// The sole-admin refusal both guards share (`member.py:122-128,165-171`).
const SOLE_ADMIN_MEMBER: &str = "User is a part of some projects where they are the only admin, they should either leave that project or promote another user to admin.";
/// `leave`'s own wording (`member.py:206-212`).
const SOLE_ADMIN_SELF: &str = "You are a part of some projects where you are the only admin, you should either leave the project or promote another user to admin.";

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset(VIEW)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?
        .action("leave", leave)?;
    Ok(())
}

/// `Response({"error": …})` as a `Result`.
fn fail(status: u16, message: &str) -> Result<ViewResponse, ViewError> {
    Ok(write::error_body(status, message))
}

/// `is_sole_admin_of_any_project(slug, user_id)` (`member.py:31-43`).
async fn sole_admin_of_any_project(
    client: &deadpool_postgres::Client,
    slug: &str,
    user_id: Uuid,
) -> Result<bool, ViewError> {
    let statement = queries_b::SOLE_ADMIN_OF_ANY_PROJECT.prepare(client).await?;
    let row = client.query_one(&statement, &[&slug, &user_id]).await?;
    Ok(row.get(0))
}

// ---------------------------------------------------------------------------
// 194 PATCH
// ---------------------------------------------------------------------------

/// `WorkSpaceMemberViewSet.partial_update` (`member.py:96-139`).
fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        // `select_related("member", "member__avatar_asset").get(…)`
        // (`member.py:98-100`) — a bot, an inactive or a soft-deleted member is
        // the family 404.
        let statement = queries_b::MEMBER_TARGET_FOR_UPDATE.prepare(client).await?;
        let Some(row) = client.query_opt(&statement, &[&slug, &pk]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let member = WorkspaceMember::from_row(&row, 0)?;
        let member_user = UserRow::from_row(&row, 17)?;

        if user == member.member_id {
            return fail(400, "You cannot update your own role");
        }

        // Only `role` is writable (`member.py:107-113`).
        if !only_role(data) {
            return fail(400, "Only the role of a workspace member can be updated");
        }

        let mut serializer = Serializer::new(ctx, true)?;
        serializer.integer_choice_field("role", ROLE_CHOICES);
        let role = match serializer.finish() {
            Ok(values) => values
                .integer("role")
                .ok_or_else(|| ViewError::Unknown("KeyError: 'role'".to_string()))?,
            Err(errors) => return Ok(errors.into_response()),
        };
        let role = i16::try_from(role)
            .map_err(|_| ViewError::Unknown("smallint out of range".to_string()))?;

        // A guest cannot be the only admin of a project (`member.py:120-128`).
        let is_guest = role == ROLE_GUEST;
        if is_guest && sole_admin_of_any_project(client, slug, member.member_id).await? {
            return fail(400, SOLE_ADMIN_MEMBER);
        }

        // `with transaction.atomic():` (`member.py:130-137`).
        client.batch_execute("BEGIN").await?;
        let mut applied = Ok(());
        if is_guest {
            let statement = queries_b::PROJECT_MEMBERS_DEMOTE.prepare(client).await?;
            if let Err(error) = client
                .execute(
                    &statement,
                    &[
                        &ROLE_GUEST,
                        &write::now(),
                        &member.member_id,
                        &member.workspace_id,
                    ],
                )
                .await
            {
                applied = Err(ViewError::from(error));
            }
        }
        let updated_at = write::now();
        if applied.is_ok() {
            let statement = queries_b::MEMBER_UPDATE_ROLE.prepare(client).await?;
            if let Err(error) = client
                .execute(&statement, &[&role, &updated_at, &Some(user), &member.id])
                .await
            {
                applied = Err(ViewError::from(error));
            }
        }
        match applied {
            Ok(()) => client.batch_execute("COMMIT").await?,
            Err(error) => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(error);
            }
        }

        let mut updated = member;
        updated.role = role;
        updated.updated_at = updated_at;
        updated.updated_by_id = Audit::on_update(Some(user));
        let user_lite = member_user.lite();
        let serializer = member_serializer(&updated, &user_lite);
        let body = rows::render_plain(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// 194 DELETE
// ---------------------------------------------------------------------------

/// `WorkSpaceMemberViewSet.destroy` (`member.py:141-180`).
fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let statement = queries_b::MEMBER_TARGET_FOR_DESTROY.prepare(client).await?;
        let Some(row) = client.query_opt(&statement, &[&slug, &pk]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let member = WorkspaceMember::from_row(&row, 0)?;

        // `WorkspaceMember.objects.get(workspace__slug=slug,
        // member=request.user, is_active=True)` (`member.py:149-151`).
        let statement = shared_queries::WORKSPACE_MEMBER_ACTIVE
            .prepare(client)
            .await?;
        let requester = client.query(&statement, &[&user, &slug]).await?;
        let (requester_id, requester_role) = match requester.len() {
            0 => return Err(ViewError::ObjectDoesNotExist),
            1 => (
                requester[0].get::<_, Uuid>(0),
                requester[0].get::<_, i16>(3),
            ),
            _ => return Err(rows::multiple_objects("WorkspaceMember")),
        };

        if member.id == requester_id {
            return fail(
                400,
                "You cannot remove yourself from the workspace. Please use leave workspace",
            );
        }
        if requester_role < member.role {
            return fail(400, "You cannot remove a user having role higher than you");
        }
        if sole_admin_of_any_project(client, slug, member.member_id).await? {
            return fail(400, SOLE_ADMIN_MEMBER);
        }

        // `ProjectMember.objects.filter(workspace__slug=slug,
        // member_id=…, is_active=True).update(is_active=False, updated_at=now)`
        // (`member.py:174-176`).
        let statement = queries_b::PROJECT_MEMBERS_DEACTIVATE_WS
            .prepare(client)
            .await?;
        client
            .execute(&statement, &[&write::now(), &member.member_id, &slug])
            .await
            .map_err(deactivate_error)?;

        // The full save (`member.py:178-179`).
        let statement = queries_b::MEMBER_DEACTIVATE.prepare(client).await?;
        client
            .execute(&statement, &[&write::now(), &Some(user), &member.id])
            .await
            .map_err(deactivate_error)?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// 195 leave
// ---------------------------------------------------------------------------

/// `WorkSpaceMemberViewSet.leave` (`member.py:182-222`).
fn leave<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // The three `@invalidate_cache` decorators, outermost first
        // (`member.py:182-189`). The last path has **no** leading slash, as in
        // the Python.
        cache::invalidate_cache(
            ctx,
            Some("/api/workspaces/:slug/members/"),
            true,
            false,
            true,
        )
        .await?;
        cache::invalidate_cache(ctx, Some("/api/users/me/settings/"), false, true, false).await?;
        cache::invalidate_cache(ctx, Some("api/users/me/workspaces/"), false, false, true).await?;

        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let statement = shared_queries::WORKSPACE_MEMBER_ACTIVE
            .prepare(client)
            .await?;
        let rows = client.query(&statement, &[&user, &slug]).await?;
        let (member_id, member_role) = match rows.len() {
            0 => return Err(ViewError::ObjectDoesNotExist),
            1 => (rows[0].get::<_, Uuid>(0), rows[0].get::<_, i16>(3)),
            _ => return Err(rows::multiple_objects("WorkspaceMember")),
        };

        // The only-active-admin guard (`member.py:195-204`): `.count() > 1`.
        if member_role == ROLE_ADMIN {
            let statement = shared_queries::WORKSPACE_ADMIN_COUNT
                .prepare(client)
                .await?;
            let row = client.query_one(&statement, &[&slug]).await?;
            let admins: i64 = row.get(0);
            if admins <= 1 {
                return fail(
                    400,
                    "You cannot leave the workspace as you are the only admin of the workspace you will have to either delete the workspace or promote another user to admin.",
                );
            }
        }

        if sole_admin_of_any_project(client, slug, user).await? {
            return fail(400, SOLE_ADMIN_SELF);
        }

        let statement = queries_b::PROJECT_MEMBERS_DEACTIVATE_WS
            .prepare(client)
            .await?;
        client
            .execute(&statement, &[&write::now(), &user, &slug])
            .await
            .map_err(deactivate_error)?;

        let statement = queries_b::MEMBER_DEACTIVATE.prepare(client).await?;
        client
            .execute(&statement, &[&write::now(), &Some(user), &member_id])
            .await
            .map_err(deactivate_error)?;
        Ok(ViewResponse::empty(204))
    })
}

/// The two deactivating UPDATEs are plain queryset writes and full saves, so an
/// `IntegrityError` is the family 400 and anything else the 500 — the same
/// mapping `projects::member_write::db_error` uses.
fn deactivate_error(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}
