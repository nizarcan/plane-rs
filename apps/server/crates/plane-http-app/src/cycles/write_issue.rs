// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `CycleIssueViewSet.create` and `destroy` (routes 33 and 34;
//! `plane/app/views/cycle/issue.py:223-356`).
//!
//! ## create (`:223-311`)
//!
//! 1. `issues = request.data.get("issues", [])`; a falsy value (missing,
//!    `null`, `[]`, `""`) is `400 {"error": "Issues are required"}`. A
//!    non-list is coerced by `issue_id__in` (a string iterates its characters)
//!    and fails the UUID coercion → 400 `Please provide valid detail`, or is a
//!    `TypeError` → 500.
//! 2. `Cycle.objects.get(workspace__slug, project_id, pk)` — the soft-delete
//!    manager, no archived filter; a miss is the family 404.
//! 3. A cycle whose `end_date` is in the past rejects the add:
//!    `400 {"error": "The Cycle has already been completed so no new issues can
//!    be added"}`.
//! 4. Existing `CycleIssue` rows for the requested issues **in another cycle**
//!    are moved here; issues already in *this* cycle stay in `new_issues`
//!    because the lookup excludes this cycle, so their INSERT hits the partial
//!    unique constraint. `bulk_create` has no `ignore_conflicts` and no
//!    `except IntegrityError`, so that is the family 400 — the upstream
//!    re-add bug, ported as-is.
//! 5. `bulk_create(..., batch_size=10)` (one all-or-nothing atomic) writes the
//!    new rows with `created_by_id`/`updated_by_id` = the request user (bulk
//!    bypasses H01) and `workspace_id = cycle.workspace_id`;
//!    `bulk_update(rows, ["cycle_id"], batch_size=100)` moves the existing ones
//!    (FK only, no `updated_at`).
//! 6. One `cycle.activity.created` with `requested_data =
//!    {"cycles_list": <raw body list>}` and `current_instance =
//!    {"updated_cycle_issues": [...], "created_cycle_issues":
//!    serializers.serialize("json", created)}`; 201 `{"message": "success"}`.
//!
//! The created list is in `Issue.issue_objects`' `-created_at` order (the set
//! difference is immediately re-ordered by the issue query), so the payload is
//! deterministic regardless of `PYTHONHASHSEED`.
//!
//! ## destroy (`:313-356`)
//!
//! Publishes `cycle.activity.deleted` **before** the H04 delete (a bare
//! `deleted_at = now`, no audit/cascade), and answers 204 even when no row
//! matched. `PUT`/`PATCH` are refused by the viewset guard (405).

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::hooks::issue_m2m::{MembershipRow, insert_membership};
use plane_db::pool::DbError;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_query::json::Json;
use uuid::Uuid;

use super::base::{error_body, uuid_kwarg};
use super::write_issue_queries as queries_write;
use crate::common::jobs;
use crate::write;
use crate::write::django_serialize::{
    DjangoField, DjangoObject, DjangoValue, django_json, django_serialize,
};

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.cycle.issue.CycleIssueViewSet")?
        .action("create", create)?
        .action("destroy", destroy)?;
    Ok(())
}

/// `Cycle.objects.get(...)` plus the completed guard (`:229-237`).
struct CycleForIssues {
    id: Uuid,
    workspace_id: Uuid,
    end_date: Option<DateTime<Utc>>,
}

async fn cycle_for_issues(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    cycle_id: Uuid,
) -> Result<CycleForIssues, ViewError> {
    let statement = queries_write::CYCLE_FOR_ISSUES.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&slug, &project_id, &cycle_id])
        .await?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Ok(CycleForIssues {
        id: row.get(0),
        end_date: row.get(1),
        workspace_id: row.get(2),
    })
}

pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = crate::common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let user = jobs::user_id(ctx)?;
        let now = write::now();
        let data = ctx.data()?;

        // `issues = request.data.get("issues", [])`; `if not issues` (`:225-228`).
        let raw_issues = match raw_field(data, "issues") {
            RawField::Absent | RawField::Null => {
                return Ok(error_body(400, "Issues are required"));
            }
            RawField::Text(text) => {
                if text.is_empty() {
                    return Ok(error_body(400, "Issues are required"));
                }
                // A non-empty string is truthy; `issue_id__in` iterates its
                // characters and every one fails the UUID coercion.
                return Err(ViewError::Validation);
            }
            RawField::Other(value) => value,
        };
        let Some(issues) = request_issues(raw_issues)? else {
            return Ok(error_body(400, "Issues are required"));
        };

        let cycle = cycle_for_issues(client, &slug, project_id, cycle_id).await?;

        // `if cycle.end_date is not None and cycle.end_date < timezone.now()`.
        if cycle.end_date.is_some_and(|end| end < now) {
            return Ok(error_body(
                400,
                "The Cycle has already been completed so no new issues can be added",
            ));
        }

        // Rows of the requested issues in another cycle (`:243-249`), in
        // `-created_at` order.
        let statement = queries_write::EXISTING_CYCLE_ISSUES.prepare(client).await?;
        let rows = client
            .query(&statement, &[&cycle_id, &issues, &project_id, &slug])
            .await?;
        let mut existing: Vec<(Uuid, Uuid, Uuid)> = Vec::with_capacity(rows.len());
        for row in &rows {
            existing.push((row.get(0), row.get(1), row.get(2)));
        }

        // `new_issues = set(issues) - set(existing_issues)`, then re-ordered by
        // the issue query (`:251-261`).
        let existing_issue_ids: std::collections::HashSet<Uuid> =
            existing.iter().map(|(_, _, issue)| *issue).collect();
        let candidates: Vec<Uuid> = {
            let mut seen = std::collections::HashSet::new();
            issues
                .iter()
                .copied()
                .filter(|issue| !existing_issue_ids.contains(issue) && seen.insert(*issue))
                .collect()
        };
        let statement = queries_write::NEW_ISSUE_IDS.prepare(client).await?;
        let rows = client
            .query(&statement, &[&project_id, &slug, &candidates])
            .await?;
        let new_issues: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();

        // `bulk_create(..., batch_size=10)` with no `ignore_conflicts` (`:272-284`).
        let created: Vec<CycleIssueRow> = new_issues
            .iter()
            .map(|issue_id| CycleIssueRow {
                id: Uuid::new_v4(),
                created_at: now,
                updated_at: now,
                created_by_id: Some(user),
                updated_by_id: Some(user),
                project_id,
                workspace_id: cycle.workspace_id,
                issue_id: *issue_id,
                cycle_id: cycle.id,
            })
            .collect();
        let membership: Vec<MembershipRow> = created
            .iter()
            .map(|row| MembershipRow {
                id: row.id,
                project_id: row.project_id,
                workspace_id: row.workspace_id,
                created_by_id: row.created_by_id,
                updated_by_id: row.updated_by_id,
                issue_id: row.issue_id,
                other_id: row.cycle_id,
            })
            .collect();
        insert_membership(client, "cycle_issues", "cycle_id", &membership, now)
            .await
            .map_err(map_membership_error)?;

        // `bulk_update(rows, ["cycle_id"], batch_size=100)` (`:303`): all rows
        // move to the URL cycle, FK only.
        if !existing.is_empty() {
            let ids: Vec<Uuid> = existing.iter().map(|(id, _, _)| *id).collect();
            let statement = queries_write::MOVE_CYCLE_ISSUES.prepare(client).await?;
            client.execute(&statement, &[&cycle_id, &ids]).await?;
        }

        // The activity (`:305-319`).
        let updated: Vec<Json> = existing
            .iter()
            .map(|(_, old_cycle_id, issue_id)| {
                Json::object(vec![
                    ("old_cycle_id", Json::uuid(*old_cycle_id)),
                    ("new_cycle_id", Json::uuid(cycle_id)),
                    ("issue_id", Json::uuid(*issue_id)),
                ])
            })
            .collect();
        let created_json = django_serialize(
            &created
                .iter()
                .map(CycleIssueRow::django_object)
                .collect::<Vec<_>>(),
        )?;
        let current_instance = Json::object(vec![
            ("updated_cycle_issues", Json::Array(updated)),
            ("created_cycle_issues", Json::string(created_json)),
        ]);
        let requested_data = Json::object(vec![("cycles_list", pyjson_to_json(raw_issues))]);
        publish_activity(
            ctx,
            "cycle.activity.created",
            TaskArg::json_text(django_json(&requested_data)?),
            Some(TaskArg::json_text(django_json(&current_instance)?)),
            None,
            project_id,
            user,
            now,
        )
        .await?;

        Ok(plain_message(201, "success"))
    })
}

pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = crate::common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let user = jobs::user_id(ctx)?;
        let now = write::now();

        let requested_data = Json::object(vec![
            ("cycle_id", Json::uuid(cycle_id)),
            ("issues", Json::Array(vec![Json::uuid(issue_id)])),
        ]);
        publish_activity(
            ctx,
            "cycle.activity.deleted",
            TaskArg::json_text(django_json(&requested_data)?),
            None,
            Some(issue_id),
            project_id,
            user,
            now,
        )
        .await?;

        // `cycle_issue.delete()` — H04 (`:355`).
        let statement = queries_write::SOFT_DELETE_CYCLE_ISSUE
            .prepare(client)
            .await?;
        client
            .execute(
                &statement,
                &[&now, &issue_id, &project_id, &cycle_id, &slug],
            )
            .await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// One inserted `CycleIssue` as the activity's Django serializer renders it.
struct CycleIssueRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    project_id: Uuid,
    workspace_id: Uuid,
    issue_id: Uuid,
    cycle_id: Uuid,
}

impl CycleIssueRow {
    /// `_meta.local_fields` order with `db.cycleissue`'s model label.
    fn django_object(&self) -> DjangoObject<'_> {
        DjangoObject {
            model: "db.cycleissue",
            pk: self.id,
            fields: vec![
                DjangoField {
                    name: "created_at",
                    value: DjangoValue::DateTime(self.created_at),
                },
                DjangoField {
                    name: "updated_at",
                    value: DjangoValue::DateTime(self.updated_at),
                },
                DjangoField {
                    name: "created_by",
                    value: DjangoValue::OptUuid(self.created_by_id),
                },
                DjangoField {
                    name: "updated_by",
                    value: DjangoValue::OptUuid(self.updated_by_id),
                },
                DjangoField {
                    name: "deleted_at",
                    value: DjangoValue::OptDateTime(None),
                },
                DjangoField {
                    name: "project",
                    value: DjangoValue::Uuid(self.project_id),
                },
                DjangoField {
                    name: "workspace",
                    value: DjangoValue::Uuid(self.workspace_id),
                },
                DjangoField {
                    name: "issue",
                    value: DjangoValue::Uuid(self.issue_id),
                },
                DjangoField {
                    name: "cycle",
                    value: DjangoValue::Uuid(self.cycle_id),
                },
            ],
        }
    }
}

/// `request.data.get(name)` with the JSON shape kept (see set C's identical
/// helper: `Data::get` only returns strings).
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

/// `issues` as the list of `UUID`s `issue_id__in` coerces; `None` means the
/// value is empty (the view's 400), `Err` is Django's `ValidationError`.
fn request_issues(value: &plane_serde::pyjson::Value) -> Result<Option<Vec<Uuid>>, ViewError> {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::List(items) => {
            if items.is_empty() {
                return Ok(None);
            }
            items
                .iter()
                .map(|item| match item {
                    Py::Str(text) => text
                        .as_str()
                        .and_then(|text| Uuid::parse_str(text).ok())
                        .ok_or(ViewError::Validation),
                    // `UUIDField.to_python(int)` is `uuid.UUID(int=value)`.
                    Py::Int(number) => number
                        .as_i64()
                        .and_then(|value| u128::try_from(value).ok())
                        .map(Uuid::from_u128)
                        .ok_or(ViewError::Validation),
                    _ => Err(ViewError::Validation),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        }
        Py::Dict(object) => {
            if object.iter().next().is_none() {
                Ok(None)
            } else {
                Err(ViewError::Validation)
            }
        }
        Py::Int(number) => {
            if number.as_i64() == Some(0) {
                Ok(None)
            } else {
                Err(ViewError::Unknown(
                    "TypeError: 'int' object is not iterable".to_owned(),
                ))
            }
        }
        _ => Err(ViewError::Unknown(
            "TypeError: object is not iterable".to_owned(),
        )),
    }
}

fn map_membership_error(error: DbError) -> ViewError {
    match error {
        DbError::Postgres(error)
            if error.code() == Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION) =>
        {
            // `bulk_create`'s IntegrityError, uncaught by the view.
            ViewError::Integrity
        }
        other => ViewError::Unknown(other.to_string()),
    }
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

/// `{"message": "success"}` (DRF renders it compact).
fn plain_message(status: u16, message: &str) -> ViewResponse {
    let body = Json::object(vec![("message", Json::string(message))]);
    ViewResponse::raw(status, APPLICATION_JSON, body.to_drf())
}

/// `issue_activity.delay(...)` in the View shape (`notification=True`,
/// `origin` last). `issue_id` is `None` on create and the issue string on
/// delete, as at the two call sites.
#[allow(clippy::too_many_arguments)]
async fn publish_activity(
    ctx: &ViewCtx<'_>,
    activity_type: &str,
    requested_data: TaskArg,
    current_instance: Option<TaskArg>,
    issue_id: Option<Uuid>,
    project_id: Uuid,
    user: Uuid,
    epoch: DateTime<Utc>,
) -> Result<(), ViewError> {
    let base = IssueActivityBase::new(
        activity_type,
        requested_data,
        user.to_string(),
        issue_id.map_or(TaskArg::Null, |id| TaskArg::str(id.to_string())),
        TaskArg::str(project_id.to_string()),
        current_instance.unwrap_or(TaskArg::Null),
        epoch.timestamp(),
    );
    let message = tasks::issue_activity(IssueActivityCall::view(
        base,
        super::write_issue_queries::base_host(ctx)?,
    ));
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;
    Ok(())
}
