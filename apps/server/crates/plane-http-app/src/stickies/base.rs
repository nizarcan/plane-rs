// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.workspace.sticky.WorkspaceStickyViewSet`
//! (`plane/app/views/workspace/sticky.py`).
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/stickies/` | `WorkspaceStickyViewSet.list` | `sticky.py:47-59` |
//! | `GET /api/workspaces/{slug}/stickies/{pk}/` | `retrieve` (a DRF mixin action) | `sticky.py` |
//!
//! The detail route is refused by the runtime's `BaseViewSet` guard: the viewset
//! never implements `retrieve`, and its only permission class is
//! `IsAuthenticated` (`plane/app/views/base.py:103-198`), so DRF's
//! `RetrieveModelMixin` would run with no authorization. `COVERAGE.md` records
//! the two 405s the generated scenario sees. Registering the action is what
//! makes the guard reachable; the handler itself can never run (see
//! [`crate::cycles::base::guard_only`]).
//!
//! ## The body
//!
//! `list` is the only route in this group that paginates, and it does so
//! through `BasePaginator.paginate` with `default_per_page=20`
//! (`sticky.py:55-59`), i.e. `OffsetPaginator`. The envelope is the paginator's
//! dict, in its own key order; `results` is `StickySerializer(many=True)`.
//! `StickySerializer` is a plain `ModelSerializer` (`plane/app/serializers/
//! workspace.py:346`), **not** `DynamicBaseSerializer`, so neither `?fields=`
//! nor `?expand=` is read: the view never passes them to the constructor.
//!
//! ## Paginator details ported from `plane/utils/paginator.py:110-180`
//!
//! * `per_page`/`cursor` parsing and validation: [`PageQuery::parse_with`].
//! * `page_results.count() > limit` decides `next_page_results`; that count is
//!   over the same `[offset:offset+limit+1]` slice the page statement returns,
//!   so it equals `rows.len() > limit` and the extra count statement Django
//!   issues is not needed (two statements instead of three).
//! * `total_pages = ceil(total / limit)`; `math.ceil(0 / limit)` is 0, so an
//!   empty page reports `0`.
//! * The `is_prev` branch re-slices with a negative index
//!   (`results[-(limit + 1):]`), which Django refuses on a sliced queryset
//!   (`django/db/models/query.py:410-417` raises `ValueError: Negative indexing
//!   is not supported.`) — a 500, ported as such.

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::error::QueryError;
use plane_query::json::Json;
use plane_query::paginator::{Cursor, PageQuery};
use plane_serializers::generated::app_workspace::StickySerializer;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, PgJson, RenderCtx};

use crate::common::client;
use crate::common::render::RequestRender;
use crate::stickies::queries;

/// `paginate(default_per_page=20)` (`plane/app/views/workspace/sticky.py:55-59`).
const DEFAULT_PER_PAGE: u32 = 20;
/// `paginate(max_per_page=1000)` — `BasePaginator.paginate`'s default.
const MAX_PER_PAGE: u32 = 1000;

/// One page row, owned so the borrowing serializers can be built from it.
struct Row {
    id: uuid::Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    name: Option<String>,
    description: Option<JsonText>,
    description_html: String,
    description_stripped: Option<String>,
    description_binary: Option<Vec<u8>>,
    logo_props: Option<JsonText>,
    color: Option<String>,
    background_color: Option<String>,
    sort_order: f64,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    workspace: uuid::Uuid,
    owner: uuid::Uuid,
}

impl Row {
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            created_at: row.try_get(1).map_err(ViewError::from)?,
            updated_at: row.try_get(2).map_err(ViewError::from)?,
            deleted_at: row.try_get(3).map_err(ViewError::from)?,
            name: row.try_get(4).map_err(ViewError::from)?,
            description: row.try_get(5).map_err(ViewError::from)?,
            description_html: row.try_get(6).map_err(ViewError::from)?,
            description_stripped: row.try_get(7).map_err(ViewError::from)?,
            description_binary: row.try_get(8).map_err(ViewError::from)?,
            logo_props: row.try_get(9).map_err(ViewError::from)?,
            color: row.try_get(10).map_err(ViewError::from)?,
            background_color: row.try_get(11).map_err(ViewError::from)?,
            sort_order: row.try_get(12).map_err(ViewError::from)?,
            created_by: row.try_get(13).map_err(ViewError::from)?,
            updated_by: row.try_get(14).map_err(ViewError::from)?,
            workspace: row.try_get(15).map_err(ViewError::from)?,
            owner: row.try_get(16).map_err(ViewError::from)?,
        })
    }

    fn value(&self) -> StickySerializer<'_> {
        StickySerializer {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            name: self.name.as_deref(),
            description: self.description.as_ref().map(PgJson::from),
            description_html: &self.description_html,
            description_stripped: self.description_stripped.as_deref(),
            description_binary: self.description_binary.as_deref(),
            logo_props: self.logo_props.as_ref().map(PgJson::from),
            color: self.color.as_deref(),
            background_color: self.background_color.as_deref(),
            sort_order: self.sort_order,
            created_by: self.created_by,
            updated_by: self.updated_by,
            workspace: self.workspace,
            owner: self.owner,
        }
    }
}

/// `WorkspaceStickyViewSet.list` (`sticky.py:47-59`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let owner = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

        let page =
            match PageQuery::parse_with(ctx.query(), DEFAULT_PER_PAGE, MAX_PER_PAGE, None, None) {
                Ok(page) => page,
                Err(error) => return map_query_error(error),
            };

        let limit = i64::from(page.per_page.min(plane_query::paginator::MAX_LIMIT));
        let offset = match page.cursor.offset.checked_mul(limit) {
            Some(offset) => offset,
            // Python has unbounded ints, so the slice simply became an
            // unreachable offset; the statement fails in Postgres instead.
            None => return Err(ViewError::Unknown("pagination offset overflow".to_owned())),
        };
        // `paginator.py:145,160`: the slice is evaluated first, then (for a
        // reverse page whose cursor value differs from the page size) re-sliced
        // with a negative index, which a sliced QuerySet rejects.
        if page.cursor.value != f64::from(page.per_page) && page.cursor.is_prev {
            return Err(ViewError::Unknown(
                "ValueError: Negative indexing is not supported.".to_owned(),
            ));
        }

        // `query = request.query_params.get("query", False)` — an empty value is
        // falsy, so `?query=` does not filter (`sticky.py:48-53`).
        let needle = ctx
            .query()
            .get("query")
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{}%", escape_like(value)));

        let client = client(ctx)?;
        let stop = limit + 1;
        let rows = match &needle {
            Some(needle) => {
                let statement = client
                    .prepare_cached(queries::STICKY_PAGE_FILTERED.sql)
                    .await
                    .map_err(ViewError::from)?;
                client
                    .query(&statement, &[&slug, &owner, &stop, &offset, needle])
                    .await
                    .map_err(ViewError::from)?
            }
            None => {
                let statement = client
                    .prepare_cached(queries::STICKY_PAGE.sql)
                    .await
                    .map_err(ViewError::from)?;
                client
                    .query(&statement, &[&slug, &owner, &stop, &offset])
                    .await
                    .map_err(ViewError::from)?
            }
        };

        let total: i64 = match &needle {
            Some(needle) => {
                let statement = client
                    .prepare_cached(queries::STICKY_COUNT_FILTERED.sql)
                    .await
                    .map_err(ViewError::from)?;
                client
                    .query_one(&statement, &[&slug, &owner, needle])
                    .await
                    .map_err(ViewError::from)?
                    .get(0)
            }
            None => {
                let statement = client
                    .prepare_cached(queries::STICKY_COUNT.sql)
                    .await
                    .map_err(ViewError::from)?;
                client
                    .query_one(&statement, &[&slug, &owner])
                    .await
                    .map_err(ViewError::from)?
                    .get(0)
            }
        };

        let next_page_results = rows.len() as i64 > limit;
        let shown = rows.len().min(limit as usize);
        let mut decoded = Vec::with_capacity(shown);
        for row in rows.iter().take(shown) {
            decoded.push(Row::decode(row)?);
        }
        let stickies: Vec<StickySerializer<'_>> = decoded.iter().map(Row::value).collect();

        let next = Cursor::new(limit, page.cursor.offset + 1, false, next_page_results);
        let previous = Cursor::new(limit, page.cursor.offset - 1, true, page.cursor.offset > 0);
        let total_pages = if total == 0 {
            0
        } else {
            // `math.ceil(count / limit)`.
            (total + limit - 1) / limit
        };

        let envelope = Page {
            total,
            next: next.to_string(),
            previous: previous.to_string(),
            next_page_results,
            prev_page_results: page.cursor.offset > 0,
            count: shown as i64,
            total_pages,
            items: &stickies,
        };

        let prepared = RequestRender::from_ctx(ctx);
        let body = plane_serializers::render(&envelope, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// The action the `BaseViewSet` guard refuses; see the module docs.
pub fn guard_only<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    crate::cycles::base::guard_only(ctx)
}

/// The paginator's response dict (`paginator.py:740-755`), in key order.
#[derive(Debug)]
struct Page<'a> {
    total: i64,
    next: String,
    previous: String,
    next_page_results: bool,
    prev_page_results: bool,
    count: i64,
    total_pages: i64,
    items: &'a [StickySerializer<'a>],
}

impl DrfSerialize for Page<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.null_field("grouped_by")?;
        obj.null_field("sub_grouped_by")?;
        obj.field("total_count", &self.total)?;
        obj.field("next_cursor", &self.next)?;
        obj.field("prev_cursor", &self.previous)?;
        obj.field("next_page_results", &self.next_page_results)?;
        obj.field("prev_page_results", &self.prev_page_results)?;
        obj.field("count", &self.count)?;
        obj.field("total_pages", &self.total_pages)?;
        obj.field("total_results", &self.total)?;
        obj.null_field("extra_stats")?;
        obj.field_nested_list("results", ctx, self.items)?;
        obj.end();
        Ok(())
    }
}

/// `django/db/backends/utils.py` `prep_for_like_query`: `\`, `%` and `_` are
/// escaped so a user-typed `%` is a literal, not a wildcard.
fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

/// `QueryError` → the response the family `handle_exception` would produce.
pub(crate) fn map_query_error(error: QueryError) -> Result<ViewResponse, ViewError> {
    match error {
        QueryError::Api(api) => {
            let detail = api.detail();
            let body = if api.code.is_none() && api.errors.is_none() {
                Json::object(vec![("detail", detail)])
            } else {
                detail
            };
            Ok(ViewResponse::raw(
                api.status,
                APPLICATION_JSON,
                body.to_drf(),
            ))
        }
        QueryError::BadRequest(message) => Ok(ViewResponse::raw(
            400,
            APPLICATION_JSON,
            Json::object(vec![("error", Json::string(message))]).to_drf(),
        )),
        other => Err(ViewError::Unknown(other.message())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_needles_are_escaped_like_django() {
        assert_eq!(escape_like("a%b_c\\d"), "a\\%b\\_c\\\\d");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn the_paginator_envelope_is_in_key_order() {
        let page = Page {
            total: 1,
            next: "20:1:0".to_owned(),
            previous: "20:-1:1".to_owned(),
            next_page_results: false,
            prev_page_results: false,
            count: 1,
            total_pages: 1,
            items: &[],
        };
        let body =
            plane_serializers::render(&page, &RenderCtx::full(&chrono_tz::UTC)).expect("render");
        assert_eq!(
            String::from_utf8(body).expect("utf-8"),
            r#"{"grouped_by":null,"sub_grouped_by":null,"total_count":1,"next_cursor":"20:1:0","prev_cursor":"20:-1:1","next_page_results":false,"prev_page_results":false,"count":1,"total_pages":1,"total_results":1,"extra_stats":null,"results":[]}"#
        );
    }
}
