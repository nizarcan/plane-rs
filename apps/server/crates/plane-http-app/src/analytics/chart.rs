// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/build_chart.py` — the `custom-work-items` chart payload.
//!
//! `build_analytics_chart` is the other half of the analytics surface: unlike
//! `build_graph_plot` it takes the Python-side axis *names* (`STATES`,
//! `PRIORITY`, …), counts `distinct` issues, and has a grouped shape whose rows
//! are folded into one object per key (`build_chart.py:78-98`).
//!
//! Two quirks carried over:
//!
//! * `process_grouped_data` writes `"none"` for a falsy key in the grouped
//!   branch but the simple branch writes `"None"` — different case, different
//!   spelling (`build_chart.py:88,145-147`);
//! * `additional_filter or {}` is always truthy (`{}` is falsy, so the
//!   expression is `{}` only when there is no filter) — the same condition
//!   Django reads as "apply the filter when there is one".
//!
//! The axis names are the Python dictionary's keys, so an unknown one is a DRF
//! `ValidationError` whose body is the one-element list
//! `["Invalid x_axis field: NOPE"]` (`rest_framework/exception_handler`).

use chrono_tz::Tz;
use plane_core::ApiError;
use plane_drf::error::ViewError;
use plane_query::issue::model::Relation;
use plane_query::json::Json;
use plane_query::sql::{Frag, JoinKind};
use tokio_postgres::Row;

use crate::analytics::plot::{ValueKind, py_str, read_value, truthy};
use crate::analytics::sql::{self, QueryBase};

/// `x_axis_mapper` (`plane/utils/build_chart.py:20-34`).
const CHART_AXES: [&str; 13] = [
    "STATES",
    "STATE_GROUPS",
    "LABELS",
    "ASSIGNEES",
    "ESTIMATE_POINTS",
    "CYCLES",
    "MODULES",
    "PRIORITY",
    "START_DATE",
    "TARGET_DATE",
    "CREATED_AT",
    "COMPLETED_AT",
    "CREATED_BY",
];

/// One column of `get_x_axis_field()` (`build_chart.py:44-75`).
struct ChartField {
    expression: Frag,
    kind: ValueKind,
}

/// The SQL for one of the axis column names.
///
/// `created_at__date`/`completed_at__date` are `__date` transforms, which
/// Django renders as `(col AT TIME ZONE <active timezone>)::date`.
fn expression(column: &str, timezone: Tz) -> ChartField {
    match column {
        "created_at__date" => (
            date_expression("issues.created_at", timezone),
            ValueKind::Date,
        )
            .into(),
        "completed_at__date" => (
            date_expression("issues.completed_at", timezone),
            ValueKind::Date,
        )
            .into(),
        "state__id" | "state__group" | "state__name" => (
            Frag::raw(match column {
                // `F("state__id")` is the FK column on `issues`, not the
                // joined table's primary key: the plan (and therefore the
                // tie order of `ORDER BY count DESC`) depends on it.
                "state__id" => "issues.state_id",
                "state__group" => "states.group",
                _ => "states.name",
            }),
            match column {
                "state__id" => ValueKind::Uuid,
                _ => ValueKind::Text,
            },
        )
            .into(),
        "labels__id" => (Frag::raw("issue_labels.label_id"), ValueKind::Uuid).into(),
        "labels__name" => (Frag::raw("labels.name"), ValueKind::Text).into(),
        "assignees__id" => (Frag::raw("issue_assignees.assignee_id"), ValueKind::Uuid).into(),
        "assignees__display_name" => {
            (Frag::raw("assignee_user.display_name"), ValueKind::Text).into()
        }
        "estimate_point__key" => (Frag::raw("estimate_points.key"), ValueKind::Int).into(),
        "estimate_point__value" => (Frag::raw("estimate_points.value"), ValueKind::Text).into(),
        "issue_cycle__cycle_id" => (Frag::raw("cycle_issues.cycle_id"), ValueKind::Uuid).into(),
        "issue_cycle__cycle__name" => (Frag::raw("cycles.name"), ValueKind::Text).into(),
        "issue_module__module_id" => (Frag::raw("module_issues.module_id"), ValueKind::Uuid).into(),
        "issue_module__module__name" => (Frag::raw("modules.name"), ValueKind::Text).into(),
        "priority" => (Frag::raw("issues.priority"), ValueKind::Text).into(),
        "start_date" => (Frag::raw("issues.start_date"), ValueKind::Date).into(),
        "target_date" => (Frag::raw("issues.target_date"), ValueKind::Date).into(),
        "created_by_id" => (Frag::raw("issues.created_by_id"), ValueKind::Uuid).into(),
        "created_by__display_name" => {
            (Frag::raw("creator_user.display_name"), ValueKind::Text).into()
        }
        // Unreachable: every caller passes a column from `CHART_AXES`.
        _ => (Frag::raw("issues.id"), ValueKind::Uuid).into(),
    }
}

impl From<(Frag, ValueKind)> for ChartField {
    fn from((expression, kind): (Frag, ValueKind)) -> Self {
        Self { expression, kind }
    }
}

fn date_expression(column: &str, timezone: Tz) -> Frag {
    let mut frag = Frag::raw("(");
    frag.push_raw(column);
    frag.push_raw(" ");
    frag.push_raw(&sql::timezone_literal(timezone));
    frag.push_raw(")::date");
    frag
}

/// The `(id_field, name_field)` pair of `get_x_axis_field()` (`build_chart.py:44-75`).
fn fields(axis: &str) -> (&'static str, &'static str) {
    match axis {
        "STATES" => ("state__id", "state__name"),
        "STATE_GROUPS" => ("state__group", "state__group"),
        "LABELS" => ("labels__id", "labels__name"),
        "ASSIGNEES" => ("assignees__id", "assignees__display_name"),
        "ESTIMATE_POINTS" => ("estimate_point__key", "estimate_point__value"),
        "CYCLES" => ("issue_cycle__cycle_id", "issue_cycle__cycle__name"),
        "MODULES" => ("issue_module__module_id", "issue_module__module__name"),
        "PRIORITY" => ("priority", "priority"),
        "START_DATE" => ("start_date", "start_date"),
        "TARGET_DATE" => ("target_date", "target_date"),
        "CREATED_AT" => ("created_at__date", "created_at__date"),
        "COMPLETED_AT" => ("completed_at__date", "completed_at__date"),
        "CREATED_BY" => ("created_by_id", "created_by__display_name"),
        _ => ("state__id", "state__name"),
    }
}

/// The `additional_filter` of one axis (`build_chart.py:48-68`).
fn additional_filter(axis: &str) -> Option<&'static str> {
    match axis {
        "LABELS" => Some("issue_labels.deleted_at"),
        "ASSIGNEES" => Some("issue_assignees.deleted_at"),
        "CYCLES" => Some("cycle_issues.deleted_at"),
        "MODULES" => Some("module_issues.deleted_at"),
        _ => None,
    }
}

/// The joins an axis column needs, with Django's `LEFT OUTER` default.
fn require_join(base: &mut QueryBase, column: &str) {
    match column {
        "state__id" | "state__group" | "state__name" => {
            base.require_relation(Relation::States, JoinKind::Left);
        }
        "labels__id" | "labels__name" => {
            base.require_relation(Relation::IssueLabels, JoinKind::Left);
            base.require(plane_query::sql::Join {
                kind: JoinKind::Left,
                table: "labels",
                alias: "labels",
                on: Frag::raw("issue_labels.label_id = labels.id"),
            });
        }
        "assignees__id" | "assignees__display_name" => {
            base.require_relation(Relation::IssueAssignees, JoinKind::Left);
            sql::require_assignee_users(base, JoinKind::Left);
        }
        "estimate_point__key" | "estimate_point__value" => sql::require_estimate_points(base),
        "issue_cycle__cycle_id" | "issue_cycle__cycle__name" => {
            base.require_relation(Relation::CycleIssues, JoinKind::Left);
            base.require(plane_query::sql::Join {
                kind: JoinKind::Left,
                table: "cycles",
                alias: "cycles",
                on: Frag::raw("cycle_issues.cycle_id = cycles.id"),
            });
        }
        "issue_module__module_id" | "issue_module__module__name" => {
            base.require_relation(Relation::ModuleIssues, JoinKind::Left);
            base.require(plane_query::sql::Join {
                kind: JoinKind::Left,
                table: "modules",
                alias: "modules",
                on: Frag::raw("module_issues.module_id = modules.id"),
            });
        }
        "created_by_id" | "created_by__display_name" => {
            sql::require_creator_users(base, JoinKind::Left);
        }
        _ => {}
    }
}

fn validation(message: String) -> ViewError {
    ViewError::Api(ApiError::ValidationError(serde_json::json!([message])))
}

/// `build_analytics_chart(queryset, x_axis, group_by)`
/// (`plane/utils/build_chart.py:153-194`).
pub(crate) async fn build_analytics_chart(
    client: &deadpool_postgres::Client,
    base: QueryBase,
    x_axis: &str,
    group_by: Option<&str>,
    timezone: Tz,
) -> Result<Json, ViewError> {
    if !CHART_AXES.contains(&x_axis) {
        return Err(validation(format!("Invalid x_axis field: {x_axis}")));
    }
    if let Some(group_by) = group_by
        && !CHART_AXES.contains(&group_by)
    {
        return Err(validation(format!("Invalid group_by field: {group_by}")));
    }

    let mut base = base;
    let (id_column, name_column) = fields(x_axis);
    for column in [id_column, name_column] {
        require_join(&mut base, column);
    }
    if let Some(column) = additional_filter(x_axis) {
        base.and(Frag::is_null(column));
    }
    let group = group_by.map(|axis| {
        let (group_id, group_name) = fields(axis);
        for column in [group_id, group_name] {
            require_join(&mut base, column);
        }
        if let Some(column) = additional_filter(axis) {
            base.and(Frag::is_null(column));
        }
        (group_id, group_name)
    });

    let id = expression(id_column, timezone);
    let name = expression(name_column, timezone);
    if let Some((group_id, group_name)) = group {
        let group_id = expression(group_id, timezone);
        let group_name = expression(group_name, timezone);
        // Django's projection order: key, group_key, group_name, display_name,
        // count (`build_chart.py:120-128`).
        let mut ordered = Frag::raw("");
        ordered.push(id.expression.clone());
        ordered.push_raw(" AS key, ");
        ordered.push(group_id.expression.clone());
        ordered.push_raw(" AS group_key, ");
        ordered.push(group_name.expression.clone());
        ordered.push_raw(" AS group_name, ");
        ordered.push(name.expression.clone());
        ordered.push_raw(" AS display_name, COUNT(DISTINCT issues.id) AS count");
        let tail = group_by_tail(
            &[
                &id.expression,
                &group_id.expression,
                &group_name.expression,
                &name.expression,
            ],
            " ORDER BY 5 DESC",
        );
        let statement = base.statement(ordered, &tail);
        let rows = sql::fetch(client, &statement).await?;
        return grouped(rows, id.kind, name.kind, group_id.kind, group_name.kind);
    }

    let mut projection = Frag::raw("");
    projection.push(id.expression.clone());
    projection.push_raw(" AS key, ");
    projection.push(name.expression.clone());
    projection.push_raw(" AS display_name, COUNT(DISTINCT issues.id) AS count");
    let tail = group_by_tail(&[&id.expression, &name.expression], " ORDER BY 1 ASC");
    let statement = base.statement(projection, &tail);
    let rows = sql::fetch(client, &statement).await?;

    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        let key = read_value(row, 0, id.kind)?;
        let name = read_value(row, 1, name.kind)?;
        let count = read_count(row, 2)?;
        data.push(Json::Object(vec![
            (
                "key".to_owned(),
                if truthy(&key) {
                    key
                } else {
                    Json::Str("None".to_owned())
                },
            ),
            (
                "name".to_owned(),
                if truthy(&name) {
                    name
                } else {
                    Json::Str("None".to_owned())
                },
            ),
            ("count".to_owned(), count),
        ]));
    }

    Ok(Json::Object(vec![
        ("data".to_owned(), Json::Array(data)),
        ("schema".to_owned(), Json::Object(Vec::new())),
    ]))
}

/// One accumulator of `process_grouped_data` (`build_chart.py:78-98`).
struct Bucket {
    key: Json,
    name: Json,
    count: i64,
    /// `group_key → accumulated count`, in first-appearance order.
    values: Vec<(String, i64)>,
}

fn grouped(
    rows: Vec<Row>,
    key_kind: ValueKind,
    name_kind: ValueKind,
    group_key_kind: ValueKind,
    group_name_kind: ValueKind,
) -> Result<Json, ViewError> {
    let mut buckets: Vec<Bucket> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut schema: Vec<(String, Json)> = Vec::new();
    let mut schema_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for row in &rows {
        let key = read_value(row, 0, key_kind)?;
        let group_key = read_value(row, 1, group_key_kind)?;
        let group_name = read_value(row, 2, group_name_kind)?;
        let name = read_value(row, 3, name_kind)?;
        let count = match read_count(row, 4)? {
            Json::Int(value) => value,
            _ => 0,
        };

        let raw_key = py_str(&key);
        let slot = match index.get(&raw_key) {
            Some(slot) => *slot,
            None => {
                index.insert(raw_key.clone(), buckets.len());
                buckets.push(Bucket {
                    key: if truthy(&key) {
                        key
                    } else {
                        Json::Str("none".to_owned())
                    },
                    name: if truthy(&name) {
                        name
                    } else {
                        Json::Str("None".to_owned())
                    },
                    count: 0,
                    values: Vec::new(),
                });
                buckets.len() - 1
            }
        };

        let group_key_string = if truthy(&group_key) {
            py_str(&group_key)
        } else {
            "none".to_owned()
        };
        let bucket = &mut buckets[slot];
        match bucket
            .values
            .iter_mut()
            .find(|(existing, _)| *existing == group_key_string)
        {
            Some((_, slot)) => *slot += count,
            None => bucket.values.push((group_key_string.clone(), count)),
        }
        bucket.count += count;

        if !schema_index.contains_key(&group_key_string) {
            schema_index.insert(group_key_string.clone(), schema.len());
            schema.push((
                group_key_string,
                if truthy(&group_name) {
                    group_name
                } else {
                    Json::Str("None".to_owned())
                },
            ));
        }
    }

    let data = buckets
        .into_iter()
        .map(|bucket| {
            let mut entries = Vec::with_capacity(3 + bucket.values.len());
            entries.push(("key".to_owned(), bucket.key));
            entries.push(("name".to_owned(), bucket.name));
            entries.push(("count".to_owned(), Json::Int(bucket.count)));
            for (key, count) in bucket.values {
                entries.push((key, Json::Int(count)));
            }
            Json::Object(entries)
        })
        .collect();

    Ok(Json::Object(vec![
        ("data".to_owned(), Json::Array(data)),
        ("schema".to_owned(), Json::Object(schema)),
    ]))
}

/// Django's `GROUP BY <ordinals>` for a `values()` query
/// (`django/db/models/sql/compiler.py:get_group_by`).
///
/// Two details decide the ordinals, and the *order* is observable because it
/// feeds the aggregate's row order, which in turn decides how tied
/// `ORDER BY count DESC` rows come out:
///
/// * an expression that appears twice in the `SELECT` (x axis == display name,
///   or `group_key` == `group_name`) is emitted **once**, as its *last*
///   ordinal (`selected_expr_positions` maps the expression to the highest
///   position);
/// * the survivors keep the order of their first appearance in `SELECT`.
///
/// For `x_axis=STATE_GROUPS&group_by=STATES` the select is
/// `group, state_id, name, group` and Django groups by `4, 2, 3`.
fn group_by_tail(expressions: &[&Frag], order_by: &str) -> String {
    let mut distinct: Vec<(&str, usize)> = Vec::new();
    for (index, expression) in expressions.iter().enumerate() {
        match distinct
            .iter_mut()
            .find(|(existing, _)| *existing == expression.sql)
        {
            Some((_, position)) => *position = index + 1,
            None => distinct.push((expression.sql.as_str(), index + 1)),
        }
    }
    let ordinals: Vec<String> = distinct
        .iter()
        .map(|(_, position)| position.to_string())
        .collect();
    format!(" GROUP BY {}{order_by}", ordinals.join(", "))
}

fn read_count(row: &Row, index: usize) -> Result<Json, ViewError> {
    match row.try_get::<_, i64>(index) {
        Ok(value) => Ok(Json::Int(value)),
        Err(error) => Err(ViewError::from(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag(sql: &str) -> Frag {
        Frag::raw(sql.to_owned())
    }

    /// `django/db/models/sql/compiler.py:get_group_by` emits the *last*
    /// ordinal of each distinct select expression, in first-appearance order.
    /// The ordinals come from recorded Django output; the order matters
    /// because it decides how tied `ORDER BY count DESC` rows come out.
    #[test]
    fn group_by_ordinals_match_django() {
        // x_axis=STATES (state_id, name), group_by=STATE_GROUPS (group, group).
        let key = frag("issues.state_id");
        let group_a = frag("states.group");
        let group_b = frag("states.group");
        let name = frag("states.name");
        assert_eq!(
            group_by_tail(&[&key, &group_a, &group_b, &name], " ORDER BY 5 DESC"),
            " GROUP BY 1, 3, 4 ORDER BY 5 DESC"
        );

        // x_axis=STATE_GROUPS, group_by=STATES: select is
        // group, state_id, name, group -> Django groups by `4, 2, 3`.
        let key = frag("states.group");
        let group_a = frag("issues.state_id");
        let group_b = frag("states.name");
        let name = frag("states.group");
        assert_eq!(
            group_by_tail(&[&key, &group_a, &group_b, &name], " ORDER BY 5 DESC"),
            " GROUP BY 4, 2, 3 ORDER BY 5 DESC"
        );

        // Same expression in every position collapses to the last ordinal.
        let group = frag("states.group");
        assert_eq!(
            group_by_tail(&[&group, &group, &group, &group], " ORDER BY 5 DESC"),
            " GROUP BY 4 ORDER BY 5 DESC"
        );

        // The simple chart: key == display name -> `GROUP BY 2`.
        let priority = frag("issues.priority");
        assert_eq!(
            group_by_tail(&[&priority, &priority], " ORDER BY 1 ASC"),
            " GROUP BY 2 ORDER BY 1 ASC"
        );
        assert_eq!(
            group_by_tail(
                &[&frag("issue_labels.label_id"), &frag("labels.name")],
                " ORDER BY 1 ASC"
            ),
            " GROUP BY 1, 2 ORDER BY 1 ASC"
        );
    }
}
