// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ProjectArchiveUnarchiveEndpoint` (`plane/app/views/project/base.py:428-445`).
//!
//! | Route | Method | Handler |
//! | --- | --- | --- |
//! | `…/projects/{project_id}/archive/` | `POST` | [`archive`] |
//! | same | `DELETE` | [`unarchive`] |
//!
//! ## What Django does
//!
//! **archive**:
//!
//! 1. `Project.objects.get(pk=project_id, workspace__slug=slug)` → a miss is
//!    `Project.DoesNotExist` → 404 `{"error": "The required object does not
//!    exist."}`;
//! 2. `project.archived_at = timezone.now()`; `project.save()` — **no**
//!    `update_fields`, so `Model.save()` rewrites every column. H09 runs first
//!    (`Project.save`, `plane/db/models/project.py:193-227`): `identifier` is
//!    stripped and upper-cased (a no-op on a stored row), the create-only
//!    timezone branch is skipped, and the timezone has not changed so there is
//!    no `_retime_intake_snoozes`; H01 then sets `updated_by = request.user`
//!    and `auto_now` sets `updated_at`;
//! 3. `UserFavorite.objects.filter(workspace__slug=slug, project=project_id)
//!    .delete()` — H04: `SoftDeletionQuerySet.delete` is `self.update(
//!    deleted_at=now)`, so it is **one bare `UPDATE`**: no `updated_at`, no
//!    `updated_by`, no `soft_delete_related_objects` task;
//! 4. `Response({"archived_at": str(project.archived_at)}, 200)` — `str()` of a
//!    datetime, i.e. `isoformat(" ")`.
//!
//! **unarchive**: the same first two steps with `archived_at = None`, then
//! `204` with no body and no favourites sweep.
//!
//! ## Statements
//!
//! | Action | Django | Here |
//! | --- | ---: | ---: |
//! | archive | 3 (SELECT, UPDATE project, UPDATE favourites) | 3 |
//! | unarchive | 2 (SELECT, UPDATE project) | 2 |

use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use crate::common::client;
use crate::projects::write_queries as queries;
use crate::projects::write_support::{self, ProjectRow};
use crate::write;

/// Register the endpoint's two unsafe methods.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.project.base.ProjectArchiveUnarchiveEndpoint")?
        .method(http::Method::POST, archive)?
        .method(http::Method::DELETE, unarchive)?;
    Ok(())
}

/// `ProjectArchiveUnarchiveEndpoint.post` (`base.py:429-435`).
pub fn archive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, user) = write_support::target(ctx)?;
        let client = client(ctx)?;

        let mut project = load_project(client, project_id, slug).await?;
        let archived_at = write::now();
        project.archived_at = Some(archived_at);
        stamp_and_save(client, &mut project, user).await?;

        // H04: the queryset delete is one `UPDATE … SET deleted_at`, with no
        // `updated_at` and no cascade task (`plane/db/mixins.py:47-52`).
        client
            .execute(
                queries::PROJECT_FAVORITES_SOFT_DELETE.sql,
                &[&project_id, &archived_at, &slug],
            )
            .await
            .map_err(write_support::integrity)?;

        Ok(write_support::archived_at_body(archived_at))
    })
}

/// `ProjectArchiveUnarchiveEndpoint.delete` (`base.py:437-445`).
pub fn unarchive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, user) = write_support::target(ctx)?;
        let client = client(ctx)?;

        let mut project = load_project(client, project_id, slug).await?;
        project.archived_at = None;
        stamp_and_save(client, &mut project, user).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `Project.objects.get(pk=project_id, workspace__slug=slug)` plus H09's
/// identifier normalisation (`plane/db/models/project.py:196`).
async fn load_project(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    slug: &str,
) -> Result<ProjectRow, ViewError> {
    let row = client
        .query_opt(
            queries::PROJECT_BY_ID_IN_WORKSPACE.sql,
            &[&project_id, &slug],
        )
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    let mut project = ProjectRow::from_row(&row, 0)?;
    project.identifier = hooks::project::before_update(&project.identifier);
    Ok(project)
}

/// `project.save()`: H01 (`updated_by = user`, `auto_now` re-stamps
/// `updated_at`) then the full-row `UPDATE` the ORM emits.
async fn stamp_and_save(
    client: &deadpool_postgres::Client,
    project: &mut ProjectRow,
    user: Uuid,
) -> Result<(), ViewError> {
    project.updated_at = write::now();
    project.updated_by_id = Some(user);
    client
        .execute(queries::PROJECT_UPDATE.sql, &project.update_args())
        .await
        .map_err(write_support::integrity)?;
    Ok(())
}
