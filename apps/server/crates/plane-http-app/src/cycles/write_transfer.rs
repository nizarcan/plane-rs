// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `TransferCycleIssueEndpoint.post` (route 38;
//! `plane/app/views/cycle/base.py:600-631` and
//! `plane/utils/cycle_transfer_issues.py:35-486`).
//!
//! ## Statement order (all of it observable)
//!
//! 1. `new_cycle_id = request.data.get("new_cycle_id", False)`; a falsy value
//!    is `400 {"error": "New Cycle Id is required"}`.
//! 2. `new_cycle` lookup; **no `None` check** — an unknown id reaches
//!    `new_cycle.end_date` (`:61`) and is an `AttributeError` → family 500.
//! 3. A destination whose `end_date` is in the past is
//!    `400 {"error": "The cycle where the issues are transferred is already
//!    completed"}`.
//! 4. `old_cycle` with the six annotated counts; `None` is
//!    `400 {"error": "Source cycle not found"}`.
//! 5. `estimate_type` (project estimate is of type `points`).
//! 6. When estimates exist: the two estimate distributions and
//!    `burndown_plot(plot_type="points")`.
//! 7. Always: the assignee and label issue distributions and
//!    `burndown_plot(plot_type="issues")`.
//! 8. `current_cycle.progress_snapshot = {…}` and
//!    `save(update_fields=["progress_snapshot"])` — that one column only, so
//!    `updated_at`/`updated_by` keep their values (the row is not re-saved).
//! 9. The unfinished, non-archived, non-draft issues of the source cycle
//!    (`-created_at` order) get `cycle_id = new_cycle_id` via
//!    `bulk_update(["cycle_id"], batch_size=100)` — the FK only, no
//!    `updated_at`.
//! 10. `cycle.activity.created` with `requested_data={"cycles_list": []}` and
//!     `current_instance={"updated_cycle_issues": [...],
//!     "created_cycle_issues": []}`; `200 {"message": "Success"}`.
//!
//! Transferring a cycle into itself is allowed: the stale `CycleIssue` rows of
//! the same cycle are "moved" to the same value (and still listed in the
//! activity) and only the snapshot is written.

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_query::json::Json;
use uuid::Uuid;

use super::base::{
    Chart, ChartValue, Distribution, DistributionKind, DistributionList, distribute, error_body,
    estimate_type_exists, issues_chart, params, points_chart, uuid_kwarg,
};
use super::queries;
use super::write_issue_queries as queries_write;
use crate::common::jobs;
use crate::write;
use crate::write::django_serialize::django_json;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.cycle.base.TransferCycleIssueEndpoint")?
        .method(http::Method::POST, post)?;
    Ok(())
}

pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = crate::common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let user = jobs::user_id(ctx)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let now = write::now();
        let data = ctx.data()?;

        // `new_cycle_id = request.data.get("new_cycle_id", False)`;
        // `if not new_cycle_id` (`base.py:602-608`).
        let new_cycle_id = match raw_field(data, "new_cycle_id") {
            RawField::Absent | RawField::Null => {
                return Ok(error_body(400, "New Cycle Id is required"));
            }
            RawField::Text(text) => {
                if text.is_empty() {
                    return Ok(error_body(400, "New Cycle Id is required"));
                }
                Uuid::parse_str(text).map_err(|_| ViewError::Validation)?
            }
            RawField::Other(value) => return non_string_id(value),
        };

        // `new_cycle = Cycle.objects.filter(...).first()` — no None check
        // (`cycle_transfer_issues.py:58-65`).
        let statement = queries_write::NEW_CYCLE.prepare(client).await?;
        let Some(new_cycle) = client
            .query_opt(&statement, &[&slug, &project_id, &new_cycle_id])
            .await?
        else {
            // `new_cycle.end_date` on `None` is an `AttributeError`.
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'end_date'".to_owned(),
            ));
        };
        let new_end: Option<chrono::DateTime<chrono::Utc>> = new_cycle.get(1);
        if new_end.is_some_and(|end| end < now) {
            return Ok(error_body(
                400,
                "The cycle where the issues are transferred is already completed",
            ));
        }

        // `old_cycle` with its six counts (`:68-172`).
        let statement = queries_write::OLD_CYCLE.prepare(client).await?;
        let Some(old_cycle) = client
            .query_opt(&statement, &[&slug, &project_id, &cycle_id])
            .await?
        else {
            return Ok(error_body(400, "Source cycle not found"));
        };
        let start: Option<chrono::DateTime<chrono::Utc>> = old_cycle.get(0);
        let end: Option<chrono::DateTime<chrono::Utc>> = old_cycle.get(1);
        let counts = CycleCounts {
            total_issues: old_cycle.get(2),
            completed_issues: old_cycle.get(3),
            cancelled_issues: old_cycle.get(4),
            started_issues: old_cycle.get(5),
            unstarted_issues: old_cycle.get(6),
            backlog_issues: old_cycle.get(7),
        };

        let params = params(ctx)?;
        let estimate_type = estimate_type_exists(client, &params).await?;
        let dates = match (start, end) {
            (Some(start), Some(end)) => Some((start, end)),
            _ => None,
        };

        // The estimate distributions and points chart (`:176-281`).
        let mut estimate_labels: Vec<Distribution> = Vec::new();
        let mut estimate_assignees: Vec<Distribution> = Vec::new();
        let mut estimate_chart = Chart(Vec::new());
        if estimate_type {
            estimate_assignees = distribute(
                client,
                &queries::CYCLE_ANALYTICS_ASSIGNEES_POINTS,
                &[&cycle_id, &params.project_id, &params.slug],
                DistributionKind::AnalyticsAssigneesPoints,
            )
            .await?;
            estimate_labels = distribute(
                client,
                &queries::CYCLE_ANALYTICS_LABELS_POINTS,
                &[&cycle_id, &params.project_id, &params.slug],
                DistributionKind::AnalyticsLabelsPoints,
            )
            .await?;
            if let Some((start, end)) = dates {
                estimate_chart =
                    points_chart(client, &params, cycle_id, start, end, timezone).await?;
            }
        }

        // The issue distributions and chart (`:284-425`).
        let assignees = distribute(
            client,
            &queries_write::TRANSFER_ASSIGNEES_ISSUES,
            &[&cycle_id, &params.project_id, &params.slug],
            DistributionKind::AnalyticsAssigneesIssues,
        )
        .await?;
        let labels = distribute(
            client,
            &queries_write::TRANSFER_LABELS_ISSUES,
            &[&cycle_id, &params.project_id, &params.slug],
            DistributionKind::AnalyticsLabelsIssues,
        )
        .await?;
        let completion_chart = match dates {
            Some((start, end)) => {
                issues_chart(
                    client,
                    &params,
                    cycle_id,
                    start,
                    end,
                    counts.total_issues,
                    timezone,
                )
                .await?
            }
            None => Chart(Vec::new()),
        };

        // `current_cycle.progress_snapshot = {…}` and
        // `save(update_fields=["progress_snapshot"])` (`:427-434`).
        let snapshot = Json::object(vec![
            ("total_issues", Json::Int(counts.total_issues)),
            ("completed_issues", Json::Int(counts.completed_issues)),
            ("cancelled_issues", Json::Int(counts.cancelled_issues)),
            ("started_issues", Json::Int(counts.started_issues)),
            ("unstarted_issues", Json::Int(counts.unstarted_issues)),
            ("backlog_issues", Json::Int(counts.backlog_issues)),
            (
                "distribution",
                Json::object(vec![
                    ("labels", distribution_json(&labels)?),
                    ("assignees", distribution_json(&assignees)?),
                    ("completion_chart", chart_json(&completion_chart)),
                ]),
            ),
            (
                "estimate_distribution",
                if estimate_type {
                    Json::object(vec![
                        ("labels", distribution_json(&estimate_labels)?),
                        ("assignees", distribution_json(&estimate_assignees)?),
                        ("completion_chart", chart_json(&estimate_chart)),
                    ])
                } else {
                    Json::Object(Vec::new())
                },
            ),
        ]);
        let snapshot_text = JsonText::new(django_json(&snapshot)?);
        let statement = queries_write::SAVE_PROGRESS_SNAPSHOT
            .prepare(client)
            .await?;
        client
            .execute(&statement, &[&snapshot_text, &cycle_id])
            .await?;

        // `cycle_issues = CycleIssue.objects.filter(...)` then `bulk_update`
        // (`:437-471`).
        let statement = queries_write::TRANSFER_ISSUES.prepare(client).await?;
        let rows = client
            .query(&statement, &[&cycle_id, &project_id, &slug])
            .await?;
        let moved: Vec<(Uuid, Uuid)> = rows
            .iter()
            .map(|row| (row.get::<_, Uuid>(0), row.get::<_, Uuid>(1)))
            .collect();
        if !moved.is_empty() {
            let ids: Vec<Uuid> = moved.iter().map(|(id, _)| *id).collect();
            let statement = queries_write::MOVE_CYCLE_ISSUES.prepare(client).await?;
            client.execute(&statement, &[&new_cycle_id, &ids]).await?;
        }

        // The activity (`:473-492`).
        let updated: Vec<Json> = moved
            .iter()
            .map(|(_, issue_id)| {
                Json::object(vec![
                    ("old_cycle_id", Json::uuid(cycle_id)),
                    ("new_cycle_id", Json::uuid(new_cycle_id)),
                    ("issue_id", Json::uuid(*issue_id)),
                ])
            })
            .collect();
        let requested_data = Json::object(vec![("cycles_list", Json::Array(Vec::new()))]);
        let current_instance = Json::object(vec![
            ("updated_cycle_issues", Json::Array(updated)),
            ("created_cycle_issues", Json::Array(Vec::new())),
        ]);
        publish_activity(
            ctx,
            TaskArg::json_text(django_json(&requested_data)?),
            Some(TaskArg::json_text(django_json(&current_instance)?)),
            project_id,
            user,
            now,
        )
        .await?;

        Ok(plain_message(200, "Success"))
    })
}

// ---------------------------------------------------------------------------
// Snapshot pieces
// ---------------------------------------------------------------------------

struct CycleCounts {
    total_issues: i64,
    completed_issues: i64,
    cancelled_issues: i64,
    started_issues: i64,
    unstarted_issues: i64,
    backlog_issues: i64,
}

/// One distribution list as the snapshot embeds it. The renderer is the read
/// path's `DistributionList` (same keys and value types), so the two cannot
/// drift.
fn distribution_json(distributions: &[Distribution]) -> Result<Json, ViewError> {
    let prepared = plane_serializers::RenderCtx::new(&chrono_tz::UTC, None, &[]);
    let bytes = plane_serializers::render(&DistributionList(distributions), &prepared)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let parsed = plane_serde::pyjson::loads(&bytes, "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(pyjson_to_json(&parsed))
}

/// `burndown_plot`'s return value: an ordered `{date: value}` object.
fn chart_json(chart: &Chart) -> Json {
    Json::Object(
        chart
            .0
            .iter()
            .map(|(date, value)| {
                let value = match value {
                    ChartValue::Null => Json::Null,
                    ChartValue::Int(value) => Json::Int(*value),
                    ChartValue::Float(value) => Json::Float(*value),
                };
                (date.clone(), value)
            })
            .collect(),
    )
}

fn pyjson_to_json(value: &plane_serde::pyjson::Value) -> Json {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Null => Json::Null,
        Py::Bool(flag) => Json::Bool(*flag),
        Py::Int(number) => number.as_i64().map_or_else(
            || Json::Float(number.to_string().parse().unwrap_or_default()),
            Json::Int,
        ),
        Py::Float(number) => Json::Float(*number),
        Py::Str(text) => Json::string(text.as_str().unwrap_or_default()),
        Py::List(items) => Json::Array(items.iter().map(pyjson_to_json).collect()),
        Py::Dict(object) => Json::Object(
            object
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str().unwrap_or_default().to_owned(),
                        pyjson_to_json(value),
                    )
                })
                .collect(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

enum RawField<'a> {
    Absent,
    Null,
    Text(&'a str),
    Other(&'a plane_serde::pyjson::Value),
}

fn raw_field<'a>(data: &'a Data, name: &str) -> RawField<'a> {
    use plane_serde::pyjson::Value as Py;
    match data {
        Data::Form(query) => query.get(name).map_or(RawField::Absent, RawField::Text),
        Data::Json(object) => match object.get(name) {
            None => RawField::Absent,
            Some(Py::Null) => RawField::Null,
            Some(Py::Str(text)) => text.as_str().map_or_else(
                || RawField::Other(object.get(name).expect("the key was just read")),
                RawField::Text,
            ),
            Some(other) => RawField::Other(other),
        },
    }
}

/// A truthy non-string `new_cycle_id`: `0`/`False` are falsy (the 400 above),
/// any other value goes through `UUIDField.get_prep_value`.
fn non_string_id(value: &plane_serde::pyjson::Value) -> Result<ViewResponse, ViewError> {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Bool(false) => Ok(error_body(400, "New Cycle Id is required")),
        Py::Int(number) if number.as_i64() == Some(0) => {
            Ok(error_body(400, "New Cycle Id is required"))
        }
        Py::List(items) if items.is_empty() => Ok(error_body(400, "New Cycle Id is required")),
        Py::Dict(object) if object.iter().next().is_none() => {
            Ok(error_body(400, "New Cycle Id is required"))
        }
        _ => Err(ViewError::Validation),
    }
}

fn plain_message(status: u16, message: &str) -> ViewResponse {
    let body = Json::object(vec![("message", Json::string(message))]);
    ViewResponse::raw(status, APPLICATION_JSON, body.to_drf())
}

async fn publish_activity(
    ctx: &ViewCtx<'_>,
    requested_data: TaskArg,
    current_instance: Option<TaskArg>,
    project_id: Uuid,
    user: Uuid,
    epoch: chrono::DateTime<chrono::Utc>,
) -> Result<(), ViewError> {
    let base = IssueActivityBase::new(
        "cycle.activity.created",
        requested_data,
        user.to_string(),
        TaskArg::Null,
        TaskArg::str(project_id.to_string()),
        current_instance.unwrap_or(TaskArg::Null),
        epoch.timestamp(),
    );
    let message = tasks::issue_activity(IssueActivityCall::view(
        base,
        queries_write::base_host(ctx)?,
    ));
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::from_message(&message),
    )
    .await?;
    Ok(())
}
