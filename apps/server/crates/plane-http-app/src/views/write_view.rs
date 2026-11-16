// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The saved-view writes (`plane/app/views/view/base.py`), routes 178-181.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/projects/{project_id}/views/` | 178 [`issue_create`] | `base.py:295-303` |
//! | `PATCH …/projects/{project_id}/views/{pk}/` | 179 [`issue_partial_update`] | `base.py:385-405` |
//! | `DELETE …/projects/{project_id}/views/{pk}/` | 179 [`issue_destroy`] | `base.py:407-440` |
//! | `PUT` on either `{pk}` route | 179/181 — the guard 405 | `ROUTES.md:630-631` |
//! | `POST …/views/` | 180 [`workspace_create`] | `base.py:57-65` |
//! | `PATCH …/views/{pk}/` | 181 [`workspace_partial_update`] | `base.py:93-113` |
//! | `DELETE …/views/{pk}/` | 181 [`workspace_destroy`] | `base.py:149-170` |
//!
//! The writes are H24 (`IssueView.save`) plus H01, and the PATCH path is the
//! only one that opens a transaction (`with transaction.atomic():`,
//! `base.py:95`, `:387`). Its two 400 branches `return` **inside** the block, so
//! the transaction commits; only the `.get()` `DoesNotExist` leaves the block
//! and rolls back.
//!
//! Ported upstream quirks:
//!
//! * the workspace routes look the row up by `(pk, workspace__slug)` only, so a
//!   **project** view can be patched or destroyed through `/views/{pk}/`
//!   (`base.py:96`, `:151`);
//! * the decorator's creator test is `created_by` while the handler's is
//!   `owned_by` (`base.py:98-106`, `:390-398`), so an owner who did not create
//!   the view passes the decorator and gets `400 {"error": …}` from the
//!   handler;
//! * destroy deletes favourites for **every** user (no `user=` filter) and the
//!   project destroy hard-deletes every user's recent visit, while the
//!   workspace destroy leaves recent visits alone (`base.py:159-164`,
//!   `:422-434`);
//! * `deleted_at` is writable (`serializers/view.py:63-72` omits it from
//!   `read_only_fields`), so a create can insert a pre-deleted view.

use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use crate::common::{client, jobs, lookup};
use crate::views::base::Row;
use crate::views::write_view_queries as queries;
use crate::write::generated::issue_view_serializer;
use crate::write::{self, Serializer, Validated};

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// The model defaults for `filters`, `rich_filters`, `logo_props` and the
/// recomputed `query` when `filters` is falsy — all `{}`.
fn empty_object() -> JsonText {
    JsonText::new("{}")
}

/// The `display_filters` model default (`plane/db/models/view.py:28-37`), in
/// the literal's key order: the create response renders the in-memory dict, so
/// the order is part of the bytes.
const DEFAULT_DISPLAY_FILTERS: &str = concat!(
    "{\"group_by\": null, \"order_by\": \"-created_at\", \"type\": null, ",
    "\"sub_issue\": true, \"show_empty_groups\": true, \"layout\": \"list\", ",
    "\"calendar_date_range\": \"\"}",
);

/// The `display_properties` model default (`plane/db/models/view.py:40-55`).
const DEFAULT_DISPLAY_PROPERTIES: &str = concat!(
    "{\"assignee\": true, \"attachment_count\": true, \"created_on\": true, ",
    "\"due_date\": true, \"estimate\": true, \"key\": true, \"labels\": true, ",
    "\"link\": true, \"priority\": true, \"start_date\": true, \"state\": true, ",
    "\"sub_issue_count\": true, \"updated_on\": true}",
);

fn timezone(ctx: &ViewCtx<'_>) -> Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

/// `json.loads(self.filters)` for H24 — the column keeps any JSON value.
fn filters_json(text: &JsonText) -> Result<Json, ViewError> {
    plane_serde::pyjson::loads(text.as_str().as_bytes(), "utf-8")
        .map_err(|error| ViewError::Unknown(format!("json.loads({text:?}): {error}")))
}

/// H24's first half: `self.query = issue_filters(filters, "POST") if filters
/// else {}` (`plane/db/models/view.py:80-81`).
fn query_from(filters: &Json) -> Result<JsonText, ViewError> {
    if !hooks::issue_view::truthy(filters) {
        return Ok(empty_object());
    }
    hooks::issue_view::query_from_filters(filters, chrono::Utc::now().date_naive())
        .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `Workspace.objects.get(slug=slug)` (`base.py:64`).
async fn workspace_by_slug(client: &Client, slug: &str) -> Result<Uuid, ViewError> {
    let statement = client
        .prepare_cached(queries::VIEW_WORKSPACE_BY_SLUG.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .query_opt(&statement, &[&slug])
        .await
        .map_err(ViewError::from)?
        .map(|row| row.try_get(0).map_err(ViewError::from))
        .transpose()?
        .ok_or(ViewError::ObjectDoesNotExist)
}

/// `ProjectMember.objects.filter(…, role=20, is_active=True).exists()`
/// (`base.py:410-417`).
async fn project_admin_exists(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    user: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::VIEW_PROJECT_ADMIN_EXISTS.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&slug, &project_id, &user])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// `WorkspaceMember.objects.filter(…, role=20, is_active=True).exists()`
/// (`base.py:153-156`).
async fn workspace_admin_exists(
    client: &Client,
    slug: &str,
    user: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::VIEW_WORKSPACE_ADMIN_EXISTS.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&slug, &user])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// The values `ModelSerializer.update` applies to the loaded instance — one
/// column per key the body carried, each independent of the others.
fn apply(row: &mut Row, values: &Validated) {
    if let Some(name) = values.text("name") {
        row.name = name.to_owned();
    }
    if let Some(description) = values.text("description") {
        row.description = description.to_owned();
    }
    if let Some(filters) = values.json("filters") {
        row.filters = filters.clone();
    }
    if let Some(value) = values.json("display_filters") {
        row.display_filters = value.clone();
    }
    if let Some(value) = values.json("display_properties") {
        row.display_properties = value.clone();
    }
    if let Some(value) = values.json("rich_filters") {
        row.rich_filters = value.clone();
    }
    if let Some(sort_order) = values.float("sort_order") {
        row.sort_order = sort_order;
    }
    if let Some(value) = values.json("logo_props") {
        row.logo_props = value.clone();
    }
    if values.contains("archived_at") {
        row.archived_at = values.datetime("archived_at");
    }
    if values.contains("deleted_at") {
        row.deleted_at = values.datetime("deleted_at");
    }
}

/// `Response(serializer.data, status=…)` from the in-memory instance.
fn render_row(row: &Row, timezone: Tz, status: u16) -> Result<ViewResponse, ViewError> {
    let bytes =
        plane_serializers::render(&row.value(), &plane_serializers::RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, bytes))
}

// ---------------------------------------------------------------------------
// Routes 178/180 — create
// ---------------------------------------------------------------------------

/// The shared create body. `project_id` is `Some` for 178 and `None` for 180.
///
/// Statement order (`CreateModelMixin.create` + `perform_create` + H24):
/// body parse → validation → [`workspace_by_slug`] (180 only, `base.py:64`) →
/// `issue_filters` → `MAX(sort_order)` → INSERT. 178's project read happens
/// inside `WorkspaceBaseModel.save`, i.e. **after** H24's aggregate
/// (`plane/db/models/view.py:95`).
async fn create(ctx: &ViewCtx<'_>, project_id: Option<Uuid>) -> Result<ViewResponse, ViewError> {
    let client = client(ctx)?;
    let slug = ctx.kwarg("slug").map(str::to_owned).ok_or(ViewError::Key)?;
    let user = jobs::user_id(ctx)?;
    let timezone = timezone(ctx);

    let mut serializer = Serializer::new(ctx, false)?;
    issue_view_serializer::validate(&mut serializer, client).await?;
    let values = match serializer.finish() {
        Ok(values) => values,
        Err(errors) => return Ok(errors.into_response()),
    };

    // 180's `perform_create` reads the workspace before `serializer.save()`;
    // 178 passes only `project_id` (`base.py:63-65`, `:302-303`).
    let workspace = match project_id {
        Some(_) => None,
        None => Some(workspace_by_slug(client, &slug).await?),
    };

    // H24's first half.
    let filters = values.json("filters").cloned().unwrap_or_else(empty_object);
    let query = query_from(&filters_json(&filters)?)?;

    // H24's second half, then (178 only) `WorkspaceBaseModel.save`'s project
    // read, which is what fills `workspace_id`.
    let requested_sort_order = values.float("sort_order").unwrap_or(65535.0);
    let (workspace_id, project, sort_order) = match (project_id, workspace) {
        (Some(project_id), _) => {
            let sort_order = hooks::issue_view::before_insert(
                client,
                hooks::issue_view::SortScope::Project(project_id),
                requested_sort_order,
            )
            .await
            .map_err(ViewError::from)?;
            let statement = client
                .prepare_cached(queries::VIEW_PROJECT_WORKSPACE.sql)
                .await
                .map_err(ViewError::from)?;
            let workspace_id = client
                .query_opt(&statement, &[&project_id])
                .await
                .map_err(ViewError::from)?
                .map(|row| row.try_get(0).map_err(ViewError::from))
                .transpose()?
                .ok_or_else(|| {
                    // `self.project` on a missing row raises
                    // `Project.DoesNotExist`.
                    ViewError::Unknown("Project.DoesNotExist".to_owned())
                })?;
            (workspace_id, Some(project_id), sort_order)
        }
        (None, Some(workspace_id)) => {
            let sort_order = hooks::issue_view::before_insert(
                client,
                hooks::issue_view::SortScope::Workspace(workspace_id),
                requested_sort_order,
            )
            .await
            .map_err(ViewError::from)?;
            (workspace_id, None, sort_order)
        }
        (None, None) => unreachable!("one of the two routes set the scope"),
    };

    // `IssueView.objects.create(**validated_data, …)`: the model defaults fill
    // every column the body could not set.
    let created_at = write::now();
    let updated_at = write::now();
    let audit = hooks::audit::Audit::on_create(Some(user));
    let row = Row {
        id: Uuid::new_v4(),
        is_favorite: None,
        created_at,
        updated_at,
        deleted_at: values.datetime("deleted_at"),
        name: values
            .text("name")
            .ok_or_else(|| ViewError::Unknown("validated name vanished".to_owned()))?
            .to_owned(),
        description: values.text("description").unwrap_or_default().to_owned(),
        query,
        filters,
        display_filters: values
            .json("display_filters")
            .cloned()
            .unwrap_or_else(|| JsonText::new(DEFAULT_DISPLAY_FILTERS)),
        display_properties: values
            .json("display_properties")
            .cloned()
            .unwrap_or_else(|| JsonText::new(DEFAULT_DISPLAY_PROPERTIES)),
        rich_filters: values
            .json("rich_filters")
            .cloned()
            .unwrap_or_else(empty_object),
        access: 1,
        sort_order,
        logo_props: values
            .json("logo_props")
            .cloned()
            .unwrap_or_else(empty_object),
        is_locked: false,
        archived_at: values.datetime("archived_at"),
        created_by: audit.created_by_id,
        updated_by: audit.updated_by_id,
        workspace: workspace_id,
        project,
        owned_by: user,
    };
    client
        .execute(
            queries::VIEW_INSERT.sql,
            &[
                &row.created_at,
                &row.updated_at,
                &row.created_by,
                &row.updated_by,
                &row.deleted_at,
                &row.id,
                &row.workspace,
                &row.project,
                &row.name,
                &row.description,
                &row.query,
                &row.filters,
                &row.display_filters,
                &row.display_properties,
                &row.rich_filters,
                &row.access,
                &row.sort_order,
                &row.logo_props,
                &row.owned_by,
                &row.is_locked,
                &row.archived_at,
            ],
        )
        .await
        .map_err(ViewError::from)?;

    render_row(&row, timezone, 201)
}

/// `IssueViewViewSet.create` (`base.py:295-303`).
pub fn issue_create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        create(ctx, Some(project_id)).await
    })
}

/// `WorkspaceViewViewSet.create` (`base.py:57-65`).
pub fn workspace_create<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(create(ctx, None))
}

// ---------------------------------------------------------------------------
// Routes 179/181 — partial_update
// ---------------------------------------------------------------------------

/// The shared PATCH body. `project_id` is `Some` for 179 and `None` for 181.
///
/// `with transaction.atomic():` wraps the row lookup, both 400 guards and the
/// validation: the guards and a validation error `return` inside the block and
/// therefore commit, while a missing row raises out of it and rolls back.
async fn partial_update(
    ctx: &ViewCtx<'_>,
    project_id: Option<Uuid>,
) -> Result<ViewResponse, ViewError> {
    let client = client(ctx)?;
    let slug = ctx.kwarg("slug").map(str::to_owned).ok_or(ViewError::Key)?;
    let user = jobs::user_id(ctx)?;
    let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
    let timezone = timezone(ctx);

    // 179 adds the project predicate to the lookup (`base.py:388`); 181 does
    // not, which is what lets it reach a project view (`base.py:96`).
    let row_sql = if project_id.is_some() {
        queries::VIEW_PATCH_PROJECT_ROW.sql
    } else {
        queries::VIEW_PATCH_ROW.sql
    };
    let row_statement = client
        .prepare_cached(row_sql)
        .await
        .map_err(ViewError::from)?;
    let project_statement = client
        .prepare_cached(queries::VIEW_PROJECT_WORKSPACE.sql)
        .await
        .map_err(ViewError::from)?;

    // `client.transaction()` needs `&mut Client`, which `ViewCtx::db` does not
    // offer; `BEGIN`/`COMMIT`/`ROLLBACK` on the same connection is the shape
    // `hooks::issue` already uses, and it is still one connection and one
    // transaction.
    client
        .batch_execute("BEGIN")
        .await
        .map_err(ViewError::from)?;
    let statements = PatchStatements {
        row: &row_statement,
        project: &project_statement,
    };
    let params = PatchParams {
        slug: &slug,
        project_id,
        user,
        pk,
        timezone,
    };
    match locked_partial_update(ctx, client, &statements, &params).await {
        Ok(response) => {
            client
                .batch_execute("COMMIT")
                .await
                .map_err(ViewError::from)?;
            Ok(response)
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(error)
        }
    }
}

/// The statements the PATCH path prepared before `BEGIN`.
struct PatchStatements<'a> {
    row: &'a tokio_postgres::Statement,
    project: &'a tokio_postgres::Statement,
}

/// The URL and user values the `atomic` block works on.
struct PatchParams<'a> {
    slug: &'a str,
    project_id: Option<Uuid>,
    user: Uuid,
    pk: Uuid,
    timezone: Tz,
}

/// The body of the `atomic` block. `Ok` is a response Django returns *inside*
/// the block (so the caller commits); `Err` leaves it (so the caller rolls
/// back).
async fn locked_partial_update(
    ctx: &ViewCtx<'_>,
    client: &Client,
    statements: &PatchStatements<'_>,
    params: &PatchParams<'_>,
) -> Result<ViewResponse, ViewError> {
    let PatchParams {
        slug,
        project_id,
        user,
        pk,
        timezone,
    } = *params;
    let PatchStatements {
        row: row_statement,
        project: project_statement,
    } = *statements;
    // `IssueView.objects.select_for_update().get(pk=pk, workspace__slug=slug)`
    // — plus `project_id=project_id` on 179.
    let stored = match project_id {
        Some(project_id) => {
            client
                .query_opt(row_statement, &[&pk, &project_id, &slug])
                .await
        }
        None => client.query_opt(row_statement, &[&pk, &slug]).await,
    }
    .map_err(ViewError::from)?;
    let Some(stored) = stored else {
        // `DoesNotExist` leaves the `atomic` block: the caller rolls back.
        return Err(ViewError::ObjectDoesNotExist);
    };
    let mut row = Row::decode(&stored, false)?;

    if row.is_locked {
        return Ok(write::error_body(400, "view is locked"));
    }
    if row.owned_by != user {
        return Ok(write::error_body(
            400,
            "Only the owner of the view can update the view",
        ));
    }

    let mut serializer = Serializer::new(ctx, true)?;
    issue_view_serializer::validate(&mut serializer, client).await?;
    let values = match serializer.finish() {
        Ok(values) => values,
        Err(errors) => return Ok(errors.into_response()),
    };
    apply(&mut row, &values);

    // H24: the save recomputes `query` from the (possibly new) `filters`.
    let filters = filters_json(&row.filters)?;
    row.query = query_from(&filters)?;

    // `WorkspaceBaseModel.save` re-derives `workspace` from the row's project.
    if let Some(project_id) = row.project {
        row.workspace = client
            .query_opt(project_statement, &[&project_id])
            .await
            .map_err(ViewError::from)?
            .map(|row| row.try_get(0).map_err(ViewError::from))
            .transpose()?
            .ok_or_else(|| ViewError::Unknown("Project.DoesNotExist".to_owned()))?;
    }

    // H01 + `auto_now`.
    row.updated_by = Some(user);
    row.updated_at = write::now();

    client
        .execute(
            queries::VIEW_UPDATE.sql,
            &[
                &row.created_at,
                &row.updated_at,
                &row.created_by,
                &row.updated_by,
                &row.deleted_at,
                &row.workspace,
                &row.project,
                &row.name,
                &row.description,
                &row.query,
                &row.filters,
                &row.display_filters,
                &row.display_properties,
                &row.rich_filters,
                &row.access,
                &row.sort_order,
                &row.logo_props,
                &row.owned_by,
                &row.is_locked,
                &row.archived_at,
                &row.id,
            ],
        )
        .await
        .map_err(ViewError::from)?;

    render_row(&row, timezone, 200)
}

/// `IssueViewViewSet.partial_update` (`base.py:385-405`).
pub fn issue_partial_update<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        partial_update(ctx, Some(project_id)).await
    })
}

/// `WorkspaceViewViewSet.partial_update` (`base.py:93-113`).
pub fn workspace_partial_update<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(partial_update(ctx, None))
}

// ---------------------------------------------------------------------------
// Routes 179/181 — destroy
// ---------------------------------------------------------------------------

/// The shared destroy body. `project_id` is `Some` for 179 and `None` for 181.
///
/// Statement order: `.get()` (404) → the role `exists()` → H03's full save (the
/// recomputed `query` included) → the cascade task → the favourites H04 (all
/// users) → (179 only) the recent visits hard delete.
async fn destroy(ctx: &ViewCtx<'_>, project_id: Option<Uuid>) -> Result<ViewResponse, ViewError> {
    let client = client(ctx)?;
    let slug = ctx.kwarg("slug").map(str::to_owned).ok_or(ViewError::Key)?;
    let user = jobs::user_id(ctx)?;
    let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;

    let lookup_sql = if project_id.is_some() {
        queries::VIEW_DESTROY_PROJECT_LOOKUP.sql
    } else {
        queries::VIEW_DESTROY_WORKSPACE_LOOKUP.sql
    };
    let statement = client
        .prepare_cached(lookup_sql)
        .await
        .map_err(ViewError::from)?;
    let stored = match project_id {
        Some(project_id) => {
            client
                .query_opt(&statement, &[&pk, &project_id, &slug])
                .await
        }
        None => client.query_opt(&statement, &[&pk, &slug]).await,
    }
    .map_err(ViewError::from)?;
    let Some(stored) = stored else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let owned_by: Uuid = stored.try_get(0).map_err(ViewError::from)?;
    let filters: JsonText = stored.try_get(1).map_err(ViewError::from)?;

    let is_admin = match project_id {
        Some(project_id) => project_admin_exists(client, &slug, project_id, user).await?,
        None => workspace_admin_exists(client, &slug, user).await?,
    };
    if !is_admin && owned_by != user {
        return Ok(write::error_body(
            400,
            "Only admin or owner can delete the view",
        ));
    }

    // `delete()` → `self.deleted_at = timezone.now(); self.save()`: H24
    // recomputes `query`, H01 sets `updated_by`, `auto_now` calls the clock a
    // second time.
    let query = query_from(&filters_json(&filters)?)?;
    let deleted_at = write::now();
    let updated_at = write::now();
    client
        .execute(
            queries::VIEW_SOFT_DELETE.sql,
            &[&query, &deleted_at, &updated_at, &Some(user), &pk],
        )
        .await
        .map_err(ViewError::from)?;
    write::publish_soft_delete_cascade(ctx, "db", "issueview", pk).await?;

    match project_id {
        Some(project_id) => {
            // H04 for every user; the H04 timestamp is one `timezone.now()`.
            client
                .execute(
                    queries::VIEW_FAVORITE_SOFT_DELETE.sql,
                    &[&write::now(), &pk, &project_id, &slug],
                )
                .await
                .map_err(ViewError::from)?;
            // `delete(soft=False)`: a hard DELETE that keeps the manager's
            // `deleted_at IS NULL` predicate.
            client
                .execute(
                    queries::VIEW_RECENT_VISIT_HARD_DELETE.sql,
                    &[&project_id, &pk, &slug],
                )
                .await
                .map_err(ViewError::from)?;
        }
        None => {
            client
                .execute(
                    queries::VIEW_WORKSPACE_FAVORITE_SOFT_DELETE.sql,
                    &[&write::now(), &pk, &slug],
                )
                .await
                .map_err(ViewError::from)?;
        }
    }
    Ok(ViewResponse::empty(204))
}

/// `IssueViewViewSet.destroy` (`base.py:407-440`).
pub fn issue_destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        destroy(ctx, Some(project_id)).await
    })
}

/// `WorkspaceViewViewSet.destroy` (`base.py:149-170`).
pub fn workspace_destroy<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(destroy(ctx, None))
}
