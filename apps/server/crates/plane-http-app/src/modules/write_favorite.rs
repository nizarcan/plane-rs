// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ModuleFavoriteViewSet.create` / `.destroy`
//! (`plane/app/views/module/base.py:798-844`).
//!
//! | Route | Method | Handler |
//! | --- | --- | --- |
//! | `…/user-favorite-modules/` (110) | `POST` | [`create`] |
//! | `…/user-favorite-modules/{module_id}/` (111) | `DELETE` | [`destroy`] |
//!
//! The viewset is `model = UserFavorite` with `ProjectLitePermission`, so the
//! runtime has already checked the caller is a project member before either
//! handler runs.
//!
//! ## create (`base.py:816-831`)
//!
//! 1. `get_favorite_entities("module", request.user.id, project_id).get(pk=
//!    request.data.get("module"), workspace__slug=slug)`
//!    (`plane/app/serializers/favorite.py:76-104`): a live module of the
//!    project, visible to an active member of the unarchived project. This is
//!    the visibility gate *and* the 404: a missing `module`, a non-UUID, or a
//!    module outside the project all answer before anything is written
//!    (`None` → `DoesNotExist` → 404; a bad UUID is Django's `ValidationError`
//!    → 400 `Please provide valid detail`).
//! 2. `UserFavorite.objects.create(project_id=…, user=…, entity_type="module",
//!    entity_identifier=request.data.get("module"))` — **no** `workspace`:
//!    `WorkspaceBaseModel.save` copies it from the project
//!    (`plane/db/models/workspace.py:192-195`); H25 sets `sequence` from
//!    `MAX(sequence)` over every live favourite of that workspace
//!    (`plane/db/models/favorite.py:52-64`); `created_by` is the caller (H01).
//! 3. `204`, empty body.
//!
//! There is **no** uniqueness guard: the partial unique constraint
//! `(entity_type, entity_identifier, user) WHERE deleted_at IS NULL` makes a
//! second `POST` an `IntegrityError`, which `BaseViewSet.handle_exception`
//! renders as `400 {"error": "The payload is not valid"}`
//! (`plane/app/views/base.py:306-310`).
//!
//! ## destroy (`base.py:833-844`)
//!
//! `UserFavorite.objects.get(project_id, user, workspace__slug,
//! entity_type="module", entity_identifier=module_id)` → a miss is 404; then
//! `module_favorite.delete(soft=False)`. `SoftDeleteModel.delete` hands the
//! instance to `super().delete()` (`plane/db/mixins.py:72-82`), so this is
//! **not** a bare `DELETE … WHERE id = $1`: Django's deletion collector follows
//! the self-referential `user_favorites.parent` CASCADE
//! (`plane/db/models/favorite.py:25-31`) at every depth, soft-deleted children
//! included, and deletes the whole tree in one statement
//! (`django/db/models/deletion.py:398-410,497-510`). [`write::favorite`] is the
//! shared port of that collector.

use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use super::write_issue_queries as queries;
use crate::common::{self, jobs};
use crate::write;

/// `ModuleFavoriteViewSet.create` (`base.py:816-831`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = super::base::uuid_kwarg(ctx, "project_id")?;
        let user = jobs::user_id(ctx)?;

        // `request.data.get("module")`: missing/null reaches `.get(pk=None)` and
        // is a 404; a non-UUID is Django's `ValidationError` → 400.
        let Some(raw) = ctx.data()?.get("module") else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let module_id = Uuid::parse_str(raw).map_err(|_| ViewError::Validation)?;

        // `get_favorite_entities(...).get(pk=…, workspace__slug=…)`.
        let statement = queries::MODULE_FAVORITE_GATE.prepare(client).await?;
        let visible: bool = client
            .query_one(&statement, &[&project_id, &module_id, &slug, &user])
            .await?
            .get(0);
        if !visible {
            return Err(ViewError::ObjectDoesNotExist);
        }

        // `UserFavorite.objects.create(...)` — `WorkspaceBaseModel.save` reads
        // `self.project` through the base manager, then H25.
        let statement = queries::PROJECT_WORKSPACE_ANY.prepare(client).await?;
        let workspace_id: Uuid = client
            .query_opt(&statement, &[&project_id])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?
            .get(0);
        let sequence = hooks::favorite::before_insert(client, workspace_id, None).await?;

        let now = write::now();
        let id = Uuid::new_v4();
        let statement = queries::USER_FAVORITE_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &user,
                    &id,
                    &workspace_id,
                    &project_id,
                    &user,
                    &module_id,
                    &sequence,
                ],
            )
            .await
            .map_err(integrity)?;

        Ok(ViewResponse::empty(204))
    })
}

/// `ModuleFavoriteViewSet.destroy` (`base.py:833-844`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = super::base::uuid_kwarg(ctx, "project_id")?;
        let module_id = super::base::uuid_kwarg(ctx, "module_id")?;
        let user = jobs::user_id(ctx)?;

        // `UserFavorite.objects.get(...)`.
        let statement = queries::USER_FAVORITE_LOOKUP.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&project_id, &user, &slug, &module_id])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let id: Uuid = row.get(0);

        // `delete(soft=False)` → Django's deletion collector over the
        // self-referential `parent` CASCADE, shared with 126 and 184.
        write::favorite::hard_delete(client, id).await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `IntegrityError` → `BaseViewSet.handle_exception`'s
/// `400 {"error": "The payload is not valid"}` (`base.py:306-310`).
fn integrity(error: tokio_postgres::Error) -> ViewError {
    if error.code() == Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION) {
        ViewError::Integrity
    } else {
        ViewError::Unknown(error.to_string())
    }
}
