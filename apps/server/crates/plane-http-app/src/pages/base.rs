// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/page/base.py` — the four `GET` handlers of the pages group:
//!
//! * `GET …/projects/{project_id}/pages-summary/` — `PageViewSet.summary`;
//! * `GET …/projects/{project_id}/pages/` — `PageViewSet.list`;
//! * `GET …/projects/{project_id}/pages/{page_id}/` — `PageViewSet.retrieve`;
//! * `GET …/projects/{project_id}/pages/{page_id}/description/` —
//!   `PagesDescriptionViewSet.retrieve` (a `StreamingHttpResponse` of the
//!   page's binary description).
//!
//! `get_queryset` (`base.py:83-143`) is the whole of `list` and `retrieve`: the
//! filters (`workspace__slug`, an active project membership, an unarchived
//! project, `parent__isnull`, `owned_by | access=0`, and an active
//! `ProjectPage` link to the URL project), the two `Exists` annotations and the
//! two `ARRAY_AGG`s are one statement, catalogued in
//! [`queries`](crate::pages::queries).
//!
//! `list` additionally loads the project (`base.py:288`) and, for a guest in a
//! project without `guest_view_all_features`, narrows the queryset to the
//! guest's own pages (`base.py:289-299`). `summary` computes three counts over
//! the same queryset (`base.py:448-464`).
//!
//! `retrieve` appends `issue_ids` to the serialized page
//! (`base.py:226-230`) and enqueues `recent_visited_task` unless
//! `?track_visit=false` (`base.py:220,231-238`).
//!
//! `description` is a `StreamingHttpResponse` (`base.py:508-516`): one chunk
//! with the whole `description_binary` (or `b""`), `Content-Type:
//! application/octet-stream`, `Content-Disposition: attachment;
//! filename="page_description.bin"`, no `Content-Length`, and — because
//! `GZipMiddleware` exempts streaming responses from its 200-byte rule — gzip
//! whatever the body's size. [`ViewResponse::streaming_raw`] carries the
//! framing through the pipeline.
//!
//! A response field the serializers declare but the queryset always annotates
//! (`is_favorite`, `label_ids`, `project_ids`) is `FieldValue::value(…)`: the
//! attribute exists on every row, so DRF never takes the `SkipField` path
//! (`rest_framework/serializers.py:541-546`). `view_props`/`logo_props` are
//! non-null `jsonb` columns, so they are rendered from Postgres' text
//! (`PgJson`), keeping jsonb's key order.

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_serializers::generated::app_page::PageSerializer;
use plane_serializers::{
    DrfDateTime, DrfError, DrfSerialize, DrfUuid, DrfUuidList, DrfWriter, FieldValue, PgJson,
    PyDate, RenderCtx,
};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::common::{client, jobs, lookup};
use crate::cycles::base as cycle_base;
use crate::pages::queries;

/// The URL kwargs and user id every route of this module needs.
pub(crate) struct Params {
    pub(crate) slug: String,
    pub(crate) project_id: Uuid,
    pub(crate) user_id: Uuid,
}

pub(crate) fn params(ctx: &ViewCtx<'_>) -> Result<Params, ViewError> {
    Ok(Params {
        slug: ctx
            .kwarg("slug")
            .map(str::to_string)
            .ok_or(ViewError::Key)?,
        project_id: lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?,
        user_id: jobs::user_id(ctx)?,
    })
}

pub(crate) fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    Ok(lookup::uuid_kwarg(
        name,
        ctx.kwarg(name).ok_or(ViewError::Key)?,
    )?)
}

/// The active timezone; `TimezoneMixin` leaves it `None` only for an anonymous
/// caller, which no route in this group allows.
pub(crate) fn timezone(ctx: &ViewCtx<'_>) -> chrono_tz::Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

/// One row of the shared page statement.
#[derive(Debug)]
pub(crate) struct PageRow {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) owned_by_id: Uuid,
    pub(crate) access: i16,
    pub(crate) color: String,
    pub(crate) parent_id: Option<Uuid>,
    pub(crate) is_favorite: bool,
    pub(crate) is_locked: bool,
    pub(crate) archived_at: Option<NaiveDate>,
    pub(crate) workspace_id: Uuid,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) created_by_id: Option<Uuid>,
    pub(crate) updated_by_id: Option<Uuid>,
    pub(crate) view_props: JsonText,
    pub(crate) logo_props: JsonText,
    pub(crate) label_ids: Vec<Uuid>,
    pub(crate) project_ids: Vec<Uuid>,
    pub(crate) description_html: String,
}

impl PageRow {
    /// Decode the select list of
    /// [`PAGES_LIST`](queries::PAGES_LIST)/[`PAGES_RETRIEVE`](queries::PAGES_RETRIEVE).
    pub(crate) fn decode(row: &Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            name: row.try_get(1).map_err(ViewError::from)?,
            owned_by_id: row.try_get(2).map_err(ViewError::from)?,
            access: row.try_get(3).map_err(ViewError::from)?,
            color: row.try_get(4).map_err(ViewError::from)?,
            parent_id: row.try_get(5).map_err(ViewError::from)?,
            is_favorite: row.try_get(6).map_err(ViewError::from)?,
            is_locked: row.try_get(7).map_err(ViewError::from)?,
            archived_at: row.try_get(8).map_err(ViewError::from)?,
            workspace_id: row.try_get(9).map_err(ViewError::from)?,
            created_at: row.try_get(10).map_err(ViewError::from)?,
            updated_at: row.try_get(11).map_err(ViewError::from)?,
            created_by_id: row.try_get(12).map_err(ViewError::from)?,
            updated_by_id: row.try_get(13).map_err(ViewError::from)?,
            view_props: row.try_get(14).map_err(ViewError::from)?,
            logo_props: row.try_get(15).map_err(ViewError::from)?,
            label_ids: row.try_get(16).map_err(ViewError::from)?,
            project_ids: row.try_get(17).map_err(ViewError::from)?,
            description_html: row.try_get(18).map_err(ViewError::from)?,
        })
    }

    /// `PageSerializer(page).data` (`page/base.py:300`).
    pub(crate) fn serializer(&self) -> PageSerializer<'_> {
        PageSerializer {
            id: self.id,
            name: &self.name,
            owned_by: self.owned_by_id,
            access: i64::from(self.access),
            color: &self.color,
            parent: self.parent_id,
            is_favorite: FieldValue::value(self.is_favorite),
            is_locked: self.is_locked,
            archived_at: self.archived_at,
            workspace: self.workspace_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            created_by: self.created_by_id,
            updated_by: self.updated_by_id,
            view_props: Some(PgJson::from(&self.view_props)),
            logo_props: Some(PgJson::from(&self.logo_props)),
            label_ids: FieldValue::value(self.label_ids.as_slice()),
            project_ids: FieldValue::value(self.project_ids.as_slice()),
        }
    }
}

/// `PageDetailSerializer(page).data` followed by the view's
/// `data["issue_ids"] = issue_ids` (`page/base.py:229-230`).
///
/// `PageDetailSerializer` is `PageSerializer` plus `description_html`
/// (`plane/app/serializers/page.py:129-133`), so the key order is the generated
/// serializer's 19 fields in order and then the appended `issue_ids`. DRF
/// evaluates the appended `ValuesListQuerySet` only when it renders the
/// response (`rest_framework/utils/encoders.py`), which is why the ids come last
/// and are an array, not a string (a `QuerySet` encodes as a tuple).
#[derive(Debug)]
pub(crate) struct PageDetailBody<'a> {
    pub(crate) page: &'a PageRow,
    pub(crate) issue_ids: &'a [Option<Uuid>],
}

impl DrfSerialize for PageDetailBody<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let page = self.page;
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(page.id))?;
        obj.field("name", &page.name)?;
        obj.field_dyn("owned_by", ctx, &DrfUuid(page.owned_by_id))?;
        obj.field("access", &i64::from(page.access))?;
        obj.field("color", &page.color)?;
        obj.field_dyn("parent", ctx, &page.parent_id.map(DrfUuid))?;
        obj.field_optional("is_favorite", ctx, &FieldValue::value(page.is_favorite))?;
        obj.field("is_locked", &page.is_locked)?;
        obj.field("archived_at", &page.archived_at.map(PyDate))?;
        obj.field_dyn("workspace", ctx, &DrfUuid(page.workspace_id))?;
        obj.field_dyn(
            "created_at",
            ctx,
            &DrfDateTime(page.created_at.with_timezone(ctx.tz())),
        )?;
        obj.field_dyn(
            "updated_at",
            ctx,
            &DrfDateTime(page.updated_at.with_timezone(ctx.tz())),
        )?;
        obj.field_dyn("created_by", ctx, &page.created_by_id.map(DrfUuid))?;
        obj.field_dyn("updated_by", ctx, &page.updated_by_id.map(DrfUuid))?;
        obj.field_json("view_props", Some(PgJson::from(&page.view_props)))?;
        obj.field_json("logo_props", Some(PgJson::from(&page.logo_props)))?;
        obj.field("label_ids", &DrfUuidList(&page.label_ids))?;
        obj.field("project_ids", &DrfUuidList(&page.project_ids))?;
        obj.field("description_html", &page.description_html)?;
        let ids: Vec<Option<DrfUuid>> = self.issue_ids.iter().map(|id| id.map(DrfUuid)).collect();
        obj.field("issue_ids", &ids)?;
        obj.end();
        Ok(())
    }
}

/// `PageViewSet.get_queryset`'s `sanitize_order_by(order_by, PAGE_ORDER_BY_ALLOWLIST,
/// default="-created_at")` (`plane/utils/order_queryset.py:135-156`), reduced to
/// the eight values that survive it.
///
/// `order_queryset.py` strips at most one `-`, rejects a second one, and
/// replaces anything outside the allowlist with the default; the match below is
/// that function for `PAGE_ORDER_BY_ALLOWLIST` (`order_queryset.py:79-84`).
pub(crate) fn sanitize_order_by(value: Option<&str>) -> &'static str {
    match value {
        Some("created_at") => "created_at",
        Some("-created_at") => "-created_at",
        Some("updated_at") => "updated_at",
        Some("-updated_at") => "-updated_at",
        Some("name") => "name",
        Some("-name") => "-name",
        Some("sort_order") => "sort_order",
        Some("-sort_order") => "-sort_order",
        _ => "-created_at",
    }
}

/// The `ORDER BY` clause for a sanitized value. `-is_favorite` is first and
/// `id` is the stable tiebreak (`page/base.py:113-121`).
pub(crate) fn order_clause(order: &str) -> &'static str {
    match order {
        "created_at" => " ORDER BY is_favorite DESC, p.created_at ASC, p.id ASC",
        "-created_at" => queries::PAGES_DEFAULT_ORDER,
        "updated_at" => " ORDER BY is_favorite DESC, p.updated_at ASC, p.id ASC",
        "-updated_at" => " ORDER BY is_favorite DESC, p.updated_at DESC, p.id ASC",
        "name" => " ORDER BY is_favorite DESC, p.name ASC, p.id ASC",
        "-name" => " ORDER BY is_favorite DESC, p.name DESC, p.id ASC",
        "sort_order" => " ORDER BY is_favorite DESC, p.sort_order ASC, p.id ASC",
        "-sort_order" => " ORDER BY is_favorite DESC, p.sort_order DESC, p.id ASC",
        _ => queries::PAGES_DEFAULT_ORDER,
    }
}

/// `django.db.models.fields.Field.get_prep_value`'s LIKE escaping, which
/// `icontains` applies before wrapping the term in `%` (`django/db/models/lookups.py`,
/// `pattern_esc`). Identical to `users_me`'s copy; the second occurrence is what
/// `HANDLERS.md`'s "a second copy moves it to `common`" rule counts.
pub(crate) fn escape_like(term: &str) -> String {
    term.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// DRF's `SearchFilter.get_search_terms` (`rest_framework/filters.py:75-79`):
/// drop NUL characters and split on runs of whitespace.
pub(crate) fn search_terms(ctx: &ViewCtx<'_>) -> Vec<String> {
    ctx.query()
        .get_or("search", "")
        .replace('\0', "")
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// `PageViewSet.get_queryset`'s `filter_queryset(...)` with no `?search=` and
/// the default order — the cached statement.
async fn fetch_default(client: &Client, params: &Params) -> Result<Vec<Row>, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGES_LIST.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .query(
            &statement,
            &[&params.slug, &params.project_id, &params.user_id],
        )
        .await
        .map_err(ViewError::from)
}

/// `PageViewSet.get_queryset` for every other shape: `?search=` terms, a
/// non-default `order_by`, the guest filter, or a combination. Dynamic SQL, so
/// it is an unnamed statement (`DECISIONS.md` D1's amendment).
async fn fetch_dynamic(
    client: &Client,
    params: &Params,
    terms: &[String],
    order: &str,
    only_owned: bool,
) -> Result<Vec<Row>, ViewError> {
    let patterns: Vec<String> = terms
        .iter()
        .map(|term| format!("%{}%", escape_like(term)))
        .collect();
    let mut sql = String::with_capacity(queries::PAGES_SELECT.len() + 8 * patterns.len() + 96);
    sql.push_str(queries::PAGES_SELECT);
    let mut bind = 3usize;
    for _ in &patterns {
        bind += 1;
        sql.push_str(&format!(" AND UPPER(p.name::text) LIKE UPPER(${bind})"));
    }
    if only_owned {
        sql.push_str(" AND p.owned_by_id = $3");
    }
    sql.push_str(queries::PAGES_GROUP_BY);
    sql.push_str(order_clause(order));

    let mut values: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
        vec![&params.slug, &params.project_id, &params.user_id];
    for pattern in &patterns {
        values.push(pattern);
    }
    client
        .query(sql.as_str(), &values)
        .await
        .map_err(ViewError::from)
}

/// `PageViewSet.list` (`page/base.py:286-301`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let timezone = timezone(ctx);

        // `Project.objects.get(pk=project_id)` and the guest probe
        // (`page/base.py:288-298`). A missing project is the `DoesNotExist` the
        // family handler renders as `404 {"error": "The required object does not
        // exist."}`; `Project.objects.get` does *not* scope to the URL
        // workspace, so neither does the statement.
        let statement = client
            .prepare_cached(queries::PAGES_PROJECT_GUEST.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_opt(
                &statement,
                &[&params.user_id, &params.project_id, &params.slug],
            )
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let guest_view_all: bool = row.try_get(0).map_err(ViewError::from)?;
        let guest_member: bool = row.try_get(1).map_err(ViewError::from)?;
        // `ProjectMember…role=5…exists() and not project.guest_view_all_features`
        // (`page/base.py:289-298`).
        let only_owned = guest_member && !guest_view_all;

        let order = sanitize_order_by(ctx.query().get("order_by"));
        let terms = search_terms(ctx);
        let rows = if order == "-created_at" && !only_owned && terms.is_empty() {
            fetch_default(client, &params).await?
        } else {
            fetch_dynamic(client, &params, &terms, order, only_owned).await?
        };
        let pages = rows
            .iter()
            .map(PageRow::decode)
            .collect::<Result<Vec<_>, _>>()?;
        // `PageSerializer(queryset, many=True)` is constructed without
        // `fields=`/`expand=`, so neither parameter applies (`BaseSerializer`,
        // not `DynamicBaseSerializer`).
        let values: Vec<PageSerializer<'_>> = pages.iter().map(PageRow::serializer).collect();
        let bytes = plane_serializers::render_list(&values, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

/// `PageViewSet.retrieve` (`page/base.py:218-239`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let page_id = uuid_kwarg(ctx, "page_id")?;
        let timezone = timezone(ctx);

        let order = sanitize_order_by(ctx.query().get("order_by"));
        let terms = search_terms(ctx);
        let rows = if order == "-created_at" && terms.is_empty() {
            let statement = client
                .prepare_cached(queries::PAGES_RETRIEVE.sql)
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
            // Same statement with the search filter and the chosen ordering;
            // the pk filter keeps the result to at most one row.
            let patterns: Vec<String> = terms
                .iter()
                .map(|term| format!("%{}%", escape_like(term)))
                .collect();
            let mut sql = String::with_capacity(queries::PAGES_SELECT.len() + 128);
            sql.push_str(queries::PAGES_SELECT);
            let mut bind = 4usize;
            for _ in &patterns {
                bind += 1;
                sql.push_str(&format!(" AND UPPER(p.name::text) LIKE UPPER(${bind})"));
            }
            sql.push_str(" AND p.id = $4");
            sql.push_str(queries::PAGES_GROUP_BY);
            sql.push_str(order_clause(order));
            sql.push_str(" LIMIT 1");
            let mut values: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                vec![&params.slug, &params.project_id, &params.user_id, &page_id];
            for pattern in &patterns {
                values.push(pattern);
            }
            client
                .query(sql.as_str(), &values)
                .await
                .map_err(ViewError::from)?
        };
        let Some(row) = rows.first() else {
            // `page is None` (`page/base.py:223-224`).
            return Ok(cycle_base::error_body(404, "Page not found"));
        };
        let page = PageRow::decode(row)?;

        // `request.query_params.get("track_visit", "true").lower() == "true"`
        // (`page/base.py:220`).
        let track_visit = ctx
            .query()
            .get_or("track_visit", "true")
            .to_lowercase()
            .eq("true");
        if track_visit {
            jobs::recent_visited_entity(
                ctx,
                "page",
                TaskArg::uuid(page_id),
                Some(TaskArg::uuid(params.project_id)),
                TaskArg::uuid(params.user_id),
            )
            .await?;
        }

        let issue_ids = issue_ids(client, page_id).await?;
        let body = PageDetailBody {
            page: &page,
            issue_ids: &issue_ids,
        };
        let bytes = plane_serializers::render(&body, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

/// `PageLog.objects.filter(page_id=page_id, entity_name="issue").values_list(
/// "entity_identifier", flat=True)` (`page/base.py:226-228`).
async fn issue_ids(client: &Client, page_id: Uuid) -> Result<Vec<Option<Uuid>>, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGES_ISSUE_IDS.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&page_id])
        .await
        .map_err(ViewError::from)?;
    rows.iter()
        .map(|row| row.try_get(0).map_err(ViewError::from))
        .collect()
}

/// `PageViewSet.summary`'s `{"public_pages", "private_pages", "archived_pages"}`
/// (`page/base.py:448-464`), in the order `aggregate(**kwargs)` builds the dict.
#[derive(Debug, serde::Serialize)]
pub(crate) struct SummaryBody {
    pub(crate) public_pages: i64,
    pub(crate) private_pages: i64,
    pub(crate) archived_pages: i64,
}

/// `PageViewSet.summary` (`page/base.py:416-464`).
pub fn summary<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;

        let statement = client
            .prepare_cached(queries::PAGES_PROJECT_GUEST.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_opt(
                &statement,
                &[&params.user_id, &params.project_id, &params.slug],
            )
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let guest_view_all: bool = row.try_get(0).map_err(ViewError::from)?;
        let guest_member: bool = row.try_get(1).map_err(ViewError::from)?;
        let only_owned = guest_member && !guest_view_all;

        let query = if only_owned {
            &queries::PAGES_SUMMARY_COUNTS_GUEST
        } else {
            &queries::PAGES_SUMMARY_COUNTS
        };
        let statement = client
            .prepare_cached(query.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_one(
                &statement,
                &[&params.slug, &params.user_id, &params.project_id],
            )
            .await
            .map_err(ViewError::from)?;
        let body = SummaryBody {
            public_pages: row.try_get(0).map_err(ViewError::from)?,
            private_pages: row.try_get(1).map_err(ViewError::from)?,
            archived_pages: row.try_get(2).map_err(ViewError::from)?,
        };
        let bytes = plane_serde::to_vec(plane_serde::Dialect::Drf, &body)
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

/// `PagesDescriptionViewSet.retrieve` (`page/base.py:498-516`).
///
/// The view is a `StreamingHttpResponse` over `stream_data()`, which yields the
/// whole `description_binary` when it is truthy and `b""` otherwise
/// (`base.py:508-512`) — so exactly one chunk. `GZipMiddleware` then compresses
/// it regardless of length because the response is streaming
/// (`django/middleware/gzip.py:27-30,52-56`), and the ASGI server frames it as
/// `Transfer-Encoding: chunked` with no `Content-Length`.
///
/// A missing page is `Page.DoesNotExist`, which the family handler renders as
/// `404 {"error": "The required object does not exist."}`
/// (`plane/app/views/base.py:318-322`).
pub fn description<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let page_id = uuid_kwarg(ctx, "page_id")?;

        let statement = client
            .prepare_cached(queries::PAGES_DESCRIPTION_BINARY.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(
                &statement,
                &[&params.user_id, &page_id, &params.project_id, &params.slug],
            )
            .await
            .map_err(ViewError::from)?;
        // `.get()`: zero rows is `DoesNotExist`, more than one is
        // `MultipleObjectsReturned`; both are an `ObjectDoesNotExist`.
        if rows.len() != 1 {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let binary: Option<Vec<u8>> = rows[0].try_get(0).map_err(ViewError::from)?;
        Ok(ViewResponse::streaming_raw(
            200,
            "application/octet-stream",
            // `if binary_data: yield binary_data else: yield b""`
            binary.unwrap_or_default(),
        )
        .with_header(
            "Content-Disposition",
            "attachment; filename=\"page_description.bin\"",
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The eight values `sanitize_order_by` keeps
    /// (`plane/utils/order_queryset.py:79-84,135-156`).
    const ORDER_VALUES: &[&str] = &[
        "created_at",
        "-created_at",
        "updated_at",
        "-updated_at",
        "name",
        "-name",
        "sort_order",
        "-sort_order",
    ];

    /// `sanitize_order_by` is `plane/utils/order_queryset.py:135-156` for the
    /// page allowlist: one optional `-`, the bare name in the allowlist, and the
    /// default for everything else (empty, `--x`, an unknown field).
    #[test]
    fn sanitize_order_by_matches_the_python_sanitizer() {
        for value in ORDER_VALUES {
            assert_eq!(sanitize_order_by(Some(value)), *value);
            let bare = value.trim_start_matches('-');
            assert!(
                queries::PAGE_ORDER_BY_ALLOWLIST.contains(&bare),
                "{value} is not in the allowlist"
            );
        }
        assert_eq!(sanitize_order_by(None), "-created_at");
        assert_eq!(sanitize_order_by(Some("")), "-created_at");
        assert_eq!(sanitize_order_by(Some("--created_at")), "-created_at");
        assert_eq!(sanitize_order_by(Some("owned_by__password")), "-created_at");
    }

    /// Every sanitized value gets a clause, and only the default one is the
    /// catalogued statement's tail.
    #[test]
    fn every_order_has_a_clause() {
        for value in ORDER_VALUES {
            assert!(
                order_clause(value).starts_with(" ORDER BY is_favorite DESC, p."),
                "{value} has no clause"
            );
        }
        assert_eq!(order_clause("-created_at"), queries::PAGES_DEFAULT_ORDER);
    }
}
