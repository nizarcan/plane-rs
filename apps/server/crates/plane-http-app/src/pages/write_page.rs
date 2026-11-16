// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The page-core writes (`plane/app/views/page/base.py`), routes 124, 125 and
//! 127.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/projects/{project_id}/pages/` | 124 [`create`] | `page/base.py:145-168` + `serializers/page.py:61-106` |
//! | `PATCH …/pages/{page_id}/` | 125 [`partial_update`] | `page/base.py:170-216` + `serializers/page.py:108-126` |
//! | `DELETE …/pages/{page_id}/` | 125 [`destroy`] | `page/base.py:363-414` |
//! | `POST …/pages/{page_id}/archive/` | 127 [`archive`] | `page/base.py:303-332` |
//! | `DELETE …/pages/{page_id}/archive/` | 127 [`unarchive`] | `page/base.py:334-361` |
//!
//! None of the five views opens `transaction.atomic()`, so nothing here does
//! either: the order of the statements is the behaviour. The quirks worth
//! naming, all ported as they are:
//!
//! * **create can answer 404 after writing.** Its response comes from
//!   `get_queryset().get(pk=…)` (`page/base.py:165`), which filters
//!   `parent__isnull=True` and applies the request's `?search=` terms
//!   (`plane/app/views/base.py:72`). A POST with a `parent`, or with a
//!   `?search=` the new page does not match, writes the page, the project link
//!   and the labels, publishes the transaction task and then misses its own row
//!   → `404 {"error": "The required object does not exist."}`.
//! * `description_html: null` is not validated — the view reads it raw
//!   (`page/base.py:153`) — and the column is `NOT NULL`
//!   (`models/page.py:34`), so the INSERT raises `IntegrityError` and the
//!   family handler answers `400 {"error": "The payload is not valid"}`
//!   (`plane/app/views/base.py:209-213`). Nothing before the INSERT has written
//!   a row, so the handler answers that body without running the statement.
//! * the description columns are passed through unvalidated: `description_json:
//!   null` is SQL NULL for a `NOT NULL` column (the same 400), and a string
//!   `description_binary` fails psycopg's `bytea` dumper with
//!   `TypeError: bytes or buffer expected` (a 500), both before the INSERT.
//! * `label_ids`/`project_ids` are writable `ListField`s
//!   (`serializers/page.py:33-34`) that no code pops: on create they reach
//!   `Page.objects.create(**validated_data)` and raise the `TypeError` Django's
//!   `Model.__init__` raises for an unknown keyword → family 500, *after* the
//!   project lookup. On PATCH they reach `ModelSerializer.update`'s
//!   `setattr(instance, …)`, which sets a plain attribute: nothing is written,
//!   but DRF's `get_attribute` now finds it and the response carries the key
//!   (17 or 18 keys instead of 16).
//! * PATCH's `.get()` failure and its bad-`parent` lookup both return
//!   `400 {"error": "Access cannot be updated since this page is owned by
//!   someone else"}` (`page/base.py:212-216`), and `is_locked` is checked
//!   before both (`:179-180`). Only the bad-`parent` case is reachable over
//!   HTTP: `ProjectPagePermission` resolves the same page with the same
//!   predicates before the handler, so a page the `.get()` would miss is
//!   already a 403 (`permissions/page.py:48-55`).
//! * the `access` guard compares `page.access` with
//!   `request.data.get("access", page.access)` — a *string* from a form (or a
//!   `null`) never equals the integer, so such a body takes the owner branch
//!   even when the value is numerically the same (`:192-196`).
//! * `created_by` is writable (`serializers/page.py:59` makes only
//!   `workspace`/`owned_by` read-only) and H01 only overwrites `updated_by`,
//!   so PATCH can rewrite the creator (`models/base.py:35-42`).
//! * `labels` accepts **any** live `Label` row, cross-workspace included
//!   (`serializers/page.py:28`).
//! * destroy keeps the manager predicate on its two queryset deletes
//!   (`deleted_at IS NULL` on favourites and recent visits,
//!   `plane/db/mixins.py:48-58`), and the page itself is H03's full save.
//! * archive's CTE writes `datetime.now()` — a **naive** datetime, UTC in the
//!   reference process because Django sets `TZ` to `TIME_ZONE` when its
//!   settings load (`django/conf/__init__.py`) — into a `DateField` for the
//!   page and every descendant, ignores `deleted_at`, bypasses H26/H01 and does
//!   not touch `updated_at` (`page/base.py:61-75`, `MODEL_HOOKS.md:1759-1761`);
//!   the body re-reads the clock and renders Python `str(datetime.now())`
//!   (`:332`). The favourites delete covers **every** user's row.
//! * unarchive detaches an archived parent with `save(update_fields=["parent"])`
//!   — one column, no `updated_at` (`:355-357`) — and then NULLs the subtree
//!   (`:359`).

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks;
use plane_db::models::page::Page;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_query::json::Json as Doc;
use plane_query::sql::{Sql, Value as SqlValue};
use plane_serde::pyjson::{PyStr, Value as PyJson};
use uuid::Uuid;

use crate::common::client;
use crate::pages::base::{self, PageDetailBody, PageRow, Params};
use crate::pages::queries as read_queries;
use crate::pages::write_page_queries as queries;
use crate::write::generated::{page_detail_serializer, page_serializer};
use crate::write::{self, Serializer};

/// `PageLabel.objects.bulk_create(..., batch_size=10)` (`serializers/page.py:104`).
const LABEL_BATCH_SIZE: usize = 10;

/// `Response({"error": "Access cannot be updated since this page is owned by
/// someone else"})` — PATCH's `DoesNotExist` branch and its explicit guard
/// (`page/base.py:192-196`, `:212-216`).
const NOT_OWNER: &str = "Access cannot be updated since this page is owned by someone else";

/// One raw request value, before DRF's field parsing: JSON bodies keep their
/// type, a form/multipart body is a string (`html.is_html_input`).
#[derive(Debug, Clone, Copy)]
enum Raw<'a> {
    Json(&'a PyJson),
    Text(&'a str),
}

impl<'a> Raw<'a> {
    /// Python truthiness (`if parent:`, `if request.data.get(…)`).
    fn truthy(self) -> bool {
        match self {
            Self::Json(value) => hooks::issue_view::truthy(value),
            Self::Text(text) => !text.is_empty(),
        }
    }
}

/// Python's `type(value).__name__` for the `description_binary` error.
fn python_type_name_of(value: Raw<'_>) -> &'static str {
    match value {
        Raw::Text(_) => "str",
        Raw::Json(PyJson::Bool(_)) => "bool",
        Raw::Json(PyJson::Int(_)) => "int",
        Raw::Json(PyJson::Float(_)) => "float",
        Raw::Json(PyJson::Str(_)) => "str",
        Raw::Json(PyJson::Null) => "NoneType",
        Raw::Json(PyJson::List(_)) => "list",
        Raw::Json(PyJson::Dict(_)) => "dict",
    }
}

/// `request.data.get(key)` with its Python type intact.
fn raw<'a>(data: &'a Data, key: &str) -> Option<Raw<'a>> {
    match data {
        Data::Json(value) => value.get(key).map(Raw::Json),
        Data::Form(query) => query.get(key).map(Raw::Text),
    }
}

/// `PageViewSet.create` (`page/base.py:145-168`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let timezone = base::timezone(ctx);

        let mut serializer = Serializer::new(ctx, false)?;
        page_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `PageSerializer.create` (`serializers/page.py:61-70`):
        // `labels = validated_data.pop("labels", None)` is the only pop; the
        // project read follows the validation and precedes everything else.
        let labels = values.uuid_list("labels").map(<[Uuid]>::to_vec);
        let statement = client
            .prepare_cached(queries::PAGE_PROJECT_BY_PK.sql)
            .await
            .map_err(ViewError::from)?;
        let workspace_id: Uuid = match client
            .query_opt(&statement, &[&params.project_id])
            .await
            .map_err(ViewError::from)?
        {
            Some(row) => row.try_get(0).map_err(ViewError::from)?,
            None => return Err(ViewError::ObjectDoesNotExist),
        };

        // `Page.objects.create(**validated_data, …)`: `label_ids`/`project_ids`
        // are still in `validated_data` and Django's `Model.__init__` rejects
        // them. It reads the kwargs in `validated_data` order, so the first
        // unknown one is named; the family handler renders the 500.
        let unknown: Vec<&str> = ["label_ids", "project_ids"]
            .into_iter()
            .filter(|name| values.contains(name))
            .collect();
        if !unknown.is_empty() {
            let names: Vec<String> = unknown.iter().map(|name| format!("'{name}'")).collect();
            return Err(ViewError::Unknown(format!(
                "TypeError: Page() got unexpected keyword arguments: {}",
                names.join(", ")
            )));
        }

        // The view hands the three description columns over raw
        // (`page/base.py:151-153`).
        let data = ctx.data()?;
        // A non-NULL `description_binary` is a 500: psycopg's `bytea` dumper
        // refuses anything but bytes (`escape_bytea` raises
        // `TypeError: bytes or buffer expected`), and the view passes the raw
        // body value through (`page/base.py:152`).
        let description_binary: Option<Vec<u8>> = match raw(data, "description_binary") {
            None | Some(Raw::Json(PyJson::Null)) => None,
            Some(value) => {
                return Err(ViewError::Unknown(format!(
                    "TypeError: bytes or buffer expected, got {}",
                    python_type_name_of(value)
                )));
            }
        };
        let description_json = match raw(data, "description_json") {
            None => JsonText::new("{}"),
            // `JSONField.get_prep_value(None)` returns SQL NULL, which the
            // `NOT NULL` column rejects: the same `IntegrityError` body as
            // `description_html: null`, before the INSERT.
            Some(Raw::Json(PyJson::Null)) => {
                return Ok(write::error_body(400, "The payload is not valid"));
            }
            Some(Raw::Json(value)) => JsonText::from_json(value).map_err(ViewError::from)?,
            Some(Raw::Text(text)) => {
                JsonText::from_json(&PyJson::Str(PyStr::new(text))).map_err(ViewError::from)?
            }
        };
        let description_html = match raw(data, "description_html") {
            None => "<p></p>".to_owned(),
            Some(Raw::Json(PyJson::Null)) => {
                // `IntegrityError` on the `NOT NULL` column; nothing has been
                // written yet (`plane/app/views/base.py:209-213`).
                return Ok(write::error_body(400, "The payload is not valid"));
            }
            Some(Raw::Json(PyJson::Str(value))) => value
                .as_str()
                .ok_or_else(|| ViewError::Unknown("non-UTF-8 description_html".to_owned()))?
                .to_owned(),
            Some(Raw::Text(text)) => text.to_owned(),
            Some(other) => {
                return Err(ViewError::Unknown(format!(
                    "DataError: description_html is not text: {other:?}"
                )));
            }
        };
        let description_stripped = hooks::page::before_save(Some(&description_html));
        let description_html_arg = match raw(data, "description_html") {
            None => TaskArg::Str("<p></p>".to_owned()),
            Some(Raw::Json(value)) => TaskArg::Json(value.clone()),
            Some(Raw::Text(text)) => TaskArg::Str(text.to_owned()),
        };

        // `Page.objects.create(**validated_data, …)`: H01 then H26 then the
        // INSERT.
        let created_at = write::now();
        let updated_at = write::now();
        let audit = hooks::audit::Audit::on_create(Some(params.user_id));
        let page = Page {
            created_at,
            updated_at,
            created_by_id: audit.created_by_id,
            updated_by_id: audit.updated_by_id,
            deleted_at: None,
            id: Uuid::new_v4(),
            workspace_id,
            name: values.text("name").unwrap_or_default().to_owned(),
            description_json,
            description_binary,
            description_html,
            description_stripped,
            owned_by_id: params.user_id,
            access: values.integer("access").unwrap_or(0) as i16,
            color: values.text("color").unwrap_or_default().to_owned(),
            parent_id: values.uuid("parent"),
            archived_at: values.date("archived_at"),
            is_locked: values.bool("is_locked").unwrap_or(false),
            view_props: values
                .json("view_props")
                .cloned()
                .unwrap_or_else(|| JsonText::new("{\"full_width\": false}")),
            logo_props: values
                .json("logo_props")
                .cloned()
                .unwrap_or_else(|| JsonText::new("{}")),
            is_global: false,
            moved_to_page: None,
            moved_to_project: None,
            sort_order: 65535.0,
            external_id: None,
            external_source: None,
        };
        insert_page(client, &page).await?;

        // `ProjectPage.objects.create(...)` (`serializers/page.py:83-89`).
        insert_project_page(client, &page, params.project_id).await?;

        // `PageLabel.objects.bulk_create([...], batch_size=10)`
        // (`serializers/page.py:92-105`).
        if let Some(label_ids) = labels {
            insert_page_labels(client, &page, &label_ids, write::now()).await?;
        }

        // `page_transaction.delay(...)` (`page/base.py:160-164`): the page id
        // is `serializer.data["id"]`, a UUID object, not a string.
        publish_page_transaction(ctx, description_html_arg, None, page.id).await?;

        // `self.get_queryset().get(pk=serializer.data["id"])` (`:165`).
        let terms = base::search_terms(ctx);
        let Some(row) = refetch_created(client, &params, page.id, &terms).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        let body = PageDetailBody::created(&row);
        let bytes =
            plane_serializers::render(&body, &plane_serializers::RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, bytes))
    })
}

/// `PageViewSet.partial_update` (`page/base.py:170-216`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let page_id = base::uuid_kwarg(ctx, "page_id")?;
        let timezone = base::timezone(ctx);

        let mut page = match load_page(client, &params, page_id).await? {
            Some(page) => page,
            // `except Page.DoesNotExist` (`:212-216`).
            None => return Ok(write::error_body(400, NOT_OWNER)),
        };

        // `if page.is_locked` (`:179-180`).
        if page.is_locked {
            return Ok(write::error_body(400, "Page is locked"));
        }

        // `parent = request.data.get("parent", None); if parent: Page.objects
        // .get(pk=parent, …)` (`:182-189`). A miss is the same 400 as a missing
        // page; an unparsable value is Django's `ValidationError`.
        let data = ctx.data()?;
        let parent = raw(data, "parent");
        if let Some(parent) = parent.filter(|value| value.truthy()) {
            let parent_id = match python_uuid(parent) {
                Some(parent_id) => parent_id,
                None => return Err(ViewError::Validation),
            };
            if load_page(client, &params, parent_id).await?.is_none() {
                return Ok(write::error_body(400, NOT_OWNER));
            }
        }

        // `page.access != request.data.get("access", page.access)`
        // (`:192-196`).
        if access_changed(raw(data, "access"), page.access) && page.owned_by_id != params.user_id {
            return Ok(write::error_body(400, NOT_OWNER));
        }

        // `PageDetailSerializer(page, data=request.data, partial=True)`
        // (`:198`): every field of `PageDetailSerializer`, `description_html`
        // included — which is **not** sanitised on this route.
        let mut serializer = Serializer::new(ctx, true)?;
        page_detail_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `PageSerializer.update` (`serializers/page.py:108-126`): the labels
        // are replaced **before** `super().update()`, with the instance's
        // pre-update audit ids, then the instance is saved.
        let labels = values.uuid_list("labels").map(<[Uuid]>::to_vec);
        if let Some(label_ids) = labels {
            soft_delete_page_labels(client, page_id, write::now()).await?;
            insert_page_labels(client, &page, &label_ids, write::now()).await?;
        }

        // `page_description = page.description_html` is read before validation
        // (`page/base.py:199`); the task carries it as `old_description_html`.
        let old_description_html = page.description_html.clone();
        let description_html_arg = raw(data, "description_html").filter(|value| value.truthy());

        // `ModelSerializer.update`: one `setattr` per key in field order, then a
        // full `instance.save()` (H26, H01 and `auto_now`).
        apply(&mut page, &values);
        page.updated_by_id = Some(params.user_id);
        page.updated_at = write::now();
        page.description_stripped = hooks::page::before_save(Some(&page.description_html));
        update_page(client, &page).await?;

        // `if request.data.get("description_html"): page_transaction.delay(…)`
        // (`:203-208`).
        if let Some(new_html) = description_html_arg {
            let arg = match new_html {
                Raw::Json(value) => TaskArg::Json(value.clone()),
                Raw::Text(text) => TaskArg::Str(text.to_owned()),
            };
            publish_page_transaction(ctx, arg, Some(old_description_html), page_id).await?;
        }

        // `serializer.data`: 16 keys normally; `label_ids`/`project_ids` were
        // `setattr`-ed on the instance when the body carried them, so DRF
        // renders those too (`rest_framework/serializers.py:541-546`).
        let label_ids = values.uuid_list("label_ids");
        let project_ids = values.uuid_list("project_ids");
        let row = row_from_page(&page);
        let body = match (label_ids, project_ids) {
            (None, None) => PageDetailBody::updated(&row),
            (label_ids, project_ids) => PageDetailBody::updated_with(&row, label_ids, project_ids),
        };
        let bytes =
            plane_serializers::render(&body, &plane_serializers::RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

/// `PageViewSet.destroy` (`page/base.py:363-414`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let page_id = base::uuid_kwarg(ctx, "page_id")?;

        let Some(page) = load_page(client, &params, page_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        // `if page.archived_at is None` (`:371-375`).
        if page.archived_at.is_none() {
            return Ok(write::error_body(
                400,
                "The page should be archived before deleting",
            ));
        }

        // `if page.owned_by_id != request.user.id and not (admin)` (`:377-389`).
        if page.owned_by_id != params.user_id && !project_admin_exists(client, &params).await? {
            return Ok(write::error_body(
                403,
                "Only admin or owner can delete the page",
            ));
        }

        // `Page.objects.filter(parent_id=…, …).update(parent=None)` (`:392-397`).
        let statement = client
            .prepare_cached(queries::PAGE_CHILDREN_DETACH.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(&statement, &[&page_id, &params.project_id, &params.slug])
            .await
            .map_err(ViewError::from)?;

        // `page.delete()` → H03 (`plane/db/mixins.py:72-78`): deleted_at and
        // updated_at, H26 re-derives description_stripped, H01 sets updated_by,
        // then the cascade task.
        let deleted_at = write::now();
        let updated_at = write::now();
        let statement = client
            .prepare_cached(queries::PAGE_SOFT_DELETE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &hooks::page::before_save(Some(&page.description_html)),
                    &deleted_at,
                    &updated_at,
                    &Some(params.user_id),
                    &page_id,
                ],
            )
            .await
            .map_err(ViewError::from)?;
        write::publish_soft_delete_cascade(ctx, "db", "page", page_id).await?;

        // `UserFavorite.objects.filter(...).delete()` (`:401-406`) — H04 for
        // every user.
        let statement = client
            .prepare_cached(queries::PAGE_FAVORITE_SOFT_DELETE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[&write::now(), &page_id, &params.project_id, &params.slug],
            )
            .await
            .map_err(ViewError::from)?;

        // `UserRecentVisit.objects.filter(...).delete(soft=False)`
        // (`:408-413`): the manager predicate stays in the SQL.
        let statement = client
            .prepare_cached(queries::PAGE_RECENT_VISIT_HARD_DELETE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(&statement, &[&page_id, &params.project_id, &params.slug])
            .await
            .map_err(ViewError::from)?;

        Ok(ViewResponse::empty(204))
    })
}

/// `PageViewSet.archive` (`page/base.py:303-332`).
pub fn archive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let page_id = base::uuid_kwarg(ctx, "page_id")?;

        let Some(page) = load_page(client, &params, page_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        // Only the owner or a project admin may archive (`:311-321`).
        if page.owned_by_id != params.user_id && member_role_lte15(client, &params).await? {
            return Ok(write::error_body(
                400,
                "Only the owner or admin can archive the page",
            ));
        }

        // `UserFavorite.objects.filter(...).delete()` (`:323-328`), every user.
        let statement = client
            .prepare_cached(queries::PAGE_FAVORITE_SOFT_DELETE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[&write::now(), &page_id, &params.project_id, &params.slug],
            )
            .await
            .map_err(ViewError::from)?;

        // `unarchive_archive_page_and_descendants(page_id, datetime.now())`
        // (`:330`): a naive datetime narrowed by `DateField.to_python`.
        // Django sets the process timezone to `TIME_ZONE` ("UTC") when the
        // settings load (`django/conf/__init__.py`), so `datetime.now()` here
        // is UTC, not the host's local zone.
        let archived_at = Utc::now().date_naive();
        let statement = client
            .prepare_cached(queries::PAGE_ARCHIVE_CTE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(&statement, &[&page_id, &Some(archived_at)])
            .await
            .map_err(ViewError::from)?;

        // `{"archived_at": str(datetime.now())}` (`:332`), a second clock read.
        let body = Doc::object(vec![(
            "archived_at",
            Doc::string(python_str_datetime(Utc::now().naive_utc())),
        )]);
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body.to_drf()))
    })
}

/// `PageViewSet.unarchive` (`page/base.py:334-361`).
pub fn unarchive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let page_id = base::uuid_kwarg(ctx, "page_id")?;

        let Some(page) = load_page(client, &params, page_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        // Only the owner or a project admin may unarchive (`:342-352`).
        if page.owned_by_id != params.user_id && member_role_lte15(client, &params).await? {
            return Ok(write::error_body(
                400,
                "Only the owner or admin can un archive the page",
            ));
        }

        // `if page.parent_id and page.parent.archived_at:` (`:354-357`). The
        // forward FK reads through `_base_manager`, so a soft-deleted parent is
        // still found; a missing one is a `DoesNotExist` (404).
        if let Some(parent_id) = page.parent_id {
            let statement = client
                .prepare_cached(queries::PAGE_PARENT_ARCHIVED.sql)
                .await
                .map_err(ViewError::from)?;
            let Some(row) = client
                .query_opt(&statement, &[&parent_id])
                .await
                .map_err(ViewError::from)?
            else {
                return Err(ViewError::ObjectDoesNotExist);
            };
            let archived_at: Option<NaiveDate> = row.try_get(0).map_err(ViewError::from)?;
            if archived_at.is_some() {
                // `page.save(update_fields=["parent"])`.
                let statement = client
                    .prepare_cached(queries::PAGE_PARENT_DETACH.sql)
                    .await
                    .map_err(ViewError::from)?;
                client
                    .execute(&statement, &[&page_id])
                    .await
                    .map_err(ViewError::from)?;
            }
        }

        // `unarchive_archive_page_and_descendants(page_id, None)` (`:359`).
        let statement = client
            .prepare_cached(queries::PAGE_ARCHIVE_CTE.sql)
            .await
            .map_err(ViewError::from)?;
        let none: Option<NaiveDate> = None;
        client
            .execute(&statement, &[&page_id, &none])
            .await
            .map_err(ViewError::from)?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Statements and small helpers
// ---------------------------------------------------------------------------

/// `Page.objects.get(...)` for the four `{page_id}` routes
/// (`page/base.py:172-177`, `:304-309`, `:335-340`, `:364-369`).
///
/// `Ok(None)` is the `DoesNotExist` the caller renders; more than one row is
/// `MultipleObjectsReturned`, which the family handler renders as a 500.
async fn load_page(
    client: &Client,
    params: &Params,
    page_id: Uuid,
) -> Result<Option<Page>, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_ROW_BY_PROJECT.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&page_id, &params.project_id, &params.slug])
        .await
        .map_err(ViewError::from)?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(Some(Page::from_row(&rows[0], 0)?)),
        _ => Err(ViewError::Unknown(
            "Page.MultipleObjectsReturned: the get() matched more than one row".to_owned(),
        )),
    }
}

/// `Page.objects.create(...)` (`serializers/page.py:73-80`).
async fn insert_page(client: &Client, page: &Page) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &page.created_at,
                &page.updated_at,
                &page.created_by_id,
                &page.updated_by_id,
                &page.deleted_at,
                &page.id,
                &page.workspace_id,
                &page.name,
                &page.description_json,
                &page.description_binary,
                &page.description_html,
                &page.description_stripped,
                &page.owned_by_id,
                &page.access,
                &page.color,
                &page.parent_id,
                &page.archived_at,
                &page.is_locked,
                &page.view_props,
                &page.logo_props,
                &page.is_global,
                &page.moved_to_page,
                &page.moved_to_project,
                &page.sort_order,
                &page.external_id,
                &page.external_source,
            ],
        )
        .await
        .map_err(ViewError::from)?;
    Ok(())
}

/// `instance.save()` for a full-row update.
async fn update_page(client: &Client, page: &Page) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_UPDATE.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &page.created_at,
                &page.updated_at,
                &page.created_by_id,
                &page.updated_by_id,
                &page.deleted_at,
                &page.workspace_id,
                &page.name,
                &page.description_json,
                &page.description_binary,
                &page.description_html,
                &page.description_stripped,
                &page.owned_by_id,
                &page.access,
                &page.color,
                &page.parent_id,
                &page.archived_at,
                &page.is_locked,
                &page.view_props,
                &page.logo_props,
                &page.is_global,
                &page.moved_to_page,
                &page.moved_to_project,
                &page.sort_order,
                &page.external_id,
                &page.external_source,
                &page.id,
            ],
        )
        .await
        .map_err(ViewError::from)?;
    Ok(())
}

/// `ProjectPage.objects.create(...)` (`serializers/page.py:83-89`).
async fn insert_project_page(
    client: &Client,
    page: &Page,
    project_id: Uuid,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_PROJECT_PAGE_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &write::now(),
                &write::now(),
                &page.created_by_id,
                &page.updated_by_id,
                &Uuid::new_v4(),
                &page.workspace_id,
                &project_id,
                &page.id,
            ],
        )
        .await
        .map_err(ViewError::from)?;
    Ok(())
}

/// `PageLabel.objects.filter(page=instance).delete()` — H04
/// (`serializers/page.py:111`).
async fn soft_delete_page_labels(
    client: &Client,
    page_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_LABEL_SOFT_DELETE.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(&statement, &[&now, &page_id])
        .await
        .map_err(ViewError::from)?;
    Ok(())
}

/// `PageLabel.objects.bulk_create([...], batch_size=10)` (`serializers/page.py:93-105`,
/// `:112-124`).
///
/// Django's `bulk_create` wraps **every** batch in one
/// `transaction.atomic(savepoint=False)` (`django/db/models/query.py`), so in
/// autocommit mode this is one outermost `BEGIN … COMMIT`. Each object's
/// `pre_save` calls `timezone.now()`, so `created_at` advances by one
/// microsecond per row; the caller's `now` is the first row's, exactly like
/// `plane_db::hooks::issue_m2m`.
///
/// There is no `ignore_conflicts` here and `page_labels` has no unique
/// constraint, so the statement is a plain INSERT.
async fn insert_page_labels(
    client: &Client,
    page: &Page,
    label_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    // `bulk_create([])` returns before the atomic (`query.py`, `if not objs`).
    if label_ids.is_empty() {
        return Ok(());
    }
    client
        .batch_execute("BEGIN")
        .await
        .map_err(ViewError::from)?;
    let outcome: Result<(), tokio_postgres::Error> = async {
        for (batch_index, batch) in label_ids.chunks(LABEL_BATCH_SIZE).enumerate() {
            let mut sql = String::from(
                "INSERT INTO page_labels (created_at, updated_at, created_by_id, \
                 updated_by_id, deleted_at, id, label_id, page_id, workspace_id) VALUES ",
            );
            let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> =
                Vec::with_capacity(batch.len() * 9);
            for (index, label_id) in batch.iter().enumerate() {
                if index != 0 {
                    sql.push_str(", ");
                }
                // `deleted_at` is the literal NULL, so each row binds eight
                // parameters, not nine.
                let base = index * 8;
                sql.push_str(&format!(
                    "(${}, ${}, ${}, ${}, NULL, ${}, ${}, ${}, ${})",
                    base + 1,
                    base + 2,
                    base + 3,
                    base + 4,
                    base + 5,
                    base + 6,
                    base + 7,
                    base + 8
                ));
                let created_at = now
                    + chrono::Duration::microseconds(
                        (batch_index * LABEL_BATCH_SIZE + index) as i64,
                    );
                params.push(Box::new(created_at));
                params.push(Box::new(created_at));
                params.push(Box::new(page.created_by_id));
                params.push(Box::new(page.updated_by_id));
                params.push(Box::new(Uuid::new_v4()));
                params.push(Box::new(*label_id));
                params.push(Box::new(page.id));
                params.push(Box::new(page.workspace_id));
            }
            let statement = client.prepare(&sql).await?;
            let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
                .iter()
                .map(|value| value.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
                .collect();
            client.execute(&statement, &refs).await?;
        }
        Ok(())
    }
    .await;

    match outcome {
        Ok(()) => {
            client
                .batch_execute("COMMIT")
                .await
                .map_err(ViewError::from)?;
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(ViewError::from(error))
        }
    }
}

/// `page_transaction.delay(new_description_html=…, old_description_html=…,
/// page_id=…)` (`page/base.py:160-164`, `:203-208`).
///
/// `page_id` is a `uuid.UUID` object at both call sites — the create's
/// `serializer.data["id"]` is a `PrimaryKeyRelatedField` representation (the pk
/// object) and the PATCH's is the URL converter's value — so the kombu body
/// carries its `uuid` envelope.
async fn publish_page_transaction(
    ctx: &ViewCtx<'_>,
    new_description_html: TaskArg,
    old_description_html: Option<String>,
    page_id: Uuid,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::PAGE_TRANSACTION;
    // `tasks::page_transaction`'s kwarg order and encodings, with the raw
    // request value in place of its `String` parameter.
    let kwargs = vec![
        ("new_description_html", new_description_html),
        (
            "old_description_html",
            old_description_html.map_or(TaskArg::Null, TaskArg::Str),
        ),
        ("page_id", TaskArg::uuid(page_id)),
    ];
    let message = plane_jobs::celery::Message::new(task, Vec::new(), kwargs);
    crate::common::jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task))
        .await?;
    Ok(())
}

/// `ProjectMember.objects.filter(project_id=…, member=request.user,
/// is_active=True, role__lte=15).exists()` (`page/base.py:312-317`,
/// `:343-348`).
async fn member_role_lte15(client: &Client, params: &Params) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_MEMBER_ROLE_LTE15.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&params.project_id, &params.user_id])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// `ProjectMember.objects.filter(workspace__slug=…, member=…, role=20,
/// project_id=…, is_active=True).exists()` (`page/base.py:377-385`).
async fn project_admin_exists(client: &Client, params: &Params) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_PROJECT_ADMIN_EXISTS.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(
            &statement,
            &[&params.slug, &params.project_id, &params.user_id],
        )
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// `self.get_queryset().get(pk=…)` for `create` (`page/base.py:165`).
///
/// The shared statement is [`queries::PAGES_RETRIEVE`] with the request's
/// `?search=` terms appended (`plane/app/views/base.py:72`), exactly as
/// `retrieve` builds its own shape. `order_by` is irrelevant to a pk lookup
/// and is therefore not applied.
async fn refetch_created(
    client: &Client,
    params: &Params,
    page_id: Uuid,
    terms: &[String],
) -> Result<Option<PageRow>, ViewError> {
    let rows = if terms.is_empty() {
        let statement = client
            .prepare_cached(read_queries::PAGES_RETRIEVE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .query(
                &statement,
                &[&params.slug, &params.project_id, &params.user_id, &page_id],
            )
            .await
            .map_err(ViewError::from)?
    } else {
        let patterns: Vec<String> = terms
            .iter()
            .map(|term| format!("%{}%", base::escape_like(term)))
            .collect();
        let mut sql = String::with_capacity(read_queries::PAGES_SELECT.len() + 128);
        sql.push_str(read_queries::PAGES_SELECT);
        let mut bind = 4usize;
        for _ in &patterns {
            bind += 1;
            sql.push_str(&format!(" AND UPPER(p.name::text) LIKE UPPER(${bind})"));
        }
        sql.push_str(" AND p.id = $4");
        sql.push_str(read_queries::PAGES_GROUP_BY);
        sql.push_str(read_queries::PAGES_DEFAULT_ORDER);
        sql.push_str(" LIMIT 1");
        let mut values = vec![
            SqlValue::Text(params.slug.clone()),
            SqlValue::Uuid(params.project_id),
            SqlValue::Uuid(params.user_id),
            SqlValue::Uuid(page_id),
        ];
        for pattern in &patterns {
            values.push(SqlValue::Text(pattern.clone()));
        }
        Sql {
            text: sql,
            params: values,
        }
        .query(client)
        .await
        .map_err(ViewError::from)?
    };
    match rows.len() {
        0 => Ok(None),
        1 => Ok(Some(PageRow::decode(&rows[0])?)),
        _ => Err(ViewError::Unknown(
            "Page.MultipleObjectsReturned: the get() matched more than one row".to_owned(),
        )),
    }
}

/// `ModelSerializer.update`'s `setattr(instance, field, value)` for every
/// field PATCH can write. `updated_by` is deliberately absent: H01 overwrites
/// it (`models/base.py:41-42`).
fn apply(page: &mut Page, values: &write::Validated) {
    if values.contains("name") {
        page.name = values.text("name").unwrap_or_default().to_owned();
    }
    if values.contains("access") {
        page.access = values.integer("access").unwrap_or(0) as i16;
    }
    if values.contains("color") {
        page.color = values.text("color").unwrap_or_default().to_owned();
    }
    if values.contains("parent") {
        page.parent_id = values.uuid("parent");
    }
    if values.contains("is_locked") {
        page.is_locked = values.bool("is_locked").unwrap_or(false);
    }
    if values.contains("archived_at") {
        page.archived_at = values.date("archived_at");
    }
    if values.contains("created_by") {
        page.created_by_id = values.uuid("created_by");
    }
    if values.contains("description_html") {
        page.description_html = values
            .text("description_html")
            .unwrap_or_default()
            .to_owned();
    }
    if let Some(view_props) = values.json("view_props") {
        page.view_props = view_props.clone();
    }
    if let Some(logo_props) = values.json("logo_props") {
        page.logo_props = logo_props.clone();
    }
}

/// The loaded instance, in the shape [`PageDetailBody`] renders. The three
/// annotation fields are placeholders: the PATCH body decides whether they are
/// rendered (`PageDetailBody::updated_with`), never this row.
fn row_from_page(page: &Page) -> PageRow {
    PageRow {
        id: page.id,
        name: page.name.clone(),
        owned_by_id: page.owned_by_id,
        access: page.access,
        color: page.color.clone(),
        parent_id: page.parent_id,
        is_favorite: false,
        is_locked: page.is_locked,
        archived_at: page.archived_at,
        workspace_id: page.workspace_id,
        created_at: page.created_at,
        updated_at: page.updated_at,
        created_by_id: page.created_by_id,
        updated_by_id: page.updated_by_id,
        view_props: page.view_props.clone(),
        logo_props: page.logo_props.clone(),
        label_ids: Vec::new(),
        project_ids: Vec::new(),
        description_html: page.description_html.clone(),
    }
}

/// `Page.objects.get(pk=<value>)`'s `UUIDField.to_python` for the PATCH
/// `parent` pre-check (`page/base.py:182-189`). A failure is Django's
/// `ValidationError` → `400 {"error": "Please provide valid detail"}`.
fn python_uuid(value: Raw<'_>) -> Option<Uuid> {
    match value {
        Raw::Json(PyJson::Str(text)) => Uuid::parse_str(text.as_str()?).ok(),
        Raw::Json(PyJson::Int(number)) => number
            .to_decimal_string()
            .parse::<u128>()
            .ok()
            .map(Uuid::from_u128),
        Raw::Json(PyJson::Bool(flag)) => Some(Uuid::from_u128(u128::from(*flag))),
        Raw::Text(text) => Uuid::parse_str(text).ok(),
        // A float or a composite reaches `UUIDField.to_python` and raises its
        // `ValidationError`; `null` is filtered out by the truthiness test.
        _ => None,
    }
}

/// `page.access != request.data.get("access", page.access)` with Python's
/// `!=` semantics: `None` and text never equal an integer, a bool does
/// (`True == 1`).
fn access_changed(value: Option<Raw<'_>>, access: i16) -> bool {
    match value {
        None => false,
        Some(Raw::Text(_)) => true,
        Some(Raw::Json(PyJson::Bool(flag))) => i16::from(*flag) != access,
        Some(Raw::Json(PyJson::Int(number))) => match number.as_i64() {
            Some(number) => number != i64::from(access),
            None => true,
        },
        Some(Raw::Json(PyJson::Float(number))) => *number != f64::from(access),
        Some(Raw::Json(PyJson::Str(_) | PyJson::Null | PyJson::List(_) | PyJson::Dict(_))) => true,
    }
}

/// Python `str(datetime.now())`: `YYYY-MM-DD HH:MM:SS` with a `.ffffff`
/// fraction only when the microsecond is non-zero (`datetime.__str__` is
/// `isoformat(sep=" ")`). The reference process runs with `TZ` set to Django's
/// `TIME_ZONE` ("UTC"), so the value is a naive UTC datetime.
fn python_str_datetime(value: chrono::NaiveDateTime) -> String {
    if value.and_utc().timestamp_subsec_micros() == 0 {
        value.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        value.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
    }
}
