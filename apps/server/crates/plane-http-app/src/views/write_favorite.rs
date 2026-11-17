// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IssueViewFavoriteViewSet.create` / `.destroy`
//! (`plane/app/views/view/base.py:441-479`).
//!
//! | Route | Method | Handler |
//! | --- | --- | --- |
//! | `…/projects/{project_id}/user-favorite-views/` (183) | `POST` | [`create`] |
//! | `…/projects/{project_id}/user-favorite-views/{view_id}/` (184) | `DELETE` | [`destroy`] |
//!
//! Both methods carry `@allow_permission([ROLE.ADMIN, ROLE.MEMBER])` with the
//! default `PROJECT` level (`base.py:459`, `:471`), so the runtime has already
//! answered 403 for a guest, an inactive member and a non-member before either
//! handler runs. The 183 `GET` is the runtime's guard 405 and is registered by
//! [`super::base`].
//!
//! ## create (`base.py:459-469`)
//!
//! 1. `get_favorite_entities("view", request.user.id, project_id).get(
//!    pk=request.data.get("view"), workspace__slug=slug)`
//!    (`plane/app/serializers/favorite.py:76-104`): a live view of the project,
//!    whose caller is an active member of the unarchived project, and which is
//!    either owned by the caller or `access == 1` and not restricted from the
//!    caller as a project guest. This is the visibility gate **and** the 404: a
//!    missing `view`, a `null`, a non-UUID (400) and a view of another project
//!    all answer before anything is written.
//! 2. `UserFavorite.objects.create(user=…, entity_identifier=…,
//!    entity_type="view", project_id=…)` — **no** `workspace`:
//!    `WorkspaceBaseModel.save` copies it from the project
//!    (`plane/db/models/workspace.py:192-195`); H25 sets `sequence` from
//!    `MAX(sequence)` over every live favourite of that workspace
//!    (`plane/db/models/favorite.py:52-65`); `created_by` is the caller (H01).
//! 3. `204`, empty body.
//!
//! There is no uniqueness guard: the partial unique constraint
//! `(entity_type, entity_identifier, user) WHERE deleted_at IS NULL` turns a
//! second `POST` into an `IntegrityError`, which the family
//! `handle_exception` renders as `400 {"error": "The payload is not valid"}`
//! (`plane/app/views/base.py:210-214`).
//!
//! ## destroy (`base.py:471-479`)
//!
//! `UserFavorite.objects.get(project=…, user=…, workspace__slug=…,
//! entity_type="view", entity_identifier=view_id)` → a miss is the family 404;
//! then `view_favorite.delete(soft=False)`. `SoftDeleteModel.delete` hands the
//! instance to `super().delete()` (`plane/db/mixins.py:72-78`), so this is
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

use super::write_queries as queries;
use super::write_support::{self, integrity};
use crate::common::{self, jobs};
use crate::write;

/// `IssueViewFavoriteViewSet.create` (`base.py:459-469`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = crate::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let user = jobs::user_id(ctx)?;

        // `.get(pk=request.data.get("view"), …)`: the body is parsed (400/415 on
        // a bad body) and the value coerced before the SELECT.
        let Some(view_id) = write_support::view_id(ctx.data()?)? else {
            // `pk=None` → `id IS NULL` → `DoesNotExist`.
            return Err(ViewError::ObjectDoesNotExist);
        };

        let statement = queries::VIEW_FAVORITE_GATE.prepare(client).await?;
        let visible: bool = client
            .query_one(&statement, &[&project_id, &view_id, &slug, &user])
            .await?
            .get(0);
        if !visible {
            return Err(ViewError::ObjectDoesNotExist);
        }

        // `UserFavorite.objects.create(...)` — `WorkspaceBaseModel.save` reads
        // `self.project` through the base manager, then H25.
        let statement = queries::PROJECT_WORKSPACE.prepare(client).await?;
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
                    &view_id,
                    &sequence,
                ],
            )
            .await
            .map_err(integrity)?;

        Ok(ViewResponse::empty(204))
    })
}

/// `IssueViewFavoriteViewSet.destroy` (`base.py:471-479`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = crate::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let view_id = crate::common::lookup::uuid_kwarg(
            "view_id",
            ctx.kwarg("view_id").ok_or(ViewError::Key)?,
        )?;
        let user = jobs::user_id(ctx)?;

        // `UserFavorite.objects.get(...)` — the manager's `deleted_at IS NULL`
        // applies, so a soft-deleted favourite is a miss (404).
        let statement = queries::USER_FAVORITE_LOOKUP.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&project_id, &user, &slug, &view_id])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let id: Uuid = row.get(0);

        // `delete(soft=False)` → Django's deletion collector over the
        // self-referential `parent` CASCADE, shared with 126 and 111.
        write::favorite::hard_delete(client, id).await?;

        Ok(ViewResponse::empty(204))
    })
}
