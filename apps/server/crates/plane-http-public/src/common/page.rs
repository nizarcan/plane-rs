// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `BasePaginator.paginate` + `OffsetPaginator` for a v1 list.
//!
//! Every paginated `/api/v1/` list goes through
//! `plane.utils.paginator.BasePaginator.paginate` with the default
//! `OffsetPaginator` (`plane/utils/paginator.py:635-755`). The cursor format,
//! the slice arithmetic and the envelope are the app surface's; what differs is
//! that v1 never passes `order_by` (the queryset's own order survives) and only
//! the issue list passes `total_count_queryset` (`plane/api/views/issue.py:508-513`),
//! so `total_count` counts every issue a restricted guest cannot see.
//!
//! The envelope is
//!
//! ```text
//! grouped_by, sub_grouped_by, total_count, next_cursor, prev_cursor,
//! next_page_results, prev_page_results, count, total_pages, total_results,
//! extra_stats, results
//! ```
//!
//! with `grouped_by`/`sub_grouped_by`/`extra_stats` always `null` on this
//! surface (no v1 list groups and none passes `extra_stats`), `count` the
//! number of rows on this page, and `total_pages = ceil(total / per_page)` —
//! `0` for an empty queryset (`paginator.py:172-174`).
//!
//! Three v1 lists are **not** paginated and must not call [`page`]: project
//! members and workspace members (plain arrays, `member.py:77-99,129-149`),
//! the invitations list (`invite.py:55-58`), `users/me`, both api-roots, search
//! (`issue.py:2502`), the relations object, the `external_id` single-object
//! answer, and `?cycle_view=current` (`cycle.py:203-212`).

use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;
use plane_query::Json;
use plane_query::paginator::{
    Cursor, MAX_LIMIT, OffsetPage, OffsetWindow, PageQuery, ceil_div, envelope,
};

/// Parse `?per_page=`/`?cursor=` the way `BasePaginator.paginate` does.
///
/// `default_per_page`/`max_per_page` are the view's `paginate(...)` arguments;
/// the effective cap is `max(max_per_page, default_per_page)`
/// (`paginator.py:658-660`). A bad value is DRF's
/// `400 {"detail": "Invalid per_page parameter."}` (or the cursor message),
/// which is a `ParseError` — not the family's `{"error": …}` shape.
///
/// # Errors
///
/// [`ViewError::Api`] with `ApiError::ParseError` for `per_page`/`cursor`
/// values Django rejects.
pub fn query(
    ctx: &ViewCtx<'_>,
    default_per_page: u32,
    max_per_page: u32,
) -> Result<PageQuery, ViewError> {
    PageQuery::parse_with(ctx.query(), default_per_page, max_per_page, None, None)
        .map_err(query_error)
}

/// [`query`] with `paginate()`'s defaults (1000/1000), which every v1 list
/// except the stickies view uses.
///
/// # Errors
///
/// See [`query`].
pub fn default_query(ctx: &ViewCtx<'_>) -> Result<PageQuery, ViewError> {
    query(ctx, MAX_LIMIT, MAX_LIMIT)
}

/// `BasePaginator.get_per_page` alone, for a view that composes its own
/// statement instead of using [`plane_query::paginator::offset_page`].
///
/// # Errors
///
/// See [`query`].
pub fn per_page_only(
    ctx: &ViewCtx<'_>,
    default_per_page: u32,
    max_per_page: u32,
) -> Result<u32, ViewError> {
    plane_query::paginator::per_page(ctx.query(), default_per_page, max_per_page)
        .map_err(query_error)
}

/// The `Response({...})` dict for one plain page (`paginator.py:733-755`).
///
/// `results` is already serialised: `on_results(cursor_result.results)` runs
/// before the envelope is built, and `paginate` puts its return value straight
/// into `results`. `count` is that list's length (`CursorResult.__len__`), so
/// it is read off `results`; a non-array `results` cannot happen on this
/// surface (every v1 list answers an array) and falls back to the page size.
#[must_use]
pub fn page(page: &OffsetPage, results: Json) -> Json {
    let count = match &results {
        Json::Array(items) => items.len(),
        _ => page.rows.len(),
    };
    envelope(
        None,
        None,
        page.total,
        page.next.clone(),
        page.prev.clone(),
        count,
        ceil_div(page.total, page.window.limit),
        results,
    )
}

/// The same envelope with an already-rendered `results` array.
///
/// A handler that renders each row through its own serializer (a manual
/// serializer, `expand=` resolutions, an issue bundle) cannot use [`page`]'s
/// `Json` path without re-parsing the bytes, so it builds the array in place.
#[must_use]
pub fn page_with_bytes(
    total: i64,
    window: &OffsetWindow,
    next: &Cursor,
    prev: &Cursor,
    count: usize,
    results: &[u8],
) -> Vec<u8> {
    let total_pages = ceil_div(total, window.limit);
    let head = format!(
        "{{\"grouped_by\":null,\"sub_grouped_by\":null,\"total_count\":{total},\"next_cursor\":\"{next}\",\"prev_cursor\":\"{prev}\",\"next_page_results\":{},\"prev_page_results\":{},\"count\":{count},\"total_pages\":{total_pages},\"total_results\":{total},\"extra_stats\":null,\"results\":",
        next.has_results, prev.has_results,
    );
    let mut body = head.into_bytes();
    body.extend_from_slice(results);
    body.push(b'}');
    body
}

/// `QueryError` → the DRF error the request answers with.
fn query_error(error: plane_query::QueryError) -> ViewError {
    match error {
        plane_query::QueryError::Api(api) => {
            ViewError::Api(plane_core::error::ApiError::ParseError(api.message))
        }
        other => ViewError::Unknown(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_query::paginator::{Cursor, OffsetWindow};

    fn cursors(limit: i64, page_index: i64, has_results: bool) -> (Cursor, Cursor) {
        (
            Cursor::new(limit, page_index + 1, false, has_results),
            Cursor::new(limit, page_index - 1, true, page_index > 0),
        )
    }

    /// One page of a 5-row queryset at `per_page=2`, page 0: Django's
    /// `OffsetPaginator` answer, from `plane/utils/paginator.py:733-755`.
    #[test]
    fn a_plain_page_has_the_python_envelope() {
        let (next, prev) = cursors(2, 0, true);
        let page = OffsetPage {
            rows: Vec::new(),
            total: 5,
            window: OffsetWindow {
                limit: 2,
                page_index: 0,
                offset: 0,
            },
            next,
            prev,
        };
        let body = super::page(&page, Json::Array(vec![Json::Int(1), Json::Int(2)]));
        assert_eq!(
            body.to_drf(),
            br#"{"grouped_by":null,"sub_grouped_by":null,"total_count":5,"next_cursor":"2:1:0","prev_cursor":"2:-1:1","next_page_results":true,"prev_page_results":false,"count":2,"total_pages":3,"total_results":5,"extra_stats":null,"results":[1,2]}"#
                .to_vec()
        );
    }

    /// `total_pages` is `math.ceil(total / limit)`, and `0` when there are no
    /// rows (`paginator.py:172-174`).
    #[test]
    fn total_pages_rounds_up_and_is_zero_when_empty() {
        assert_eq!(ceil_div(0, 1000), 0);
        assert_eq!(ceil_div(1, 1000), 1);
        assert_eq!(ceil_div(1000, 1000), 1);
        assert_eq!(ceil_div(1001, 1000), 2);
        assert_eq!(ceil_div(5, 2), 3);
    }
}
