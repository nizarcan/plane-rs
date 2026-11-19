// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/analytic_plot_export.py:350-448` — `analytic_export_task`.
//!
//! Two side effects: a CSV built from `build_graph_plot`
//! (`plane/utils/analytics_plot.py:71-147`) and one e-mail with that CSV
//! attached (`send_export_email`, `:45-88`). There is no database write.
//!
//! # Filters
//!
//! `issue_filters(data, "POST")` (`plane/utils/issue_filters.py:300-365`) maps
//! the payload's keys to ORM lookups in the dictionary's fixed order (not the
//! payload's), then the task adds the initiator's active project membership.
//! `Issue.issue_objects` excludes triage states, archived issues/projects and
//! drafts (`plane/db/models/issue.py:92-101`). The SQL below reproduces every
//! lookup the function can emit; a key it cannot express (`logged_by`, which
//! `Issue` does not have) fails the task exactly as Django's `FieldError` does.
//!
//! # Distribution
//!
//! The x-axis becomes a `dimension`: a date axis is `EXTRACT(year) || '-' ||
//! EXTRACT(month)` (unpadded: `2026-1`), everything else the column itself.
//! Rows are `COUNT(*)` or `SUM(CAST(estimate_point.value AS double precision))`
//! per dimension (and per segment), ordered by the dimension. Python then
//! groups by `str(dimension)` and applies `sort_data`
//! (`analytics_plot.py:56-68`): priority uses the fixed low/medium/high/
//! urgent/none order (dropping anything else), everything else sorts
//! lexicographically with `"none"` last.
//!
//! **Upstream bug kept:** the MODULE segment branch of
//! `generate_segmented_rows` looks names up in `label_details`
//! (`analytic_plot_export.py:278-282`), so a module segment keeps the raw
//! module id (and, with labels also requested, raises `KeyError` and the task
//! logs and returns). The port therefore never resolves a module segment name.
//!
//! **Upstream bug kept:** `generate_segmented_rows` builds its segment columns
//! from `list(set(...))` (`analytic_plot_export.py:200`), so the column order
//! depends on Python's per-process string hash seed. The port keeps first-seen
//! SQL order. A run with more than one segment value can therefore differ from
//! a particular Django process; `difftest task` uses one segment value.
//!
//! # Ownership
//!
//! The only producer is the analytics export POST in
//! `plane/app/views/analytics/base.py`, a Rust `/api/` route
//! (`ROUTES.md`), so the task moves with this change.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_mail::Message;
use serde_json::Value;
use uuid::Uuid;

use crate::{TaskContext, kwargs, string_value, swallow, uuid_value};

/// The Celery task name (`plane/bgtasks/analytic_plot_export.py:350`).
pub const TASK: &str = "plane.bgtasks.analytic_plot_export.analytic_export_task";

/// Register the handler.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    registry.register(TASK, move |job| {
        let context = Arc::clone(&context);
        Box::pin(async move { handle(&context, job).await })
    });
}

#[derive(Debug, Clone)]
struct Args {
    email: String,
    data: Value,
    slug: String,
    user_id: Uuid,
}

fn decode(payload: &Value) -> Option<Args> {
    let kw = kwargs(payload)?;
    Some(Args {
        email: string_value(kw.get("email"))?,
        data: kw.get("data")?.clone(),
        slug: string_value(kw.get("slug"))?,
        user_id: uuid_value(kw.get("user_id"))?,
    })
}

async fn handle(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    let Some(args) = decode(&job.payload) else {
        // A missing/invalid argument raises inside the task's try and is
        // logged; the task returns.
        swallow("invalid analytic_export_task payload", TASK);
        return Ok(());
    };
    if let Err(error) = run(context, &job.client, &args).await {
        // `analytic_plot_export.py:441-448`: log and return.
        swallow(error, TASK);
    }
    Ok(())
}

/// A task failure, rendered as the outer `except`'s log line.
type Failure = String;

async fn run(context: &TaskContext, client: &Client, args: &Args) -> Result<(), Failure> {
    let data = args
        .data
        .as_object()
        .ok_or_else(|| "analytic_export_task data is not a mapping".to_string())?;
    let mut filter = Filter::default();
    build_filters(&mut filter, data)?;

    // `filters.update(project__project_projectmember__member_id=...,
    // is_active=True, deleted_at__isnull=True)`.
    let member_index = filter.bind(args.user_id);
    filter.parts.push(format!(
        "EXISTS (SELECT 1 FROM project_members a_pm WHERE a_pm.project_id = i.project_id \
         AND a_pm.member_id = ${member_index} AND a_pm.is_active = TRUE \
         AND a_pm.deleted_at IS NULL)"
    ));

    let x_axis = data
        .get("x_axis")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let y_axis = data
        .get("y_axis")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let segment = data
        .get("segment")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if !VALID_ANALYTICS_FIELDS.contains(&x_axis.as_str()) {
        return Err(format!("Invalid x_axis value: {x_axis}"));
    }
    if !VALID_YAXIS.contains(&y_axis.as_str()) {
        return Err(format!("Invalid y_axis value: {y_axis}"));
    }
    if !segment.is_empty() && !VALID_ANALYTICS_FIELDS.contains(&segment.as_str()) {
        return Err(format!("Invalid segment value: {segment}"));
    }

    let distribution = build_graph_plot(
        client,
        &args.slug,
        &filter,
        &x_axis,
        &y_axis,
        if segment.is_empty() {
            None
        } else {
            Some(&segment)
        },
    )
    .await?;

    let key = if y_axis == "issue_count" {
        "count"
    } else {
        "estimate"
    };
    let assignee_details = if x_axis == ASSIGNEE_ID || segment == ASSIGNEE_ID {
        detail_rows(client, &args.slug, &filter, Detail::Assignees).await?
    } else {
        Vec::new()
    };
    let label_details = if x_axis == LABEL_ID || segment == LABEL_ID {
        detail_rows(client, &args.slug, &filter, Detail::Labels).await?
    } else {
        Vec::new()
    };
    let state_details = if x_axis == STATE_ID || segment == STATE_ID {
        detail_rows(client, &args.slug, &filter, Detail::States).await?
    } else {
        Vec::new()
    };
    let cycle_details = if x_axis == CYCLE_ID || segment == CYCLE_ID {
        detail_rows(client, &args.slug, &filter, Detail::Cycles).await?
    } else {
        Vec::new()
    };
    let module_details = if x_axis == MODULE_ID || segment == MODULE_ID {
        detail_rows(client, &args.slug, &filter, Detail::Modules).await?
    } else {
        Vec::new()
    };
    let details = Details {
        assignees: assignee_details,
        labels: label_details,
        states: state_details,
        cycles: cycle_details,
        modules: module_details,
    };

    let rows = if segment.is_empty() {
        non_segmented_rows(&distribution, &x_axis, &y_axis, key, &details)
    } else {
        segmented_rows(&distribution, &x_axis, &y_axis, &segment, key, &details)
    };

    let csv = csv_from_rows(&rows);

    // `send_export_email(email, slug, csv_buffer, rows)`.
    let html = plane_mail::render("emails/exports/analytics.html", &serde_json::json!({}))
        .map_err(|error| error.to_string())?;
    let text = plane_mail::generate_plain_text_from_html(&html);
    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    let message = Message::with_text_attachment(
        "Your Export is ready",
        text,
        configuration.from.clone(),
        vec![args.email.clone()],
        format!("{}-analytics.csv", args.slug),
        csv,
    );
    plane_mail::send(&configuration, &message)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

const ASSIGNEE_ID: &str = "assignees__id";
const LABEL_ID: &str = "labels__id";
const STATE_ID: &str = "state_id";
const CYCLE_ID: &str = "issue_cycle__cycle_id";
const MODULE_ID: &str = "issue_module__module_id";

const VALID_ANALYTICS_FIELDS: [&str; 12] = [
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

const VALID_YAXIS: [&str; 2] = ["issue_count", "estimate"];

/// The WHERE clauses and their bound parameters.
#[derive(Default)]
struct Filter {
    parts: Vec<String>,
    params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>>,
}

impl Filter {
    fn bind<T>(&mut self, value: T) -> usize
    where
        T: tokio_postgres::types::ToSql + Sync + Send + 'static,
    {
        self.params.push(Box::new(value));
        self.params.len()
    }

    fn where_sql(&self) -> String {
        if self.parts.is_empty() {
            "TRUE".to_string()
        } else {
            self.parts.join(" AND ")
        }
    }

    fn params(&self) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
        self.params
            .iter()
            .map(|value| &**value as &(dyn tokio_postgres::types::ToSql + Sync))
            .collect()
    }
}

/// `issue_filters(data, "POST")`. The order is the `ISSUE_FILTER` dictionary's
/// order, which decides the parameter order.
fn build_filters(
    filter: &mut Filter,
    data: &serde_json::Map<String, Value>,
) -> Result<(), Failure> {
    let values = |key: &str| data.get(key);
    let present = |key: &str| data.contains_key(key);

    // In this fixed order: state, state_group, estimate_point, priority,
    // parent, labels, assignees, mentions, created_by, logged_by, name,
    // created_at, updated_at, start_date, target_date, completed_at, type,
    // project, cycle, module, intake_status, inbox_status, sub_issue,
    // subscriber, start_target_date.
    if present("state")
        && let Some(list) = uuid_list(values("state"), "state")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter
            .parts
            .push(format!("i.state_id = ANY(${index}::uuid[])"));
    }
    if present("state_group")
        && let Some(list) = text_list(values("state_group"), "state_group")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter
            .parts
            .push(format!("s.\"group\" = ANY(${index}::text[])"));
    }
    if present("estimate_point")
        && let Some(list) = uuid_list(values("estimate_point"), "estimate_point")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter
            .parts
            .push(format!("i.estimate_point_id = ANY(${index}::uuid[])"));
    }
    if present("priority")
        && let Some(list) = text_list(values("priority"), "priority")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter
            .parts
            .push(format!("i.priority = ANY(${index}::text[])"));
    }
    if present("parent")
        && let Some(list) = uuid_list(values("parent"), "parent")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter
            .parts
            .push(format!("i.parent_id = ANY(${index}::uuid[])"));
    }
    if present("labels")
        && let Some(list) = uuid_list(values("labels"), "labels")?
    {
        filter.parts.push(
            "EXISTS (SELECT 1 FROM issue_labels fl WHERE fl.issue_id = i.id \
                 AND fl.deleted_at IS NULL)"
                .to_string(),
        );
        if !list.is_empty() {
            let index = filter.bind(list);
            filter.parts.push(format!(
                "EXISTS (SELECT 1 FROM issue_labels fl2 WHERE fl2.issue_id = i.id \
                     AND fl2.deleted_at IS NULL AND fl2.label_id = ANY(${index}::uuid[]))"
            ));
        }
    }
    if present("assignees")
        && let Some(list) = uuid_list(values("assignees"), "assignees")?
    {
        filter.parts.push(
            "EXISTS (SELECT 1 FROM issue_assignees fa WHERE fa.issue_id = i.id \
                 AND fa.deleted_at IS NULL)"
                .to_string(),
        );
        if !list.is_empty() {
            let index = filter.bind(list);
            filter.parts.push(format!(
                "EXISTS (SELECT 1 FROM issue_assignees fa2 WHERE fa2.issue_id = i.id \
                     AND fa2.deleted_at IS NULL AND fa2.assignee_id = ANY(${index}::uuid[]))"
            ));
        }
    }
    if present("mentions")
        && let Some(list) = uuid_list(values("mentions"), "mentions")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter.parts.push(format!(
            "EXISTS (SELECT 1 FROM issue_mentions fm WHERE fm.issue_id = i.id \
                     AND fm.deleted_at IS NULL AND fm.mention_id = ANY(${index}::uuid[]))"
        ));
    }
    if present("created_by")
        && let Some(list) = uuid_list(values("created_by"), "created_by")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter
            .parts
            .push(format!("i.created_by_id = ANY(${index}::uuid[])"));
    }
    if present("logged_by") {
        // `filter_logged_by` emits `logged_by__in` and `Issue` has no such
        // field: Django raises FieldError and the task logs and returns.
        return Err("Cannot resolve keyword 'logged_by' into field.".to_string());
    }
    if present("name")
        && let Some(name) = string_value(values("name"))
        && !name.is_empty()
    {
        let index = filter.bind(format!("%{name}%"));
        filter.parts.push(format!("i.name ILIKE ${index}"));
    }
    if present("created_at") {
        date_filter(filter, values("created_at"), "created_at")?;
    }
    if present("updated_at") {
        date_filter(filter, values("updated_at"), "updated_at")?;
    }
    if present("start_date")
        && let Some(date) = string_value(values("start_date"))
        && !date.is_empty()
    {
        let index = filter.bind(date);
        filter.parts.push(format!("i.start_date = ${index}::date"));
    }
    if present("target_date")
        && let Some(date) = string_value(values("target_date"))
        && !date.is_empty()
    {
        let index = filter.bind(date);
        filter.parts.push(format!("i.target_date = ${index}::date"));
    }
    if present("completed_at") {
        date_filter(filter, values("completed_at"), "completed_at")?;
    }
    if present("type") {
        let kind = values("type")
            .and_then(Value::as_str)
            .unwrap_or("all")
            .to_string();
        let groups = match kind.as_str() {
            "backlog" => vec!["backlog".to_string()],
            "active" => vec!["unstarted".to_string(), "started".to_string()],
            _ => ["backlog", "unstarted", "started", "completed", "cancelled"]
                .iter()
                .map(|group| (*group).to_string())
                .collect(),
        };
        let index = filter.bind(groups);
        filter
            .parts
            .push(format!("s.\"group\" = ANY(${index}::text[])"));
    }
    if present("project")
        && let Some(list) = uuid_list(values("project"), "project")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter
            .parts
            .push(format!("i.project_id = ANY(${index}::uuid[])"));
    }
    if present("cycle")
        && let Some(list) = uuid_list(values("cycle"), "cycle")?
    {
        filter.parts.push(
            "EXISTS (SELECT 1 FROM cycle_issues fc WHERE fc.issue_id = i.id \
                 AND fc.deleted_at IS NULL)"
                .to_string(),
        );
        if !list.is_empty() {
            let index = filter.bind(list);
            filter.parts.push(format!(
                "EXISTS (SELECT 1 FROM cycle_issues fc2 WHERE fc2.issue_id = i.id \
                     AND fc2.deleted_at IS NULL AND fc2.cycle_id = ANY(${index}::uuid[]))"
            ));
        }
    }
    if present("module")
        && let Some(list) = uuid_list(values("module"), "module")?
    {
        filter.parts.push(
            "EXISTS (SELECT 1 FROM module_issues fm2 WHERE fm2.issue_id = i.id \
                 AND fm2.deleted_at IS NULL)"
                .to_string(),
        );
        if !list.is_empty() {
            let index = filter.bind(list);
            filter.parts.push(format!(
                "EXISTS (SELECT 1 FROM module_issues fm3 WHERE fm3.issue_id = i.id \
                     AND fm3.deleted_at IS NULL AND fm3.module_id = ANY(${index}::uuid[]))"
            ));
        }
    }
    // `filter_intake_status` and `filter_inbox_status` both read
    // `inbox_status` in the POST branch (`issue_filters.py:253-279`).
    if (present("intake_status") || present("inbox_status"))
        && let Some(list) = text_list(values("inbox_status"), "inbox_status")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter.parts.push(format!(
            "EXISTS (SELECT 1 FROM intake_issues fi WHERE fi.issue_id = i.id \
                     AND fi.status = ANY(${index}::text[]))"
        ));
    }
    if present("sub_issue") {
        let toggle = values("sub_issue")
            .and_then(Value::as_str)
            .unwrap_or("false");
        if toggle == "false" {
            filter.parts.push("i.parent_id IS NULL".to_string());
        }
    }
    if present("subscriber")
        && let Some(list) = uuid_list(values("subscriber"), "subscriber")?
        && !list.is_empty()
    {
        let index = filter.bind(list);
        filter.parts.push(format!(
            "EXISTS (SELECT 1 FROM issue_subscribers fs WHERE fs.issue_id = i.id \
                     AND fs.deleted_at IS NULL AND fs.subscriber_id = ANY(${index}::uuid[]))"
        ));
    }
    if present("start_target_date") {
        let toggle = values("start_target_date")
            .and_then(Value::as_str)
            .unwrap_or("false");
        if toggle == "true" {
            filter.parts.push("i.start_date IS NOT NULL".to_string());
            filter.parts.push("i.target_date IS NOT NULL".to_string());
        }
    }
    Ok(())
}

/// `filter_valid_uuids`: a list of UUID strings, invalid ones dropped unless
/// the whole lookup cannot be parsed (Django validates and raises).
fn uuid_list(value: Option<&Value>, key: &str) -> Result<Option<Vec<Uuid>>, Failure> {
    let Some(value) = value else {
        return Ok(None);
    };
    let items: Vec<String> = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| string_value(Some(item)))
            .filter(|text| text != "null")
            .collect(),
        Value::String(text) if !text.is_empty() && text != "null" => vec![text.clone()],
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for item in items {
        match Uuid::parse_str(&item) {
            Ok(id) => out.push(id),
            Err(_) => {
                return Err(format!(
                    "\u{201c}{item}\u{201d} is not a valid UUID ({key})."
                ));
            }
        }
    }
    Ok(Some(out))
}

fn text_list(value: Option<&Value>, _key: &str) -> Result<Option<Vec<String>>, Failure> {
    let Some(value) = value else {
        return Ok(None);
    };
    let items = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| string_value(Some(item)))
            .filter(|text| text != "null")
            .collect(),
        other => match string_value(Some(other)) {
            Some(text) if !text.is_empty() && text != "null" => vec![text],
            _ => Vec::new(),
        },
    };
    Ok(Some(items))
}

/// `date_filter` (`issue_filters.py:48-70`) for a datetime `__date` lookup.
fn date_filter(filter: &mut Filter, value: Option<&Value>, column: &str) -> Result<(), Failure> {
    let Some(value) = value else {
        return Ok(());
    };
    let queries: Vec<String> = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| string_value(Some(item)))
            .collect(),
        other => vec![string_value(Some(other)).unwrap_or_default()],
    };
    for query in queries {
        if query.is_empty() {
            continue;
        }
        let parts: Vec<&str> = query.split(';').collect();
        if parts.len() >= 2 {
            if let Some((duration, term)) = relative_duration(parts[0]) {
                let offset = parts.get(2).copied().unwrap_or("");
                let after = parts[1] == "after";
                let from_now = offset == "fromnow";
                let today = Utc::now().date_naive();
                let days = if term == "months" {
                    duration * 30
                } else {
                    duration * 7
                };
                let date = if after {
                    if from_now {
                        today + chrono::Duration::days(days)
                    } else {
                        today - chrono::Duration::days(days)
                    }
                } else if from_now {
                    today + chrono::Duration::days(days)
                } else {
                    today - chrono::Duration::days(days)
                };
                let operator = if after { ">=" } else { "<=" };
                let index = filter.bind(date);
                filter.parts.push(format!(
                    "({column} AT TIME ZONE 'UTC')::date {operator} ${index}::date"
                ));
            } else if parts.contains(&"after") {
                let index = filter.bind(parts[0].to_string());
                filter.parts.push(format!(
                    "({column} AT TIME ZONE 'UTC')::date >= ${index}::date"
                ));
            } else {
                let index = filter.bind(parts[0].to_string());
                filter.parts.push(format!(
                    "({column} AT TIME ZONE 'UTC')::date <= ${index}::date"
                ));
            }
        } else {
            let index = filter.bind(query);
            filter.parts.push(format!(
                "({column} AT TIME ZONE 'UTC')::date::text LIKE '%' || ${index} || '%'"
            ));
        }
    }
    Ok(())
}

fn relative_duration(text: &str) -> Option<(i64, &'static str)> {
    let (digits, term) = text.split_once('_')?;
    let duration: i64 = digits.parse().ok()?;
    match term {
        "weeks" => Some((duration, "weeks")),
        "months" => Some((duration, "months")),
        _ => None,
    }
}

/// The base `Issue.issue_objects` FROM/WHERE, with the project and state joins
/// the filters and axes need.
fn base_from(filter: &Filter, joins: &str) -> String {
    format!(
        "FROM issues i \
         JOIN projects p ON p.id = i.project_id \
         LEFT JOIN states s ON s.id = i.state_id{joins} \
         WHERE i.deleted_at IS NULL AND i.archived_at IS NULL \
           AND (s.\"group\" IS DISTINCT FROM 'triage') \
           AND p.archived_at IS NULL AND i.is_draft = FALSE \
           AND i.workspace_id = (SELECT id FROM workspaces WHERE slug = ${{slug}} AND deleted_at IS NULL) \
           AND {}",
        filter.where_sql()
    )
}

/// The joins an axis needs, with their aliases reused when the x-axis and the
/// segment touch the same table (Django reuses the join for one relation).
#[derive(Default)]
struct AxisJoins {
    labels: Option<String>,
    assignees: Option<String>,
    estimate: Option<String>,
    cycles: Option<String>,
    modules: Option<String>,
    next: usize,
}

impl AxisJoins {
    fn for_axis(&mut self, field: &str, joins: &mut String) -> String {
        match field {
            "labels__id" => {
                let alias = self
                    .labels
                    .get_or_insert_with(|| {
                        let alias = format!("axlab{}", self.next);
                        self.next += 1;
                        joins.push_str(&format!(
                            " JOIN issue_labels {alias} ON {alias}.issue_id = i.id"
                        ));
                        alias
                    })
                    .clone();
                format!("{alias}.label_id")
            }
            "assignees__id" => {
                let alias = self
                    .assignees
                    .get_or_insert_with(|| {
                        let alias = format!("axasg{}", self.next);
                        self.next += 1;
                        joins.push_str(&format!(
                            " JOIN issue_assignees {alias} ON {alias}.issue_id = i.id"
                        ));
                        alias
                    })
                    .clone();
                format!("{alias}.assignee_id")
            }
            "estimate_point__value" => {
                let alias = self
                    .estimate
                    .get_or_insert_with(|| {
                        let alias = format!("axest{}", self.next);
                        self.next += 1;
                        joins.push_str(&format!(
                            " JOIN estimate_points {alias} ON {alias}.id = i.estimate_point_id"
                        ));
                        alias
                    })
                    .clone();
                format!("{alias}.value")
            }
            "issue_cycle__cycle_id" => {
                let alias = self
                    .cycles
                    .get_or_insert_with(|| {
                        let alias = format!("axcyc{}", self.next);
                        self.next += 1;
                        joins.push_str(&format!(
                            " JOIN cycle_issues {alias} ON {alias}.issue_id = i.id"
                        ));
                        alias
                    })
                    .clone();
                format!("{alias}.cycle_id")
            }
            "issue_module__module_id" => {
                let alias = self
                    .modules
                    .get_or_insert_with(|| {
                        let alias = format!("axmod{}", self.next);
                        self.next += 1;
                        joins.push_str(&format!(
                            " JOIN module_issues {alias} ON {alias}.issue_id = i.id"
                        ));
                        alias
                    })
                    .clone();
                format!("{alias}.module_id")
            }
            "state_id" => "i.state_id".to_string(),
            "state__group" => "s.\"group\"".to_string(),
            "priority" => "i.priority".to_string(),
            other => {
                // A date axis: `EXTRACT(year) || '-' || EXTRACT(month)`.
                format!(
                    "(EXTRACT(YEAR FROM i.{other})::text || '-' || EXTRACT(MONTH FROM i.{other})::text)"
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
struct PlotRow {
    segment: Option<String>,
    count: Option<i64>,
    estimate: Option<f64>,
}

async fn build_graph_plot(
    client: &Client,
    slug: &str,
    filter: &Filter,
    x_axis: &str,
    y_axis: &str,
    segment: Option<&str>,
) -> Result<Vec<(String, Vec<PlotRow>)>, Failure> {
    let mut joins = String::new();
    let mut axes = AxisJoins::default();
    let dimension = axes.for_axis(x_axis, &mut joins);
    let segmented = segment.map(|segment| {
        // `annotate_with_monthly_dimension` replaces the axis with
        // `segmented` for the date fields; the column expression is the same.
        axes.for_axis(segment, &mut joins)
    });

    let mut select = format!("SELECT {dimension}::text AS dimension");
    if segmented.is_some() {
        select.push_str(", ax_segment::text AS segment");
    }
    if y_axis == "issue_count" {
        select.push_str(", COUNT(*)::bigint AS count");
    } else {
        select.push_str(", SUM(CAST(ep_sum.value AS double precision)) AS estimate");
        joins.push_str(" LEFT JOIN estimate_points ep_sum ON ep_sum.id = i.estimate_point_id");
    }
    let from =
        base_from(filter, &joins).replace("${slug}", &format!("${}", filter.params.len() + 1));
    let mut sql = format!("{select} {from}");
    if let Some(segmented) = &segmented {
        // The alias is not usable in GROUP BY's expression form on every
        // server, so repeat the expression; PostgreSQL accepts the select
        // alias in GROUP BY.
        let _ = segmented;
        sql.push_str(&format!(" GROUP BY {dimension}"));
        sql.push_str(&format!(" , {segmented}"));
    } else {
        sql.push_str(&format!(" GROUP BY {dimension}"));
    }
    sql.push_str(&format!(" ORDER BY {dimension}"));
    let slug_index = filter.params.len() + 1;
    let mut params = filter.params();
    params.push(&slug as &(dyn tokio_postgres::types::ToSql + Sync));
    // The bind order must match: filter params then slug.
    let _ = slug_index;

    let rows = client
        .query(&sql, &params)
        .await
        .map_err(|error| format!("analytic plot query: {error}\n{sql}"))?;

    let mut grouped: BTreeMap<String, Vec<PlotRow>> = BTreeMap::new();
    for row in rows {
        let dimension: String = row.get(0);
        let segment: Option<String> = if segmented.is_some() {
            row.get(1)
        } else {
            None
        };
        let (count, estimate) = if y_axis == "issue_count" {
            (row.get(1 + usize::from(segmented.is_some())), None)
        } else {
            (None, row.get(1 + usize::from(segmented.is_some())))
        };
        grouped.entry(dimension).or_default().push(PlotRow {
            segment,
            count,
            estimate,
        });
    }
    Ok(sort_data(grouped, x_axis))
}

/// `sort_data` (`analytics_plot.py:56-68`), as an ordered list: a priority
/// axis uses the fixed dictionary order, which a sorted map would destroy.
fn sort_data(data: BTreeMap<String, Vec<PlotRow>>, axis: &str) -> Vec<(String, Vec<PlotRow>)> {
    if axis == "priority" {
        let order = ["low", "medium", "high", "urgent", "none"];
        return order
            .iter()
            .filter_map(|key| {
                data.get(*key)
                    .map(|value| ((*key).to_string(), value.clone()))
            })
            .collect();
    }
    let mut keys: Vec<&String> = data.keys().collect();
    keys.sort_by(|a, b| {
        let left = (*a == "none", a.as_str());
        let right = (*b == "none", b.as_str());
        left.cmp(&right)
    });
    keys.into_iter()
        .map(|key| (key.clone(), data[key].clone()))
        .collect()
}

/// `get_*_details` (`analytic_plot_export.py:90-192`). `DISTINCT ON` needs the
/// detail id first in `ORDER BY`, as Django's `.distinct(...)` emits.
#[derive(Clone, Copy)]
enum Detail {
    Assignees,
    Labels,
    States,
    Cycles,
    Modules,
}

async fn detail_rows(
    client: &Client,
    slug: &str,
    filter: &Filter,
    detail: Detail,
) -> Result<Vec<(String, String)>, Failure> {
    let (id_column, name_column, distinct_column) = match detail {
        Detail::Assignees => (
            "u.id::text",
            "trim(both ' ' from u.first_name || ' ' || u.last_name)",
            "u.id",
        ),
        Detail::Labels => ("l.id::text", "l.name", "l.id"),
        Detail::States => ("s.id::text", "s.name", "s.id"),
        Detail::Cycles => ("c.id::text", "c.name", "c.id"),
        Detail::Modules => ("m.id::text", "m.name", "m.id"),
    };
    let joins = match detail {
        Detail::Assignees => {
            " JOIN issue_assignees da ON da.issue_id = i.id \
                               AND da.deleted_at IS NULL JOIN users u ON u.id = da.assignee_id"
        }
        Detail::Labels => {
            " JOIN issue_labels dl ON dl.issue_id = i.id \
                            AND dl.deleted_at IS NULL JOIN labels l ON l.id = dl.label_id"
        }
        Detail::States => "",
        Detail::Cycles => {
            " JOIN cycle_issues dc ON dc.issue_id = i.id \
                            AND dc.deleted_at IS NULL JOIN cycles c ON c.id = dc.cycle_id"
        }
        Detail::Modules => {
            " JOIN module_issues dm ON dm.issue_id = i.id \
                             AND dm.deleted_at IS NULL JOIN modules m ON m.id = dm.module_id"
        }
    };
    // The detail queries are `Issue.issue_objects.filter(workspace__slug=slug,
    // **filters)` plus the join; the base already has the project/state joins.
    let slug_index = filter.params.len() + 1;
    let mut params = filter.params();
    params.push(&slug as &(dyn tokio_postgres::types::ToSql + Sync));
    let sql = format!(
        "SELECT DISTINCT ON ({distinct_column}) {id_column}, {name_column} \
         FROM issues i JOIN projects p ON p.id = i.project_id \
         LEFT JOIN states s ON s.id = i.state_id{joins} \
         WHERE i.deleted_at IS NULL AND i.archived_at IS NULL \
           AND (s.\"group\" IS DISTINCT FROM 'triage') \
           AND p.archived_at IS NULL AND i.is_draft = FALSE \
           AND i.workspace_id = (SELECT id FROM workspaces WHERE slug = ${slug_index} AND deleted_at IS NULL) \
           AND {} ORDER BY {distinct_column}",
        filter.where_sql()
    );
    let rows = client
        .query(&sql, &params)
        .await
        .map_err(|error| format!("analytic detail query: {error}\n{sql}"))?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect())
}

#[derive(Default)]
struct Details {
    assignees: Vec<(String, String)>,
    labels: Vec<(String, String)>,
    states: Vec<(String, String)>,
    cycles: Vec<(String, String)>,
    modules: Vec<(String, String)>,
}

fn x_header(axis: &str) -> &'static str {
    match axis {
        "state__name" => "State",
        "state__group" => "State Group",
        "labels__id" => "Label",
        "assignees__id" => "Assignee Name",
        "start_date" => "Start Date",
        "target_date" => "Due Date",
        "completed_at" => "Completed At",
        "created_at" => "Created At",
        "priority" => "Priority",
        "issue_cycle__cycle_id" => "Cycle",
        "issue_module__module_id" => "Module",
        _ => "X-Axis",
    }
}

/// The segment-column name. `analytic_plot_export.py:278-282` reads
/// `label_details` for a module segment, so a module id is never resolved here.
fn display_segment(value: &str, axis: &str, details: &Details) -> String {
    if axis == MODULE_ID {
        return value.to_string();
    }
    display(value, axis, details)
}

fn display(value: &str, axis: &str, details: &Details) -> String {
    let list = match axis {
        ASSIGNEE_ID => Some(&details.assignees),
        LABEL_ID => Some(&details.labels),
        STATE_ID => Some(&details.states),
        CYCLE_ID => Some(&details.cycles),
        MODULE_ID => Some(&details.modules),
        _ => None,
    };
    if let Some(list) = list
        && let Some((_, name)) = list.iter().find(|(id, _)| id == value)
    {
        return name.clone();
    }
    value.to_string()
}

/// One CSV cell: a string, an integer or a float (or `None`).
#[derive(Debug, Clone, PartialEq)]
enum CsvValue {
    Text(String),
    Int(i64),
    Float(f64),
    Null,
}

fn csv_value_for(row: &PlotRow, key: &str) -> CsvValue {
    if key == "count" {
        row.count.map_or(CsvValue::Null, CsvValue::Int)
    } else {
        row.estimate.map_or(CsvValue::Null, CsvValue::Float)
    }
}

fn non_segmented_rows(
    distribution: &[(String, Vec<PlotRow>)],
    x_axis: &str,
    y_axis: &str,
    key: &str,
    details: &Details,
) -> Vec<Vec<CsvValue>> {
    let mut rows = vec![vec![
        CsvValue::Text(x_header(x_axis).to_string()),
        CsvValue::Text(if y_axis == "issue_count" {
            "Issue Count".to_string()
        } else {
            "Estimate".to_string()
        }),
    ]];
    for (item, data) in distribution {
        let value = data
            .first()
            .map_or(CsvValue::Null, |row| csv_value_for(row, key));
        rows.push(vec![CsvValue::Text(display(item, x_axis, details)), value]);
    }
    rows
}

fn segmented_rows(
    distribution: &[(String, Vec<PlotRow>)],
    x_axis: &str,
    y_axis: &str,
    segment: &str,
    key: &str,
    details: &Details,
) -> Vec<Vec<CsvValue>> {
    // `list(set(...))`: first-seen here (see the module docs).
    let mut segment_zero: Vec<String> = Vec::new();
    for (_, rows) in distribution {
        for row in rows {
            if let Some(value) = &row.segment
                && !segment_zero.contains(value)
            {
                segment_zero.push(value.clone());
            }
        }
    }
    let mut row_zero = vec![
        CsvValue::Text(x_header(x_axis).to_string()),
        CsvValue::Text(if y_axis == "issue_count" {
            "Issue Count".to_string()
        } else {
            "Estimate".to_string()
        }),
    ];
    for value in &segment_zero {
        row_zero.push(CsvValue::Text(display_segment(value, segment, details)));
    }
    let mut rows = vec![row_zero];
    for (item, data) in distribution {
        // `sum(obj.get(key) for obj in data if obj.get(key) is not None)`.
        let total = if key == "count" {
            CsvValue::Int(data.iter().filter_map(|row| row.count).sum())
        } else {
            let sum: f64 = data.iter().filter_map(|row| row.estimate).sum();
            CsvValue::Float(sum)
        };
        let mut row = vec![CsvValue::Text(display(item, x_axis, details)), total];
        for value in &segment_zero {
            let cell = data
                .iter()
                .find(|row| row.segment.as_deref() == Some(value.as_str()))
                .map_or(CsvValue::Null, |row| csv_value_for(row, key));
            row.push(cell);
        }
        rows.push(row);
    }
    rows
}

/// `csv.writer(buffer, delimiter=",", quoting=csv.QUOTE_ALL)` over
/// `sanitize_csv_row` (`analytic_plot_export.py:195-200`).
fn csv_from_rows(rows: &[Vec<CsvValue>]) -> String {
    let mut out = String::new();
    for row in rows {
        let mut first = true;
        for value in row {
            if !first {
                out.push(',');
            }
            first = false;
            let text = match value {
                CsvValue::Text(text) => sanitize(text),
                CsvValue::Int(number) => number.to_string(),
                CsvValue::Float(number) => {
                    plane_serde::float_repr(*number).unwrap_or_else(|| number.to_string())
                }
                CsvValue::Null => String::new(),
            };
            out.push('"');
            out.push_str(&text.replace('"', "\"\""));
            out.push('"');
        }
        out.push_str("\r\n");
    }
    out
}

fn sanitize(value: &str) -> String {
    match value.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r' | '\n') => {
            format!("'{value}")
        }
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_csv_quotes_every_field() {
        let rows = vec![
            vec![
                CsvValue::Text("Priority".into()),
                CsvValue::Text("Issue Count".into()),
            ],
            vec![CsvValue::Text("high".into()), CsvValue::Int(2)],
            vec![CsvValue::Text("=1".into()), CsvValue::Null],
        ];
        assert_eq!(
            csv_from_rows(&rows),
            "\"Priority\",\"Issue Count\"\r\n\"high\",\"2\"\r\n\"'=1\",\"\"\r\n"
        );
    }

    #[test]
    fn priority_order_is_the_fixed_one() {
        let row = |_dimension: &str| PlotRow {
            segment: None,
            count: Some(1),
            estimate: None,
        };
        let mut data = BTreeMap::new();
        for key in ["none", "urgent", "high", "medium", "low", "other"] {
            data.insert(key.to_string(), vec![row(key)]);
        }
        let sorted = sort_data(data, "priority");
        let keys: Vec<&str> = sorted.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["low", "medium", "high", "urgent", "none"]);
    }

    #[test]
    fn other_axes_sort_none_last() {
        let row = |_dimension: &str| PlotRow {
            segment: None,
            count: Some(1),
            estimate: None,
        };
        let mut data = BTreeMap::new();
        for key in ["none", "b", "a"] {
            data.insert(key.to_string(), vec![row(key)]);
        }
        let sorted = sort_data(data, "priority_id");
        let keys: Vec<&str> = sorted.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["a", "b", "none"]);
    }
}
