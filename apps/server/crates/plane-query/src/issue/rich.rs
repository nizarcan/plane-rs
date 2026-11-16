// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The newer filters: `IssueFilterSet` (`plane/utils/filters/filterset.py`) and
//! `ComplexFilterBackend` (`filter_backend.py`).
//!
//! `IssueViewSet` runs both filter systems on the same queryset:
//! `self.filter_queryset(...)` (django-filter, driven by plain query
//! parameters) and then `issue_filters(...)` (the legacy module). The public
//! saved-view endpoint instead sends a JSON tree in `?filters=`.
//!
//! The quirks that decide rows:
//!
//! * Every relation filter adds its `deleted_at IS NULL` condition, so
//!   `label_id` joins `issue_labels` once whether the value matches or not.
//! * `is_archived` maps to `archived_at IS NULL/IS NOT NULL`; on the
//!   `Issue.issue_objects` queryset `is_archived=true` can never match (the
//!   manager already excludes archived issues) and `is_draft=true` never
//!   matches either.
//! * `created_at`/`updated_at` filters compare the **date in the active
//!   timezone** (`TimeZoneMixin` activates the user's profile timezone), not the
//!   UTC date, and `__range` covers whole days.
//! * `start_date__range`/`target_date__range` are `BaseRangeFilter`s: they need
//!   one comma-separated pair and reject everything else with "Range query
//!   expects two values." — including two repeated query parameters.
//! * A filter whose value fails validation makes the whole request a 400 whose
//!   body is the per-field error dict; an undeclared field in a JSON leaf is a
//!   400 with `{"message": …, "code": …}`.
//! * `build_combined_q` only processes filters that are present in the request
//!   (`provided_filters`), in the FilterSet's declaration order, not the order
//!   the parameters arrived in.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use plane_drf::QueryDict;

use crate::error::{ApiError, QueryError};
use crate::issue::filter::parse_uuid;
use crate::issue::model::{Relation, ValueField};
use crate::json::Json;
use crate::sql::{Frag, JoinKind, Value};

/// A rich filter's joins and predicate.
#[derive(Debug, Clone)]
pub struct RichFilters {
    pub joins: Vec<(Relation, JoinKind)>,
    pub predicate: Frag,
}

impl RichFilters {
    fn from_parts(mut joins: Vec<(Relation, JoinKind)>, predicate: Frag) -> Option<Self> {
        if predicate.is_empty() && joins.is_empty() {
            return None;
        }
        // One join per relation; a later INNER requirement wins.
        let mut deduped: Vec<(Relation, JoinKind)> = Vec::new();
        for (relation, kind) in joins.drain(..) {
            match deduped.iter_mut().find(|(item, _)| *item == relation) {
                Some(slot) => {
                    if kind == JoinKind::Inner {
                        slot.1 = JoinKind::Inner;
                    }
                }
                None => deduped.push((relation, kind)),
            }
        }
        Some(Self {
            joins: deduped,
            predicate,
        })
    }
}

/// The kind of a declared filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterKind {
    /// `UUIDFilter(field_name=…)`.
    Uuid(&'static str),
    /// `UUIDInFilter(field_name=…)`: comma separated, `in` lookup.
    UuidIn(&'static str),
    /// `UUIDFilter(method="filter_<relation>_id")`.
    UuidRelation(Relation, &'static str),
    /// `UUIDInFilter(method="filter_<relation>_id_in")`.
    UuidRelationIn(Relation, &'static str),
    /// `BooleanFilter(method="filter_is_archived")`.
    IsArchived,
    /// `BooleanFilter(field_name="is_draft")`.
    Bool(&'static str),
    /// `CharFilter(field_name="state__group")`.
    Char(&'static str),
    /// `CharInFilter(field_name="state__group")`.
    CharIn(&'static str),
    /// `ChoiceFilter` over the model's priority choices.
    Priority,
    /// `CharInFilter(field_name="priority")`.
    PriorityIn,
    /// `DateFilter(field_name=<col>, lookup_expr="date")`.
    DateExact(&'static str),
    /// `DateCSVRangeFilter(field_name=<col>, lookup_expr="date__range")`.
    DateCsvRange(&'static str),
    /// `DateFilter(field_name=<col>)` on a `DateField`.
    DatePlain(&'static str),
    /// `BaseRangeFilter(field_name=<col>, lookup_expr="range")` on a `DateField`.
    DateRange(&'static str),
}

/// `IssueFilterSet.base_filters`, in declaration order (the order
/// `cleaned_data` iterates, which is the order the conditions are ANDed in).
const FILTERS: &[(&str, FilterKind)] = &[
    (
        "assignee_id",
        FilterKind::UuidRelation(Relation::IssueAssignees, "assignee_id"),
    ),
    (
        "assignee_id__exact",
        FilterKind::UuidRelation(Relation::IssueAssignees, "assignee_id"),
    ),
    (
        "assignee_id__in",
        FilterKind::UuidRelationIn(Relation::IssueAssignees, "assignee_id"),
    ),
    ("created_at", FilterKind::DateExact("created_at")),
    ("created_at__exact", FilterKind::DateExact("created_at")),
    ("created_at__range", FilterKind::DateCsvRange("created_at")),
    ("created_by_id", FilterKind::Uuid("created_by_id")),
    ("created_by_id__exact", FilterKind::Uuid("created_by_id")),
    ("created_by_id__in", FilterKind::UuidIn("created_by_id")),
    (
        "cycle_id",
        FilterKind::UuidRelation(Relation::CycleIssues, "cycle_id"),
    ),
    (
        "cycle_id__exact",
        FilterKind::UuidRelation(Relation::CycleIssues, "cycle_id"),
    ),
    (
        "cycle_id__in",
        FilterKind::UuidRelationIn(Relation::CycleIssues, "cycle_id"),
    ),
    ("is_archived", FilterKind::IsArchived),
    ("is_archived__exact", FilterKind::IsArchived),
    ("is_draft", FilterKind::Bool("is_draft")),
    ("is_draft__exact", FilterKind::Bool("is_draft")),
    (
        "label_id",
        FilterKind::UuidRelation(Relation::IssueLabels, "label_id"),
    ),
    (
        "label_id__exact",
        FilterKind::UuidRelation(Relation::IssueLabels, "label_id"),
    ),
    (
        "label_id__in",
        FilterKind::UuidRelationIn(Relation::IssueLabels, "label_id"),
    ),
    (
        "mention_id",
        FilterKind::UuidRelation(Relation::IssueMentions, "mention_id"),
    ),
    (
        "mention_id__exact",
        FilterKind::UuidRelation(Relation::IssueMentions, "mention_id"),
    ),
    (
        "mention_id__in",
        FilterKind::UuidRelationIn(Relation::IssueMentions, "mention_id"),
    ),
    (
        "module_id",
        FilterKind::UuidRelation(Relation::ModuleIssues, "module_id"),
    ),
    (
        "module_id__exact",
        FilterKind::UuidRelation(Relation::ModuleIssues, "module_id"),
    ),
    (
        "module_id__in",
        FilterKind::UuidRelationIn(Relation::ModuleIssues, "module_id"),
    ),
    ("priority", FilterKind::Priority),
    ("priority__exact", FilterKind::Priority),
    ("priority__in", FilterKind::PriorityIn),
    ("project_id", FilterKind::Uuid("project_id")),
    ("project_id__exact", FilterKind::Uuid("project_id")),
    ("project_id__in", FilterKind::UuidIn("project_id")),
    ("start_date", FilterKind::DatePlain("start_date")),
    ("start_date__exact", FilterKind::DatePlain("start_date")),
    ("start_date__range", FilterKind::DateRange("start_date")),
    ("state_group", FilterKind::Char("state__group")),
    ("state_group__exact", FilterKind::Char("state__group")),
    ("state_group__in", FilterKind::CharIn("state__group")),
    ("state_id", FilterKind::Uuid("state_id")),
    ("state_id__exact", FilterKind::Uuid("state_id")),
    ("state_id__in", FilterKind::UuidIn("state_id")),
    (
        "subscriber_id",
        FilterKind::UuidRelation(Relation::IssueSubscribers, "subscriber_id"),
    ),
    (
        "subscriber_id__exact",
        FilterKind::UuidRelation(Relation::IssueSubscribers, "subscriber_id"),
    ),
    (
        "subscriber_id__in",
        FilterKind::UuidRelationIn(Relation::IssueSubscribers, "subscriber_id"),
    ),
    ("target_date", FilterKind::DatePlain("target_date")),
    ("target_date__exact", FilterKind::DatePlain("target_date")),
    ("target_date__range", FilterKind::DateRange("target_date")),
    ("updated_at", FilterKind::DateExact("updated_at")),
    ("updated_at__exact", FilterKind::DateExact("updated_at")),
    ("updated_at__range", FilterKind::DateCsvRange("updated_at")),
];

/// The declared filter names, as `ComplexFilterBackend._validate_fields` sees
/// them.
pub fn filter_names() -> Vec<&'static str> {
    FILTERS.iter().map(|(name, _)| *name).collect()
}

/// The value a filter produces after validation.
#[derive(Debug, Clone, PartialEq)]
enum Cleaned {
    Uuid(Uuid),
    Uuids(Vec<Uuid>),
    Text(String),
    Texts(Vec<String>),
    Bool(bool),
    Date(NaiveDate),
    DateRange(NaiveDate, NaiveDate),
}

/// One error per filter, as django-filter's form reports them.
type Errors = Vec<(String, Vec<String>)>;

fn push_error(errors: &mut Errors, field: &str, message: impl Into<String>) {
    if let Some(slot) = errors.iter_mut().find(|(name, _)| name == field) {
        slot.1.push(message.into());
    } else {
        errors.push((field.to_owned(), vec![message.into()]));
    }
}

fn errors_detail(errors: &Errors) -> Json {
    Json::Object(
        errors
            .iter()
            .map(|(field, messages)| {
                (
                    field.clone(),
                    Json::Array(messages.iter().map(Json::string).collect()),
                )
            })
            .collect(),
    )
}

/// `IssueFilterSet(data=params, queryset=…)` plus `DjangoFilterBackend`'s
/// "raise on invalid" behaviour, then `build_combined_q`.
pub fn parse_filterset(
    params: &QueryDict,
    timezone: Tz,
) -> Result<Option<RichFilters>, QueryError> {
    let mut cleaned: Vec<(&'static str, FilterKind, Cleaned)> = Vec::new();
    let mut errors: Errors = Vec::new();

    for (name, kind) in FILTERS {
        // `QueryDict.get(name)`: the last value. A missing key is not provided.
        let Some(raw) = params.get(name) else {
            continue;
        };
        match clean_value(name, *kind, raw, timezone) {
            Ok(None) => {}
            Ok(Some(value)) => cleaned.push((name, *kind, value)),
            Err(message) => push_error(&mut errors, name, message),
        }
    }
    if !errors.is_empty() {
        return Err(QueryError::Api(ApiError {
            status: 400,
            message: String::new(),
            code: None,
            errors: Some(errors_detail(&errors)),
        }));
    }

    let mut joins = Vec::new();
    let mut predicate = Frag::empty();
    for (name, kind, value) in cleaned {
        let (filter_joins, condition) = condition_for(name, kind, &value, timezone);
        joins.extend(filter_joins);
        predicate = predicate.and(condition);
    }
    Ok(RichFilters::from_parts(joins, predicate))
}

/// Validate one raw query-parameter value the way django-filter's form field
/// does. `Ok(None)` means "valid but empty", which adds no condition.
fn clean_value(
    name: &str,
    kind: FilterKind,
    raw: &str,
    timezone: Tz,
) -> Result<Option<Cleaned>, String> {
    match kind {
        FilterKind::Uuid(_) | FilterKind::UuidRelation(..) => parse_uuid(raw)
            .map(Cleaned::Uuid)
            .map(Some)
            .ok_or_else(|| "Enter a valid UUID.".to_owned()),
        FilterKind::UuidIn(_) | FilterKind::UuidRelationIn(..) => {
            let mut ids = Vec::new();
            for item in raw.split(',') {
                ids.push(parse_uuid(item).ok_or_else(|| "Enter a valid UUID.".to_owned())?);
            }
            Ok(Some(Cleaned::Uuids(ids)))
        }
        FilterKind::Char(_) => Ok(Some(Cleaned::Text(raw.to_owned()))),
        FilterKind::CharIn(_) => Ok(Some(Cleaned::Texts(
            raw.split(',').map(str::to_owned).collect(),
        ))),
        FilterKind::IsArchived | FilterKind::Bool(_) => Ok(boolean(raw).map(Cleaned::Bool)),
        FilterKind::Priority => {
            if ["urgent", "high", "medium", "low", "none"].contains(&raw) {
                Ok(Some(Cleaned::Text(raw.to_owned())))
            } else {
                Err(format!(
                    "Select a valid choice. {raw} is not one of the available choices."
                ))
            }
        }
        FilterKind::PriorityIn => Ok(Some(Cleaned::Texts(
            raw.split(',').map(str::to_owned).collect(),
        ))),
        FilterKind::DateExact(column) | FilterKind::DatePlain(column) => {
            let _ = column;
            parse_date(raw)
                .map(|date| Some(Cleaned::Date(date)))
                .ok_or_else(|| "Enter a valid date.".to_owned())
        }
        FilterKind::DateCsvRange(_) | FilterKind::DateRange(_) => {
            let parts: Vec<&str> = raw.split(',').collect();
            if parts.len() != 2 {
                return Err("Range query expects two values.".to_owned());
            }
            let start = parse_date(parts[0]).ok_or_else(|| "Enter a valid date.".to_owned())?;
            let stop = parse_date(parts[1]).ok_or_else(|| "Enter a valid date.".to_owned())?;
            Ok(Some(Cleaned::DateRange(start, stop)))
        }
    }
    .inspect(|_value| {
        let _ = (name, timezone);
    })
}

/// `forms.BooleanField`/`NullBooleanField`: only the recognised spellings map
/// to a boolean; anything else is `None` (valid, no condition).
fn boolean(raw: &str) -> Option<bool> {
    match raw {
        "true" | "True" | "1" => Some(true),
        "false" | "False" | "0" => Some(false),
        _ => None,
    }
}

/// `django.forms.DateField.to_python`: `YYYY-MM-DD` (through
/// `django.utils.dateparse.parse_date`).
fn parse_date(value: &str) -> Option<NaiveDate> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return None;
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

/// The joins and condition for one validated filter.
fn condition_for(
    name: &str,
    kind: FilterKind,
    value: &Cleaned,
    timezone: Tz,
) -> (Vec<(Relation, JoinKind)>, Frag) {
    condition_for_negated(name, kind, value, timezone, false)
}

/// `Query.build_filter`'s null handling for a negated branch
/// (`django/db/models/sql/query.py:1600-1620`): a negated `col = value` gets an
/// `AND col IS NOT NULL` conjunct when the column is nullable or comes from a
/// `LEFT OUTER` join, so SQL's three-valued logic matches Python's
/// `not (None == value)`. Without it the strict port silently drops rows.
fn needs_null_guard(column: &str) -> bool {
    match column {
        // NOT NULL columns of `issues`.
        "issues.priority" | "issues.is_draft" | "issues.project_id" => false,
        // An `isnull` lookup never gets the guard.
        "issues.archived_at" => false,
        _ => true,
    }
}

fn condition_for_negated(
    name: &str,
    kind: FilterKind,
    value: &Cleaned,
    timezone: Tz,
    negated: bool,
) -> (Vec<(Relation, JoinKind)>, Frag) {
    let mut joins = Vec::new();
    let column = match kind {
        FilterKind::Uuid(column) => Some(column),
        FilterKind::UuidIn(column) => Some(column),
        FilterKind::Bool(column) => Some(column),
        FilterKind::Char(column) => Some(column),
        FilterKind::CharIn(column) => Some(column),
        _ => None,
    };
    if let Some(column) = column {
        let column = match column {
            "is_draft" => "issues.is_draft",
            "state__group" => "states.group",
            other => match other {
                "created_by_id" => "issues.created_by_id",
                "project_id" => "issues.project_id",
                "state_id" => "issues.state_id",
                "priority" => "issues.priority",
                other => other,
            },
        };
        if column.starts_with("states.") {
            joins.push((Relation::States, JoinKind::Left));
        }
        let condition = match value {
            Cleaned::Uuid(id) => Frag::compare(column, "=", Value::Uuid(*id)),
            Cleaned::Uuids(ids) => Frag::in_uuids(column, ids),
            Cleaned::Text(text) => Frag::compare(column, "=", Value::Text(text.clone())),
            Cleaned::Texts(texts) => Frag::in_texts(column, texts),
            Cleaned::Bool(flag) => Frag::compare(column, "=", Value::Bool(*flag)),
            _ => Frag::empty(),
        };
        let condition = if negated && needs_null_guard(column) {
            condition.and(Frag::is_not_null(column))
        } else {
            condition
        };
        return (joins, condition);
    }

    match kind {
        FilterKind::UuidRelation(relation, column)
        | FilterKind::UuidRelationIn(relation, column) => {
            joins.push((relation, JoinKind::Left));
            let condition = match value {
                Cleaned::Uuid(id) => Frag::compare(
                    &format!("{}.{column}", relation.alias()),
                    "=",
                    Value::Uuid(*id),
                ),
                Cleaned::Uuids(ids) => {
                    Frag::in_uuids(&format!("{}.{column}", relation.alias()), ids)
                }
                _ => Frag::empty(),
            };
            // Every relation method adds `deleted_at IS NULL`
            // (`filterset.py:215-297`).
            let deleted_at = Frag::is_null(&format!("{}.deleted_at", relation.alias()));
            let column = format!("{}.{column}", relation.alias());
            let condition = condition.and(deleted_at);
            let condition = if negated {
                condition.and(Frag::is_not_null(&column))
            } else {
                condition
            };
            let _ = name;
            (joins, condition)
        }
        FilterKind::IsArchived => {
            let condition = match value {
                Cleaned::Bool(true) => Frag::is_not_null("issues.archived_at"),
                Cleaned::Bool(false) => Frag::is_null("issues.archived_at"),
                _ => Frag::empty(),
            };
            (joins, condition)
        }
        FilterKind::Priority => match value {
            Cleaned::Text(text) => (
                joins,
                Frag::compare("issues.priority", "=", Value::Text(text.clone())),
            ),
            _ => (joins, Frag::empty()),
        },
        FilterKind::PriorityIn => match value {
            Cleaned::Texts(texts) => (joins, Frag::in_texts("issues.priority", texts)),
            _ => (joins, Frag::empty()),
        },
        FilterKind::DateExact(column) => {
            let condition = match value {
                Cleaned::Date(date) => date_equals(column, *date, timezone),
                _ => Frag::empty(),
            };
            (joins, condition)
        }
        FilterKind::DateCsvRange(column) => {
            let condition = match value {
                Cleaned::DateRange(start, stop) => date_range(column, *start, *stop, timezone),
                _ => Frag::empty(),
            };
            (joins, condition)
        }
        FilterKind::DatePlain(column) => {
            let column_name = format!("issues.{column}");
            let condition = match value {
                Cleaned::Date(date) => Frag::compare(&column_name, "=", Value::Date(*date)),
                _ => Frag::empty(),
            };
            let condition = if negated && !condition.is_empty() {
                condition.and(Frag::is_not_null(&column_name))
            } else {
                condition
            };
            (joins, condition)
        }
        FilterKind::DateRange(column) => {
            let mut condition = match value {
                Cleaned::DateRange(start, stop) => {
                    let mut frag = Frag::raw(format!("issues.{column} BETWEEN "));
                    frag.push(Frag::bind(Value::Date(*start)));
                    frag.push_raw(" AND ");
                    frag.push(Frag::bind(Value::Date(*stop)));
                    frag
                }
                _ => Frag::empty(),
            };
            if negated && !condition.is_empty() {
                condition = condition.and(Frag::is_not_null(&format!("issues.{column}")));
            }
            (joins, condition)
        }
        FilterKind::Uuid(_)
        | FilterKind::UuidIn(_)
        | FilterKind::Bool(_)
        | FilterKind::Char(_)
        | FilterKind::CharIn(_) => (joins, Frag::empty()),
    }
}

/// `(created_at AT TIME ZONE tz)::date = date`.
fn date_equals(column: &str, date: NaiveDate, timezone: Tz) -> Frag {
    let mut frag = Frag::raw("(issues.");
    frag.push_raw(column);
    frag.push_raw(" AT TIME ZONE ");
    frag.push(Frag::bind(Value::Text(timezone.name().to_owned())));
    frag.push_raw(")::date = ");
    frag.push(Frag::bind(Value::Date(date)));
    frag
}

/// `(created_at AT TIME ZONE tz)::date BETWEEN start AND stop`.
fn date_range(column: &str, start: NaiveDate, stop: NaiveDate, timezone: Tz) -> Frag {
    let mut frag = Frag::raw("(issues.");
    frag.push_raw(column);
    frag.push_raw(" AT TIME ZONE ");
    frag.push(Frag::bind(Value::Text(timezone.name().to_owned())));
    frag.push_raw(")::date BETWEEN ");
    frag.push(Frag::bind(Value::Date(start)));
    frag.push_raw(" AND ");
    frag.push(Frag::bind(Value::Date(stop)));
    frag
}

// ---------------------------------------------------------------------------
// ComplexFilterBackend
// ---------------------------------------------------------------------------

/// `ComplexFilterBackend.default_max_depth`.
pub const DEFAULT_MAX_DEPTH: u32 = 5;

/// `ComplexFilterBackend.filter_queryset`'s JSON branch.
pub fn parse_complex_json(
    filter_data: &JsonValue,
    timezone: Tz,
) -> Result<Option<RichFilters>, QueryError> {
    let depth = DEFAULT_MAX_DEPTH;
    validate_structure(filter_data, depth, 1)?;
    validate_fields(filter_data)?;
    let mut joins = Vec::new();
    let predicate = evaluate_node(filter_data, timezone, &mut joins)?;
    Ok(RichFilters::from_parts(
        joins,
        predicate.unwrap_or_default(),
    ))
}

/// Read and parse the `filters` query parameter.
pub fn parse_complex_query(
    params: &QueryDict,
    timezone: Tz,
) -> Result<Option<RichFilters>, QueryError> {
    let Some(raw) = params.get("filters") else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let parsed: JsonValue = serde_json::from_str(raw).map_err(|_| {
        QueryError::Api(ApiError {
            status: 400,
            message: "Invalid JSON for 'filter'. Expected a valid JSON object.".to_owned(),
            code: Some("invalid_json".to_owned()),
            errors: None,
        })
    })?;
    if !parsed.is_object() {
        return Err(QueryError::Api(ApiError {
            status: 400,
            message: "'filter' must be a dict or a JSON string.".to_owned(),
            code: Some("invalid_filter_type".to_owned()),
            errors: None,
        }));
    }
    parse_complex_json(&parsed, timezone)
}

fn coded(message: impl Into<String>, code: &str) -> QueryError {
    QueryError::Api(ApiError {
        status: 400,
        message: message.into(),
        code: Some(code.to_owned()),
        errors: None,
    })
}

/// `_validate_structure`.
fn validate_structure(
    node: &JsonValue,
    max_depth: u32,
    current_depth: u32,
) -> Result<(), QueryError> {
    if current_depth > max_depth {
        return Err(coded(
            format!("Filter nesting is too deep (max {max_depth}); found depth {current_depth}"),
            "max_depth_exceeded",
        ));
    }
    let Some(object) = node.as_object() else {
        return Err(coded(
            "Each filter node must be a JSON object",
            "invalid_filter_node",
        ));
    };
    if object.is_empty() {
        return Err(coded(
            "Filter objects must not be empty",
            "empty_filter_object",
        ));
    }
    let logical: Vec<&String> = object
        .keys()
        .filter(|key| matches!(key.to_ascii_lowercase().as_str(), "or" | "and" | "not"))
        .collect();
    if logical.len() > 1 {
        return Err(coded(
            "A filter object cannot contain multiple logical operators at the same level",
            "multiple_logical_operators",
        ));
    }
    if let Some(op_key) = logical.first() {
        if object.len() != 1 {
            return Err(coded(
                format!("Cannot mix logical operator '{op_key}' with field keys at the same level"),
                "mixed_operator_and_fields",
            ));
        }
        let op = op_key.to_ascii_lowercase();
        let value = &object[op_key.as_str()];
        if op == "or" || op == "and" {
            let Some(children) = value.as_array() else {
                return Err(coded(
                    format!("'{op}' must be a non-empty list of filter objects"),
                    "invalid_operator_children",
                ));
            };
            if children.is_empty() {
                return Err(coded(
                    format!("'{op}' must be a non-empty list of filter objects"),
                    "invalid_operator_children",
                ));
            }
            for child in children {
                if !child.is_object() {
                    return Err(coded(
                        format!("All children of '{op}' must be JSON objects"),
                        "invalid_operator_child_type",
                    ));
                }
                validate_structure(child, max_depth, current_depth + 1)?;
            }
            return Ok(());
        }
        if op == "not" {
            if !value.is_object() {
                return Err(coded(
                    "'not' must be a single JSON object",
                    "invalid_not_child",
                ));
            }
            return validate_structure(value, max_depth, current_depth + 1);
        }
    }
    validate_leaf(object)
}

fn validate_leaf(object: &serde_json::Map<String, JsonValue>) -> Result<(), QueryError> {
    if object.is_empty() {
        return Err(coded(
            "Leaf filter must be a non-empty JSON object",
            "invalid_leaf",
        ));
    }
    for (key, value) in object {
        if matches!(key.to_ascii_lowercase().as_str(), "or" | "and" | "not") {
            return Err(coded(
                "Logical operators cannot appear in a leaf filter object",
                "operator_in_leaf",
            ));
        }
        if let Some(items) = value.as_array() {
            if items.is_empty() {
                return Err(coded(
                    format!("List value for '{key}' must not be empty"),
                    "empty_list_value",
                ));
            }
            for item in items {
                if !is_scalar(item) {
                    return Err(coded(
                        format!("List value for '{key}' must contain only scalar items"),
                        "non_scalar_list_item",
                    ));
                }
            }
            continue;
        }
        if !is_scalar(value) {
            return Err(coded(
                format!("Value for '{key}' must be a scalar, null, or list/tuple of scalars"),
                "invalid_value_type",
            ));
        }
    }
    Ok(())
}

fn is_scalar(value: &JsonValue) -> bool {
    matches!(
        value,
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_)
    )
}

/// `_validate_fields`: every leaf key must be a declared filter name.
fn validate_fields(node: &JsonValue) -> Result<(), QueryError> {
    for field in extract_field_names(node) {
        if !FILTERS.iter().any(|(name, _)| *name == field) {
            return Err(coded(
                format!("Filtering on field '{field}' is not allowed"),
                "invalid_filter_field",
            ));
        }
    }
    Ok(())
}

fn extract_field_names(node: &JsonValue) -> Vec<String> {
    let Some(object) = node.as_object() else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    for (key, value) in object {
        match key.to_ascii_lowercase().as_str() {
            "not" => {
                if value.is_object() {
                    fields.extend(extract_field_names(value));
                }
            }
            "or" | "and" => {
                if let Some(children) = value.as_array() {
                    for child in children {
                        fields.extend(extract_field_names(child));
                    }
                }
            }
            _ => fields.push(key.clone()),
        }
    }
    fields
}

/// `_evaluate_node`: returns the condition, or `None` for a node that adds
/// nothing (`Q()` in Python).
fn evaluate_node(
    node: &JsonValue,
    timezone: Tz,
    joins: &mut Vec<(Relation, JoinKind)>,
) -> Result<Option<Frag>, QueryError> {
    evaluate_node_negated(node, timezone, joins, false)
}

fn evaluate_node_negated(
    node: &JsonValue,
    timezone: Tz,
    joins: &mut Vec<(Relation, JoinKind)>,
    negated: bool,
) -> Result<Option<Frag>, QueryError> {
    let Some(object) = node.as_object() else {
        return Ok(None);
    };
    if let Some(children) = object.get("or") {
        let Some(children) = children.as_array() else {
            return Ok(None);
        };
        if children.is_empty() {
            return Ok(None);
        }
        let mut combined = Frag::empty();
        for child in children {
            if let Some(child) = evaluate_node_negated(child, timezone, joins, negated)? {
                combined = combined.or(child);
            }
        }
        return Ok(Some(combined));
    }
    if let Some(children) = object.get("and") {
        let Some(children) = children.as_array() else {
            return Ok(None);
        };
        if children.is_empty() {
            return Ok(None);
        }
        let mut combined = Frag::empty();
        for child in children {
            if let Some(child) = evaluate_node_negated(child, timezone, joins, negated)? {
                combined = combined.and(child);
            }
        }
        return Ok(Some(combined));
    }
    if let Some(child) = object.get("not") {
        if !child.is_object() {
            return Ok(None);
        }
        let Some(condition) = evaluate_node_negated(child, timezone, joins, !negated)? else {
            return Ok(None);
        };
        return Ok(Some(condition.negate()));
    }
    build_leaf(object, timezone, joins, negated).map(Some)
}

/// `_build_leaf_q`: a leaf dict is one QueryDict per field, validated through
/// the same filterset, then ANDed.
fn build_leaf(
    object: &serde_json::Map<String, JsonValue>,
    timezone: Tz,
    joins: &mut Vec<(Relation, JoinKind)>,
    negated: bool,
) -> Result<Frag, QueryError> {
    let mut predicate = Frag::empty();
    for (key, value) in object {
        let Some((_, kind)) = FILTERS.iter().find(|(name, _)| name == key) else {
            return Err(coded(
                format!("Filtering on field '{key}' is not allowed"),
                "invalid_filter_field",
            ));
        };
        // The backend serialises a list by repeating the key, and the
        // filterset's `__in`/range fields take a comma list; joining with
        // commas matches both.
        let raw = if let Some(items) = value.as_array() {
            // `_build_leaf_q` writes a list with `qd.setlist(key, …)`, and the
            // filterset reads `data.get(key)`, which returns the **last**
            // value — so `{"priority__in": ["urgent", "high"]}` silently
            // filters on `high` only. The legacy `?priority__in=urgent,high`
            // form is a single CSV value and keeps both.
            items
                .last()
                .map(|item| match item {
                    JsonValue::String(text) => text.clone(),
                    JsonValue::Bool(flag) => flag.to_string(),
                    JsonValue::Number(number) => number.to_string(),
                    JsonValue::Null => String::new(),
                    other => other.to_string(),
                })
                .unwrap_or_default()
        } else {
            match value {
                JsonValue::Null => String::new(),
                JsonValue::String(text) => text.clone(),
                JsonValue::Bool(flag) => flag.to_string(),
                JsonValue::Number(number) => number.to_string(),
                _other => {
                    return Err(coded(
                        format!(
                            "Value for '{key}' must be a scalar, null, or list/tuple of scalars"
                        ),
                        "invalid_value_type",
                    ));
                }
            }
        };
        let cleaned = clean_value(key, *kind, &raw, timezone).map_err(|message| {
            QueryError::Api(ApiError {
                status: 400,
                message: "Invalid filter parameters".to_owned(),
                code: Some("invalid_filterset".to_owned()),
                errors: Some(errors_detail(&vec![(key.clone(), vec![message])])),
            })
        })?;
        let Some(cleaned) = cleaned else {
            continue;
        };
        // A negated condition on a multi-valued relation is a `split_exclude`
        // in Django: the relation is tested with `EXISTS` subqueries and the
        // whole conjunction is negated, so an issue that has the value *anywhere*
        // is excluded. A `LEFT JOIN` + `NOT` would keep it (other rows satisfy
        // the negation). Django emits one `EXISTS` per condition
        // (`label_issue__deleted_at__isnull` and `label_issue__label_id`), which
        // matters when the matching row is soft-deleted.
        if negated
            && let FilterKind::UuidRelation(relation, column)
            | FilterKind::UuidRelationIn(relation, column) = kind
        {
            let exists_live = Frag::raw(format!(
                "EXISTS(SELECT 1 FROM {} WHERE {}.issue_id = issues.id AND {}.deleted_at IS NULL)",
                relation.table(),
                relation.table(),
                relation.table()
            ));
            let relation_column = format!("{}.{column}", relation.table());
            let mut exists_value = match &cleaned {
                Cleaned::Uuid(id) => Frag::raw(format!(
                    "EXISTS(SELECT 1 FROM {table} WHERE {column} = ",
                    table = relation.table(),
                    column = relation_column
                ))
                .append(Frag::bind(Value::Uuid(*id))),
                Cleaned::Uuids(ids) => {
                    let mut frag = Frag::raw(format!(
                        "EXISTS(SELECT 1 FROM {table} WHERE {column} IN (",
                        table = relation.table(),
                        column = relation_column
                    ));
                    for (index, id) in ids.iter().enumerate() {
                        if index > 0 {
                            frag.push_raw(", ");
                        }
                        frag.push(Frag::bind(Value::Uuid(*id)));
                    }
                    frag.push_raw(")");
                    frag
                }
                _ => Frag::empty(),
            };
            exists_value.push_raw(&format!(" AND {}.issue_id = issues.id)", relation.table()));
            predicate = predicate.and(exists_live).and(exists_value);
            continue;
        }
        let (mut filter_joins, condition) =
            condition_for_negated(key, *kind, &cleaned, timezone, negated);
        joins.append(&mut filter_joins);
        predicate = predicate.and(condition);
    }
    Ok(predicate)
}

/// Sort a list of `(group, count)` totals the way Python's dict keeps them:
/// insertion order. Provided for the paginator.
pub type Totals = BTreeMap<String, i64>;

/// A `DateTime<Utc>` helper for callers that build their own `now`.
pub fn utc_now() -> DateTime<Utc> {
    Utc::now()
}

/// The value fields a `filters`-only request returns (kept here so the module
/// documents the dependency).
pub const _VALUE_FIELDS: [ValueField; 1] = [ValueField::Id];

#[cfg(test)]
mod tests {
    use super::*;

    fn filters(query: &str) -> Result<Option<RichFilters>, QueryError> {
        let params = QueryDict::parse(query, None).expect("query");
        parse_filterset(&params, Tz::UTC)
    }

    #[test]
    fn invalid_uuid_is_a_field_error() {
        let error = filters("state_id=not-a-uuid").expect_err("error");
        assert_eq!(error.status(), Some(400));
        assert_eq!(
            error.detail().expect("detail").to_serde(),
            serde_json::json!({"state_id": ["Enter a valid UUID."]})
        );
    }

    #[test]
    fn invalid_choice_names_the_value() {
        let error = filters("priority=nonsense").expect_err("error");
        assert_eq!(
            error.detail().expect("detail").to_serde(),
            serde_json::json!({"priority": ["Select a valid choice. nonsense is not one of the available choices."]})
        );
    }

    #[test]
    fn unknown_boolean_is_an_empty_filter() {
        assert!(filters("is_archived=nonsense").expect("ok").is_none());
    }

    #[test]
    fn range_needs_one_csv_pair() {
        let error = filters("start_date__range=2025-01-01").expect_err("error");
        assert_eq!(
            error.detail().expect("detail").to_serde(),
            serde_json::json!({"start_date__range": ["Range query expects two values."]})
        );
    }

    #[test]
    fn filter_names_match_the_declaration_list() {
        let names = filter_names();
        assert_eq!(names.len(), 49);
        assert!(names.contains(&"label_id__in"));
        assert!(names.contains(&"created_at__range"));
        assert!(!names.contains(&"filters"));
    }

    #[test]
    fn json_structure_errors_match_the_backend() {
        let data = serde_json::json!({"and": [{"priority": "urgent"}], "priority": "low"});
        let error = parse_complex_json(&data, Tz::UTC).expect_err("error");
        assert_eq!(error.status(), Some(400));
        assert_eq!(
            error.detail().expect("detail").to_serde(),
            serde_json::json!({"message": "Cannot mix logical operator 'and' with field keys at the same level", "code": "mixed_operator_and_fields"})
        );
    }

    #[test]
    fn json_field_allowlist_is_enforced() {
        let data = serde_json::json!({"unknown_field": "x"});
        let error = parse_complex_json(&data, Tz::UTC).expect_err("error");
        assert_eq!(
            error.detail().expect("detail").to_serde(),
            serde_json::json!({"message": "Filtering on field 'unknown_field' is not allowed", "code": "invalid_filter_field"})
        );
    }
}
