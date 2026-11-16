// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/analytics_plot.py` — the `distribution` payload.
//!
//! `build_graph_plot` runs one aggregate over the filtered issue set and then
//! reshapes the rows in Python. Both halves are behaviour:
//!
//! * the aggregate is grouped by the x axis only (`GROUP BY 1`, or `1, 2` with
//!   a segment) and ordered by it, so the *segment* rows inside one dimension
//!   keep the order Postgres produced — the response is a JSON array built in
//!   that order;
//! * `groupby(...)` then `sort_data(...)` decides the ordering of the
//!   `distribution` object: `priority` uses the fixed
//!   `["low", "medium", "high", "urgent", "none"]` order, everything else is
//!   sorted by the stringified dimension with `"none"` last
//!   (`analytics_plot.py:64-70`).
//!
//! ## The four date axes are year-month strings, and NULL is `"-"`
//!
//! `annotate_with_monthly_dimension` concatenates `ExtractYear`, `"-"` and
//! `ExtractMonth` (`analytics_plot.py:43-50`). `Concat` coalesces each part to
//! the empty string, so a NULL `completed_at` still yields the string `"-"` —
//! which is *not* null and therefore survives
//! `exclude(dimension__isnull=True)`. The fixture response for
//! `x_axis=completed_at` is headed by a `"-"` bucket; that is Django's
//! behaviour, not a mistake here.
//!
//! `Extract`/`Trunc` apply `AT TIME ZONE <active timezone>` for the two
//! `DateTimeField`s (`created_at`, `completed_at`) but not for the two
//! `DateField`s (`start_date`, `target_date`), and the timezone is the caller's
//! — `RUST_REWRITE.md` "Timezones".

use chrono::NaiveDate;
use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_query::issue::model::Relation;
use plane_query::json::Json;
use plane_query::sql::{Frag, JoinKind};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::analytics::sql::{self, QueryBase};

/// `VALID_ANALYTICS_FIELDS` (`plane/utils/analytics_plot.py:25-38`).
pub(crate) const VALID_ANALYTICS_FIELDS: [&str; 12] = [
    "state_id",
    "state__group",
    "labels__id",
    "assignees__id",
    "estimate_point__value",
    "issue_cycle__cycle_id",
    "issue_module__module_id",
    "priority",
    "start_date",
    "target_date",
    "created_at",
    "completed_at",
];

/// `VALID_YAXIS` (`analytics_plot.py:40`).
pub(crate) const VALID_YAXIS: [&str; 2] = ["issue_count", "estimate"];

pub(crate) fn is_valid_field(name: &str) -> bool {
    VALID_ANALYTICS_FIELDS.contains(&name)
}

pub(crate) fn is_valid_y_axis(name: &str) -> bool {
    VALID_YAXIS.contains(&name)
}

/// The join a column needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxisJoin {
    None,
    StateGroup,
    Label,
    Assignee,
    EstimatePoint,
    Cycle,
    Module,
}

/// The value a column carries, which decides how DRF's encoder renders it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueKind {
    Uuid,
    Text,
    Date,
    Int,
}

/// `F(x_axis)` for one of `VALID_ANALYTICS_FIELDS`; the ORM resolves the
/// lookup path to a column.
fn column(name: &str) -> (&'static str, ValueKind, AxisJoin) {
    match name {
        "state_id" => ("issues.state_id", ValueKind::Uuid, AxisJoin::None),
        "state__group" => ("states.group", ValueKind::Text, AxisJoin::StateGroup),
        "labels__id" => ("issue_labels.label_id", ValueKind::Uuid, AxisJoin::Label),
        "assignees__id" => (
            "issue_assignees.assignee_id",
            ValueKind::Uuid,
            AxisJoin::Assignee,
        ),
        "estimate_point__value" => (
            "estimate_points.value",
            ValueKind::Text,
            AxisJoin::EstimatePoint,
        ),
        "issue_cycle__cycle_id" => ("cycle_issues.cycle_id", ValueKind::Uuid, AxisJoin::Cycle),
        "issue_module__module_id" => ("module_issues.module_id", ValueKind::Uuid, AxisJoin::Module),
        "priority" => ("issues.priority", ValueKind::Text, AxisJoin::None),
        "start_date" => ("issues.start_date", ValueKind::Date, AxisJoin::None),
        "target_date" => ("issues.target_date", ValueKind::Date, AxisJoin::None),
        "created_at" => ("issues.created_at", ValueKind::Text, AxisJoin::None),
        "completed_at" => ("issues.completed_at", ValueKind::Text, AxisJoin::None),
        // Unreachable: callers validate against `VALID_ANALYTICS_FIELDS`.
        _ => ("issues.id", ValueKind::Uuid, AxisJoin::None),
    }
}

/// Add the join a column needs. Django uses `LEFT OUTER` for a `values()`
/// column and promotes it to `INNER` only when a filter needs a match
/// (`build_filter`); `require` keeps a stronger existing join.
fn require_join(base: &mut QueryBase, join: AxisJoin) {
    match join {
        AxisJoin::None => {}
        AxisJoin::StateGroup => base.require_relation(Relation::States, JoinKind::Left),
        AxisJoin::Label => base.require_relation(Relation::IssueLabels, JoinKind::Left),
        AxisJoin::Assignee => base.require_relation(Relation::IssueAssignees, JoinKind::Left),
        AxisJoin::EstimatePoint => sql::require_estimate_points(base),
        AxisJoin::Cycle => base.require_relation(Relation::CycleIssues, JoinKind::Left),
        AxisJoin::Module => base.require_relation(Relation::ModuleIssues, JoinKind::Left),
    }
}

/// Whether `name` is one of the four axes `annotate_with_monthly_dimension`
/// rewrites (`analytics_plot.py:57`).
pub(crate) fn is_month_axis(name: &str) -> bool {
    matches!(
        name,
        "created_at" | "start_date" | "target_date" | "completed_at"
    )
}

/// The value kind of a dimension: the four date axes render the concat (text).
pub(crate) fn dimension_kind(name: &str) -> ValueKind {
    if is_month_axis(name) {
        ValueKind::Text
    } else {
        column(name).1
    }
}

/// `EXTRACT(YEAR FROM …) || '-' || EXTRACT(MONTH FROM …)`, with Django's
/// `AT TIME ZONE` on the `DateTimeField`s.
pub(crate) fn monthly_dimension(name: &str, timezone: Tz) -> Frag {
    let (column, has_zone) = match name {
        "created_at" => ("issues.created_at", true),
        "completed_at" => ("issues.completed_at", true),
        other => (column(other).0, false),
    };
    let zone = sql::timezone_literal(timezone);
    let mut frag = Frag::raw("(COALESCE((EXTRACT(YEAR FROM ");
    frag.push_raw(column);
    if has_zone {
        frag.push_raw(" ");
        frag.push_raw(&zone);
    }
    frag.push_raw("))::text, '') || COALESCE((COALESCE('-', '') || COALESCE((EXTRACT(MONTH FROM ");
    frag.push_raw(column);
    if has_zone {
        frag.push_raw(" ");
        frag.push_raw(&zone);
    }
    frag.push_raw("))::text, '')), ''))");
    frag
}

/// `extract_axis` (`analytics_plot.py:53-61`): the dimension expression, with
/// the joins it needs added to `base`.
pub(crate) fn dimension(base: &mut QueryBase, name: &str, timezone: Tz) -> Frag {
    let (column, _, join) = column(name);
    require_join(base, join);
    if is_month_axis(name) {
        monthly_dimension(name, timezone)
    } else {
        Frag::raw(column)
    }
}

/// `F(segment)` (or the monthly concat) for the segment column.
fn segment_expression(base: &mut QueryBase, name: &str, timezone: Tz) -> Frag {
    if is_month_axis(name) {
        // `annotate_with_monthly_dimension(queryset, segment, "segmented")`
        // does not need a join of its own: the column is on `issues`.
        monthly_dimension(name, timezone)
    } else {
        dimension(base, name, timezone)
    }
}

/// Read one projected value as the JSON value DRF's encoder would write.
pub(crate) fn read_value(row: &Row, index: usize, kind: ValueKind) -> Result<Json, ViewError> {
    Ok(match kind {
        ValueKind::Uuid => match row.try_get::<_, Option<Uuid>>(index) {
            Ok(Some(value)) => Json::Str(value.to_string()),
            Ok(None) => Json::Null,
            Err(error) => return Err(ViewError::from(error)),
        },
        ValueKind::Text => match row.try_get::<_, Option<String>>(index) {
            Ok(Some(value)) => Json::Str(value),
            Ok(None) => Json::Null,
            Err(error) => return Err(ViewError::from(error)),
        },
        ValueKind::Date => match row.try_get::<_, Option<NaiveDate>>(index) {
            Ok(Some(value)) => Json::Date(value),
            Ok(None) => Json::Null,
            Err(error) => return Err(ViewError::from(error)),
        },
        ValueKind::Int => match row.try_get::<_, Option<i32>>(index) {
            Ok(Some(value)) => Json::Int(i64::from(value)),
            Ok(None) => Json::Null,
            Err(error) => return Err(ViewError::from(error)),
        },
    })
}

/// Python truthiness of a JSON value.
pub(crate) fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Int(value) => *value != 0,
        Json::Float(value) => *value != 0.0,
        Json::Str(value) => !value.is_empty(),
        Json::Array(values) => !values.is_empty(),
        Json::Object(values) => !values.is_empty(),
        Json::DateTime(_) | Json::DateTimeZoned(_) | Json::Date(_) => true,
    }
}

/// `str(value)`, as used to build the grouped keys.
///
/// The only columns that reach it are the `get_x_axis_field()` targets —
/// uuid, text, integer and date — so the float arm is unreachable today; it
/// renders through the DRF dialect so a Python `float.__repr__` comes out
/// rather than Rust's `Display`.
pub(crate) fn py_str(value: &Json) -> String {
    match value {
        Json::Str(value) => value.clone(),
        Json::Int(value) => value.to_string(),
        Json::Float(value) => String::from_utf8(
            plane_serde::to_vec(plane_serde::Dialect::Drf, value).unwrap_or_default(),
        )
        .unwrap_or_default(),
        Json::Date(value) => value.format("%Y-%m-%d").to_string(),
        Json::Bool(value) => {
            if *value {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Json::Null => "None".to_owned(),
        _ => String::new(),
    }
}

/// `sort_data` (`analytics_plot.py:64-70`) applied to the grouped dimensions.
fn sort_dimensions(
    mut groups: Vec<(String, Vec<Json>)>,
    temp_axis: &str,
) -> Vec<(String, Vec<Json>)> {
    if temp_axis == "priority" {
        let order = ["low", "medium", "high", "urgent", "none"];
        let mut sorted = Vec::with_capacity(groups.len());
        for key in order {
            if let Some(index) = groups.iter().position(|(existing, _)| existing == key) {
                sorted.push(groups.remove(index));
            }
        }
        sorted
    } else {
        groups.sort_by(|(left, _), (right, _)| {
            let left_key = (left.as_str() == "none", left.as_str());
            let right_key = (right.as_str() == "none", right.as_str());
            left_key.cmp(&right_key)
        });
        groups
    }
}

/// `build_graph_plot` (`plane/utils/analytics_plot.py:73-120`).
pub(crate) async fn build_graph_plot(
    client: &deadpool_postgres::Client,
    base: QueryBase,
    x_axis: &str,
    y_axis: &str,
    segment: Option<&str>,
    timezone: Tz,
) -> Result<Json, ViewError> {
    let mut base = base;
    let dimension = dimension(&mut base, x_axis, timezone);
    // `queryset.exclude(dimension__isnull=True)` (`analytics_plot.py:86`),
    // which Django renders as `NOT (<dimension> IS NULL)`.
    let mut missing = dimension.clone();
    missing.push_raw(" IS NULL");
    base.and(missing.negate());

    let segment_name = segment;
    let segment = segment_name.map(|name| segment_expression(&mut base, name, timezone));

    let mut projection = Frag::raw("");
    projection.push(dimension);
    projection.push_raw(" AS dimension");
    if segment.is_some() {
        projection.push_raw(", ");
        projection.push(segment.clone().unwrap_or_else(Frag::empty));
        projection.push_raw(" AS segment");
    }

    let tail = if y_axis == "estimate" {
        sql::require_estimate_points(&mut base);
        projection.push_raw(", SUM((estimate_points.value)::double precision) AS estimate");
        if segment.is_some() {
            " GROUP BY 1, 2 ORDER BY 1 ASC"
        } else {
            " GROUP BY 1 ORDER BY 1 ASC"
        }
    } else {
        projection.push_raw(", COUNT(*) AS count");
        if segment.is_some() {
            " GROUP BY 1, 2 ORDER BY 1 ASC"
        } else {
            " GROUP BY 1 ORDER BY 1 ASC"
        }
    };

    let statement = base.statement(projection, tail);
    let rows = sql::fetch(client, &statement).await?;

    let dimension_type = dimension_kind(x_axis);
    let segment_kind = segment_name.map(dimension_kind);
    let mut groups: Vec<(String, Vec<Json>)> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for row in &rows {
        let dimension_value = read_value(row, 0, dimension_type)?;
        let key = py_str(&dimension_value);
        let mut item = Vec::with_capacity(3);
        item.push(("dimension".to_owned(), dimension_value));
        let value_index = if segment.is_some() { 2 } else { 1 };
        if let Some(kind) = segment_kind {
            item.push(("segment".to_owned(), read_value(row, 1, kind)?));
        }
        if y_axis == "estimate" {
            item.push(("estimate".to_owned(), read_estimate(row, value_index)?));
        } else {
            item.push(("count".to_owned(), read_count(row, value_index)?));
        }
        let item = Json::Object(item);
        match index.get(&key) {
            Some(slot) => groups[*slot].1.push(item),
            None => {
                index.insert(key.clone(), groups.len());
                groups.push((key, vec![item]));
            }
        }
    }

    let sorted = sort_dimensions(groups, x_axis);
    Ok(Json::Object(
        sorted
            .into_iter()
            .map(|(key, items)| (key, Json::Array(items)))
            .collect(),
    ))
}

fn read_count(row: &Row, index: usize) -> Result<Json, ViewError> {
    match row.try_get::<_, i64>(index) {
        Ok(value) => Ok(Json::Int(value)),
        Err(error) => Err(ViewError::from(error)),
    }
}

fn read_estimate(row: &Row, index: usize) -> Result<Json, ViewError> {
    match row.try_get::<_, Option<f64>>(index) {
        Ok(Some(value)) => Ok(Json::Float(value)),
        Ok(None) => Ok(Json::Null),
        Err(error) => Err(ViewError::from(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axis_lists_match_the_python_constants() {
        assert_eq!(VALID_ANALYTICS_FIELDS.len(), 12);
        assert!(is_valid_field("issue_module__module_id"));
        assert!(!is_valid_field("estimate_point__key"));
        assert!(is_valid_y_axis("estimate"));
        assert!(!is_valid_y_axis("points"));
        // `estimate_point__value` is the only text-valued axis of the m2m
        // group; `state_id` and the three id joins are uuid columns.
        assert_eq!(dimension_kind("state_id"), ValueKind::Uuid);
        assert_eq!(dimension_kind("estimate_point__value"), ValueKind::Text);
        // The four date axes become the year-month concat, i.e. text.
        for axis in ["created_at", "completed_at", "start_date", "target_date"] {
            assert_eq!(dimension_kind(axis), ValueKind::Text, "{axis}");
        }
    }

    #[test]
    fn sort_data_uses_the_priority_order_and_none_last() {
        let groups = || {
            vec![
                ("high".to_owned(), Vec::new()),
                ("none".to_owned(), Vec::new()),
                ("low".to_owned(), Vec::new()),
                ("2026-1".to_owned(), Vec::new()),
                ("2026-10".to_owned(), Vec::new()),
            ]
        };
        let priority = sort_dimensions(groups(), "priority");
        assert_eq!(
            priority
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            vec!["low", "high", "none"]
        );
        // `sorted(..., key=lambda x: (x[0] == "none", x[0]))`: a plain
        // lexicographic sort with `"none"` forced last — "2026-10" before
        // "2026-1"? No: "-" < "0", so "2026-1" is a prefix and sorts first.
        let dated = sort_dimensions(groups(), "created_at");
        assert_eq!(
            dated
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-1", "2026-10", "high", "low", "none"]
        );
    }

    #[test]
    fn python_truthiness_and_str() {
        assert!(truthy(&Json::Str("x".to_owned())));
        assert!(!truthy(&Json::Str(String::new())));
        assert!(!truthy(&Json::Int(0)));
        assert!(truthy(&Json::Date(
            chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap_or_default()
        )));
        assert_eq!(py_str(&Json::Int(3)), "3");
        assert_eq!(
            py_str(&Json::Date(
                chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap_or_default()
            )),
            "2026-01-02"
        );
        assert_eq!(py_str(&Json::Null), "None");
    }

    #[test]
    fn the_monthly_dimension_only_casts_timestamps() {
        let timestamp = monthly_dimension("created_at", chrono_tz::Asia::Kolkata).sql;
        assert!(timestamp.contains("AT TIME ZONE 'Asia/Kolkata'"));
        assert!(timestamp.contains("EXTRACT(YEAR FROM issues.created_at"));
        let date_field = monthly_dimension("target_date", chrono_tz::Asia::Kolkata).sql;
        assert!(!date_field.contains("AT TIME ZONE"));
        assert!(date_field.starts_with("(COALESCE((EXTRACT(YEAR FROM issues.target_date))::text"));
    }
}
