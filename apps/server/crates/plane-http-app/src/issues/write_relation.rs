// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IssueRelationViewSet.create` and `remove_relation`
//! (`plane/app/views/issue/relation.py:246-352`).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/issue-relation/` | `create` | `:246-312` |
//! | `POST …/remove-relation/` | `remove_relation` | `:314-352` |
//!
//! ## create
//!
//! `relation_type` is required (`400 {"message": "Issue relation type is
//! required"}` — a `message` key, not `error`), the URL issue is bound with
//! `issue_in_project` (`Issue.issue_objects`, so an archived/draft/triage URL
//! issue is `404 {"error": "Issue not found"}`), and the body `issues` are
//! scoped to the **workspace** only — relations legitimately cross projects.
//!
//! `get_actual_relation` (`plane/utils/issue_relation_mapper.py:19-32`) stores
//! the reverse type: `start_after→start_before`, `finish_after→finish_before`,
//! `blocking→blocked_by`, and every other key unchanged (`implements` is *not*
//! mapped to `implemented_by` — only the reverse direction is). The stored row
//! is also **mirrored** for `blocking`/`start_after`/`finish_after`: the body
//! issue is `issue_id` and the URL issue is `related_issue_id`.
//!
//! `bulk_create(..., batch_size=10, ignore_conflicts=True)` bypasses
//! `BaseModel.save`, so there is no crum audit stamp — the view passes
//! `created_by`/`updated_by` explicitly — but Django's `pre_save` still fills
//! `created_at`/`updated_at`, and the returned list contains every object,
//! including rows `ON CONFLICT DO NOTHING` skipped.
//!
//! The response serializer depends on the *requested* type: `blocking`,
//! `start_after` and `finish_after` reply with `RelatedIssueSerializer` (the
//! body issues), everything else with `IssueRelationSerializer` (the URL
//! issue). Both read `issue.state.id`, which is an `AttributeError` → 500 for a
//! stateless issue.
//!
//! ## remove_relation
//!
//! The URL issue is bound the same way (404 `{"error": "Issue not found"}`),
//! then the relation is found **in either direction**: `Q(issue_id=related,
//! related_issue_id=issue) | Q(issue_id=issue, related_issue_id=related)` over
//! the workspace, `.first()` (newest first). A miss is
//! `404 {"error": "Issue relation not found"}` — the handler that replaced the
//! old `None.delete()` 500. `current_instance` is the serialized row (with the
//! *other* issue's fields), the row is soft-deleted, and the activity carries
//! `requested_data=json.dumps(request.data)`.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::hooks;
use plane_db::models::issue::IssueRelation;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_query::json::Json;
use uuid::Uuid;

use super::lists::list_common::{client, user_timezone};
use super::sub_resources::sub_common::{self as shared, issue_not_found_response};
use super::write_sub_queries as queries;
use crate::common::jobs;
use crate::write;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.relation.IssueRelationViewSet")?
        .action("create", create)?
        .action("remove_relation", remove_relation)?;
    Ok(())
}

/// `get_actual_relation` (`plane/utils/issue_relation_mapper.py:19-32`).
#[must_use]
pub fn get_actual_relation(relation_type: &str) -> &str {
    match relation_type {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocking" => "blocked_by",
        "blocked_by" => "blocked_by",
        "start_before" => "start_before",
        "finish_before" => "finish_before",
        "implemented_by" => "implemented_by",
        "implements" => "implemented_by",
        other => other,
    }
}

/// The three types whose stored row is mirrored and whose reply uses
/// `RelatedIssueSerializer` (`relation.py:283,304`).
fn mirrored(relation_type: &str) -> bool {
    matches!(relation_type, "blocking" | "start_after" | "finish_after")
}

/// The columns both reply serializers read off an issue
/// (`plane/app/serializers/issue.py:412-486`).
struct IssueRef {
    id: Uuid,
    project_id: Uuid,
    sequence_id: i64,
    name: String,
    state_id: Option<Uuid>,
    priority: String,
}

impl IssueRef {
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.get(0),
            project_id: row.get(1),
            sequence_id: i64::from(row.get::<_, i32>(2)),
            name: row.get(3),
            state_id: row.get(4),
            priority: row.get(5),
        })
    }
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_project(ctx)?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user = shared::user_id(ctx)?;
        let data = ctx.data()?;

        // `relation_type = request.data.get("relation_type", None)`; a missing
        // or null value is the view's own 400 (`:247-252`).
        let relation_type = match queries::raw_field(data, "relation_type") {
            queries::RawField::Absent | queries::RawField::Null => {
                return Ok(message_body(400, "Issue relation type is required"));
            }
            queries::RawField::Text(text) => text.to_owned(),
            queries::RawField::Other(other) => relation_type_string(other)?,
        };

        // `Project.objects.get(pk=project_id)` (`:259`).
        let workspace_id = project_workspace(client, project_id).await?;

        // `issue_in_project(issue_id, slug, project_id)` (`:265-266`).
        let statement = queries::ISSUE_LIVE_IN_PROJECT.prepare(client).await?;
        let live: bool = client
            .query_one(&statement, &[&issue_id, &project_id, &slug])
            .await?
            .get(0);
        if !live {
            return Ok(issue_not_found_response());
        }

        // `issues = list(Issue.issue_objects.filter(workspace__slug=slug,
        // pk__in=issues).values_list("id", flat=True))` (`:270-277`).
        let requested = requested_issue_ids(data)?;
        let statement = queries::RELATION_ISSUES.prepare(client).await?;
        let rows = client.query(&statement, &[&slug, &requested]).await?;
        let issues: Vec<IssueRef> = rows
            .iter()
            .map(IssueRef::decode)
            .collect::<Result<_, _>>()?;

        // `IssueRelation.objects.bulk_create([…], batch_size=10,
        // ignore_conflicts=True)` (`:279-302`).
        let now = write::now();
        let relation_type_stored = get_actual_relation(&relation_type).to_owned();
        let mut inserted: Vec<IssueRelation> = Vec::with_capacity(issues.len());
        for issue in &issues {
            let (row_issue, row_related) = if mirrored(&relation_type) {
                (issue.id, issue_id)
            } else {
                (issue_id, issue.id)
            };
            let relation = IssueRelation {
                created_at: now,
                updated_at: now,
                created_by_id: Some(user),
                updated_by_id: Some(user),
                deleted_at: None,
                id: Uuid::new_v4(),
                project_id,
                workspace_id,
                issue_id: row_issue,
                related_issue_id: row_related,
                relation_type: relation_type_stored.clone(),
            };
            let statement = queries::RELATION_INSERT.prepare(client).await?;
            client
                .execute(
                    &statement,
                    &[
                        &relation.created_at,
                        &relation.updated_at,
                        &relation.created_by_id,
                        &relation.updated_by_id,
                        &relation.id,
                        &relation.project_id,
                        &relation.workspace_id,
                        &relation.issue_id,
                        &relation.related_issue_id,
                        &relation.relation_type,
                    ],
                )
                .await?;
            inserted.push(relation);
        }

        // `requested_data=json.dumps({"relation_type": relation_type,
        // "issues": issues}, cls=DjangoJSONEncoder)` — the UUID list in the
        // queryset's order (`:305-315`).
        let requested_data = Json::object(vec![
            ("relation_type", Json::string(relation_type.as_str())),
            (
                "issues",
                Json::Array(issues.iter().map(|issue| Json::uuid(issue.id)).collect()),
            ),
        ]);
        publish_activity(
            ctx,
            "issue_relation.activity.created",
            TaskArg::json_text(queries::django_json(&requested_data)?),
            TaskArg::Null,
            now,
            issue_id,
            project_id,
            user,
        )
        .await?;

        // The reply (`:304-312`). Both serializers project the **body**
        // issue: for `blocking`/`start_after`/`finish_after` the row is
        // mirrored (`issue_id` = body issue, `RelatedIssueSerializer.id` =
        // `issue.id`), and for every other type `related_issue_id` is the body
        // issue (`IssueRelationSerializer.id` = `related_issue.id`). Only the
        // key order differs.
        //
        // `state_id` is `source="…state.id"`: on a stateless issue DRF's
        // `get_attribute` raises `AttributeError`, the field is not required
        // (read-only), so `Serializer.to_representation` raises `SkipField`
        // and the key is **omitted** — not null, not a 500.
        let timezone = user_timezone(ctx);
        let body = if mirrored(&relation_type) {
            let entries: Vec<Json> = inserted
                .iter()
                .zip(&issues)
                .map(|(relation, issue)| relation_issue_json(relation, issue, timezone, false))
                .collect::<Result<_, _>>()?;
            Json::Array(entries).to_drf()
        } else {
            let entries: Vec<Json> = inserted
                .iter()
                .zip(&issues)
                .map(|(relation, issue)| relation_issue_json(relation, issue, timezone, true))
                .collect::<Result<_, _>>()?;
            Json::Array(entries).to_drf()
        };
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// remove_relation
// ---------------------------------------------------------------------------

pub fn remove_relation<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_project(ctx)?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user = shared::user_id(ctx)?;
        let timezone = user_timezone(ctx);
        let data = ctx.data()?;

        // `related_issue = request.data.get("related_issue", None)` (`:315`).
        let related_issue = match queries::raw_field(data, "related_issue") {
            queries::RawField::Absent | queries::RawField::Null => None,
            queries::RawField::Text(text) => {
                Some(Uuid::parse_str(text).map_err(|_| ViewError::Validation)?)
            }
            queries::RawField::Other(_) => return Err(ViewError::Validation),
        };

        // `issue_in_project(issue_id, slug, project_id)` (`:324-325`).
        let statement = queries::ISSUE_LIVE_IN_PROJECT.prepare(client).await?;
        let live: bool = client
            .query_one(&statement, &[&issue_id, &project_id, &slug])
            .await?
            .get(0);
        if !live {
            return Ok(issue_not_found_response());
        }

        // The two-direction lookup (`:327-332`). A `None` related issue is
        // bound as NULL, which no row matches.
        let statement = queries::RELATION_LOOKUP.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&slug, &issue_id, &related_issue])
            .await?;
        let Some(row) = row else {
            return Ok(error_body(404, "Issue relation not found"));
        };
        let relation = IssueRelation::from_row(&row, 0)
            .map_err(|error| ViewError::Unknown(format!("decoding the relation: {error}")))?;
        let related = IssueRef::decode_relation(&row, IssueRelation::COLUMN_COUNT)?;

        // `current_instance = json.dumps(IssueRelationSerializer(
        // issue_relations).data)` (`:336`).
        let current_instance =
            queries::django_json(&relation_json(&relation, &related, timezone)?)?;

        // `issue_relations.delete()` — H03.
        let now = write::now();
        hooks::soft_delete::delete_instance(
            client,
            "issue_relations",
            "id",
            relation.id,
            Some(user),
            now,
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "issuerelation", relation.id).await?;

        // `requested_data=json.dumps(request.data)` (`:339-349`).
        publish_activity(
            ctx,
            "issue_relation.activity.deleted",
            queries::request_data_arg(ctx)?,
            TaskArg::json_text(current_instance),
            now,
            issue_id,
            project_id,
            user,
        )
        .await?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn kwarg_project(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    shared::kwarg_uuid(ctx, "project_id")
}

/// `{"message": "…"}` (`relation.py:249-252`).
fn message_body(status: u16, message: &str) -> ViewResponse {
    ViewResponse::json(status, serde_json::json!({ "message": message }))
}

/// `{"error": "…"}` in the DRF dialect.
fn error_body(status: u16, message: &str) -> ViewResponse {
    ViewResponse::json(status, serde_json::json!({ "error": message }))
}

/// `relation_type` as the string Django's `CharField.get_prep_value` would
/// store. A structured value is compared with `in` and then stored, so it is
/// left as an error the family handler renders as 500.
fn relation_type_string(value: &plane_serde::pyjson::Value) -> Result<String, ViewError> {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Bool(flag) => Ok(if *flag { "True" } else { "False" }.to_owned()),
        Py::Int(number) => Ok(number.as_i64().map_or_else(
            || plane_query::pyfloat::repr(number.to_string().parse().unwrap_or_default()),
            |value| value.to_string(),
        )),
        Py::Float(number) => Ok(plane_query::pyfloat::repr(*number)),
        Py::Null | Py::Str(_) | Py::List(_) | Py::Dict(_) => Err(ViewError::Unknown(
            "TypeError: unhashable relation_type".to_owned(),
        )),
    }
}

/// `Issue.issue_objects.filter(pk__in=issues)` argument coercion: every
/// element goes through `UUIDField.get_prep_value`, and a failure is Django's
/// `ValidationError` → 400 `{"error": "Please provide valid detail"}`.
fn requested_issue_ids(data: &Data) -> Result<Vec<Uuid>, ViewError> {
    use plane_serde::pyjson::Value as Py;
    match queries::raw_field(data, "issues") {
        queries::RawField::Absent => Ok(Vec::new()),
        queries::RawField::Text(_) => {
            // A string is iterable in Python, so each character is a pk
            // candidate and every one of them fails the UUID coercion.
            Err(ViewError::Validation)
        }
        queries::RawField::Null => Err(ViewError::Unknown(
            "TypeError: 'NoneType' object is not iterable".to_owned(),
        )),
        queries::RawField::Other(value) => match value {
            Py::List(items) => items.iter().map(parse_uuid).collect(),
            _ => Err(ViewError::Unknown(
                "TypeError: object is not iterable".to_owned(),
            )),
        },
    }
}

/// `UUIDField.get_prep_value` (`django/db/models/fields/__init__.py`): accepts
/// a UUID or its canonical string; anything else is `ValidationError`.
fn parse_uuid(value: &plane_serde::pyjson::Value) -> Result<Uuid, ViewError> {
    match value {
        plane_serde::pyjson::Value::Str(text) => text
            .as_str()
            .and_then(|text| Uuid::parse_str(text).ok())
            .ok_or(ViewError::Validation),
        _ => Err(ViewError::Validation),
    }
}

/// `Project.objects.get(pk=…).workspace_id`.
async fn project_workspace(client: &Client, project_id: Uuid) -> Result<Uuid, ViewError> {
    let statement = queries::PROJECT_WORKSPACE.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(row.get(0))
}

impl IssueRef {
    fn decode_relation(row: &tokio_postgres::Row, start: usize) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.get(start),
            project_id: row.get(start + 1),
            sequence_id: i64::from(row.get::<_, i32>(start + 2)),
            name: row.get(start + 3),
            state_id: row.get(start + 4),
            priority: row.get(start + 5),
        })
    }
}

/// `IssueRelationSerializer(relation).data` in `_readable_fields` order.
/// One reply row. The `relation_side` flag swaps `updated_at`/`updated_by`
/// (the only difference between `IssueRelationSerializer` and
/// `RelatedIssueSerializer`), and `state_id` is dropped when the issue has no
/// state — DRF's `SkipField`.
fn relation_issue_json(
    relation: &IssueRelation,
    issue: &IssueRef,
    timezone: Tz,
    relation_side: bool,
) -> Result<Json, ViewError> {
    let mut entries: Vec<(&str, Json)> = vec![
        ("id", Json::uuid(issue.id)),
        ("project_id", Json::uuid(issue.project_id)),
        ("sequence_id", Json::Int(issue.sequence_id)),
        (
            "relation_type",
            Json::string(relation.relation_type.as_str()),
        ),
        ("name", Json::string(issue.name.as_str())),
    ];
    if let Some(state_id) = issue.state_id {
        entries.push(("state_id", Json::uuid(state_id)));
    }
    entries.push(("priority", Json::string(issue.priority.as_str())));
    entries.push(("created_by", Json::opt_uuid(relation.created_by_id)));
    entries.push((
        "created_at",
        queries::drf_datetime(relation.created_at, timezone),
    ));
    if relation_side {
        entries.push((
            "updated_at",
            queries::drf_datetime(relation.updated_at, timezone),
        ));
        entries.push(("updated_by", Json::opt_uuid(relation.updated_by_id)));
    } else {
        entries.push(("updated_by", Json::opt_uuid(relation.updated_by_id)));
        entries.push((
            "updated_at",
            queries::drf_datetime(relation.updated_at, timezone),
        ));
    }
    Ok(Json::object(entries))
}

/// `IssueRelationSerializer(relation).data` for `remove_relation`'s
/// `current_instance`, in `_readable_fields` order (`id` … `updated_by`).
fn relation_json(
    relation: &IssueRelation,
    related: &IssueRef,
    timezone: Tz,
) -> Result<Json, ViewError> {
    relation_issue_json(relation, related, timezone, true)
}

#[allow(clippy::too_many_arguments)]
async fn publish_activity(
    ctx: &ViewCtx<'_>,
    activity_type: &str,
    requested_data: TaskArg,
    current_instance: TaskArg,
    epoch: DateTime<Utc>,
    issue_id: Uuid,
    project_id: Uuid,
    user: Uuid,
) -> Result<(), ViewError> {
    let base = IssueActivityBase::new(
        activity_type,
        requested_data,
        user.to_string(),
        TaskArg::str(issue_id.to_string()),
        TaskArg::str(project_id.to_string()),
        current_instance,
        epoch.timestamp(),
    );
    let message = tasks::issue_activity(IssueActivityCall::view(base, queries::base_host(ctx)?));
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;
    Ok(())
}
