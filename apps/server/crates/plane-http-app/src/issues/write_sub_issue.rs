// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `SubIssuesEndpoint.post` (`plane/app/views/issue/sub_issue.py:207-298`) —
//! re-parent a batch of issues under the URL issue.
//!
//! ## What Django does
//!
//! 1. `Issue.issue_objects.filter(pk=issue_id, workspace__slug=slug,
//!    project_id=project_id).first()`; a miss is
//!    `404 {"error": "Parent issue not found"}`. (`issue_objects` excludes
//!    triage states, archived/draft issues and archived projects.)
//! 2. `sub_issue_ids = request.data.get("sub_issue_ids", [])`; a falsy value
//!    (`[]`, `""`, a missing key) is
//!    `400 {"error": "Sub Issue IDs are required"}`. An `id__in` element that
//!    is not a UUID is Django's `ValidationError` → 400
//!    `{"error": "Please provide valid detail"}`; a non-iterable value is a
//!    `TypeError` → 500.
//! 3. The ids are re-read through `issue_objects` scoped to the URL project
//!    (cross-project ids are silently dropped) in `-created_at` order, then
//!    `Issue.objects.bulk_update(sub_issues, ["parent"], batch_size=10)`:
//!    `parent_id` only — `bulk_update` fires no `save()` hook, so `updated_at`
//!    and the audit columns are untouched.
//! 4. One `issue.activity.updated` per **scoped** id (the ids actually
//!    re-parented, not the raw body) with
//!    `requested_data=json.dumps({"parent": "<url issue id>"})` and
//!    `current_instance=json.dumps({"parent": "<sub issue id>"})` — the
//!    pre-image names the sub-issue itself as its parent, which is the
//!    upstream behaviour this port keeps (see the module's tests).
//! 5. The response is `{"sub_issues": IssueSerializer(updated).data,
//!    "state_distribution": defaultdict(list)}`, 200. `IssueSerializer` is the
//!    plain one: `cycle_id`, the three m2m arrays and the three counts are
//!    declared fields with no attribute on this queryset, so DRF's
//!    `SkipField` omits them. `state_distribution` buckets each id under its
//!    `state__group`; a stateless issue keys the bucket `None`, which
//!    `json.dumps` renders as `"null"`.

use chrono::{DateTime, NaiveDate, Utc};
use plane_core::error::APPLICATION_JSON;
use plane_db::models::issue::Issue;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_query::json::Json;
use plane_serializers::FieldValue;
use plane_serializers::generated::app_issue::IssueSerializer;
use uuid::Uuid;

use super::lists::list_common::client;
use super::sub_resources::sub_common as shared;
use super::write_sub_queries as queries;
use crate::common;
use crate::common::jobs;
use crate::write;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.sub_issue.SubIssuesEndpoint")?
        .method(http::Method::POST, post)?;
    Ok(())
}

/// One row of [`queries::SUB_ISSUES`]: the issue plus its state group.
struct SubIssueRow {
    issue: Issue,
    state_group: Option<String>,
}

pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user = shared::user_id(ctx)?;
        let now = write::now();
        let data = ctx.data()?;

        // `Issue.issue_objects.filter(...).first()` (`:214-218`).
        let statement = queries::ISSUE_LIVE_IN_PROJECT.prepare(client).await?;
        let parent: bool = client
            .query_one(&statement, &[&issue_id, &project_id, &slug])
            .await?
            .get(0);
        if !parent {
            return Ok(plain_error(404, "Parent issue not found"));
        }

        // `sub_issue_ids = request.data.get("sub_issue_ids", [])`;
        // `if not len(sub_issue_ids)` (`:219-226`).
        let requested = match queries::raw_field(data, "sub_issue_ids") {
            queries::RawField::Absent => Vec::new(),
            queries::RawField::Null => {
                return Err(ViewError::Unknown(
                    "TypeError: object of type 'NoneType' has no len()".to_owned(),
                ));
            }
            queries::RawField::Text(text) => {
                // A string has a length and is iterable: each character is a
                // pk candidate and fails the UUID coercion.
                if text.is_empty() {
                    Vec::new()
                } else {
                    return Err(ViewError::Validation);
                }
            }
            queries::RawField::Other(value) => match value {
                plane_serde::pyjson::Value::List(items) => items
                    .iter()
                    .map(parse_uuid)
                    .collect::<Result<Vec<Uuid>, ViewError>>()?,
                plane_serde::pyjson::Value::Dict(object) => {
                    if object.iter().next().is_none() {
                        Vec::new()
                    } else {
                        return Err(ViewError::Validation);
                    }
                }
                plane_serde::pyjson::Value::Int(number) => {
                    if number.as_i64() == Some(0) {
                        Vec::new()
                    } else {
                        return Err(ViewError::Unknown(
                            "TypeError: object of type 'int' has no len()".to_owned(),
                        ));
                    }
                }
                _ => {
                    return Err(ViewError::Unknown(
                        "TypeError: object has no len()".to_owned(),
                    ));
                }
            },
        };
        if requested.is_empty() {
            return Ok(plain_error(400, "Sub Issue IDs are required"));
        }

        // `Issue.issue_objects.filter(id__in=…, workspace__slug=slug,
        // project_id=…)` (`:229-232`), iterated in `-created_at` order.
        let statement = queries::SUB_ISSUE_IDS.prepare(client).await?;
        let rows = client
            .query(&statement, &[&project_id, &slug, &requested])
            .await?;
        let scoped: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();

        // `Issue.objects.bulk_update(sub_issues, ["parent"], batch_size=10)`
        // (`:236`): `parent_id` only, no audit stamp, no `updated_at`.
        if !scoped.is_empty() {
            let statement = queries::SUB_ISSUE_SET_PARENT.prepare(client).await?;
            client.execute(&statement, &[&issue_id, &scoped]).await?;
        }

        // The activity per scoped id (`:243-257`). `requested_data` names the
        // URL parent; `current_instance` names the **sub-issue's own id** as
        // the parent, which is what the upstream code does.
        let requested_data = format!("{{\"parent\": \"{issue_id}\"}}");
        for sub_issue_id in &scoped {
            let current_instance = format!("{{\"parent\": \"{sub_issue_id}\"}}");
            let base = IssueActivityBase::new(
                "issue.activity.updated",
                TaskArg::json_text(requested_data.clone()),
                user.to_string(),
                TaskArg::str(sub_issue_id.to_string()),
                TaskArg::str(project_id.to_string()),
                TaskArg::json_text(current_instance),
                now.timestamp(),
            );
            let message =
                tasks::issue_activity(IssueActivityCall::view(base, queries::base_host(ctx)?));
            let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
            jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;
        }

        // `updated_sub_issues = Issue.issue_objects.filter(...)
        // .annotate(state_group=F("state__group"))` (`:238-240`).
        let statement = queries::SUB_ISSUES.prepare(client).await?;
        let rows = client
            .query(&statement, &[&project_id, &slug, &scoped])
            .await?;
        let mut updated: Vec<SubIssueRow> = Vec::with_capacity(rows.len());
        for row in &rows {
            let issue = Issue::from_row(row, 0)
                .map_err(|error| ViewError::Unknown(format!("decoding the issue: {error}")))?;
            updated.push(SubIssueRow {
                issue,
                state_group: row.get(Issue::COLUMN_COUNT),
            });
        }

        // `serializer = IssueSerializer(updated_sub_issues, many=True)`.
        let prepared = common::RequestRender::from_ctx(ctx);
        let serializers: Vec<IssueSerializer<'_>> = updated
            .iter()
            .map(|row| issue_serializer(&row.issue))
            .collect();
        let sub_issues = common::render_list(&serializers, &prepared.render_ctx())?;

        // `result = defaultdict(list)` (`:260-263`): insertion order, with a
        // `None` key rendered as `"null"` by `json.dumps`.
        let mut distribution: Vec<(String, Json)> = Vec::new();
        for row in &updated {
            let key = row.state_group.clone().unwrap_or_else(|| "null".to_owned());
            let id = Json::uuid(row.issue.id);
            match distribution
                .iter_mut()
                .find(|(existing, _)| *existing == key)
            {
                Some((_, Json::Array(items))) => items.push(id),
                Some((_, slot)) => *slot = Json::Array(vec![id]),
                None => distribution.push((key, Json::Array(vec![id]))),
            }
        }
        let distribution = Json::Object(distribution);

        let mut body = Vec::with_capacity(sub_issues.len() + 64);
        body.extend_from_slice(b"{\"sub_issues\":");
        body.extend_from_slice(&sub_issues);
        body.extend_from_slice(b",\"state_distribution\":");
        body.extend_from_slice(&distribution.to_drf());
        body.push(b'}');
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `IssueSerializer` over an `Issue` row. The declared fields that the
/// queryset does not annotate are `Absent`, which is exactly DRF's
/// `SkipField`: `cycle_id`, `module_ids`, `label_ids`, `assignee_ids`,
/// `sub_issues_count`, `attachment_count` and `link_count`.
fn issue_serializer(issue: &Issue) -> IssueSerializer<'_> {
    IssueSerializer {
        id: issue.id,
        name: &issue.name,
        state_id: issue.state_id,
        sort_order: issue.sort_order,
        completed_at: issue.completed_at,
        estimate_point: issue.estimate_point_id,
        priority: &issue.priority,
        start_date: issue.start_date,
        target_date: issue.target_date,
        sequence_id: i64::from(issue.sequence_id),
        project_id: issue.project_id,
        parent_id: issue.parent_id,
        cycle_id: FieldValue::Absent,
        module_ids: FieldValue::Absent,
        label_ids: FieldValue::Absent,
        assignee_ids: FieldValue::Absent,
        sub_issues_count: FieldValue::Absent,
        created_at: issue.created_at,
        updated_at: issue.updated_at,
        created_by: issue.created_by_id,
        updated_by: issue.updated_by_id,
        attachment_count: FieldValue::Absent,
        link_count: FieldValue::Absent,
        is_draft: issue.is_draft,
        archived_at: issue.archived_at,
    }
}

/// `UUIDField.get_prep_value`: a canonical string or a UUID.
fn parse_uuid(value: &plane_serde::pyjson::Value) -> Result<Uuid, ViewError> {
    match value {
        plane_serde::pyjson::Value::Str(text) => text
            .as_str()
            .and_then(|text| Uuid::parse_str(text).ok())
            .ok_or(ViewError::Validation),
        _ => Err(ViewError::Validation),
    }
}

/// `{"error": "…"}` with Django's rendering.
fn plain_error(status: u16, message: &str) -> ViewResponse {
    ViewResponse::json(status, serde_json::json!({ "error": message }))
}

/// The unused imports that keep the module's doc links honest.
#[allow(dead_code)]
fn _anchors(_: DateTime<Utc>, _: Option<NaiveDate>) {}
