// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `PageFavoriteViewSet.create` / `.destroy`
//! (`plane/app/views/page/base.py:467-492`), route 126.
//!
//! | Route | Method | Handler |
//! | --- | --- | --- |
//! | `…/projects/{project_id}/favorite-pages/{page_id}/` | `POST` | [`create`] |
//! | the same | `DELETE` | [`destroy`] |
//!
//! Both methods carry `@allow_permission([ROLE.ADMIN, ROLE.MEMBER])` with the
//! default `PROJECT` level (`base.py:470`, `:482`), so the runtime has already
//! answered 403 for a guest, an inactive member and a non-member before either
//! handler runs. Unlike the module and view favourites, the entity id is the
//! URL's `page_id`, not `request.data`: neither handler parses the body.
//!
//! ## create (`base.py:470-480`)
//!
//! 1. `get_favorite_entities("page", request.user.id, project_id).get(
//!    pk=page_id, workspace__slug=slug)`
//!    (`plane/app/serializers/favorite.py:70-104`): a live page linked to the
//!    project through a live `ProjectPage`, whose caller is an active member of
//!    the unarchived project, and which is either owned by the caller or
//!    `access == 0` (public) and not restricted from the caller as a
//!    restricted project guest. This is the visibility gate **and** the 404: a
//!    missing page, a page linked only to another project, a private page of
//!    someone else and a page whose link is soft-deleted all answer here. The
//!    page's own `archived_at` is not part of the gate — an archived page is
//!    favouritable, like an archived view against 183.
//! 2. `UserFavorite.objects.create(project_id=…, entity_identifier=page_id,
//!    entity_type="page", user=…)` — no `workspace`:
//!    `WorkspaceBaseModel.save` copies it from the project
//!    (`plane/db/models/workspace.py:192-195`); H25 sets `sequence` from
//!    `MAX(sequence)` over every live favourite of that workspace
//!    (`plane/db/models/favorite.py:52-64`); `created_by` is the caller (H01).
//! 3. `204`, empty body.
//!
//! There is no uniqueness guard: the partial unique constraint
//! `(entity_type, entity_identifier, user) WHERE deleted_at IS NULL` makes a
//! second `POST` an `IntegrityError`, which `BaseViewSet.handle_exception`
//! renders as `400 {"error": "The payload is not valid"}`
//! (`plane/app/views/base.py:302-312`).
//!
//! ## destroy (`base.py:482-492`)
//!
//! `UserFavorite.objects.get(project=…, user=…, workspace__slug=…,
//! entity_identifier=page_id, entity_type="page")` → a miss is 404; then
//! `page_favorite.delete(soft=False)`. That is **not** a bare
//! `DELETE … WHERE id = $1`: `SoftDeleteModel.delete` calls
//! `super().delete()` (`plane/db/mixins.py:72-82`), i.e. Django's deletion
//! collector, and `UserFavorite.parent` is a self-referential
//! `on_delete=CASCADE` FK (`plane/db/models/favorite.py:25-31`). The collector
//! therefore also deletes every row reachable through `parent_id` — at every
//! depth, and including rows whose `deleted_at` is set, because the cascade
//! relation is read through the related model's `_base_manager` (a plain
//! manager, `django/db/models/deletion.py:398-410`). [`write::favorite`] is the
//! shared port of that collector; its module doc shows the emitted SQL.

use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use super::queries;
use crate::common::{self, jobs};
use crate::write;

/// `PageFavoriteViewSet.create` (`base.py:470-480`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = super::base::uuid_kwarg(ctx, "project_id")?;
        let page_id = super::base::uuid_kwarg(ctx, "page_id")?;
        let user = jobs::user_id(ctx)?;

        // `get_favorite_entities("page", …).get(pk=page_id, workspace__slug=…)`.
        let statement = queries::PAGES_FAVORITE_GATE.prepare(client).await?;
        let visible: bool = client
            .query_one(&statement, &[&project_id, &page_id, &slug, &user])
            .await?
            .get(0);
        if !visible {
            return Err(ViewError::ObjectDoesNotExist);
        }

        // `UserFavorite.objects.create(...)` — `WorkspaceBaseModel.save` reads
        // `self.project` through the base manager, then H25.
        let statement = queries::PAGES_FAVORITE_PROJECT_WORKSPACE
            .prepare(client)
            .await?;
        let workspace_id: Uuid = client
            .query_opt(&statement, &[&project_id])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?
            .get(0);
        let sequence = hooks::favorite::before_insert(client, workspace_id, None).await?;

        let now = write::now();
        let id = Uuid::new_v4();
        let statement = queries::PAGES_FAVORITE_INSERT.prepare(client).await?;
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
                    &page_id,
                    &sequence,
                ],
            )
            .await
            .map_err(integrity)?;

        Ok(ViewResponse::empty(204))
    })
}

/// `PageFavoriteViewSet.destroy` (`base.py:482-492`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = super::base::uuid_kwarg(ctx, "project_id")?;
        let page_id = super::base::uuid_kwarg(ctx, "page_id")?;
        let user = jobs::user_id(ctx)?;

        // `UserFavorite.objects.get(...)` — the manager's `deleted_at IS NULL`
        // applies, so a soft-deleted favourite is a miss (404).
        let statement = queries::PAGES_FAVORITE_LOOKUP.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&project_id, &user, &slug, &page_id])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let id: Uuid = row.get(0);

        // `delete(soft=False)` → Django's deletion collector over the
        // self-referential `parent` CASCADE (`django/db/models/deletion.py`),
        // shared with module favourites (111) and view favourites (184).
        write::favorite::hard_delete(client, id).await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `IntegrityError` → `BaseViewSet.handle_exception`'s
/// `400 {"error": "The payload is not valid"}` (`plane/app/views/base.py:306-310`).
fn integrity(error: tokio_postgres::Error) -> ViewError {
    if error.code() == Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION) {
        ViewError::Integrity
    } else {
        ViewError::Unknown(error.to_string())
    }
}
