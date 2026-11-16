// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H24 — `IssueView.save` (`plane/db/models/view.py:79-95`).
//!
//! ```python
//! def save(self, *args, **kwargs):
//!     query_params = self.filters
//!     self.query = issue_filters(query_params, "POST") if query_params else {}
//!
//!     if self._state.adding:
//!         if self.project:
//!             largest_sort_order = IssueView.objects.filter(project=self.project).aggregate(
//!                 largest=models.Max("sort_order")
//!             )["largest"]
//!         else:
//!             largest_sort_order = IssueView.objects.filter(workspace=self.workspace, project__isnull=True).aggregate(
//!                 largest=models.Max("sort_order")
//!             )["largest"]
//!         if largest_sort_order is not None:
//!             self.sort_order = largest_sort_order + 10000
//!
//!     super(IssueView, self).save(*args, **kwargs)
//! ```
//!
//! Both halves run on **every** save — create and update alike — and the
//! delete path is a save too (`SoftDeleteModel.delete` calls `self.save()`,
//! `plane/db/mixins.py:65-77`), so a soft delete rewrites `query` from the
//! stored `filters`.
//!
//! [`query_from_filters`] is `issue_filters(filters, "POST")`
//! (`plane/utils/issue_filters.py:428-463`): the fixed `ISSUE_FILTER` iteration
//! order, the POST branch of every `filter_*` (raw body values, no
//! `uuid.UUID` conversion), the unconditional `*__deleted_at__isnull` extras,
//! `type` overwriting `state__group__in`, `sub_issue`'s string comparison and
//! `intake_status` reading `inbox_status`. Python exceptions inside it
//! (`len(int)`, `int.get`, `str.split`, `int()`) are uncaught upstream, i.e.
//! the family 500; [`FilterError::Python`] carries their message.
//!
//! The dict order is the Python insertion order (a replaced key keeps its
//! position), because the *response* renders the in-memory dict while the
//! `query` column itself is `jsonb` and re-orders it.

use chrono::{Days, NaiveDate};
use deadpool_postgres::Client;
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use uuid::Uuid;

use crate::models::JsonText;
use crate::pool::DbError;

/// Why `issue_filters` could not produce a dict.
#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    /// A Python exception the function raises on a shape it does not expect:
    /// `TypeError`, `AttributeError`, `ValueError`, `OverflowError`.
    #[error("{0}")]
    Python(String),
    /// A value `json.dumps` refuses.
    #[error(transparent)]
    Json(#[from] plane_serde::Error),
}

/// Python truthiness of a JSON value: `None`, `False`, `0`, `0.0`, `""`, `[]`
/// and `{}` are falsy.
#[must_use]
pub fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Int(value) => {
            value.as_i64().is_some_and(|value| value != 0)
                || value.as_u64().is_some_and(|value| value != 0)
        }
        Json::Float(value) => *value != 0.0,
        Json::Str(value) => !value.is_empty(),
        Json::List(value) => !value.is_empty(),
        Json::Dict(value) => !value.is_empty(),
    }
}

/// `issue_filters(filters, "POST")` (`plane/utils/issue_filters.py:428-463`),
/// with `timezone.now().date()` passed in as `today` (it is only read by the
/// `"<n>_weeks"`/`"<n>_months"` duration branch of `string_date_filter`).
///
/// # Errors
///
/// [`FilterError`] for the shapes Python raises on.
pub fn query_from_filters(filters: &Json, today: NaiveDate) -> Result<JsonText, FilterError> {
    let mut out = PyDict::new();

    // The `ISSUE_FILTER` literal's order decides the output key order.
    if contains(filters, "state")? {
        raw(filters, &mut out, "state", "state__in")?;
    }
    if contains(filters, "state_group")? {
        raw(filters, &mut out, "state_group", "state__group__in")?;
    }
    if contains(filters, "estimate_point")? {
        raw(filters, &mut out, "estimate_point", "estimate_point__in")?;
    }
    if contains(filters, "priority")? {
        raw(filters, &mut out, "priority", "priority__in")?;
    }
    if contains(filters, "parent")? {
        raw(filters, &mut out, "parent", "parent__in")?;
    }
    if contains(filters, "labels")? {
        raw(filters, &mut out, "labels", "labels__in")?;
        // `filter_labels` sets this **after** its branch, unconditionally.
        out.insert(
            PyStr::new("label_issue__deleted_at__isnull"),
            Json::Bool(true),
        );
    }
    if contains(filters, "assignees")? {
        raw(filters, &mut out, "assignees", "assignees__in")?;
        out.insert(
            PyStr::new("issue_assignee__deleted_at__isnull"),
            Json::Bool(true),
        );
    }
    if contains(filters, "mentions")? {
        raw(
            filters,
            &mut out,
            "mentions",
            "issue_mention__mention__id__in",
        )?;
    }
    if contains(filters, "created_by")? {
        raw(filters, &mut out, "created_by", "created_by__in")?;
    }
    if contains(filters, "logged_by")? {
        // `logged_by__in` is not a field of `Issue`; on an issue queryset it
        // would be a `FieldError`. `IssueView.save` only stores the dict, so
        // the key is written as it is (`issue_filters.py:414-425`).
        raw(filters, &mut out, "logged_by", "logged_by__in")?;
    }
    if contains(filters, "name")? {
        // `if params.get("name", "") != "":` — no truthiness/`len` test, so an
        // explicit `null` becomes `name__icontains: null`.
        let value = get(filters, "name")?.cloned().unwrap_or(Json::Null);
        if !matches!(&value, Json::Str(text) if text.is_empty()) {
            out.insert(PyStr::new("name__icontains"), value);
        }
    }
    if contains(filters, "created_at")? {
        date_field(filters, &mut out, "created_at", "created_at__date", today)?;
    }
    if contains(filters, "updated_at")? {
        date_field(filters, &mut out, "updated_at", "updated_at__date", today)?;
    }
    if contains(filters, "start_date")? {
        raw(filters, &mut out, "start_date", "start_date")?;
    }
    if contains(filters, "target_date")? {
        raw(filters, &mut out, "target_date", "target_date")?;
    }
    if contains(filters, "completed_at")? {
        date_field(
            filters,
            &mut out,
            "completed_at",
            "completed_at__date",
            today,
        )?;
    }
    if contains(filters, "type")? {
        // `filter_issue_state_type` ignores `method` and always writes the
        // group list (`issue_filters.py:296-305`).
        let value = get(filters, "type")?.cloned().unwrap_or(Json::Null);
        let group: &[&str] = if is_str(&value, "backlog") {
            &["backlog"]
        } else if is_str(&value, "active") {
            &["unstarted", "started"]
        } else {
            &["backlog", "unstarted", "started", "completed", "cancelled"]
        };
        out.insert(
            PyStr::new("state__group__in"),
            Json::List(group.iter().map(|group| Json::from(*group)).collect()),
        );
    }
    if contains(filters, "project")? {
        raw(filters, &mut out, "project", "project__in")?;
    }
    if contains(filters, "cycle")? {
        raw(filters, &mut out, "cycle", "issue_cycle__cycle_id__in")?;
        out.insert(
            PyStr::new("issue_cycle__deleted_at__isnull"),
            Json::Bool(true),
        );
    }
    if contains(filters, "module")? {
        raw(filters, &mut out, "module", "issue_module__module_id__in")?;
        out.insert(
            PyStr::new("issue_module__deleted_at__isnull"),
            Json::Bool(true),
        );
    }
    if contains(filters, "intake_status")? {
        // The POST branch tests `intake_status` but stores `inbox_status`,
        // which may be absent — `null` in the body (`:350-362`).
        let value = get(filters, "intake_status")?
            .cloned()
            .unwrap_or(Json::Null);
        if passes(&value)? {
            let stored = get(filters, "inbox_status")?.cloned().unwrap_or(Json::Null);
            out.insert(PyStr::new("issue_intake__status__in"), stored);
        }
    }
    if contains(filters, "inbox_status")? {
        let value = get(filters, "inbox_status")?.cloned().unwrap_or(Json::Null);
        if passes(&value)? {
            out.insert(PyStr::new("issue_intake__status__in"), value);
        }
    }
    if contains(filters, "sub_issue")? {
        // `params.get("sub_issue", "false") == "false"` — a JSON `false` is not
        // the string (`:380-389`).
        let value = get(filters, "sub_issue")?.cloned().unwrap_or(Json::Null);
        if is_str(&value, "false") {
            out.insert(PyStr::new("parent__isnull"), Json::Bool(true));
        }
    }
    if contains(filters, "subscriber")? {
        raw(
            filters,
            &mut out,
            "subscriber",
            "issue_subscribers__subscriber_id__in",
        )?;
        out.insert(
            PyStr::new("issue_subscribers__deleted_at__isnull"),
            Json::Bool(true),
        );
    }
    if contains(filters, "start_target_date")? {
        let value = get(filters, "start_target_date")?
            .cloned()
            .unwrap_or(Json::Null);
        if is_str(&value, "true") {
            out.insert(PyStr::new("target_date__isnull"), Json::Bool(false));
            out.insert(PyStr::new("start_date__isnull"), Json::Bool(false));
        }
    }

    Ok(JsonText::from_json_dict(&out)?)
}

/// One `filter_*`'s shared POST body:
/// `if params.get(key, None) and len(params.get(key)) and params.get(key) != "null"`.
fn raw(params: &Json, out: &mut PyDict, key: &str, target: &str) -> Result<(), FilterError> {
    let value = get(params, key)?.cloned().unwrap_or(Json::Null);
    if passes(&value)? {
        out.insert(PyStr::new(target), value);
    }
    Ok(())
}

/// The three conditions above: truthiness, a non-zero `len()`, and the string
/// `"null"` sentinel.
fn passes(value: &Json) -> Result<bool, FilterError> {
    if !truthy(value) {
        return Ok(false);
    }
    if length(value)? == 0 {
        return Ok(false);
    }
    Ok(!is_str(value, "null"))
}

/// `filter_created_at`/`filter_updated_at`/`filter_completed_at`'s POST branch
/// (`issue_filters.py:209-244`, `:277-293`).
fn date_field(
    params: &Json,
    out: &mut PyDict,
    key: &str,
    date_term: &str,
    today: NaiveDate,
) -> Result<(), FilterError> {
    let value = get(params, key)?.cloned().unwrap_or(Json::Null);
    if !truthy(&value) || length(&value)? == 0 {
        return Ok(());
    }
    date_filter(out, date_term, &value, today)
}

/// `date_filter` (`issue_filters.py:55-81`).
fn date_filter(
    out: &mut PyDict,
    date_term: &str,
    queries: &Json,
    today: NaiveDate,
) -> Result<(), FilterError> {
    for query in iterate(queries)? {
        let Json::Str(text) = &query else {
            return Err(FilterError::Python(format!(
                "AttributeError: '{}' object has no attribute 'split'",
                type_name(&query)
            )));
        };
        let text = text.to_string_lossy();
        let parts: Vec<&str> = text.split(';').collect();
        // `str.split` never returns an empty list; the first element exists.
        let first = parts.first().copied().unwrap_or("");
        if parts.len() >= 2 {
            match duration_parts(first) {
                Some((digits, unit)) => {
                    if parts.len() == 3 {
                        let duration = python_int(digits)?;
                        string_date_filter(date_term, duration, parts[1], unit, parts[2], today)?;
                    }
                }
                None => {
                    if parts.contains(&"after") {
                        out.insert(
                            PyStr::from_string(format!("{date_term}__gte")),
                            Json::from(first),
                        );
                    } else {
                        out.insert(
                            PyStr::from_string(format!("{date_term}__lte")),
                            Json::from(first),
                        );
                    }
                }
            }
        } else {
            out.insert(
                PyStr::from_string(format!("{date_term}__contains")),
                Json::from(first),
            );
        }
    }
    Ok(())
}

/// `string_date_filter` (`issue_filters.py:29-52`).
///
/// The value it writes is a Python `datetime.date`, and `IssueView.query` is a
/// plain `JSONField` whose column adapter is psycopg's `json.dumps`: a date is
/// not serialisable, so the INSERT/UPDATE raises `TypeError: bad argument type
/// for built-in operation` after H24 built the dict and before any row is
/// written. The hook reports that failure instead of inventing a string the
/// Python never stores; the arithmetic is kept so an overflowing duration still
/// fails as `OverflowError`, like `timedelta`.
fn string_date_filter(
    date_term: &str,
    duration: i64,
    subsequent: &str,
    unit: &str,
    offset: &str,
    today: NaiveDate,
) -> Result<(), FilterError> {
    let days = match unit {
        "months" => duration
            .checked_mul(30)
            .ok_or_else(|| overflow(duration * 30))?,
        _ => duration
            .checked_mul(7)
            .ok_or_else(|| overflow(duration * 7))?,
    };
    // `timedelta(days=…)` itself refuses more than 999999999 days.
    if days.unsigned_abs() > 999_999_999 {
        return Err(overflow(days));
    }
    let shift = u64::try_from(days).map_err(|_| overflow(days))?;
    let date = if offset == "fromnow" {
        today.checked_add_days(Days::new(shift))
    } else {
        today.checked_sub_days(Days::new(shift))
    }
    .ok_or_else(|| overflow(days))?;
    let suffix = if subsequent == "after" {
        "__gte"
    } else {
        "__lte"
    };
    // The key and the date are exactly what the dict would hold; the column
    // cannot store them.
    let _ = (date_term, suffix, date, offset);
    Err(FilterError::Python(
        "TypeError: bad argument type for built-in operation".to_owned(),
    ))
}

fn overflow(days: i64) -> FilterError {
    FilterError::Python(format!(
        "OverflowError: days={days}; must have magnitude <= 999999999"
    ))
}

/// `pattern.match(date_query[0])` for `re.compile(r"\d+_(weeks|months)$")`,
/// returning the digit prefix and the unit.
fn duration_parts(text: &str) -> Option<(&str, &str)> {
    for unit in ["weeks", "months"] {
        if let Some(digits) = text
            .strip_suffix(unit)
            .and_then(|rest| rest.strip_suffix('_'))
            && !digits.is_empty()
            && digits.chars().all(|digit| digit.is_ascii_digit())
        {
            return Some((digits, unit));
        }
    }
    None
}

/// `int(digits)` for an all-digit string.
fn python_int(digits: &str) -> Result<i64, FilterError> {
    digits.parse::<i64>().map_err(|_| {
        FilterError::Python(format!(
            "ValueError: invalid literal for int() with base 10: '{digits}'"
        ))
    })
}

/// `key in params` for the `issue_filters` loop (`if key in query_params`).
fn contains(params: &Json, key: &str) -> Result<bool, FilterError> {
    match params {
        Json::Dict(object) => Ok(object.contains_key(key)),
        Json::List(items) => Ok(items.iter().any(|item| is_str(item, key))),
        Json::Str(text) => Ok(text.to_string_lossy().contains(key)),
        other => Err(FilterError::Python(format!(
            "TypeError: argument of type '{}' is not iterable",
            type_name(other)
        ))),
    }
}

/// `params.get(key)` when the parameter is a dict; `AttributeError` otherwise.
fn get<'a>(params: &'a Json, key: &str) -> Result<Option<&'a Json>, FilterError> {
    match params {
        Json::Dict(object) => Ok(object.get(key)),
        Json::List(_) => Err(attribute_error("list", "get")),
        Json::Str(_) => Err(attribute_error("str", "get")),
        other => Err(attribute_error(type_name(other), "get")),
    }
}

fn attribute_error(kind: &str, attribute: &str) -> FilterError {
    FilterError::Python(format!(
        "AttributeError: '{kind}' object has no attribute '{attribute}'"
    ))
}

/// `len(value)` — only its zero/non-zero result is ever used, so a Python
/// `TypeError` is the only thing that matters.
fn length(value: &Json) -> Result<usize, FilterError> {
    match value {
        Json::Str(text) => Ok(text.char_len()),
        Json::List(items) => Ok(items.len()),
        Json::Dict(object) => Ok(object.len()),
        other => Err(FilterError::Python(format!(
            "TypeError: object of type '{}' has no len()",
            type_name(other)
        ))),
    }
}

/// Python iteration in a `for` loop: a string yields its characters, a list its
/// items, a dict its keys.
fn iterate(value: &Json) -> Result<Vec<Json>, FilterError> {
    match value {
        Json::Str(text) => Ok(text
            .to_string_lossy()
            .chars()
            .map(|character| Json::from(character.to_string()))
            .collect()),
        Json::List(items) => Ok(items.clone()),
        Json::Dict(object) => Ok(object.keys().map(|key| Json::Str(key.clone())).collect()),
        other => Err(FilterError::Python(format!(
            "TypeError: '{}' object is not iterable",
            type_name(other)
        ))),
    }
}

/// Python's `type(value).__name__`.
fn type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::List(_) => "list",
        Json::Dict(_) => "dict",
    }
}

/// `value == "<text>"`: only a `str` value can equal a `str` in Python.
fn is_str(value: &Json, text: &str) -> bool {
    match value {
        Json::Str(value) => value.as_str() == Some(text),
        _ => false,
    }
}

/// Which aggregate H24's create half runs (`view.py:84-91`).
#[derive(Debug, Clone, Copy)]
pub enum SortScope {
    /// `filter(project=self.project)` — the project's live views.
    Project(Uuid),
    /// `filter(workspace=self.workspace, project__isnull=True)`.
    Workspace(Uuid),
}

/// The create half of H24: the `sort_order` the INSERT writes.
///
/// Both aggregates go through the default manager (`deleted_at IS NULL`). The
/// body's `sort_order` survives only when the aggregate is `NULL` (a first
/// view).
///
/// # Errors
///
/// A database failure.
pub async fn before_insert(
    client: &Client,
    scope: SortScope,
    requested: f64,
) -> Result<f64, DbError> {
    let largest: Option<f64> = match scope {
        SortScope::Project(project_id) => {
            let row = client
                .query_one(
                    "SELECT MAX(sort_order) FROM issue_views \
                     WHERE deleted_at IS NULL AND project_id = $1",
                    &[&project_id],
                )
                .await?;
            row.get(0)
        }
        SortScope::Workspace(workspace_id) => {
            let row = client
                .query_one(
                    "SELECT MAX(sort_order) FROM issue_views \
                     WHERE deleted_at IS NULL AND workspace_id = $1 \
                       AND project_id IS NULL",
                    &[&workspace_id],
                )
                .await?;
            row.get(0)
        }
    };
    Ok(largest.map_or(requested, |largest| largest + 10_000.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Json {
        plane_serde::pyjson::loads(text.as_bytes(), "utf-8").expect("json")
    }

    fn run(filters: &str) -> Result<String, FilterError> {
        let today = NaiveDate::from_ymd_opt(2026, 11, 16).expect("date");
        query_from_filters(&parse(filters), today).map(JsonText::into_string)
    }

    /// `json.dumps(issue_filters(filters, "POST"), cls=DjangoJSONEncoder)`
    /// captured from the Python in the venv; the pairs are (input, output).
    const CASES: &[(&str, &str)] = &[
        ("{}", "{}"),
        (
            r#"{"state": "abc", "priority": ["high"]}"#,
            r#"{"state__in": "abc", "priority__in": ["high"]}"#,
        ),
        (
            r#"{"state": "s", "state_group": "started", "estimate_point": "3", "priority": ["high", "low"], "parent": "p", "labels": ["l1"], "assignees": ["a1"], "mentions": ["m1"], "created_by": "cb", "logged_by": "lb", "name": "needle", "created_at": ["2026-01-01;after"], "updated_at": ["2026-02-01;before"], "start_date": "2026-03-01", "target_date": "2026-04-01", "completed_at": ["2026-05-01"], "type": "backlog", "project": "pr", "cycle": "cy", "module": "mo", "intake_status": "1", "inbox_status": "2", "sub_issue": "false", "subscriber": "sub", "start_target_date": "true"}"#,
            r#"{"state__in": "s", "state__group__in": ["backlog"], "estimate_point__in": "3", "priority__in": ["high", "low"], "parent__in": "p", "labels__in": ["l1"], "label_issue__deleted_at__isnull": true, "assignees__in": ["a1"], "issue_assignee__deleted_at__isnull": true, "issue_mention__mention__id__in": ["m1"], "created_by__in": "cb", "logged_by__in": "lb", "name__icontains": "needle", "created_at__date__gte": "2026-01-01", "updated_at__date__lte": "2026-02-01", "start_date": "2026-03-01", "target_date": "2026-04-01", "completed_at__date__contains": "2026-05-01", "project__in": "pr", "issue_cycle__cycle_id__in": "cy", "issue_cycle__deleted_at__isnull": true, "issue_module__module_id__in": "mo", "issue_module__deleted_at__isnull": true, "issue_intake__status__in": "2", "parent__isnull": true, "issue_subscribers__subscriber_id__in": "sub", "issue_subscribers__deleted_at__isnull": true, "target_date__isnull": false, "start_date__isnull": false}"#,
        ),
        (
            r#"{"type": "backlog"}"#,
            r#"{"state__group__in": ["backlog"]}"#,
        ),
        (
            r#"{"type": "active"}"#,
            r#"{"state__group__in": ["unstarted", "started"]}"#,
        ),
        (
            r#"{"type": "all"}"#,
            r#"{"state__group__in": ["backlog", "unstarted", "started", "completed", "cancelled"]}"#,
        ),
        (
            r#"{"type": 123}"#,
            r#"{"state__group__in": ["backlog", "unstarted", "started", "completed", "cancelled"]}"#,
        ),
        (
            r#"{"state_group": "started", "type": "backlog"}"#,
            r#"{"state__group__in": ["backlog"]}"#,
        ),
        (
            r#"{"state_group": "started"}"#,
            r#"{"state__group__in": "started"}"#,
        ),
        (r#"{"sub_issue": "false"}"#, r#"{"parent__isnull": true}"#),
        (r#"{"sub_issue": false}"#, "{}"),
        (r#"{"sub_issue": "true"}"#, "{}"),
        (
            r#"{"intake_status": "2"}"#,
            r#"{"issue_intake__status__in": null}"#,
        ),
        (
            r#"{"intake_status": "1", "inbox_status": "2"}"#,
            r#"{"issue_intake__status__in": "2"}"#,
        ),
        (
            r#"{"labels": null}"#,
            r#"{"label_issue__deleted_at__isnull": true}"#,
        ),
        (
            r#"{"assignees": []}"#,
            r#"{"issue_assignee__deleted_at__isnull": true}"#,
        ),
        (r#"{"logged_by": "x"}"#, r#"{"logged_by__in": "x"}"#),
        (r#"{"name": null}"#, r#"{"name__icontains": null}"#),
        (r#"{"name": ""}"#, "{}"),
        (
            r#"{"created_at": "2026-01-01"}"#,
            r#"{"created_at__date__contains": "1"}"#,
        ),
        (
            r#"{"created_at": ["2026-01-01;after"]}"#,
            r#"{"created_at__date__gte": "2026-01-01"}"#,
        ),
        (
            r#"{"created_at": ["2026-01-01;before"]}"#,
            r#"{"created_at__date__lte": "2026-01-01"}"#,
        ),
        (
            r#"{"created_at": ["2026-01-01;after;fromnow"]}"#,
            r#"{"created_at__date__gte": "2026-01-01"}"#,
        ),
        (r#"{"updated_at": ["3_months;before"]}"#, "{}"),
        (
            r#"{"start_date": "2026-01-01"}"#,
            r#"{"start_date": "2026-01-01"}"#,
        ),
        (
            r#"{"start_target_date": "true"}"#,
            r#"{"target_date__isnull": false, "start_date__isnull": false}"#,
        ),
        (
            r#"{"cycle": "x", "module": null}"#,
            r#"{"issue_cycle__cycle_id__in": "x", "issue_cycle__deleted_at__isnull": true, "issue_module__deleted_at__isnull": true}"#,
        ),
        (
            r#"{"estimate_point": "3", "mentions": ["u"], "created_by": "u2", "updated_at": ["2026-01-01;before"], "completed_at": ["2026-01-01"]}"#,
            r#"{"estimate_point__in": "3", "issue_mention__mention__id__in": ["u"], "created_by__in": "u2", "updated_at__date__lte": "2026-01-01", "completed_at__date__contains": "2026-01-01"}"#,
        ),
        (r#"{"state": null}"#, "{}"),
        (r#"{"state": ""}"#, "{}"),
        (r#"{"state": 0}"#, "{}"),
        (r#"{"state": "null"}"#, "{}"),
        (
            r#"{"assignees": "null"}"#,
            r#"{"issue_assignee__deleted_at__isnull": true}"#,
        ),
        (r#"{"nope": 1, "other": [1, 2]}"#, "{}"),
        (
            r#"{"type": "all", "state_group": "started"}"#,
            r#"{"state__group__in": ["backlog", "unstarted", "started", "completed", "cancelled"]}"#,
        ),
        // The POST branch is shared: a string filter value is iterated
        // per character by `date_filter`.
        (
            r#"{"completed_at": ["2026-01-01"]}"#,
            r#"{"completed_at__date__contains": "2026-01-01"}"#,
        ),
    ];

    #[test]
    fn query_matches_the_python_for_every_key() {
        for (input, expected) in CASES {
            assert_eq!(&run(input).expect(input), expected, "input {input}");
        }
    }

    const ERRORS: &[(&str, &str)] = &[
        (
            "{\"state\": 5}",
            "TypeError: object of type 'int' has no len()",
        ),
        (
            "{\"state\": true}",
            "TypeError: object of type 'bool' has no len()",
        ),
        (
            r#"["state"]"#,
            "AttributeError: 'list' object has no attribute 'get'",
        ),
        (
            r#""state""#,
            "AttributeError: 'str' object has no attribute 'get'",
        ),
        (
            "{\"created_at\": 5}",
            "TypeError: object of type 'int' has no len()",
        ),
        (
            r#"{"created_at": [5]}"#,
            "AttributeError: 'int' object has no attribute 'split'",
        ),
        // `string_date_filter` writes a `datetime.date`; the JSONField cannot
        // serialise it and the INSERT/UPDATE fails.
        (
            r#"{"created_at": ["2_weeks;after;fromnow"]}"#,
            "TypeError: bad argument type for built-in operation",
        ),
        ("5", "TypeError: argument of type 'int' is not iterable"),
    ];

    #[test]
    fn python_exceptions_carry_the_message() {
        for (input, expected) in ERRORS {
            let error = run(input).expect_err(input);
            assert_eq!(&error.to_string(), expected, "input {input}");
        }
    }

    #[test]
    fn truthiness_matches_python() {
        for falsy in ["null", "false", "0", "0.0", "\"\"", "[]", "{}"] {
            assert!(!truthy(&parse(falsy)), "{falsy}");
        }
        for custom in ["true", "1", "-1", "\"x\"", "[0]", "{\"a\": 0}"] {
            assert!(truthy(&parse(custom)), "{custom}");
        }
    }
}
