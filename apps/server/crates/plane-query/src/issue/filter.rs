// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/issue_filters.py`, ported.
//!
//! The Python builds a `dict` of Django lookups from the query parameters and
//! hands it to `Issue.issue_objects.filter(**filters)`. The dict is a
//! *specification*, not an implementation detail: which lookups exist decides
//! which joins Django adds, and joins decide duplicates and therefore the
//! paginator's counts. This module reproduces the lookups, their quirks
//! included:
//!
//! * `null` is dropped from every comma list but never means "is null";
//!   `None` (capital N) is what selects the null relation
//!   (`issue_filters.py:136,150,165,192,323,338,417`).
//! * An empty or wholly invalid value adds no lookup at all, so the filter
//!   silently disappears (`issue_filters.py:88,99,110,125,139,153,168`).
//! * `labels`, `assignees`, `cycle`, `module` and `subscriber` add their
//!   `deleted_at IS NULL` join condition even when the id list is empty, which
//!   is what turns the join into a `LEFT OUTER JOIN` and can multiply rows
//!   (`issue_filters.py:158,173,331,346,401`).
//! * A GET date filter takes `;after` / `;before` / `N_weeks;after[;fromnow]`;
//!   anything else becomes a `LIKE` on the date's text
//!   (`issue_filters.py:55-81`).
//! * `intake_status` on POST reads `inbox_status` — a copy-paste bug that makes
//!   the POST form of the filter crash (`issue_filters.py:361`).
//! * `sub_issue` compares against the *string* `"false"` in both branches, so a
//!   JSON `false` does not filter (`issue_filters.py:380-389`).
//! * `logged_by` adds `logged_by__in`, but `Issue` has no such column, so the
//!   request is a 500 (`issue_filters.py:414-425`).

use chrono::{Datelike, NaiveDate};
use chrono_tz::Tz;
use uuid::Uuid;

use plane_drf::QueryDict;

use crate::error::QueryError;
use crate::issue::model::JoinSet;
use crate::issue::model::Relation;
use crate::sql::{Frag, JoinKind, Value};

/// Which HTTP method's branch to run. The Python passes the literal string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMethod {
    Get,
    Post,
}

/// A parameter value as `request.data`/`request.query_params` expose it.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Missing,
    Null,
    Text(String),
    List(Vec<ParamValue>),
    Bool(bool),
    Int(i64),
    Float(f64),
}

impl ParamValue {
    /// Python truthiness (empty list/string, `0`, `False` and `None` are false).
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Missing | Self::Null => false,
            Self::Text(value) => !value.is_empty(),
            Self::List(values) => !values.is_empty(),
            Self::Bool(value) => *value,
            Self::Int(value) => *value != 0,
            Self::Float(value) => *value != 0.0,
        }
    }

    /// Python's `len(value)`; raises for the types that have no length.
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Missing | Self::Null)
            || matches!(self, Self::Text(value) if value.is_empty())
            || matches!(self, Self::List(values) if values.is_empty())
    }

    pub fn len(&self) -> Result<usize, QueryError> {
        match self {
            Self::Text(value) => Ok(value.chars().count()),
            Self::List(values) => Ok(values.len()),
            _ => Err(QueryError::unsupported("object of this type has no len()")),
        }
    }

    /// `value == "null"` (Python compares against the string).
    pub fn is_null_word(&self) -> bool {
        matches!(self, Self::Text(value) if value == "null")
    }

    /// Iterate the value the way Django's `__in` lookup does: a list yields its
    /// items, a string yields its characters, anything else raises.
    pub fn iter_in(&self) -> Result<Vec<ParamValue>, QueryError> {
        match self {
            Self::List(values) => Ok(values.clone()),
            Self::Text(value) => Ok(value.chars().map(|ch| Self::Text(ch.to_string())).collect()),
            _ => Err(QueryError::unsupported("argument of type is not iterable")),
        }
    }
}

/// The parameters a filter reads, for either method.
#[derive(Debug, Clone)]
pub enum FilterParams<'a> {
    /// `request.query_params` (a `QueryDict`: last value wins per key).
    Get(&'a QueryDict),
    /// `request.data` reduced to the keys the filters read.
    Post(Vec<(&'a str, ParamValue)>),
}

impl FilterParams<'_> {
    pub fn method(&self) -> FilterMethod {
        match self {
            Self::Get(_) => FilterMethod::Get,
            Self::Post(_) => FilterMethod::Post,
        }
    }

    pub fn get(&self, key: &str) -> ParamValue {
        match self {
            Self::Get(query) => query.get(key).map_or(ParamValue::Missing, |value| {
                ParamValue::Text(value.to_owned())
            }),
            Self::Post(entries) => entries
                .iter()
                .rev()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.clone())
                .unwrap_or(ParamValue::Missing),
        }
    }

    /// `key in query_params`.
    pub fn contains(&self, key: &str) -> bool {
        match self {
            Self::Get(query) => query.contains_key(key),
            Self::Post(entries) => entries.iter().any(|(name, _)| *name == key),
        }
    }
}

/// The text of a `Text` value, for the common GET path.
fn text(value: &ParamValue) -> Result<&str, QueryError> {
    match value {
        ParamValue::Text(value) => Ok(value),
        _ => Err(QueryError::unsupported("expected a string parameter")),
    }
}

/// `filter_valid_uuids`: silently drop the values `uuid.UUID` rejects.
fn valid_uuids(values: &[String]) -> Vec<Uuid> {
    values
        .iter()
        .filter_map(|value| parse_uuid(value))
        .collect()
}

/// Python's `uuid.UUID(value)`: hyphens optional, braces and a `urn:uuid:`
/// prefix accepted, case-insensitive.
pub fn parse_uuid(value: &str) -> Option<Uuid> {
    let trimmed = value.trim();
    let stripped = trimmed
        .strip_prefix("urn:uuid:")
        .or_else(|| trimmed.strip_prefix("URN:UUID:"))
        .unwrap_or(trimmed);
    let stripped = stripped
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or(stripped);
    Uuid::parse_str(stripped).ok()
}

/// Which date columns a `date_filter` call applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateTerm {
    CreatedAtDate,
    UpdatedAtDate,
    CompletedAtDate,
    StartDate,
    TargetDate,
}

impl DateTerm {
    fn column(self) -> &'static str {
        match self {
            Self::CreatedAtDate => "issues.created_at",
            Self::UpdatedAtDate => "issues.updated_at",
            Self::CompletedAtDate => "issues.completed_at",
            Self::StartDate => "issues.start_date",
            Self::TargetDate => "issues.target_date",
        }
    }

    /// The `__date` transform is only used by the datetime columns.
    fn has_date_transform(self) -> bool {
        matches!(
            self,
            Self::CreatedAtDate | Self::UpdatedAtDate | Self::CompletedAtDate
        )
    }
}

/// One entry of the lookup dict a `date_filter` produces.
#[derive(Debug, Clone, PartialEq)]
pub enum DateOperand {
    /// `__gte` / `__lte` with a parsed date.
    Date(NaiveDate),
    /// `__contains` with the raw text (Django's `contains` on a date column
    /// casts to text, so no date parsing happens).
    Contains(String),
}

/// The `{term}__gte`, `{term}__lte` and `{term}__contains` entries for one
/// column, in the order they were first written (a later write replaces the
/// value but keeps the position, like a Python dict).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DateFilters {
    pub entries: Vec<(DateSuffix, DateOperand)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSuffix {
    Gte,
    Lte,
    Contains,
}

impl DateFilters {
    fn insert(&mut self, suffix: DateSuffix, operand: DateOperand) {
        if let Some(slot) = self.entries.iter_mut().find(|(key, _)| *key == suffix) {
            slot.1 = operand;
        } else {
            self.entries.push((suffix, operand));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Every lookup `issue_filters` can produce.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LegacyFilters {
    pub state: Option<Vec<Uuid>>,
    pub state_group: Option<Vec<String>>,
    pub estimate_point: Option<Vec<Uuid>>,
    pub priority: Option<Vec<String>>,
    pub parent_is_null: bool,
    pub parent: Option<Vec<Uuid>>,
    pub labels_is_null: bool,
    pub labels: Option<Vec<Uuid>>,
    /// `label_issue__deleted_at__isnull=True` was requested.
    pub labels_join: bool,
    pub assignees_is_null: bool,
    pub assignees: Option<Vec<Uuid>>,
    pub assignees_join: bool,
    pub mentions: Option<Vec<Uuid>>,
    pub created_by_is_null: bool,
    pub created_by: Option<Vec<Uuid>>,
    pub name_icontains: Option<String>,
    pub created_at: DateFilters,
    pub updated_at: DateFilters,
    pub completed_at: DateFilters,
    pub start_date: DateFilters,
    pub target_date: DateFilters,
    /// POST `start_date`: an exact date, not a range (`issue_filters.py:257`).
    pub start_date_exact: Option<NaiveDate>,
    /// POST `target_date`, likewise.
    pub target_date_exact: Option<NaiveDate>,
    /// `type=backlog|active|all`: always five groups unless narrowed.
    pub state_group_type: Option<Vec<String>>,
    pub project: Option<Vec<Uuid>>,
    pub cycle_is_null: bool,
    pub cycle: Option<Vec<Uuid>>,
    pub cycle_join: bool,
    pub module_is_null: bool,
    pub module: Option<Vec<Uuid>>,
    pub module_join: bool,
    pub intake_status: Option<Vec<i32>>,
    pub parent_is_null_sub_issue: bool,
    pub subscriber: Option<Vec<Uuid>>,
    pub subscriber_join: bool,
    pub start_target_date: bool,
}

const ALL_STATE_GROUPS: [&str; 5] = ["backlog", "unstarted", "started", "completed", "cancelled"];

impl LegacyFilters {
    /// `issue_filters(query_params, method)`.
    ///
    /// `now` is `timezone.now().date()` for the relative ranges; the callers
    /// pass the wall clock, the oracle passes the frozen instant it recorded.
    pub fn parse(params: &FilterParams<'_>, now: NaiveDate) -> Result<Self, QueryError> {
        let method = params.method();
        let mut filters = Self::default();

        // The ISSUE_FILTER dict is iterated in Python dict order, and only the
        // keys present in the parameters run. Order matters for the date
        // filters' dict-write behaviour and for error precedence, so it is
        // reproduced here.
        if params.contains("state") {
            filters.state = filter_state(params, method)?;
        }
        if params.contains("state_group") {
            filters.state_group = filter_string_list(params, method, "state_group", false)?;
        }
        if params.contains("estimate_point") {
            filters.estimate_point = filter_estimate_point(params, method)?;
        }
        if params.contains("priority") {
            filters.priority = filter_string_list(params, method, "priority", false)?;
        }
        if params.contains("parent") {
            let (is_null, ids) = filter_relation(params, method, "parent", true)?;
            filters.parent_is_null = is_null;
            filters.parent = ids;
        }
        if params.contains("labels") {
            let (is_null, ids) = filter_relation(params, method, "labels", true)?;
            filters.labels_is_null = is_null;
            filters.labels = ids;
            filters.labels_join = true;
        }
        if params.contains("assignees") {
            let (is_null, ids) = filter_relation(params, method, "assignees", true)?;
            filters.assignees_is_null = is_null;
            filters.assignees = ids;
            filters.assignees_join = true;
        }
        if params.contains("mentions") {
            filters.mentions = filter_relation(params, method, "mentions", false)?.1;
        }
        if params.contains("created_by") {
            let (is_null, ids) = filter_relation(params, method, "created_by", true)?;
            filters.created_by_is_null = is_null;
            filters.created_by = ids;
        }
        if params.contains("logged_by") {
            // `logged_by__in` on Issue is a FieldError: the column does not
            // exist (`issue_filters.py:414`). Django answers 500.
            return Err(QueryError::unsupported(
                "Cannot resolve keyword 'logged_by' into field",
            ));
        }
        if let Some(value) = params.get("name").into_text()
            && !value.is_empty()
        {
            filters.name_icontains = Some(value);
        }
        if params.contains("created_at") {
            filters.created_at = date_filter(params, method, DateTerm::CreatedAtDate, now)?;
        }
        if params.contains("updated_at") {
            filters.updated_at = date_filter(params, method, DateTerm::UpdatedAtDate, now)?;
        }
        if params.contains("start_date") {
            match method {
                FilterMethod::Get => {
                    filters.start_date = date_filter(params, method, DateTerm::StartDate, now)?;
                }
                FilterMethod::Post => {
                    filters.start_date_exact = post_exact_date(params, "start_date")?;
                }
            }
        }
        if params.contains("target_date") {
            match method {
                FilterMethod::Get => {
                    filters.target_date = date_filter(params, method, DateTerm::TargetDate, now)?;
                }
                FilterMethod::Post => {
                    filters.target_date_exact = post_exact_date(params, "target_date")?;
                }
            }
        }
        if params.contains("completed_at") {
            filters.completed_at = date_filter(params, method, DateTerm::CompletedAtDate, now)?;
        }
        if params.contains("type") {
            let kind = params
                .get("type")
                .into_text()
                .unwrap_or_else(|| "all".to_owned());
            let groups: Vec<String> = match kind.as_str() {
                "backlog" => vec!["backlog".to_owned()],
                "active" => vec!["unstarted".to_owned(), "started".to_owned()],
                _ => ALL_STATE_GROUPS.iter().map(|s| (*s).to_owned()).collect(),
            };
            filters.state_group_type = Some(groups);
        }
        if params.contains("project") {
            filters.project = filter_relation(params, method, "project", false)?.1;
        }
        if params.contains("cycle") {
            let (is_null, ids) = filter_relation(params, method, "cycle", true)?;
            filters.cycle_is_null = is_null;
            filters.cycle = ids;
            filters.cycle_join = true;
        }
        if params.contains("module") {
            let (is_null, ids) = filter_relation(params, method, "module", true)?;
            filters.module_is_null = is_null;
            filters.module = ids;
            filters.module_join = true;
        }
        if params.contains("intake_status") {
            match method {
                FilterMethod::Get => {
                    filters.intake_status =
                        filter_int_list(&params.get("intake_status"), method, "intake_status")?;
                }
                FilterMethod::Post => {
                    let guard = params.get("intake_status");
                    if guard.is_truthy() && guard.len()? > 0 && !guard.is_null_word() {
                        // The POST branch reads `inbox_status`: a copy-paste
                        // bug (`issue_filters.py:361`). When that key is
                        // absent the dict value is `None` and Django's `__in`
                        // raises.
                        filters.intake_status =
                            filter_int_list(&params.get("inbox_status"), method, "inbox_status")?;
                    }
                }
            }
        }
        if params.contains("inbox_status") {
            filters.intake_status =
                filter_int_list(&params.get("inbox_status"), method, "inbox_status")?;
        }
        if params.contains("sub_issue") {
            let value = params.get("sub_issue");
            let text = value.into_text().unwrap_or_default();
            filters.parent_is_null_sub_issue = text == "false";
        }
        if params.contains("subscriber") {
            filters.subscriber = filter_relation(params, method, "subscriber", false)?.1;
            filters.subscriber_join = true;
        }
        if params.contains("start_target_date") {
            let value = params
                .get("start_target_date")
                .into_text()
                .unwrap_or_else(|| "false".to_owned());
            filters.start_target_date = value == "true";
        }
        Ok(filters)
    }

    /// Add every predicate and join this filter set requires.
    pub fn apply(&self, builder: &mut FilterBuilder) {
        if let Some(states) = &self.state {
            builder.require_inner(Relation::States);
            builder.and(Frag::in_uuids("issues.state_id", states));
        }
        if let Some(groups) = &self.state_group {
            // `filter_issue_state_type` writes the same dict key
            // (`state__group__in`) and runs later, so `type` wins whenever it
            // is present (`issue_filters.py:296-305,431-457`).
            if self.state_group_type.is_none() {
                builder.require_inner(Relation::States);
                builder.and(Frag::in_texts("states.group", groups));
            }
        }
        if let Some(points) = &self.estimate_point {
            builder.and(Frag::in_uuids("issues.estimate_point_id", points));
        }
        if let Some(priorities) = &self.priority {
            builder.and(Frag::in_texts("issues.priority", priorities));
        }
        if self.parent_is_null {
            builder.and(Frag::is_null("issues.parent_id"));
        }
        if let Some(parents) = &self.parent {
            builder.and(Frag::in_uuids("issues.parent_id", parents));
        }
        if self.labels_join {
            // The `deleted_at IS NULL` condition alone still joins the table
            // (Django uses LEFT OUTER JOIN for it); an id list promotes the
            // join to INNER because it needs a match.
            builder.require(
                Relation::IssueLabels,
                if self.labels.is_some() {
                    JoinKind::Inner
                } else {
                    JoinKind::Left
                },
            );
            if self.labels_is_null {
                builder.and(Frag::is_null("issue_labels.label_id"));
            }
            if let Some(labels) = &self.labels {
                builder.and(Frag::in_uuids("issue_labels.label_id", labels));
            }
            builder.and(Frag::is_null("issue_labels.deleted_at"));
        }
        if self.assignees_join {
            // The `deleted_at IS NULL` condition alone still joins the table
            // (Django uses LEFT OUTER JOIN for it); an id list promotes the
            // join to INNER because it needs a match.
            builder.require(
                Relation::IssueAssignees,
                if self.assignees.is_some() {
                    JoinKind::Inner
                } else {
                    JoinKind::Left
                },
            );
            if self.assignees_is_null {
                builder.and(Frag::is_null("issue_assignees.assignee_id"));
            }
            if let Some(assignees) = &self.assignees {
                builder.and(Frag::in_uuids("issue_assignees.assignee_id", assignees));
            }
            builder.and(Frag::is_null("issue_assignees.deleted_at"));
        }
        if let Some(mentions) = &self.mentions {
            builder.require_inner(Relation::IssueMentions);
            builder.and(Frag::in_uuids("issue_mentions.mention_id", mentions));
        }
        if self.created_by_is_null {
            builder.and(Frag::is_null("issues.created_by_id"));
        }
        if let Some(created_by) = &self.created_by {
            builder.and(Frag::in_uuids("issues.created_by_id", created_by));
        }
        if let Some(name) = &self.name_icontains {
            builder.and(Frag::icontains("issues.name", name));
        }
        if let Some(groups) = &self.state_group_type {
            builder.require_inner(Relation::States);
            builder.and(Frag::in_texts("states.group", groups));
        }
        if let Some(projects) = &self.project {
            builder.and(Frag::in_uuids("issues.project_id", projects));
        }
        if self.cycle_join {
            // The `deleted_at IS NULL` condition alone still joins the table
            // (Django uses LEFT OUTER JOIN for it); an id list promotes the
            // join to INNER because it needs a match.
            builder.require(
                Relation::CycleIssues,
                if self.cycle.is_some() {
                    JoinKind::Inner
                } else {
                    JoinKind::Left
                },
            );
            if self.cycle_is_null {
                builder.and(Frag::is_null("cycle_issues.cycle_id"));
            }
            if let Some(cycles) = &self.cycle {
                builder.and(Frag::in_uuids("cycle_issues.cycle_id", cycles));
            }
            builder.and(Frag::is_null("cycle_issues.deleted_at"));
        }
        if self.module_join {
            // The `deleted_at IS NULL` condition alone still joins the table
            // (Django uses LEFT OUTER JOIN for it); an id list promotes the
            // join to INNER because it needs a match.
            builder.require(
                Relation::ModuleIssues,
                if self.module.is_some() {
                    JoinKind::Inner
                } else {
                    JoinKind::Left
                },
            );
            if self.module_is_null {
                builder.and(Frag::is_null("module_issues.module_id"));
            }
            if let Some(modules) = &self.module {
                builder.and(Frag::in_uuids("module_issues.module_id", modules));
            }
            builder.and(Frag::is_null("module_issues.deleted_at"));
        }
        if let Some(statuses) = &self.intake_status {
            builder.require_inner(Relation::IntakeIssues);
            builder.and(Frag::in_list(
                "intake_issues.status",
                statuses.iter().copied().map(Value::Int).collect(),
            ));
        }
        if self.parent_is_null_sub_issue {
            builder.and(Frag::is_null("issues.parent_id"));
        }
        if self.subscriber_join {
            if self.subscriber.is_some() {
                builder.require_inner(Relation::IssueSubscribers);
            } else {
                builder.require_left(Relation::IssueSubscribers);
            }
            if let Some(subscribers) = &self.subscriber {
                builder.and(Frag::in_uuids(
                    "issue_subscribers.subscriber_id",
                    subscribers,
                ));
            }
            builder.and(Frag::is_null("issue_subscribers.deleted_at"));
        }
        if let Some(date) = self.start_date_exact {
            builder.and(Frag::compare("issues.start_date", "=", Value::Date(date)));
        }
        if let Some(date) = self.target_date_exact {
            builder.and(Frag::compare("issues.target_date", "=", Value::Date(date)));
        }
        if self.start_target_date {
            builder.and(Frag::is_not_null("issues.target_date"));
            builder.and(Frag::is_not_null("issues.start_date"));
        }
    }

    /// Apply the four date-lookup groups.
    pub fn apply_dates(&self, builder: &mut FilterBuilder, timezone: Tz) {
        for (term, filters) in [
            (DateTerm::CreatedAtDate, &self.created_at),
            (DateTerm::UpdatedAtDate, &self.updated_at),
            (DateTerm::CompletedAtDate, &self.completed_at),
            (DateTerm::StartDate, &self.start_date),
            (DateTerm::TargetDate, &self.target_date),
        ] {
            if filters.is_empty() {
                continue;
            }
            let column = if term.has_date_transform() {
                let mut frag = Frag::raw("(");
                frag.push_raw(term.column());
                frag.push_raw(" AT TIME ZONE ");
                frag.push(Frag::bind(Value::Text(timezone.name().to_owned())));
                frag.push_raw(")::date");
                frag
            } else {
                Frag::raw(term.column())
            };
            for (suffix, operand) in &filters.entries {
                match operand {
                    DateOperand::Date(date) => {
                        let mut frag = column.clone();
                        frag.push_raw(match suffix {
                            DateSuffix::Gte => " >= ",
                            DateSuffix::Lte => " <= ",
                            DateSuffix::Contains => unreachable!(),
                        });
                        frag.push(Frag::bind(Value::Date(*date)));
                        builder.and(frag);
                    }
                    DateOperand::Contains(text) => {
                        let mut frag = column.clone();
                        frag.push_raw("::text LIKE ");
                        frag.push(Frag::bind(Value::Text(format!("%{text}%"))));
                        builder.and(frag);
                    }
                }
            }
        }
    }
}

impl ParamValue {
    fn into_text(self) -> Option<String> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }
}

/// The accumulator a filter set writes into.
#[derive(Debug, Clone, Default)]
pub struct FilterBuilder {
    pub joins: JoinSet,
    pub predicate: Frag,
}

impl FilterBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn and(&mut self, predicate: Frag) {
        if predicate.is_empty() {
            return;
        }
        self.predicate = std::mem::take(&mut self.predicate).and(predicate);
    }

    pub fn require(&mut self, relation: Relation, kind: JoinKind) {
        self.joins.require(relation, kind);
    }

    pub fn require_inner(&mut self, relation: Relation) {
        self.joins.require_inner(relation);
    }

    pub fn require_left(&mut self, relation: Relation) {
        self.joins.require_left(relation);
    }
}

// ---------------------------------------------------------------------------
// The individual `filter_*` functions
// ---------------------------------------------------------------------------

/// `filter_state`.
fn filter_state(
    params: &FilterParams<'_>,
    method: FilterMethod,
) -> Result<Option<Vec<Uuid>>, QueryError> {
    match method {
        FilterMethod::Get => {
            let raw = text(&params.get("state"))?.to_owned();
            let items: Vec<String> = raw
                .split(',')
                .filter(|item| *item != "null")
                .map(str::to_owned)
                .collect();
            let ids = valid_uuids(&items);
            Ok(if ids.is_empty() { None } else { Some(ids) })
        }
        FilterMethod::Post => {
            let value = params.get("state");
            if value.is_truthy() && value.len()? > 0 && !value.is_null_word() {
                post_uuid_list(&value, "state")
            } else {
                Ok(None)
            }
        }
    }
}

/// `filter_state_group`, `filter_priority` and the other plain string lists.
fn filter_string_list(
    params: &FilterParams<'_>,
    method: FilterMethod,
    key: &str,
    allow_empty: bool,
) -> Result<Option<Vec<String>>, QueryError> {
    match method {
        FilterMethod::Get => {
            let raw = text(&params.get(key))?.to_owned();
            let items: Vec<String> = raw
                .split(',')
                .filter(|item| *item != "null")
                .map(str::to_owned)
                .collect();
            if items.is_empty() || (!allow_empty && items.iter().any(String::is_empty)) {
                Ok(None)
            } else {
                Ok(Some(items))
            }
        }
        FilterMethod::Post => {
            let value = params.get(key);
            if value.is_truthy() && value.len()? > 0 && !value.is_null_word() {
                Ok(Some(post_string_list(&value)?))
            } else {
                Ok(None)
            }
        }
    }
}

/// `filter_estimate_point`: the only GET filter that does not pre-validate its
/// UUIDs, so an invalid value reaches Django's `UUIDField` and is a 500.
fn filter_estimate_point(
    params: &FilterParams<'_>,
    method: FilterMethod,
) -> Result<Option<Vec<Uuid>>, QueryError> {
    match method {
        FilterMethod::Get => {
            let raw = text(&params.get("estimate_point"))?.to_owned();
            let items: Vec<String> = raw
                .split(',')
                .filter(|item| *item != "null")
                .map(str::to_owned)
                .collect();
            if items.is_empty() || items.iter().any(String::is_empty) {
                return Ok(None);
            }
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                ids.push(parse_uuid(&item).ok_or_else(|| {
                    QueryError::unsupported(format!("“{item}” is not a valid UUID."))
                })?);
            }
            Ok(Some(ids))
        }
        FilterMethod::Post => post_uuid_list(&params.get("estimate_point"), "estimate_point"),
    }
}

/// `filter_parent`, `filter_labels`, `filter_assignees`, `filter_mentions`,
/// `filter_created_by`, `filter_project`, `filter_cycle`, `filter_module` and
/// `filter_subscriber`: a comma list with optional `None` meaning "is null".
fn filter_relation(
    params: &FilterParams<'_>,
    method: FilterMethod,
    key: &str,
    none_means_isnull: bool,
) -> Result<(bool, Option<Vec<Uuid>>), QueryError> {
    match method {
        FilterMethod::Get => {
            let raw = text(&params.get(key))?.to_owned();
            let mut items: Vec<String> = raw
                .split(',')
                .filter(|item| *item != "null")
                .map(str::to_owned)
                .collect();
            let mut is_null = false;
            if none_means_isnull && items.iter().any(|item| item == "None") {
                is_null = true;
                items.retain(|item| item != "None");
            }
            let ids = valid_uuids(&items);
            let ids = if ids.is_empty() { None } else { Some(ids) };
            Ok((is_null, ids))
        }
        FilterMethod::Post => {
            let value = params.get(key);
            if value.is_truthy() && value.len()? > 0 && !value.is_null_word() {
                Ok((false, post_uuid_list(&value, key)?))
            } else {
                Ok((false, None))
            }
        }
    }
}

/// `filter_intake_status` / `filter_inbox_status` (both write the same lookup).
fn filter_int_list(
    value: &ParamValue,
    method: FilterMethod,
    _key: &str,
) -> Result<Option<Vec<i32>>, QueryError> {
    match method {
        FilterMethod::Get => {
            let raw = match value {
                ParamValue::Text(value) => value.clone(),
                _ => return Ok(None),
            };
            let items: Vec<String> = raw
                .split(',')
                .filter(|item| *item != "null")
                .map(str::to_owned)
                .collect();
            if items.is_empty() || items.iter().any(String::is_empty) {
                return Ok(None);
            }
            let mut statuses = Vec::with_capacity(items.len());
            for item in items {
                statuses.push(item.parse::<i32>().map_err(|error| {
                    QueryError::unsupported(format!("invalid literal for int(): {error}"))
                })?);
            }
            Ok(Some(statuses))
        }
        FilterMethod::Post => {
            if !value.is_truthy() || matches!(value, ParamValue::Missing | ParamValue::Null) {
                // `status__in = None`: Django's `In` lookup raises because the
                // value is not iterable.
                return Err(QueryError::unsupported(
                    "Field 'status' expected a number but got None",
                ));
            }
            if value.len()? > 0 && !value.is_null_word() {
                let mut statuses = Vec::new();
                for item in value.iter_in()? {
                    let text = item.into_text().ok_or_else(|| {
                        QueryError::unsupported("Field 'status' expected a number")
                    })?;
                    statuses.push(text.parse::<i32>().map_err(|error| {
                        QueryError::unsupported(format!("invalid literal for int(): {error}"))
                    })?);
                }
                Ok(Some(statuses))
            } else {
                Ok(None)
            }
        }
    }
}

/// A POST `__in` value: a list of strings, or a string that Django iterates
/// character by character (which is almost always a mistake and is preserved).
fn post_uuid_list(value: &ParamValue, key: &str) -> Result<Option<Vec<Uuid>>, QueryError> {
    let items = value.iter_in()?;
    if items.is_empty() {
        return Ok(None);
    }
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        let text = item
            .into_text()
            .ok_or_else(|| QueryError::unsupported(format!("{key} expects strings")))?;
        ids.push(
            parse_uuid(&text)
                .ok_or_else(|| QueryError::unsupported(format!("“{text}” is not a valid UUID.")))?,
        );
    }
    Ok(Some(ids))
}

/// POST `start_date`/`target_date`: assigned to the lookup as-is, so only a
/// string parses; a list is a `TypeError` in `DateField.to_python`
/// (`issue_filters.py:257,272`).
fn post_exact_date(params: &FilterParams<'_>, key: &str) -> Result<Option<NaiveDate>, QueryError> {
    let value = params.get(key);
    if !value.is_truthy() || value.len()? == 0 {
        return Ok(None);
    }
    match value {
        ParamValue::Text(text) => Ok(Some(parse_date(&text)?)),
        other => Err(QueryError::unsupported(format!(
            "fromisoformat: argument must be str (got {other:?})"
        ))),
    }
}

fn post_string_list(value: &ParamValue) -> Result<Vec<String>, QueryError> {
    let mut out = Vec::new();
    for item in value.iter_in()? {
        out.push(
            item.into_text()
                .ok_or_else(|| QueryError::unsupported("expected strings"))?,
        );
    }
    Ok(out)
}

/// `date_filter` (`issue_filters.py:55`).
fn date_filter(
    params: &FilterParams<'_>,
    method: FilterMethod,
    term: DateTerm,
    now: NaiveDate,
) -> Result<DateFilters, QueryError> {
    let queries: Vec<String> = match method {
        FilterMethod::Get => {
            let key = match term {
                DateTerm::CreatedAtDate => "created_at",
                DateTerm::UpdatedAtDate => "updated_at",
                DateTerm::CompletedAtDate => "completed_at",
                DateTerm::StartDate => "start_date",
                DateTerm::TargetDate => "target_date",
            };
            let value = params.get(key);
            let raw = match &value {
                ParamValue::Text(text) => text.clone(),
                _ => return Err(QueryError::unsupported("expected a string parameter")),
            };
            let items: Vec<String> = raw.split(',').map(str::to_owned).collect();
            if items.is_empty() || items.iter().any(String::is_empty) {
                return Ok(DateFilters::default());
            }
            items
        }
        FilterMethod::Post => {
            let key = match term {
                DateTerm::CreatedAtDate => "created_at",
                DateTerm::UpdatedAtDate => "updated_at",
                DateTerm::CompletedAtDate => "completed_at",
                DateTerm::StartDate => "start_date",
                DateTerm::TargetDate => "target_date",
            };
            let value = params.get(key);
            if !value.is_truthy() {
                return Ok(DateFilters::default());
            }
            let _ = value.len()?;
            value
                .iter_in()?
                .iter()
                .map(|item| {
                    item.clone()
                        .into_text()
                        .ok_or_else(|| QueryError::unsupported("expected a date string"))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };

    let mut out = DateFilters::default();
    for query in queries {
        let parts: Vec<&str> = query.split(';').collect();
        if parts.len() >= 2 {
            if let Some((duration, unit)) = relative_pattern(parts[0]) {
                if parts.len() == 3 {
                    let subsequent = parts[1];
                    let offset = parts[2];
                    let date = string_date_filter(duration, unit, subsequent, offset, now);
                    out.insert(
                        if subsequent == "after" {
                            DateSuffix::Gte
                        } else {
                            DateSuffix::Lte
                        },
                        DateOperand::Date(date),
                    );
                }
                // `N_weeks;after` (two parts) adds nothing: the Python only
                // applies a relative filter when the offset term is present.
            } else if parts.contains(&"after") {
                out.insert(DateSuffix::Gte, DateOperand::Date(parse_date(parts[0])?));
            } else {
                out.insert(DateSuffix::Lte, DateOperand::Date(parse_date(parts[0])?));
            }
        } else {
            out.insert(
                DateSuffix::Contains,
                DateOperand::Contains(parts[0].to_owned()),
            );
        }
    }
    Ok(out)
}

/// `\d+_(weeks|months)$`.
fn relative_pattern(value: &str) -> Option<(i64, &'static str)> {
    let (digits, unit) = value.rsplit_once('_')?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let unit = match unit {
        "weeks" => "weeks",
        "months" => "months",
        _ => return None,
    };
    Some((digits.parse().ok()?, unit))
}

/// `string_date_filter` (`issue_filters.py:29`): `now ± N days`, with months
/// counted as 30 days.
fn string_date_filter(
    duration: i64,
    unit: &str,
    subsequent: &str,
    offset: &str,
    now: NaiveDate,
) -> NaiveDate {
    let days = if unit == "months" {
        duration * 30
    } else {
        duration * 7
    };
    let from_now = offset == "fromnow";
    let delta = match (subsequent, from_now) {
        ("after", true) => days,
        ("after", false) => -days,
        (_, true) => days,
        (_, false) => -days,
    };
    now + chrono::Duration::days(delta)
}

/// `django.utils.dateparse.parse_date`, which accepts `YYYY-MM-DD` only.
fn parse_date(value: &str) -> Result<NaiveDate, QueryError> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
    if !valid {
        return Err(QueryError::unsupported(format!(
            "“{value}” value has an invalid date format. It must be in YYYY-MM-DD format."
        )));
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| QueryError::unsupported(format!("“{value}” is not a valid date.")))
}

/// `timezone.now().date()` in the frozen/real clock.
pub fn today(now: chrono::DateTime<chrono::Utc>) -> NaiveDate {
    now.date_naive()
}

/// A year's worth of dates for the relative filters; kept for callers that need
/// the same `now` the parse used.
pub fn current_year(now: NaiveDate) -> i32 {
    now.year()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(query: &str) -> QueryDict {
        QueryDict::parse(query, None).expect("query")
    }

    fn legacy(query: &str) -> LegacyFilters {
        let dict = get(query);
        let params = FilterParams::Get(&dict);
        LegacyFilters::parse(&params, NaiveDate::from_ymd_opt(2026, 6, 15).expect("now"))
            .expect("parse")
    }

    fn predicate(query: &str) -> (Vec<String>, String, Vec<Value>) {
        let filters = legacy(query);
        let mut builder = FilterBuilder::new();
        let high = Tz::UTC;
        filters.apply(&mut builder);
        filters.apply_dates(&mut builder, high);
        let joins = builder
            .joins
            .to_joins()
            .iter()
            .map(|join| format!("{:?} {}", join.kind, join.alias))
            .collect();
        let sql = builder.predicate.into_sql();
        (joins, sql.text, sql.params)
    }

    #[test]
    fn state_drops_null_and_invalid_values() {
        assert_eq!(predicate("state=null"), (vec![], String::new(), vec![]));
        assert_eq!(predicate("state=abc"), (vec![], String::new(), vec![]));
        assert_eq!(
            predicate("state=00000000-0000-0000-0000-000000000001").0,
            vec!["Inner states".to_owned()]
        );
    }

    #[test]
    fn labels_add_their_join_even_without_ids() {
        let (joins, sql, _) = predicate("labels=None");
        assert_eq!(joins, vec!["Left issue_labels".to_owned()]);
        assert_eq!(
            sql,
            "(issue_labels.label_id IS NULL) AND (issue_labels.deleted_at IS NULL)"
        );
    }

    #[test]
    fn relative_dates_need_the_third_part() {
        // `2_weeks;after` matches the pattern but has no third part, so the
        // Python adds no lookup at all (`issue_filters.py:65-74`).
        assert_eq!(predicate("created_at=2_weeks;after").1, "");
    }

    #[test]
    fn relative_dates_follow_the_python_arithmetic() {
        let (_, sql, params) = predicate("created_at=2_weeks;after;fromnow");
        assert!(sql.contains("AT TIME ZONE"));
        assert!(sql.contains(">= "));
        assert_eq!(params[0], Value::Text("UTC".to_owned()));
        assert_eq!(
            params[1],
            Value::Date(NaiveDate::from_ymd_opt(2026, 6, 29).expect("date"))
        );
    }

    #[test]
    fn date_contains_is_a_like_on_the_text_form() {
        let (_, sql, params) = predicate("created_at=2026-06");
        assert!(sql.contains("::date::text LIKE "));
        assert_eq!(params[1], Value::Text("%2026-06%".to_owned()));
    }

    #[test]
    fn sub_issue_compares_against_the_string_false() {
        assert_eq!(predicate("sub_issue=false").1, "issues.parent_id IS NULL");
        assert_eq!(predicate("sub_issue=true").1, "");
    }

    #[test]
    fn intake_status_post_reads_inbox_status() {
        let params = FilterParams::Post(vec![(
            "intake_status",
            ParamValue::List(vec![ParamValue::Text("1".into())]),
        )]);
        let error =
            LegacyFilters::parse(&params, NaiveDate::from_ymd_opt(2026, 6, 15).expect("now"));
        assert!(error.is_err());
    }
}
