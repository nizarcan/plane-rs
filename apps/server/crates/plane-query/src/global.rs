// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/global_paginator.py`, ported.
//!
//! `IssueListEndpoint` (`GET /api/workspaces/<slug>/projects/<id>/issues/`)
//! uses this paginator instead of `BasePaginator`. It differs in every detail
//! that matters:
//!
//! * the cursor is `"<page_size>:<page>:<offset>"` (the third field is unused),
//! * the defaults are `PAGINATOR_MAX_LIMIT = 1000` for both page size and
//!   maximum, and an out-of-range page size is *clamped*, not rejected,
//! * `next_cursor` is `None` (JSON `null`) on the last page — no `is_prev`,
//! * the envelope key order is `prev_cursor, cursor, next_cursor,
//!   prev_page_results, next_page_results, page_count, total_results,
//!   total_pages, results`,
//! * a malformed cursor raises `ValueError` out of `PaginateCursor.from_string`,
//!   which no view catches: the request is a 500, not a 400,
//! * the rows are converted to the *user's* timezone before rendering
//!   (`user_timezone_converter`), so `created_at`/`updated_at` carry an offset
//!   like `+09:00` instead of `Z`.

use chrono::Utc;
use chrono_tz::Tz;
use deadpool_postgres::Client;

use crate::error::QueryError;
use crate::issue::model::ValueField;
use crate::issue::query::IssueQuery;
use crate::issue::values;
use crate::json::Json;
use crate::paginator::MAX_LIMIT;

/// `PAGINATOR_MAX_LIMIT`.
pub const PAGINATOR_MAX_LIMIT: i64 = 1000;

/// `PaginateCursor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaginateCursor {
    pub current_page_size: i64,
    pub current_page: i64,
    pub offset: i64,
}

impl PaginateCursor {
    pub const DEFAULT: Self = Self {
        current_page_size: PAGINATOR_MAX_LIMIT,
        current_page: 0,
        offset: 0,
    };

    pub fn from_string(text: &str) -> Result<Self, QueryError> {
        let bits: Vec<&str> = text.split(':').collect();
        if bits.len() != 3 {
            return Err(QueryError::unsupported(
                "Invalid cursor format: Cursor must be in the format 'value:offset:is_prev'",
            ));
        }
        let parse = |value: &str| -> Result<i64, QueryError> {
            value
                .trim()
                .parse::<i64>()
                .map_err(|error| QueryError::unsupported(format!("Invalid cursor format: {error}")))
        };
        Ok(Self {
            current_page_size: parse(bits[0])?,
            current_page: parse(bits[1])?,
            offset: parse(bits[2])?,
        })
    }
}

impl std::fmt::Display for PaginateCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.current_page_size, self.current_page, self.offset
        )
    }
}

/// An extra projected column the caller appends after `fields`, in the order
/// the Python's `required_fields` list puts it.
///
/// `IssuePaginatedViewSet.list` appends `description_html` when
/// `?description=true` (`plane/app/views/issue/base.py:900-902`), after
/// `sub_issues_count` — the end of the caller's list, not a fixed position.
#[derive(Debug, Clone)]
pub struct ExtraColumn {
    pub key: &'static str,
    pub expression: crate::sql::Frag,
}

/// `paginate(base_queryset, queryset, cursor, on_result)`.
pub async fn page(
    client: &Client,
    query: &IssueQuery,
    fields: &[ValueField],
    cursor: Option<&str>,
    timezone: Tz,
) -> Result<Json, QueryError> {
    page_with(client, query, fields, &[], cursor, timezone).await
}

/// [`page`] with the columns the caller appends after its `required_fields`.
pub async fn page_with(
    client: &Client,
    query: &IssueQuery,
    fields: &[ValueField],
    extra: &[ExtraColumn],
    cursor: Option<&str>,
    timezone: Tz,
) -> Result<Json, QueryError> {
    let cursor_object = match cursor {
        None => PaginateCursor::DEFAULT,
        Some(text) => PaginateCursor::from_string(text)?,
    };
    let base = query.filtered_base();
    let total_results = count(client, &base).await?;
    let page_size = cursor_object.current_page_size.min(PAGINATOR_MAX_LIMIT);
    if page_size <= 0 {
        return Err(QueryError::unsupported(
            "division by zero in total_pages (page size is not positive)",
        ));
    }
    let total_pages = if total_results == 0 {
        0
    } else {
        (total_results + page_size - 1) / page_size
    };
    let start_index = if cursor_object.current_page > 0 {
        cursor_object.current_page * page_size
    } else {
        0
    };
    let end_index = (start_index + page_size).min(total_results);

    // `IssuePaginatedViewSet` orders by `updated_at` and never re-applies the
    // `order_by` parameter, so the page keeps the queryset's own ordering.
    let mut select = query.filtered_select();
    select.projection = values::projection_with(values::Projection::IssueListEndpoint, fields);
    for column in extra {
        let mut frag = column.expression.clone();
        frag.push_raw(" AS \"");
        frag.push_raw(column.key);
        frag.push_raw("\"");
        select.projection.push(frag);
    }
    select.order_by = vec![crate::sql::Frag::raw("issues.updated_at ASC")];
    select.distinct = false;
    select.limit = Some((end_index - start_index).max(0));
    select.offset = Some(start_index);
    let rows = fetch(client, &select.into_sql()).await?;
    let decoded = rows
        .iter()
        .map(|row| {
            let mut value = values::decode(row, fields)?;
            for (index, column) in extra.iter().enumerate() {
                let text: Option<String> = row.try_get(fields.len() + index)?;
                let Json::Object(entries) = &mut value else {
                    continue;
                };
                entries.push((column.key.to_owned(), Json::opt_string(text)));
            }
            Ok(shift_timezone(value, timezone))
        })
        .collect::<Result<Vec<_>, QueryError>>()?;

    let prev_cursor = format!("{page_size}:{}:0", cursor_object.current_page - 1);
    let current_cursor = format!("{page_size}:{}:0", cursor_object.current_page);
    let next_cursor = if end_index < total_results {
        Some(format!("{page_size}:{}:0", cursor_object.current_page + 1))
    } else {
        None
    };
    let prev_page_results = cursor_object.current_page > 0;
    let next_page_results = next_cursor.is_some();

    Ok(Json::object(vec![
        ("prev_cursor", Json::string(prev_cursor)),
        ("cursor", Json::string(current_cursor)),
        ("next_cursor", next_cursor.map_or(Json::Null, Json::string)),
        ("prev_page_results", Json::Bool(prev_page_results)),
        ("next_page_results", Json::Bool(next_page_results)),
        ("page_count", Json::Int(decoded.len() as i64)),
        ("total_results", Json::Int(total_results)),
        ("total_pages", Json::Int(total_pages)),
        ("results", Json::Array(decoded)),
    ]))
}

async fn fetch(
    client: &Client,
    sql: &crate::sql::Sql,
) -> Result<Vec<tokio_postgres::Row>, QueryError> {
    let statement = let_statement(client, &sql.text).await?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    Ok(client.query(&statement, &params).await?)
}

async fn count(client: &Client, select: &crate::sql::Select) -> Result<i64, QueryError> {
    let distinct = if select.distinct {
        Some("issues.id")
    } else {
        None
    };
    let sql = select.count_sql(distinct, None);
    crate::stats::record();
    let statement = let_statement(client, &sql.text).await?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    let row = client.query_one(&statement, &params).await?;
    Ok(row.try_get(0)?)
}

async fn let_statement(
    client: &Client,
    text: &str,
) -> Result<tokio_postgres::Statement, QueryError> {
    // Unnamed statements only; see `crate::sql` for why `prepare_cached` is
    // not used for dynamic SQL.
    Ok(client.prepare(text).await?)
}

/// `user_timezone_converter`: render `created_at`/`updated_at` in the user's
/// timezone, exactly as `astimezone()` does before DRF serialises the row.
fn shift_timezone(row: Json, timezone: Tz) -> Json {
    let Json::Object(entries) = row else {
        return row;
    };
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| {
                if (key == "created_at" || key == "updated_at")
                    && let Json::DateTime(value) = value
                {
                    let zoned = value.with_timezone(&timezone);
                    return (key, Json::DateTimeZoned(zoned.fixed_offset()));
                }
                (key, value)
            })
            .collect(),
    )
}

/// The instant a request runs at (kept next to the paginator so a handler has
/// one import for the whole read path).
pub fn now() -> chrono::DateTime<Utc> {
    Utc::now()
}

/// The maximum page size, re-exported for handlers that document it.
pub const MAX_PAGE_SIZE: u32 = MAX_LIMIT;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_defaults_to_the_maximum_page() {
        assert_eq!(PaginateCursor::DEFAULT.to_string(), "1000:0:0");
        assert_eq!(
            PaginateCursor::from_string("10:2:0").expect("cursor"),
            PaginateCursor {
                current_page_size: 10,
                current_page: 2,
                offset: 0
            }
        );
        assert!(PaginateCursor::from_string("10:2").is_err());
    }
}
