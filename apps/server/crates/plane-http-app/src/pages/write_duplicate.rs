// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `PageDuplicateEndpoint.post` — route 133,
//! `POST /api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/duplicate/`
//! (`plane/app/views/page/base.py:575-636`).
//!
//! The whole handler is one `Page.objects.get` → `page.save()` → loop of
//! `ProjectPage.objects.create` → two task publishes → an annotated re-fetch,
//! with **no** `transaction.atomic` anywhere: a failure after the first INSERT
//! leaves the copied page behind, exactly as in Python.
//!
//! ## What the full `save()` writes (`page/base.py:593-599`)
//!
//! `page.pk = None` makes the next save an INSERT of every column. Nothing else
//! is reset, so the copy keeps the source page's `workspace`, `access`,
//! `color`, `parent`, `archived_at`, `is_locked`, `view_props`, `logo_props`,
//! `is_global`, `moved_to_page`, `moved_to_project`, `sort_order`,
//! `external_id`/`external_source` and both descriptions; `name` gains
//! `" (Copy)"`, `description_binary` becomes NULL, and `owned_by`,
//! `created_by`, `updated_by` become the caller.
//!
//! `created_at`/`updated_at` are fresh because `auto_now_add`/`auto_now` fire on
//! the INSERT, and `description_stripped` is H26's recomputation from the copied
//! HTML. H01 takes its **update** branch (`self._state.adding` is still False on
//! an instance that came from the database, even with `pk = None`), so
//! `updated_by` keeps the value the view assigned — the caller — rather than the
//! `None` H01 would write for a brand-new instance.
//!
//! ## The `project_id` of the copy task
//!
//! `page/base.py:591` binds `project_ids` before the loop, but the queryset is
//! lazy; the `for` statement evaluates it after `page.save()`. The loop rebinds
//! the function's `project_id`, so `copy_s3_objects…delay(project_id=…)`
//! receives the **last** live link under `Meta.ordering = ("-created_at",)`,
//! i.e. the oldest link — not the URL project (`TASKS.md:604` is wrong). With no
//! live link the loop never runs and the URL kwarg would survive; the lookup
//! above cannot produce that case because it requires a live link to the URL
//! project.
//!
//! ## The response
//!
//! The re-fetch annotates `project_ids` only (`page/base.py:625-634`), so the
//! serializer skips `is_favorite` and `label_ids` and the body has 17 keys
//! ([`PageDetailBody::duplicated`]).
//!
//! ## Permission
//!
//! `ProjectPagePermission` (`plane/app/permissions/page.py:18-143`) runs in the
//! pipeline before the handler, and already refuses a private page to everyone
//! but its owner (`:67-68`, `:98-103`). The view's own check
//! (`page/base.py:587-588`) is therefore unreachable in practice, but it is
//! ported as written: a non-owner of a private page would get
//! `403 {"error": "Permission denied"}` instead of the runtime's 403 detail.

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::task_name;
use plane_jobs::celery::tasks;
use plane_jobs::queue::Job;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::common::{client, jobs};
use crate::pages::base::{self, PageDetailBody, PageRow};
use crate::pages::write_duplicate_queries as queries;
use crate::write;

/// The columns [`queries::PAGE_DUPLICATE_LOOKUP`] reads off the source page.
///
/// The audit columns and `created_at`/`updated_at` are absent because the INSERT
/// replaces them; `description_binary` is absent because the view clears it.
#[derive(Debug)]
struct Source {
    name: String,
    workspace_id: Uuid,
    description_json: JsonText,
    description_html: String,
    owned_by_id: Uuid,
    access: i16,
    color: String,
    parent_id: Option<Uuid>,
    archived_at: Option<NaiveDate>,
    is_locked: bool,
    view_props: JsonText,
    logo_props: JsonText,
    is_global: bool,
    moved_to_page: Option<Uuid>,
    moved_to_project: Option<Uuid>,
    sort_order: f64,
    external_id: Option<String>,
    external_source: Option<String>,
}

impl Source {
    fn decode(row: &Row) -> Result<Self, ViewError> {
        Ok(Self {
            name: row.try_get(0).map_err(ViewError::from)?,
            workspace_id: row.try_get(1).map_err(ViewError::from)?,
            description_json: row.try_get(2).map_err(ViewError::from)?,
            description_html: row.try_get(3).map_err(ViewError::from)?,
            owned_by_id: row.try_get(4).map_err(ViewError::from)?,
            access: row.try_get(5).map_err(ViewError::from)?,
            color: row.try_get(6).map_err(ViewError::from)?,
            parent_id: row.try_get(7).map_err(ViewError::from)?,
            archived_at: row.try_get(8).map_err(ViewError::from)?,
            is_locked: row.try_get(9).map_err(ViewError::from)?,
            view_props: row.try_get(10).map_err(ViewError::from)?,
            logo_props: row.try_get(11).map_err(ViewError::from)?,
            is_global: row.try_get(12).map_err(ViewError::from)?,
            moved_to_page: row.try_get(13).map_err(ViewError::from)?,
            moved_to_project: row.try_get(14).map_err(ViewError::from)?,
            sort_order: row.try_get(15).map_err(ViewError::from)?,
            external_id: row.try_get(16).map_err(ViewError::from)?,
            external_source: row.try_get(17).map_err(ViewError::from)?,
        })
    }
}

/// `Page.objects.get(...)` (`page/base.py:579-584`): one row or the 404 the
/// family handler renders as `{"error": "The required object does not exist."}`.
async fn source_page(
    client: &Client,
    page_id: Uuid,
    project_id: Uuid,
    slug: &str,
) -> Result<Source, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_DUPLICATE_LOOKUP.sql)
        .await?;
    let stored = client
        .query_opt(&statement, &[&page_id, &project_id, &slug])
        .await?;
    match stored {
        Some(row) => Source::decode(&row),
        None => Err(ViewError::ObjectDoesNotExist),
    }
}

/// `page.save()` (`page/base.py:593-599`): H26 + `auto_now*` + H01's update
/// branch, then the INSERT.
async fn insert_page(
    client: &Client,
    new_id: Uuid,
    name: String,
    source: &Source,
    user: Uuid,
) -> Result<(), ViewError> {
    // `auto_now_add` then `auto_now`, in field declaration order
    // (`plane/db/models/base.py` inherits `TimeAuditModel`).
    let created_at = write::now();
    let updated_at = write::now();
    // H26: `Page.save` recomputes `description_stripped` from the copied HTML.
    let description_stripped = hooks::page::before_save(Some(&source.description_html));
    // H01's update branch: `updated_by` stays the caller the view assigned.
    let created_by = Some(user);
    let updated_by = Some(user);
    // `page.description_binary = None` (`page/base.py:595`).
    let description_binary: Option<Vec<u8>> = None;
    let deleted_at: Option<DateTime<Utc>> = None;

    let statement = client
        .prepare_cached(queries::PAGE_DUPLICATE_PAGE_INSERT.sql)
        .await?;
    client
        .execute(
            &statement,
            &[
                &created_at,
                &updated_at,
                &created_by,
                &updated_by,
                &deleted_at,
                &new_id,
                &source.workspace_id,
                &name,
                &source.description_json,
                &description_binary,
                &source.description_html,
                &description_stripped,
                // `page.owned_by = request.user` (`page/base.py:596`).
                &user,
                &source.access,
                &source.color,
                &source.parent_id,
                &source.archived_at,
                &source.is_locked,
                &source.view_props,
                &source.logo_props,
                &source.is_global,
                &source.moved_to_page,
                &source.moved_to_project,
                &source.sort_order,
                &source.external_id,
                &source.external_source,
            ],
        )
        .await?;
    Ok(())
}

/// The `for project_id in project_ids:` loop (`page/base.py:601-608`).
///
/// Returns the loop variable for the `copy_s3_objects…` publish. It is the URL
/// project only when the queryset is empty (`page/base.py:620`); the lookup
/// guarantees at least one live link, so that arm exists to mirror the Python,
/// not because it is reachable.
async fn copy_project_links(
    client: &Client,
    page_id: Uuid,
    new_id: Uuid,
    source: &Source,
    url_project_id: Uuid,
    user: Uuid,
) -> Result<Uuid, ViewError> {
    // The `values_list` queryset evaluates here, after the page INSERT
    // (`page/base.py:591,601`).
    let statement = client
        .prepare_cached(queries::PAGE_DUPLICATE_LINK_IDS.sql)
        .await?;
    let rows = client.query(&statement, &[&page_id]).await?;

    let insert = client
        .prepare_cached(queries::PAGE_DUPLICATE_LINK_INSERT.sql)
        .await?;
    let mut loop_project_id = url_project_id;
    for row in &rows {
        let project_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        loop_project_id = project_id;
        let created_at = write::now();
        let updated_at = write::now();
        let id = Uuid::new_v4();
        client
            .execute(
                &insert,
                &[
                    &created_at,
                    &updated_at,
                    // H01's create branch: `created_by` is the request user,
                    // `updated_by` is NULL.
                    &Some(user),
                    &id,
                    &project_id,
                    &new_id,
                    &source.workspace_id,
                ],
            )
            .await?;
    }
    Ok(loop_project_id)
}

/// Build a [`PageRow`] from [`queries::PAGE_DUPLICATE_REREAD`].
///
/// `is_favorite`/`label_ids` are the two annotations the re-fetch does not
/// produce; [`PageDetailBody::duplicated`] marks them `SkipField`, so their
/// values are never rendered.
fn reread_row(row: &Row) -> Result<PageRow, ViewError> {
    Ok(PageRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        name: row.try_get(1).map_err(ViewError::from)?,
        owned_by_id: row.try_get(2).map_err(ViewError::from)?,
        access: row.try_get(3).map_err(ViewError::from)?,
        color: row.try_get(4).map_err(ViewError::from)?,
        parent_id: row.try_get(5).map_err(ViewError::from)?,
        is_favorite: false,
        is_locked: row.try_get(6).map_err(ViewError::from)?,
        archived_at: row.try_get(7).map_err(ViewError::from)?,
        workspace_id: row.try_get(8).map_err(ViewError::from)?,
        created_at: row.try_get(9).map_err(ViewError::from)?,
        updated_at: row.try_get(10).map_err(ViewError::from)?,
        created_by_id: row.try_get(11).map_err(ViewError::from)?,
        updated_by_id: row.try_get(12).map_err(ViewError::from)?,
        view_props: row.try_get(13).map_err(ViewError::from)?,
        logo_props: row.try_get(14).map_err(ViewError::from)?,
        description_html: row.try_get(15).map_err(ViewError::from)?,
        label_ids: Vec::new(),
        project_ids: row.try_get(16).map_err(ViewError::from)?,
    })
}

/// The annotated re-fetch plus `PageDetailSerializer(page).data`
/// (`page/base.py:625-636`).
async fn response_body(
    ctx: &ViewCtx<'_>,
    client: &Client,
    new_id: Uuid,
) -> Result<ViewResponse, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_DUPLICATE_REREAD.sql)
        .await?;
    let Some(row) = client.query_opt(&statement, &[&new_id]).await? else {
        // `.first()` returned `None`: Django would build
        // `PageDetailSerializer(None).data`, but the row was just inserted and
        // the re-read has no visibility window in autocommit.
        return Err(ViewError::Unknown(
            "Page duplicate re-read found no row".to_owned(),
        ));
    };
    let page = reread_row(&row)?;
    let timezone = base::timezone(ctx);
    let body = PageDetailBody::duplicated(&page);
    let bytes = plane_serializers::render(&body, &plane_serializers::RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(201, APPLICATION_JSON, bytes))
}

/// `PageDuplicateEndpoint.post` (`plane/app/views/page/base.py:578-636`).
pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = base::uuid_kwarg(ctx, "project_id")?;
        let page_id = base::uuid_kwarg(ctx, "page_id")?;
        let user = jobs::user_id(ctx)?;

        let source = source_page(client, page_id, project_id, &slug).await?;

        // `if page.access == Page.PRIVATE_ACCESS and page.owned_by_id !=
        // request.user.id` (`page/base.py:587-588`).
        if source.access == 1 && source.owned_by_id != user {
            return Ok(write::error_body(403, "Permission denied"));
        }

        let new_id = Uuid::new_v4();
        let name = format!("{} (Copy)", source.name);
        insert_page(client, new_id, name, &source, user).await?;

        // `ProjectPage.objects.create` per live link; the loop variable is what
        // the copy task receives (`page/base.py:601-608,620`).
        let copy_project_id =
            copy_project_links(client, page_id, new_id, &source, project_id, user).await?;

        // `page_transaction.delay(new_description_html=page.description_html,
        // old_description_html=None, page_id=page.id)` (`page/base.py:610-614`).
        let task = task_name::PAGE_TRANSACTION;
        let message =
            tasks::page_transaction(source.description_html.clone(), None, TaskArg::uuid(new_id));
        jobs::dispatch_task(ctx, task, &message, Job::from_message(&message)).await?;

        // `copy_s3_objects_of_description_and_assets.delay(entity_name="PAGE",
        // entity_identifier=page.id, project_id=project_id, slug=slug,
        // user_id=request.user.id)` (`page/base.py:617-623`).
        let task = task_name::COPY_S3_OBJECTS;
        let message = tasks::copy_s3_objects_of_description_and_assets(
            "PAGE".to_owned(),
            new_id,
            copy_project_id,
            slug,
            user,
        );
        jobs::dispatch_task(ctx, task, &message, Job::from_message(&message)).await?;

        response_body(ctx, client, new_id).await
    })
}
