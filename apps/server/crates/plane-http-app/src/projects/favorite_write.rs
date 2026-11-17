// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ProjectFavoritesViewSet.create` / `.destroy`
//! (`plane/app/views/project/base.py:499-537`).
//!
//! | Route | Method | Handler |
//! | --- | --- | --- |
//! | `/api/workspaces/{slug}/user-favorite-projects/` | `POST` | [`create`] |
//! | `…/user-favorite-projects/{project_id}/` | `DELETE` | [`destroy`] |
//!
//! ## create (`base.py:512-526`)
//!
//! 1. `project_id = request.data.get("project")` — no validation of its own;
//! 2. `get_favorite_entities("project", request.user.id, project_id)
//!    .get(pk=project_id, workspace__slug=slug)` — a 404 unless the project is
//!    live, in this workspace, unarchived, and the caller is an active member
//!    of it (serializer `plane/app/serializers/favorite.py:86-93`);
//! 3. `UserFavorite.objects.create(user=…, entity_type="project",
//!    entity_identifier=request.data.get("project"),
//!    project_id=request.data.get("project"))` — **no** `workspace`:
//!    `WorkspaceBaseModel.save` copies it from the project
//!    (`plane/db/models/workspace.py:192-195`); H25 sets `sequence`
//!    (`plane/db/models/favorite.py:52-64`); `created_by` is the caller (H01);
//! 4. `204`, empty body.
//!
//! There is **no uniqueness guard**: the partial unique constraint
//! `(entity_type, entity_identifier, user) WHERE deleted_at IS NULL` makes a
//! second `POST` an `IntegrityError`, which `BaseViewSet.handle_exception`
//! renders as `400 {"error": "The payload is not valid"}`
//! (`plane/app/views/base.py:306-310`).
//!
//! ## destroy (`base.py:528-537`)
//!
//! `UserFavorite.objects.get(entity_identifier=project_id,
//! entity_type="project", project=project_id, user=…, workspace__slug=slug)`
//! → `DoesNotExist` is 404; then `project_favorite.delete(soft=False)`. That is
//! **not** a single-row `DELETE`: `SoftDeleteModel.delete` hands the instance to
//! `super().delete()` (`plane/db/mixins.py:72-82`), i.e. Django's deletion
//! collector, and `UserFavorite.parent` is a self-referential
//! `on_delete=CASCADE` FK (`plane/db/models/favorite.py:25-31`). Every favourite
//! reachable through `parent_id` goes with it — at every depth, soft-deleted
//! children included, because the cascade reads the related model's
//! `_base_manager` (`django/db/models/deletion.py:398-410`). [`write::favorite`]
//! is the shared port of that collector (126, 111, 184, 37).
//!
//! Note the asymmetry, ported as it is: `create` stores the row with
//! `workspace = project.workspace`, so `destroy`'s `workspace__slug` filter
//! finds it; but a favourite created through the *other* favourites endpoints
//! (stickies, pages, views — each sets `workspace` from its own entity) is
//! found by the same filter only when the workspace matches.
//!
//! ## Statements
//!
//! | Action | Django | Here |
//! | --- | ---: | ---: |
//! | create | 3 (gate, `MAX(sequence)`, INSERT) | 3 |
//! | destroy | 1 SELECT + one `parent_id` probe per depth + the `DELETE` | same |

use plane_core::error::ApiError;
use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use crate::common::client;
use crate::projects::write_queries as queries;
use crate::projects::write_support;
use crate::write;

/// Register the two viewset actions.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.project.base.ProjectFavoritesViewSet")?
        .action("create", create)?
        .action("destroy", destroy)?;
    Ok(())
}

/// `ProjectFavoritesViewSet.create` (`base.py:512-526`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `request.data.get("project")`; a missing value reaches `.get(pk=None)`
        // and is a 404, and a non-UUID is `ValidationError` → 400.
        let Some(project) = ctx.data()?.get("project") else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let project_id = Uuid::parse_str(project).map_err(|_| ViewError::Validation)?;

        // `get_favorite_entities(...).get(pk=…, workspace__slug=…)`.
        let row = client
            .query_opt(
                queries::PROJECT_FAVORITE_GATE.sql,
                &[&project_id, &user, &slug],
            )
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let workspace_id: Uuid = row.try_get(0)?;

        // H25: `MAX(sequence) + 10000`, or the model default 65535.
        let sequence = hooks::favorite::before_insert(client, workspace_id, None).await?;

        let now = write::now();
        let id = Uuid::new_v4();
        client
            .execute(
                queries::USER_FAVORITE_INSERT.sql,
                &[
                    &now,
                    &now,
                    &user,
                    &id,
                    &workspace_id,
                    &Some(project_id),
                    &user,
                    &Some(project_id),
                    &sequence,
                ],
            )
            .await
            .map_err(write_support::integrity)?;

        Ok(ViewResponse::empty(204))
    })
}

/// `ProjectFavoritesViewSet.destroy` (`base.py:528-537`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = ctx
            .kwarg("project_id")
            .ok_or(ViewError::Key)
            .and_then(|value| Uuid::parse_str(value).map_err(|_| ViewError::Validation))?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let row = client
            .query_opt(
                queries::USER_FAVORITE_FOR_PROJECT.sql,
                &[&project_id, &user, &slug],
            )
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let id: Uuid = row.try_get(0)?;

        // `delete(soft=False)` → Django's deletion collector over the
        // self-referential `parent` CASCADE, shared with 126/111/184/37.
        write::favorite::hard_delete(client, id).await?;
        Ok(ViewResponse::empty(204))
    })
}
