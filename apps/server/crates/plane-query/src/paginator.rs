// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/paginator.py`, ported.
//!
//! Three paginators share one envelope:
//!
//! ```text
//! grouped_by, sub_grouped_by, total_count, next_cursor, prev_cursor,
//! next_page_results, prev_page_results, count, total_pages, total_results,
//! extra_stats, results
//! ```
//!
//! The cursor is `"<value>:<offset>:<is_prev>"`; the default is
//! `"<per_page>:0:0"`. `per_page` defaults to 1000 and may not exceed 1000
//! (`BasePaginator.get_per_page`), and a cursor whose value is outside
//! `0..=1000` or whose offset is negative is a 400. `?per_page=` is a 400 too:
//! Django calls `int()` on the empty string (`paginator.py:645`).
//!
//! ## What the counts mean
//!
//! * Plain paginator: `total_count`/`total_results` is
//!   `total_count_queryset.count()`, which the view sets to the *filtered*
//!   queryset before annotations — distinct issues, because `get_queryset()`
//!   ends in `.distinct()`.
//! * Grouped paginator: `count = queryset.count()` on the *windowed* query.
//!   Because the row number is part of the inner `SELECT DISTINCT`, duplicate
//!   join rows survive it, so `total_count` is the number of joined rows (412
//!   for 225 issues and six labels in the fixture), not the number of issues.
//! * `total_pages` is `ceil(largest group / per_page)`, from
//!   `COUNT(DISTINCT id) FILTER (…)`; `total_results` per group is the same
//!   filtered count, and a group whose filtered count is 0 reports **1**
//!   (`paginator.py:313`).
//! * `count` is the number of rows on this page, not the number of groups.
//!
//! ## Round trips
//!
//! Django issues three queries for a plain page (page, page-count,
//! total-count) and four to six for a grouped one (page, `exists`, `count`,
//! `max_groups`, group totals [, sub-group totals]). This port issues two for a
//! plain page (one extra row replaces the page-count) and three for a grouped
//! one (one extra row per group replaces `exists`, and the largest group comes
//! from the totals it needs anyway). See `crates/plane-query/README.md`.

use chrono::Utc;
use chrono_tz::Tz;
use deadpool_postgres::Client;
use uuid::Uuid;

use crate::error::QueryError;
use crate::issue::model::{GroupField, ValueField};
use crate::issue::query::{GroupPlan, IssueQuery, group_key};
use crate::issue::values;
use crate::json::Json;
use crate::sql::Sql;

/// `MAX_LIMIT` (`paginator.py:87`).
pub const MAX_LIMIT: u32 = 1000;

/// `Cursor`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cursor {
    /// The value as Python sees it (`int` or `float`).
    pub value: f64,
    /// The value as it is written back out (`"13"` or `"13.0"`).
    pub value_repr: String,
    pub offset: i64,
    pub is_prev: bool,
    pub has_results: bool,
}

impl Cursor {
    pub fn new(value: i64, offset: i64, is_prev: bool, has_results: bool) -> Self {
        Self {
            value: value as f64,
            value_repr: value.to_string(),
            offset,
            is_prev,
            has_results,
        }
    }

    /// `Cursor.from_string`: floats are allowed for the value; a malformed
    /// cursor is a 400 (`parse()`, not this function, raises).
    pub fn from_string(text: &str) -> Result<Self, QueryError> {
        let bits: Vec<&str> = text.split(':').collect();
        if bits.len() != 3 {
            return Err(QueryError::parse("Invalid cursor parameter."));
        }
        let value = if bits[0].contains('.') {
            PythonFloat::parse(bits[0])
                .ok_or_else(|| QueryError::parse("Invalid cursor parameter."))?
        } else {
            let parsed = PythonInt::parse(bits[0])
                .ok_or_else(|| QueryError::parse("Invalid cursor parameter."))?;
            (parsed as f64, parsed.to_string())
        };
        let offset = PythonInt::parse(bits[1])
            .ok_or_else(|| QueryError::parse("Invalid cursor parameter."))?;
        let is_prev = PythonInt::parse(bits[2])
            .ok_or_else(|| QueryError::parse("Invalid cursor parameter."))?;
        Ok(Self {
            value: value.0,
            value_repr: value.1,
            offset,
            is_prev: is_prev != 0,
            has_results: false,
        })
    }
}

impl std::fmt::Display for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.value_repr,
            self.offset,
            i32::from(self.is_prev)
        )
    }
}

/// Python's `int(text)`: surrounding whitespace, an optional sign and
/// underscores between digits.
struct PythonInt;

impl PythonInt {
    fn parse(text: &str) -> Option<i64> {
        let trimmed = text.trim();
        let (sign, digits) = match trimmed.as_bytes().first() {
            Some(b'-') => (-1i64, &trimmed[1..]),
            Some(b'+') => (1, &trimmed[1..]),
            _ => (1, trimmed),
        };
        if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') {
            return None;
        }
        let mut seen_digit = false;
        let mut previous_underscore = false;
        for ch in digits.chars() {
            if ch == '_' {
                if previous_underscore || !seen_digit {
                    return None;
                }
                previous_underscore = true;
                continue;
            }
            if !ch.is_ascii_digit() {
                return None;
            }
            previous_underscore = false;
            seen_digit = true;
        }
        if previous_underscore {
            return None;
        }
        digits
            .replace('_', "")
            .parse::<i64>()
            .ok()
            .map(|value| sign * value)
    }
}

/// Python's `float(text)` for the cursor's value.
struct PythonFloat;

impl PythonFloat {
    fn parse(text: &str) -> Option<(f64, String)> {
        let trimmed = text.trim();
        let value: f64 = trimmed.replace('_', "").parse().ok()?;
        if !value.is_finite() {
            return None;
        }
        Some((value, crate::pyfloat::repr(value)))
    }
}

/// `BasePaginator.get_per_page` (`paginator.py:642-657`).
///
/// The Python is `int(request.GET.get("per_page", default_per_page))`: a
/// **missing** parameter takes the default, while `?per_page=` (present, empty)
/// is `int("")` and raises `ValueError`, which becomes
/// `ParseError("Invalid per_page parameter.")`. `plane_core::py::int` is
/// CPython's `int()` (surrounding whitespace, one sign, Unicode decimal digits,
/// underscores between digits), so every string Django rejects is rejected
/// here with the same message.
pub fn per_page(
    query: &plane_drf::QueryDict,
    default_per_page: u32,
    max_per_page: u32,
) -> Result<u32, QueryError> {
    let effective_max = max_per_page.max(default_per_page);
    let parsed = match query.get("per_page") {
        Some(raw) => match plane_core::py::int(raw) {
            Ok(value) => value,
            // Python's `int()` is unbounded: an integer outside `i64` is still
            // parsed, and then fails the positive/maximum check below. The
            // message must be that check's, not the parse error.
            Err(plane_core::py::PyIntError::Overflow) => {
                return Err(QueryError::parse(if raw.trim_start().starts_with('-') {
                    "Invalid per_page value. Must be greater than zero.".to_string()
                } else {
                    format!("Invalid per_page value. Cannot exceed {effective_max}.")
                }));
            }
            Err(plane_core::py::PyIntError::Invalid) => {
                return Err(QueryError::parse("Invalid per_page parameter."));
            }
        },
        None => i64::from(default_per_page),
    };
    if parsed <= 0 {
        return Err(QueryError::parse(
            "Invalid per_page value. Must be greater than zero.",
        ));
    }
    if parsed > i64::from(effective_max) {
        return Err(QueryError::parse(format!(
            "Invalid per_page value. Cannot exceed {effective_max}."
        )));
    }
    Ok(parsed as u32)
}

/// Everything one page needs, already parsed.
#[derive(Debug, Clone)]
pub struct PageQuery {
    pub per_page: u32,
    pub cursor: Cursor,
    /// `cursor_cls.from_string(request.GET.get(cursor_name, f"{per_page}:0:0"))`.
    pub raw_cursor: Option<String>,
}

impl PageQuery {
    /// `BasePaginator.paginate`'s first two steps, including the allowlist
    /// checks that happen before the paginator is built.
    ///
    /// `MAX_LIMIT` is both the default and the cap, which is what every issue
    /// list passes (`plane/app/views/issue/base.py`).
    pub fn parse(
        query: &plane_drf::QueryDict,
        group_by: Option<&str>,
        sub_group_by: Option<&str>,
    ) -> Result<Self, QueryError> {
        Self::parse_with(query, MAX_LIMIT, MAX_LIMIT, group_by, sub_group_by)
    }

    /// [`PageQuery::parse`] with the `paginate(default_per_page=…,
    /// max_per_page=…)` arguments the caller's view passes. The effective cap
    /// is `max(max_per_page, default_per_page)` (`paginator.py:658-660`).
    pub fn parse_with(
        query: &plane_drf::QueryDict,
        default_per_page: u32,
        max_per_page: u32,
        group_by: Option<&str>,
        sub_group_by: Option<&str>,
    ) -> Result<Self, QueryError> {
        let per_page = per_page(query, default_per_page, max_per_page)?;
        let raw_cursor = query.get("cursor").map(str::to_owned);
        let cursor =
            Cursor::from_string(raw_cursor.as_deref().unwrap_or(&format!("{per_page}:0:0")))?;
        let effective_max = f64::from(max_per_page.max(default_per_page));
        if !(0.0..=effective_max).contains(&cursor.value) || cursor.offset < 0 {
            return Err(QueryError::parse("Invalid cursor parameter."));
        }
        if let Some(group) = group_by
            && GroupField::parse(group).is_none()
        {
            return Err(QueryError::parse(format!(
                "Invalid group_by field: {group}"
            )));
        }
        if let Some(group) = sub_group_by
            && GroupField::parse(group).is_none()
        {
            return Err(QueryError::parse(format!(
                "Invalid sub_group_by field: {group}"
            )));
        }
        Ok(Self {
            per_page,
            cursor,
            raw_cursor,
        })
    }
}

fn params_of(sql: &Sql) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
    sql.params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect()
}

async fn fetch(client: &Client, sql: &Sql) -> Result<Vec<tokio_postgres::Row>, QueryError> {
    crate::stats::record();
    let statement = client.prepare(&sql.text).await?;
    Ok(client.query(&statement, &params_of(sql)).await?)
}

async fn fetch_count(client: &Client, sql: &Sql) -> Result<i64, QueryError> {
    crate::stats::record();
    let statement = client.prepare(&sql.text).await?;
    let row = client.query_one(&statement, &params_of(sql)).await?;
    Ok(row.try_get(0)?)
}

/// Run one page of an issue list and build the response object.
///
/// `group` is `None` for the plain paginator; for the grouped ones it holds the
/// group keys `issue_group_values` produced.
pub async fn issue_list_page(
    client: &Client,
    query: &IssueQuery,
    group: Option<&GroupPlan>,
    page: &PageQuery,
) -> Result<Json, QueryError> {
    let limit = i64::from(page.per_page.min(MAX_LIMIT));
    let page_index = page.cursor.offset;
    let offset = page_index * limit;

    let Some(plan) = group else {
        let fields = ValueField::project(None, None);
        let select = query.page(&fields, limit, offset);
        let rows = fetch(client, &select.into_sql()).await?;
        let has_results = rows.len() as i64 > limit;
        let rows = truncate(rows, limit as usize);
        let distinct = if query.scope.distinct {
            Some("issues.id")
        } else {
            None
        };
        let total = fetch_count(client, &query.filtered_base().count_sql(distinct, None)).await?;
        let results = decode_rows(&rows, &fields)?;
        let count = results.len();
        return Ok(envelope(
            None,
            None,
            total,
            Cursor::new(limit, page_index + 1, false, has_results),
            Cursor::new(limit, page_index - 1, true, page_index > 0),
            count,
            ceil_div(total, limit),
            Json::Array(results),
        ));
    };

    let group_field = plan.group_by;
    let sub_group_field = plan.sub_group_by;
    let fields = ValueField::project(Some(group_field), sub_group_field);
    let select = query.grouped_page(&fields, limit, offset, true);
    let rows = fetch(client, &select.into_sql()).await?;

    // One extra row per partition answers `queryset.filter(row_number__gte=stop).exists()`.
    let group_index = fields
        .iter()
        .position(|field| *field == group_field.value_field());
    let sub_index =
        sub_group_field.and_then(|sub| fields.iter().position(|field| *field == sub.value_field()));
    let mut seen: Vec<(String, Option<String>, i64)> = Vec::new();
    let mut has_results = false;
    let mut kept: Vec<tokio_postgres::Row> = Vec::new();
    for row in rows {
        let key = match group_index {
            Some(index) => group_key_of(&row, index, group_field)?,
            None => String::new(),
        };
        let sub_key = match (sub_index, sub_group_field) {
            (Some(index), Some(field)) => Some(group_key_of(&row, index, field)?),
            _ => None,
        };
        let count = match seen
            .iter_mut()
            .find(|(existing, existing_sub, _)| *existing == key && *existing_sub == sub_key)
        {
            Some((_, _, count)) => {
                *count += 1;
                *count
            }
            None => {
                seen.push((key.clone(), sub_key.clone(), 1));
                1
            }
        };
        if count > limit {
            has_results = true;
            continue;
        }
        kept.push(row);
    }

    let hits = fetch_count(client, &query.grouped_count(&fields)).await?;
    let page_count = fetch_count(client, &query.grouped_page_count(&fields, limit, offset)).await?;
    let totals = fetch(client, &query.totals(group_field)).await?;
    let group_totals: Vec<(String, i64)> = totals
        .iter()
        .map(|row| {
            // The group column keeps its SQL type (uuid, text or date), so it
            // is rendered by the same helper the group-value queries use.
            let key = crate::issue::query::render_group_value_at(row, 0, group_field)?;
            let count: i64 = row.try_get(1).map_err(QueryError::from)?;
            // `paginator.py:313`: a group whose filtered count is 0 reports 1.
            Ok((key, if count == 0 { 1 } else { count }))
        })
        .collect::<Result<Vec<_>, QueryError>>()?;

    let sub_totals: Vec<(String, String, i64)> = match sub_group_field {
        Some(sub) => {
            let rows = fetch(client, &query.sub_totals(group_field, sub)).await?;
            rows.iter()
                .map(|row| {
                    let group = crate::issue::query::render_group_value_at(row, 0, group_field)?;
                    let sub = crate::issue::query::render_group_value_at(row, 1, sub)?;
                    let count: i64 = row.try_get(2).map_err(QueryError::from)?;
                    Ok((group, sub, count))
                })
                .collect::<Result<Vec<_>, QueryError>>()?
        }
        None => Vec::new(),
    };

    let decoded = decode_rows(&kept, &fields)?;
    let results = shape_results(
        plan,
        group_field,
        sub_group_field,
        &decoded,
        &group_totals,
        &sub_totals,
    )?;

    let max_group = group_totals
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(0);
    let max_hits = if kept.is_empty() {
        0
    } else {
        ceil_div(max_group, limit)
    };

    Ok(envelope(
        Some(group_field),
        sub_group_field,
        hits,
        Cursor::new(limit, page_index + 1, false, has_results),
        Cursor::new(limit, page_index - 1, true, page_index > 0),
        page_count as usize,
        max_hits,
        results,
    ))
}

fn ceil_div(value: i64, by: i64) -> i64 {
    if by <= 0 {
        return 0;
    }
    (value + by - 1) / by
}

fn truncate<T>(mut values: Vec<T>, limit: usize) -> Vec<T> {
    values.truncate(limit);
    values
}

fn decode_rows(
    rows: &[tokio_postgres::Row],
    fields: &[ValueField],
) -> Result<Vec<Json>, QueryError> {
    rows.iter().map(|row| values::decode(row, fields)).collect()
}

fn group_key_of(
    row: &tokio_postgres::Row,
    index: usize,
    field: GroupField,
) -> Result<String, QueryError> {
    Ok(match field {
        GroupField::Priority | GroupField::StateGroup => {
            let value: Option<String> = row.try_get(index).map_err(QueryError::from)?;
            crate::issue::group_values::render_string(value.as_deref())
        }
        GroupField::TargetDate | GroupField::StartDate => {
            let value: Option<chrono::NaiveDate> = row.try_get(index).map_err(QueryError::from)?;
            crate::issue::group_values::render_date(value)
        }
        _ => {
            let value: Option<Uuid> = row.try_get(index).map_err(QueryError::from)?;
            crate::issue::group_values::render_uuid(value)
        }
    })
}

/// The `results` half of the envelope.
fn shape_results(
    plan: &GroupPlan,
    group: GroupField,
    sub_group: Option<GroupField>,
    rows: &[Json],
    group_totals: &[(String, i64)],
    sub_totals: &[(String, String, i64)],
) -> Result<Json, QueryError> {
    let multi = group.mapper().is_some() || sub_group.is_some_and(|field| field.mapper().is_some());
    match (multi, sub_group) {
        // `__query_multi_grouper`, single group.
        (true, None) => single_multi_group(plan, group, rows, group_totals),
        // `__query_grouper`, single group.
        (false, None) => single_group(plan, group, rows, group_totals),
        // `__query_multi_grouper`, group + sub group.
        (true, Some(sub)) => multi_sub_group(plan, group, sub, rows, group_totals, sub_totals),
        // `__query_grouper`, group + sub group (blind indexing: a missing key
        // is the KeyError Django raises).
        (false, Some(sub)) => plain_sub_group(plan, group, sub, rows, group_totals, sub_totals),
    }
}

fn row_group_key(row: &Json, field: GroupField) -> String {
    match row {
        Json::Object(entries) => entries
            .iter()
            .find(|(key, _)| key == field.as_str())
            .map(|(_, value)| group_key(value))
            .unwrap_or_else(|| "None".to_owned()),
        _ => "None".to_owned(),
    }
}

fn row_id(row: &Json) -> Option<String> {
    match row {
        Json::Object(entries) => entries
            .iter()
            .find(|(key, _)| key == "id")
            .map(|(_, value)| group_key(value)),
        _ => None,
    }
}

/// The group ids of each issue, in first-appearance order, for one m2m group
/// field (`result_group_mapping` in the Python).
fn accumulate_groups(rows: &[Json], field: GroupField) -> Vec<(String, Vec<String>)> {
    let mut per_issue: Vec<(String, Vec<String>)> = Vec::new();
    for row in rows {
        let Some(id) = row_id(row) else { continue };
        let key = row_group_key(row, field);
        match per_issue.iter_mut().find(|(existing, _)| *existing == id) {
            Some((_, keys)) => {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            None => per_issue.push((id, vec![key])),
        }
    }
    per_issue
}

/// Set the mapper array on a row, the way `__query_multi_grouper` does:
/// every occurrence of the issue carries the issue's *full* group-id list.
fn annotate_arrays(
    row: &Json,
    group: Option<GroupField>,
    sub: Option<GroupField>,
    group_map: &[(String, Vec<String>)],
    sub_map: &[(String, Vec<String>)],
) -> Json {
    let mut row = row.clone();
    let Json::Object(entries) = &mut row else {
        return row;
    };
    let id = entries
        .iter()
        .find(|(key, _)| key == "id")
        .map(|(_, value)| group_key(value));
    for (field, map) in [(group, group_map), (sub, sub_map)] {
        let Some(field) = field else { continue };
        let Some(mapper) = field.mapper() else {
            continue;
        };
        let keys = id
            .as_ref()
            .and_then(|id| map.iter().find(|(existing, _)| existing == id))
            .map(|(_, keys)| keys.clone())
            .unwrap_or_default();
        entries.push((
            mapper.to_owned(),
            if keys.iter().any(|key| key == "None") {
                Json::Array(Vec::new())
            } else {
                let mut keys = keys.clone();
                keys.sort();
                Json::Array(keys.into_iter().map(Json::string).collect())
            },
        ));
    }
    row
}

fn single_multi_group(
    plan: &GroupPlan,
    group: GroupField,
    rows: &[Json],
    group_totals: &[(String, i64)],
) -> Result<Json, QueryError> {
    let per_issue = accumulate_groups(rows, group);
    // Every occurrence of an issue is appended to *every* group the issue
    // belongs to (only the first per group survives), so a group's entry for
    // an issue is the issue's first page row — which may carry another group's
    // key in its own group column.
    let mut groups: Vec<(String, Vec<Json>)> = Vec::new();
    for row in rows {
        let Some(id) = row_id(row) else { continue };
        let Some((_, keys)) = per_issue.iter().find(|(existing, _)| *existing == id) else {
            continue;
        };
        let row = annotate_arrays(row, Some(group), None, &per_issue, &[]);
        let Some(identifier) = row_id(&row) else {
            continue;
        };
        for key in keys {
            // Which of an issue's rows becomes the group's entry depends on the
            // order Postgres returns rows that share `created_at` — undefined.
            // The row is filed under `key`, so its group column is normalised
            // to `key`; the id array still lists every group of the issue.
            let row = set_group_value(&row, group, key);
            match groups.iter_mut().find(|(existing, _)| existing == key) {
                Some((_, entries)) => {
                    // `__result_already_added`: the first row of an issue wins,
                    // the rest are skipped.
                    if !entries
                        .iter()
                        .any(|existing| row_id(existing).as_deref() == Some(identifier.as_str()))
                    {
                        entries.push(row.clone());
                    }
                }
                None => groups.push((key.clone(), vec![row.clone()])),
            }
        }
    }
    // The Python's group order comes from a `set`; the golden sorts it, and so
    // does this (see the oracle's `sort_group_keys`).
    groups.sort_by(|left, right| left.0.cmp(&right.0));
    let _ = plan;
    Ok(Json::Object(
        groups
            .into_iter()
            .map(|(key, rows)| {
                let total = group_totals
                    .iter()
                    .find(|(existing, _)| *existing == key)
                    .map(|(_, count)| Json::Int(*count))
                    .unwrap_or(Json::Null);
                (
                    key,
                    Json::object(vec![
                        ("results", Json::Array(rows)),
                        ("total_results", total),
                    ]),
                )
            })
            .collect(),
    ))
}

/// Overwrite a row's group column with the key it is filed under.
fn set_group_value(row: &Json, field: GroupField, key: &str) -> Json {
    let mut row = row.clone();
    let Json::Object(entries) = &mut row else {
        return row;
    };
    let value = if key == "None" {
        Json::Null
    } else {
        Json::string(key)
    };
    match entries.iter_mut().find(|(name, _)| name == field.as_str()) {
        Some(slot) => slot.1 = value,
        None => entries.push((field.as_str().to_owned(), value)),
    }
    row
}

fn single_group(
    plan: &GroupPlan,
    group: GroupField,
    rows: &[Json],
    group_totals: &[(String, i64)],
) -> Result<Json, QueryError> {
    let _ = group;
    Ok(Json::Object(
        plan.group_fields
            .iter()
            .map(|key| {
                let matching: Vec<Json> = rows
                    .iter()
                    .filter(|row| &row_group_key(row, plan.group_by) == key)
                    .cloned()
                    .collect();
                let total = group_totals
                    .iter()
                    .find(|(existing, _)| existing == key)
                    .map(|(_, count)| *count)
                    .unwrap_or(0);
                (
                    key.clone(),
                    Json::object(vec![
                        ("results", Json::Array(matching)),
                        ("total_results", Json::Int(total)),
                    ]),
                )
            })
            .collect(),
    ))
}

fn multi_sub_group(
    plan: &GroupPlan,
    group: GroupField,
    sub: GroupField,
    rows: &[Json],
    group_totals: &[(String, i64)],
    sub_totals: &[(String, String, i64)],
) -> Result<Json, QueryError> {
    let group_map = while_m2m(group, rows);
    let sub_map = while_m2m(sub, rows);
    let annotated: Vec<Json> = rows
        .iter()
        .map(|row| {
            annotate_arrays(
                row,
                Some(group).filter(|field| field.mapper().is_some()),
                Some(sub).filter(|field| field.mapper().is_some()),
                &group_map,
                &sub_map,
            )
        })
        .collect();

    let mut out: Vec<(String, Json)> = Vec::new();
    for key in &plan.group_fields {
        let mut cells: Vec<(String, Json)> = Vec::new();
        let mut sub_keys: Vec<&String> = sub_totals
            .iter()
            .filter(|(existing, _, _)| existing == key)
            .map(|(_, sub_key, _)| sub_key)
            .collect();
        sub_keys.sort();
        sub_keys.dedup();
        for sub_key in sub_keys {
            let count = sub_totals
                .iter()
                .find(|(existing, existing_sub, _)| existing == key && existing_sub == sub_key)
                .map(|(_, _, count)| *count)
                .unwrap_or(0);
            let matching: Vec<Json> = annotated
                .iter()
                .filter(|row| {
                    &row_group_key(row, group) == key && &row_group_key(row, sub) == sub_key
                })
                .cloned()
                .collect();
            cells.push((
                sub_key.clone(),
                Json::object(vec![
                    ("results", Json::Array(matching)),
                    ("total_results", Json::Int(count)),
                ]),
            ));
        }
        let total = group_totals
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        out.push((
            key.clone(),
            Json::object(vec![
                ("results", Json::Object(cells)),
                ("total_results", Json::Int(total)),
            ]),
        ));
    }
    Ok(Json::Object(out))
}

fn while_m2m(field: GroupField, rows: &[Json]) -> Vec<(String, Vec<String>)> {
    if field.mapper().is_some() {
        accumulate_groups(rows, field)
    } else {
        Vec::new()
    }
}

fn plain_sub_group(
    plan: &GroupPlan,
    group: GroupField,
    sub: GroupField,
    rows: &[Json],
    group_totals: &[(String, i64)],
    sub_totals: &[(String, String, i64)],
) -> Result<Json, QueryError> {
    let mut out: Vec<(String, Json)> = Vec::new();
    for key in &plan.group_fields {
        let mut cells: Vec<(String, Json)> = Vec::new();
        let mut sub_keys: Vec<&String> = sub_totals
            .iter()
            .filter(|(existing, _, _)| existing == key)
            .map(|(_, sub_key, _)| sub_key)
            .collect();
        // The Python's cell order is the order Postgres returns the group rows
        // (`__get_subgroup_total_queryset` has no ORDER BY); the golden sorts
        // it, so this does too.
        sub_keys.sort();
        sub_keys.dedup();
        for sub_key in sub_keys {
            let count = sub_totals
                .iter()
                .find(|(existing, existing_sub, _)| existing == key && existing_sub == sub_key)
                .map(|(_, _, count)| *count)
                .unwrap_or(0);
            let matching: Vec<Json> = rows
                .iter()
                .filter(|row| {
                    &row_group_key(row, group) == key && &row_group_key(row, sub) == sub_key
                })
                .cloned()
                .collect();
            cells.push((
                sub_key.clone(),
                Json::object(vec![
                    ("results", Json::Array(matching)),
                    ("total_results", Json::Int(count)),
                ]),
            ));
        }
        let total = group_totals
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        out.push((
            key.clone(),
            Json::object(vec![
                ("results", Json::Object(cells)),
                ("total_results", Json::Int(total)),
            ]),
        ));
    }
    // `__query_grouper` indexes the dict without checking, so a row whose
    // group or sub-group is not in the precomputed lists is a KeyError (500).
    let available_groups: Vec<&String> = out.iter().map(|(key, _)| key).collect();
    for row in rows {
        let key = row_group_key(row, group);
        if !available_groups.iter().any(|existing| **existing == key) {
            return Err(QueryError::unsupported(format!(
                "KeyError: '{key}' (group_by value not in issue_group_values)"
            )));
        }
        let sub_key = row_group_key(row, sub);
        let cell_exists = sub_totals
            .iter()
            .any(|(existing, existing_sub, _)| *existing == key && *existing_sub == sub_key);
        if !cell_exists {
            return Err(QueryError::unsupported(format!(
                "KeyError: '{sub_key}' (sub_group_by value not in the totals)"
            )));
        }
    }
    Ok(Json::Object(out))
}

#[allow(clippy::too_many_arguments)]
fn envelope(
    group_by: Option<GroupField>,
    sub_group_by: Option<GroupField>,
    hits: i64,
    next: Cursor,
    prev: Cursor,
    count: usize,
    total_pages: i64,
    results: Json,
) -> Json {
    Json::object(vec![
        (
            "grouped_by",
            group_by.map_or(Json::Null, |field| Json::string(field.as_str())),
        ),
        (
            "sub_grouped_by",
            sub_group_by.map_or(Json::Null, |field| Json::string(field.as_str())),
        ),
        ("total_count", Json::Int(hits)),
        ("next_cursor", Json::string(next.to_string())),
        ("prev_cursor", Json::string(prev.to_string())),
        ("next_page_results", Json::Bool(next.has_results)),
        ("prev_page_results", Json::Bool(prev.has_results)),
        ("count", Json::Int(count as i64)),
        ("total_pages", Json::Int(total_pages)),
        ("total_results", Json::Int(hits)),
        ("extra_stats", Json::Null),
        ("results", results),
    ])
}

/// The `now` a request uses for its relative date filters. Kept in one place so
/// a handler cannot drift from the oracle.
pub fn request_now() -> chrono::DateTime<Utc> {
    Utc::now()
}

/// The timezone a request runs in.
pub fn request_timezone(user_timezone: &str) -> Tz {
    user_timezone.parse().unwrap_or(Tz::UTC)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips() {
        let cursor = Cursor::from_string("13:2:1").expect("cursor");
        assert_eq!(cursor.value, 13.0);
        assert_eq!(cursor.offset, 2);
        assert!(cursor.is_prev);
        assert_eq!(cursor.to_string(), "13:2:1");

        let float = Cursor::from_string("10.0:0:0").expect("cursor");
        assert_eq!(float.value, 10.0);
        assert_eq!(float.to_string(), "10.0:0:0");

        assert!(Cursor::from_string("1:2").is_err());
        assert!(Cursor::from_string("abc:0:0").is_err());
        // A negative offset parses; `PageQuery::parse` rejects it.
        assert_eq!(Cursor::from_string("1:-1:0").expect("cursor").offset, -1);
    }

    #[test]
    fn python_int_accepts_what_python_accepts() {
        assert_eq!(PythonInt::parse("10"), Some(10));
        assert_eq!(PythonInt::parse(" 10 "), Some(10));
        assert_eq!(PythonInt::parse("+10"), Some(10));
        assert_eq!(PythonInt::parse("-10"), Some(-10));
        assert_eq!(PythonInt::parse("1_0"), Some(10));
        assert_eq!(PythonInt::parse("10.0"), None);
        assert_eq!(PythonInt::parse("1__0"), None);
        assert_eq!(PythonInt::parse("_10"), None);
        assert_eq!(PythonInt::parse("abc"), None);
        assert_eq!(PythonInt::parse(""), None);
    }

    #[test]
    fn per_page_limits_match_the_python() {
        let dict = plane_drf::QueryDict::parse("per_page=0", None).expect("query");
        assert_eq!(
            per_page(&dict, MAX_LIMIT, MAX_LIMIT)
                .expect_err("error")
                .message(),
            "Invalid per_page value. Must be greater than zero."
        );
        let dict = plane_drf::QueryDict::parse("per_page=1001", None).expect("query");
        assert_eq!(
            per_page(&dict, MAX_LIMIT, MAX_LIMIT)
                .expect_err("error")
                .message(),
            "Invalid per_page value. Cannot exceed 1000."
        );
        let dict = plane_drf::QueryDict::parse("per_page=abc", None).expect("query");
        assert_eq!(
            per_page(&dict, MAX_LIMIT, MAX_LIMIT)
                .expect_err("error")
                .message(),
            "Invalid per_page parameter."
        );
    }

    #[test]
    fn per_page_distinguishes_missing_from_empty() {
        // `int(request.GET.get("per_page", default_per_page))`
        // (`plane/utils/paginator.py:645`): absent → the default,
        // present-but-empty → `int("")` → `ValueError` → 400.
        let absent = plane_drf::QueryDict::parse("", None).expect("query");
        assert_eq!(per_page(&absent, MAX_LIMIT, MAX_LIMIT), Ok(MAX_LIMIT));
        let empty = plane_drf::QueryDict::parse("per_page=", None).expect("query");
        assert_eq!(
            per_page(&empty, MAX_LIMIT, MAX_LIMIT)
                .expect_err("error")
                .message(),
            "Invalid per_page parameter."
        );
        // Every other string CPython's `int()` rejects is the same 400.
        for raw in [
            "  ", "1.0", "1e3", "0x10", "_1", "1_", "1__0", "+", "-", "1 0", "١٢a",
        ] {
            let dict =
                plane_drf::QueryDict::parse(&format!("per_page={raw}"), None).expect("query");
            assert_eq!(
                per_page(&dict, MAX_LIMIT, MAX_LIMIT)
                    .expect_err("error")
                    .message(),
                "Invalid per_page parameter.",
                "{raw:?} should be rejected"
            );
        }
        // CPython's `int()` accepts Unicode decimal digits and single
        // underscores between them; `plane_core::py::int` reproduces both.
        let unicode = plane_drf::QueryDict::parse("per_page=١٢", None).expect("query");
        assert_eq!(per_page(&unicode, MAX_LIMIT, MAX_LIMIT), Ok(12));
        let underscores = plane_drf::QueryDict::parse("per_page=1_0", None).expect("query");
        assert_eq!(per_page(&underscores, MAX_LIMIT, MAX_LIMIT), Ok(10));
    }

    #[test]
    fn per_page_outside_i64_still_fails_the_range_check() {
        // Python's `int()` is unbounded, so these are integers that fail
        // `per_page > max_per_page` / `per_page <= 0`, not parse errors.
        let huge = format!("per_page={}", "9".repeat(30));
        let dict = plane_drf::QueryDict::parse(&huge, None).expect("query");
        assert_eq!(
            per_page(&dict, MAX_LIMIT, MAX_LIMIT)
                .expect_err("error")
                .message(),
            "Invalid per_page value. Cannot exceed 1000."
        );
        let negative = format!("per_page=-{}", "9".repeat(30));
        let dict = plane_drf::QueryDict::parse(&negative, None).expect("query");
        assert_eq!(
            per_page(&dict, MAX_LIMIT, MAX_LIMIT)
                .expect_err("error")
                .message(),
            "Invalid per_page value. Must be greater than zero."
        );
    }

    #[test]
    fn page_query_parse_with_uses_the_callers_maximum() {
        // `paginate(default_per_page=10, max_per_page=20)`: the effective cap is
        // `max(20, 10)` (`paginator.py:658-660`), so 20 is accepted and 21 is
        // not. The issue lists pass 1000/1000, which is `parse`.
        let ok = plane_drf::QueryDict::parse("per_page=20&cursor=20:0:0", None).expect("query");
        assert!(PageQuery::parse_with(&ok, 10, 20, None, None).is_ok());
        let too_many = plane_drf::QueryDict::parse("per_page=21", None).expect("query");
        assert_eq!(
            PageQuery::parse_with(&too_many, 10, 20, None, None)
                .expect_err("error")
                .message(),
            "Invalid per_page value. Cannot exceed 20."
        );
    }

    #[test]
    fn ceil_division_is_upwards() {
        assert_eq!(ceil_div(225, 13), 18);
        assert_eq!(ceil_div(0, 13), 0);
        assert_eq!(ceil_div(13, 13), 1);
    }
}
