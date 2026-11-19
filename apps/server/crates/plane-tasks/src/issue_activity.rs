// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/issue_activities_task.py` — the issue activity engine.
//!
//! `issue_activity` (`:1550-1650`) takes a JSON-string `requested_data` and
//! `current_instance`, dispatches on `type` to one of 27 builders
//! (`:1586-1614`), `bulk_create`s every row a builder appended, optionally
//! enqueues `notifications` with the serialised created rows, and returns
//! `None`. It also bumps `Issue.updated_at` and stores the request origin in
//! Redis under the issue id (`:1573-1584`).
//!
//! # Ported Python details that are easy to "fix" by accident
//!
//! * **`is_valid_uuid` accepts version 4 only** (`plane/utils/uuid.py:9-14`).
//!   The task returns before doing anything when `str(project_id)` is not a v4
//!   UUID (`:1567`), so a v3/v5 id is a silent no-op (`PROGRESS.md:594`).
//! * **The outer `except Exception`** (`:1648-1650`) discards the whole
//!   `issue_activities` list when a builder raises, and skips the
//!   `notifications` publish. It also logs and returns `None`, i.e. the Celery
//!   task succeeds. The only partial effect that survives is what earlier
//!   statements committed: the `Issue.updated_at` bump, the immediate
//!   `IssueActivity.objects.create` of `create_issue_activity`, the Redis key,
//!   and the per-issue bumps of the cycle/module builders.
//! * **`create_issue_activity` inserts its row before parsing
//!   `requested_data`** (`:599-612`): a `None` payload leaves the "created the
//!   issue" row behind and then aborts. It also passes the *unparsed*
//!   `current_instance` to `track_assignees` (`:614-623`), so a JSON-string
//!   `current_instance` (as opposed to `None`) is a `'str' object has no
//!   attribute 'get'` abort.
//! * **Removing an estimate point always aborts** (`:501`: `field =
//!   "estimate_" + new_estimate.estimate.type` runs in the removal branch too,
//!   where `new_estimate` is `None`).
//! * **`track_closed_to` uses `State.objects.get`** (`:570`), not `filter`:
//!   a missing/soft-deleted/triage state aborts every activity of the call.
//! * **`create_comment_reaction_activity` unpacks `None`** (`:1191-1199`) when
//!   no reaction row matches, and `IssueComment.objects.get` raises when the
//!   comment is gone: both abort the call.
//! * **`track_description` refreshes the previous activity instead of adding
//!   one** when the newest activity is a `description` row by the same actor
//!   (`:89-96`); `save(update_fields=["created_at"])` leaves `updated_at`
//!   untouched.
//! * **`Issue.save(update_fields=["updated_at"])`** runs `Issue`'s overrides
//!   (`plane/db/models/issue.py:180-255`). For a row with a state it writes
//!   only `updated_at`; with a NULL state `_ensure_default_state` assigns the
//!   project's default state and `_sync_completed_at` adds `completed_at` to
//!   the update fields (which then writes `completed_at`, not `state_id`).
//! * **`bulk_create`** bypasses `BaseModel.save`, so `created_by_id` and
//!   `updated_by_id` are NULL even though the builders pass an actor
//!   (`plane/db/models/base.py:23-44`); `attachments` defaults to `[]`.
//! * **`notifications` receives the original kwargs unchanged** (`:1632-1645`):
//!   `issue_activities_created` is `json.dumps(IssueActivitySerializer(...).data,
//!   cls=DjangoJSONEncoder)` over the *bulk-created* rows only (not the immediate
//!   `create_issue_activity` row), and is published even when the list is empty
//!   or `type` is unknown.
//! * **Set order.** `extract_ids` returns a Python `set` (`:41-46`), so label
//!   and assignee activities are appended in CPython's hash order, which is
//!   random per process (`PYTHONHASHSEED`). The handler emits them sorted; the
//!   differential harness compares those rows as a multiset, as it does for
//!   independently generated primary keys (`DECISIONS.md` D26).
//!
//! # The notifications payload
//!
//! `notifications`' `issue_activities_created` is the DRF `ModelSerializer`
//! output for `IssueActivity` with `fields = "__all__"`:
//! `id, actor_detail, issue_detail, project_detail, workspace_detail,
//! source_data`, then the model fields in `_meta` order, then the forward
//! relations (`app/serializers/issue.py:343-361`, verified against the pinned
//! DRF). Nested serialisers are ported field for field; `source_data` is
//! always `null` because `Issue` has no `source_data` attribute in this
//! revision. `json.dumps(..., cls=DjangoJSONEncoder)` is
//! `plane_serde::Dialect::Django`.
//!
//! # Published `notifications` arguments
//!
//! `Job::from_message` stores Python runtime values, so a `uuid.UUID`
//! argument is a string in the queued payload. It also records the UUID-typed
//! arguments in `uuid_kwargs`; this handler uses that hint to recreate
//! kombu's `{"__type__": "uuid"}` envelope when it re-publishes
//! `notifications`, exactly like Django passing the object through. An
//! authored `difftest task` payload has no hint, so a string stays a string —
//! which is what Django's `task.apply()` receives in that case.
//!
//! # Known differences, all deliberate
//!
//! * `subscriber` is passed to `notifications` as a bool from its truthiness.
//!   Every call site passes a bool; a non-bool would keep its Python value in
//!   Django's message.
//! * `origin` must be a string (all sites pass `base_host(...)`). redis-py
//!   also accepts ints and floats; those would reach `SET` as their decimal
//!   text in Python.
//! * `update_issue_activity` iterates the parsed `requested_data` object.
//!   Python's `for key in requested_data` also iterates a list or a string
//!   (every dispatched key then aborts the builder at `.get`); no producer
//!   sends those shapes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use deadpool_postgres::Client;
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, NotificationsCall};
use plane_jobs::queue::Job;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_serde::pyjson::{self, PyDict, PyStr, Value as PyValue};
use serde_json::Value;
use uuid::Uuid;

use crate::{TaskContext, publish_or_enqueue, swallow};

/// `plane.bgtasks.issue_activities_task.issue_activity`.
pub const ISSUE_ACTIVITY: &str = "plane.bgtasks.issue_activities_task.issue_activity";
/// The task this handler publishes when `notification=True`.
pub const NOTIFICATIONS: &str = "plane.bgtasks.notification_task.notifications";

/// The tasks this module registers.
pub const TASKS: &[&str] = &[ISSUE_ACTIVITY];

/// Register the `issue_activity` handler.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    registry.register(ISSUE_ACTIVITY, move |job| {
        handle_issue_activity(Arc::clone(&context), job)
    });
}

/// Django's `except Exception: log_exception(e); return` (`:1648-1650`).
async fn handle_issue_activity(
    context: Arc<TaskContext>,
    job: JobContext,
) -> Result<(), HandlerError> {
    if let Err(error) = run(&context, &job.client, &job.payload).await {
        swallow(error, ISSUE_ACTIVITY);
    }
    Ok(())
}

/// One failure the Python body would catch with `except Exception`.
#[derive(Debug)]
enum Failure {
    /// A Python-level exception (`AttributeError`, `TypeError`, a DRF/Django
    /// `ValidationError`, …). `Display` carries the message into the log.
    Python(String),
    /// A database error, including the constraint violations Django lets
    /// propagate out of `bulk_create`.
    Db(tokio_postgres::Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Python(message) => formatter.write_str(message),
            Self::Db(error) => write!(formatter, "{error}"),
        }
    }
}

impl From<tokio_postgres::Error> for Failure {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Db(error)
    }
}

impl From<pyjson::LoadError> for Failure {
    fn from(error: pyjson::LoadError) -> Self {
        Self::Python(format!("json.loads: {}", error.message()))
    }
}

impl From<plane_cache::Error> for Failure {
    fn from(error: plane_cache::Error) -> Self {
        Self::Python(format!("redis: {error}"))
    }
}

type R<T> = Result<T, Failure>;

fn python<T>(message: impl Into<String>) -> R<T> {
    Err(Failure::Python(message.into()))
}

fn db(error: tokio_postgres::Error) -> Failure {
    Failure::Db(error)
}

// ---------------------------------------------------------------------------
// The task body
// ---------------------------------------------------------------------------

struct Request {
    activity_type: String,
    requested_data: Option<PyValue>,
    current_instance: Option<PyValue>,
    issue_id: Option<PyValue>,
    actor_id: Option<PyValue>,
    project_id: Option<PyValue>,
    epoch: Option<PyValue>,
    subscriber: bool,
    notification: bool,
    origin: Option<PyValue>,
}

async fn run(context: &TaskContext, client: &Client, payload: &Value) -> R<()> {
    let Some(kwargs) = crate::kwargs(payload) else {
        return python("payload has no kwargs object");
    };
    // `Job::from_message` records which kwargs were `uuid.UUID` objects; the
    // re-published `notifications` message must keep kombu's envelope for
    // them (`DECISIONS.md` D25: a handler reproduces every task it publishes).
    let uuid_kwargs: BTreeSet<String> = payload
        .get("uuid_kwargs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let request = Request {
        activity_type: match kwargs.get("type") {
            Some(Value::String(text)) => text.clone(),
            Some(other) => return python(format!("unhashable activity type: {other}")),
            None => return python("missing kwarg: type"),
        },
        requested_data: kwargs.get("requested_data").map(serde_to_py),
        current_instance: kwargs.get("current_instance").map(serde_to_py),
        issue_id: kwargs.get("issue_id").map(serde_to_py),
        actor_id: kwargs.get("actor_id").map(serde_to_py),
        project_id: kwargs.get("project_id").map(serde_to_py),
        epoch: kwargs.get("epoch").map(serde_to_py),
        subscriber: kwargs
            .get("subscriber")
            .is_none_or(|value| serde_truthy(Some(value))),
        notification: serde_truthy(kwargs.get("notification")),
        origin: kwargs.get("origin").map(serde_to_py),
    };

    // `if not is_valid_uuid(str(project_id)): return` (`:1567-1568`).
    let project_text = py_str_opt(request.project_id.as_ref());
    if !is_valid_uuid(&project_text) {
        return Ok(());
    }
    let Some(project_id) = parse_uuid_text(&project_text) else {
        return Ok(());
    };
    let Some(project) = load_project(client, project_id).await? else {
        // `Project.objects.get(pk=project_id)` → DoesNotExist.
        return python("Project matching query does not exist.");
    };

    let mut engine = Engine {
        client,
        project,
        issue_id: request.issue_id.clone(),
        actor_id: request.actor_id.clone(),
        epoch: request.epoch.clone(),
        rows: Vec::new(),
    };

    // `if issue_id is not None:` (`:1573-1584`).
    if let Some(issue_value) = &request.issue_id
        && !issue_value.is_null()
    {
        if py_truthy(request.origin.as_ref()) {
            let Some(redis) = &context.redis else {
                return python("redis is not configured for this worker");
            };
            let key = py_str(issue_value);
            let origin = match request.origin.as_ref() {
                Some(PyValue::Str(text)) => text.to_string_lossy().into_owned(),
                Some(other) => return python(format!("redis value is not a string: {other:?}")),
                None => String::new(),
            };
            redis.set_bytes_ex(&key, origin.as_bytes(), 600).await?;
        }
        let issue_uuid = python_uuid(issue_value)?;
        if let Some(issue) = load_issue_for_bump(client, issue_uuid, Some(project_id)).await? {
            bump_issue(client, &issue).await?;
        }
    }

    // `ACTIVITY_MAPPER.get(type)` (`:1586-1616`); an unknown type is a no-op
    // builder that still reaches `bulk_create([])` and the notification.
    engine.build(&request).await?;

    let inserted = engine.insert(client).await?;

    if request.notification {
        let created = serialize_activities(client, &inserted).await?;
        let encoded = plane_serde::to_vec(plane_serde::Dialect::Django, &created)
            .map_err(|error| Failure::Python(format!("json.dumps: {error}")))?;
        let encoded = String::from_utf8(encoded)
            .map_err(|error| Failure::Python(format!("json.dumps: {error}")))?;
        let message = tasks::notifications(NotificationsCall {
            activity_type: request.activity_type.clone(),
            issue_id: task_arg_typed(request.issue_id.as_ref(), uuid_kwargs.contains("issue_id")),
            actor_id: task_arg_typed(request.actor_id.as_ref(), uuid_kwargs.contains("actor_id")),
            project_id: task_arg_typed(
                request.project_id.as_ref(),
                uuid_kwargs.contains("project_id"),
            ),
            subscriber: request.subscriber,
            issue_activities_created: TaskArg::json_text(encoded),
            requested_data: task_arg(request.requested_data.as_ref()),
            current_instance: task_arg(request.current_instance.as_ref()),
        });
        let job = Job::from_message(&message);
        let task = message.task().to_string();
        publish_or_enqueue(context, &***client, &task, &message, job)
            .await
            .map_err(|error| Failure::Python(error.to_string()))?;
    }
    Ok(())
}

/// `ACTIVITY_MAPPER` (`:1586-1614`).
async fn dispatch(engine: &mut Engine<'_>, request: &Request) -> R<()> {
    match request.activity_type.as_str() {
        "issue.activity.created" => {
            create_issue_activity(engine, &request.requested_data, &request.current_instance).await
        }
        "issue.activity.updated" => {
            update_issue_activity(engine, &request.requested_data, &request.current_instance).await
        }
        "issue.activity.deleted" => delete_issue_activity(engine),
        "comment.activity.created" => {
            create_comment_activity(engine, &request.requested_data, &request.current_instance)
        }
        "comment.activity.updated" => {
            update_comment_activity(engine, &request.requested_data, &request.current_instance)
        }
        "comment.activity.deleted" => delete_comment_activity(engine, &request.requested_data),
        "cycle.activity.created" => {
            create_cycle_issue_activity(engine, &request.requested_data, &request.current_instance)
                .await
        }
        "cycle.activity.deleted" => {
            delete_cycle_issue_activity(engine, &request.requested_data).await
        }
        "module.activity.created" => {
            create_module_issue_activity(engine, &request.requested_data).await
        }
        "module.activity.deleted" => {
            delete_module_issue_activity(engine, &request.requested_data, &request.current_instance)
                .await
        }
        "link.activity.created" => {
            create_link_activity(engine, &request.requested_data, &request.current_instance)
        }
        "link.activity.updated" => {
            update_link_activity(engine, &request.requested_data, &request.current_instance)
        }
        "link.activity.deleted" => delete_link_activity(engine, &request.current_instance),
        "attachment.activity.created" => {
            create_attachment_activity(engine, &request.requested_data, &request.current_instance)
        }
        "attachment.activity.deleted" => delete_attachment_activity(engine),
        "issue_relation.activity.created" => {
            create_issue_relation_activity(
                engine,
                &request.requested_data,
                &request.current_instance,
            )
            .await
        }
        "issue_relation.activity.deleted" => {
            delete_issue_relation_activity(
                engine,
                &request.requested_data,
                &request.current_instance,
            )
            .await
        }
        "issue_reaction.activity.created" => {
            create_issue_reaction_activity(engine, &request.requested_data).await
        }
        "issue_reaction.activity.deleted" => {
            delete_issue_reaction_activity(engine, &request.current_instance)
        }
        "comment_reaction.activity.created" => {
            create_comment_reaction_activity(engine, &request.requested_data).await
        }
        "comment_reaction.activity.deleted" => {
            delete_comment_reaction_activity(engine, &request.current_instance).await
        }
        "issue_vote.activity.created" => {
            create_issue_vote_activity(engine, &request.requested_data)
        }
        "issue_vote.activity.deleted" => {
            delete_issue_vote_activity(engine, &request.current_instance)
        }
        "issue_draft.activity.created" => create_draft_issue_activity(engine),
        "issue_draft.activity.updated" => {
            update_draft_issue_activity(engine, &request.requested_data, &request.current_instance)
        }
        "issue_draft.activity.deleted" => delete_draft_issue_activity(engine),
        "intake.activity.created" => {
            create_intake_activity(engine, &request.requested_data, &request.current_instance)
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// The engine: the issue context and the pending rows
// ---------------------------------------------------------------------------

/// `workspace_id = project.workspace_id` plus the task's common arguments.
struct Engine<'a> {
    client: &'a Client,
    project: ProjectRow,
    issue_id: Option<PyValue>,
    actor_id: Option<PyValue>,
    epoch: Option<PyValue>,
    rows: Vec<Row>,
}

impl Engine<'_> {
    async fn build(&mut self, request: &Request) -> R<()> {
        dispatch(self, request).await
    }

    async fn insert(&self, client: &Client) -> R<Vec<Inserted>> {
        insert_rows(client, &self.rows).await
    }

    fn workspace(&self) -> Uuid {
        self.project.workspace_id
    }

    fn project_id(&self) -> Uuid {
        self.project.id
    }

    /// `issue_id=` as the UUID field will coerce it, or an error when Django's
    /// `UUIDField.to_python` would raise (`bulk_create` then writes nothing).
    fn issue(&self) -> R<Option<Uuid>> {
        match &self.issue_id {
            None | Some(PyValue::Null) => Ok(None),
            Some(value) => Ok(Some(python_uuid(value)?)),
        }
    }

    fn actor(&self) -> R<Option<Uuid>> {
        match &self.actor_id {
            None | Some(PyValue::Null) => Ok(None),
            Some(value) => Ok(Some(python_uuid(value)?)),
        }
    }

    /// A complete `IssueActivity(...)` with the common fields filled in.
    fn activity(&self, issue_id: Option<Uuid>, verb: &str, field: Option<&str>) -> R<Activity> {
        Ok(Activity {
            issue_id,
            verb: verb.to_string(),
            field: field.map(str::to_owned),
            old_value: None,
            new_value: None,
            comment: String::new(),
            issue_comment_id: None,
            actor_id: self.actor()?,
            old_identifier: None,
            new_identifier: None,
            epoch: self.epoch.clone(),
        })
    }

    fn push(&mut self, activity: Activity) {
        self.rows.push(Row::from_activity(
            activity,
            self.project_id(),
            self.workspace(),
        ));
    }

    /// An `IssueActivity` row created directly (the `create_issue_activity`
    /// immediate insert, `:600-611`), with `created_at` overridden and
    /// `updated_at` = insert time.
    async fn insert_immediate(
        &self,
        client: &Client,
        activity: Activity,
        created_at: DateTime<Utc>,
    ) -> R<()> {
        let mut row = Row::from_activity(activity, self.project_id(), self.workspace());
        row.created_at = Some(created_at);
        insert_rows(client, std::slice::from_ref(&row)).await?;
        Ok(())
    }
}

/// A pending `IssueActivity(…)` before UUID/text coercion.
struct Activity {
    issue_id: Option<Uuid>,
    verb: String,
    field: Option<String>,
    old_value: Option<String>,
    new_value: Option<String>,
    comment: String,
    issue_comment_id: Option<Uuid>,
    actor_id: Option<Uuid>,
    old_identifier: Option<Uuid>,
    new_identifier: Option<Uuid>,
    /// Raw `epoch=`; `FloatField.to_python` runs at insert, so a malformed
    /// value only fails when there is at least one row.
    epoch: Option<PyValue>,
}

/// One row as it goes into `issue_activities`.
struct Row {
    id: Uuid,
    created_at: Option<DateTime<Utc>>,
    project_id: Uuid,
    workspace_id: Uuid,
    issue_id: Option<Uuid>,
    verb: String,
    field: Option<String>,
    old_value: Option<String>,
    new_value: Option<String>,
    comment: String,
    issue_comment_id: Option<Uuid>,
    actor_id: Option<Uuid>,
    old_identifier: Option<Uuid>,
    new_identifier: Option<Uuid>,
    epoch: Option<PyValue>,
}

impl Row {
    fn from_activity(activity: Activity, project_id: Uuid, workspace_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            created_at: None,
            project_id,
            workspace_id,
            issue_id: activity.issue_id,
            verb: activity.verb,
            field: activity.field,
            old_value: activity.old_value,
            new_value: activity.new_value,
            comment: activity.comment,
            issue_comment_id: activity.issue_comment_id,
            actor_id: activity.actor_id,
            old_identifier: activity.old_identifier,
            new_identifier: activity.new_identifier,
            epoch: activity.epoch,
        }
    }
}

/// A row as inserted, with the values the notification serialiser needs.
struct Inserted {
    row: Row,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    epoch: Option<f64>,
}

const INSERT_COLUMNS: &str = "id, created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, project_id, workspace_id, issue_id, verb, field, old_value, new_value, \
     comment, attachments, issue_comment_id, actor_id, old_identifier, new_identifier, epoch";

/// `IssueActivity.objects.bulk_create(rows)`: one multi-row INSERT, so a
/// constraint or UUID validation error writes none of the rows.
async fn insert_rows(client: &Client, rows: &[Row]) -> R<Vec<Inserted>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut inserted = Vec::with_capacity(rows.len());
    let mut sql = String::from("INSERT INTO issue_activities (");
    sql.push_str(INSERT_COLUMNS);
    sql.push_str(") VALUES ");
    let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> =
        Vec::with_capacity(rows.len() * 20);
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        let base = index * 20;
        sql.push('(');
        for column in 0..20 {
            if column > 0 {
                sql.push_str(", ");
            }
            sql.push('$');
            sql.push_str(&(base + column + 1).to_string());
        }
        sql.push(')');

        let created_at = row.created_at.unwrap_or_else(Utc::now);
        let updated_at = Utc::now();
        let epoch = convert_epoch(row.epoch.as_ref())?;
        inserted.push(Inserted {
            row: Row {
                id: row.id,
                created_at: Some(created_at),
                project_id: row.project_id,
                workspace_id: row.workspace_id,
                issue_id: row.issue_id,
                verb: row.verb.clone(),
                field: row.field.clone(),
                old_value: row.old_value.clone(),
                new_value: row.new_value.clone(),
                comment: row.comment.clone(),
                issue_comment_id: row.issue_comment_id,
                actor_id: row.actor_id,
                old_identifier: row.old_identifier,
                new_identifier: row.new_identifier,
                epoch: row.epoch.clone(),
            },
            created_at,
            updated_at,
            epoch,
        });

        params.push(Box::new(row.id));
        params.push(Box::new(created_at));
        params.push(Box::new(updated_at));
        params.push(Box::new(None::<Uuid>)); // created_by_id
        params.push(Box::new(None::<Uuid>)); // updated_by_id
        params.push(Box::new(None::<DateTime<Utc>>)); // deleted_at
        params.push(Box::new(row.project_id));
        params.push(Box::new(row.workspace_id));
        params.push(Box::new(row.issue_id));
        params.push(Box::new(row.verb.clone()));
        params.push(Box::new(row.field.clone()));
        params.push(Box::new(row.old_value.clone()));
        params.push(Box::new(row.new_value.clone()));
        params.push(Box::new(row.comment.clone()));
        params.push(Box::new(Vec::<String>::new()));
        params.push(Box::new(row.issue_comment_id));
        params.push(Box::new(row.actor_id));
        params.push(Box::new(row.old_identifier));
        params.push(Box::new(row.new_identifier));
        params.push(Box::new(epoch));
    }
    let borrowed: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
        .iter()
        .map(|value| value.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    client
        .execute(&sql, borrowed.as_slice())
        .await
        .map_err(db)?;
    Ok(inserted)
}

// ---------------------------------------------------------------------------
// Python value helpers
// ---------------------------------------------------------------------------

/// Convert a `serde_json` payload value into the Python-shaped one.
fn serde_to_py(value: &Value) -> PyValue {
    match value {
        Value::Null => PyValue::Null,
        Value::Bool(value) => PyValue::Bool(*value),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                PyValue::from(value)
            } else if let Some(value) = number.as_u64() {
                PyValue::from(value as i64)
            } else {
                PyValue::Float(number.as_f64().unwrap_or(f64::NAN))
            }
        }
        Value::String(text) => PyValue::Str(PyStr::new(text)),
        Value::Array(items) => PyValue::List(items.iter().map(serde_to_py).collect()),
        Value::Object(map) => {
            let mut dict = PyDict::with_capacity(map.len());
            for (key, value) in map {
                dict.insert(PyStr::new(key), serde_to_py(value));
            }
            PyValue::Dict(dict)
        }
    }
}

/// `str(value)`.
fn py_str(value: &PyValue) -> String {
    match value {
        PyValue::Null => "None".to_string(),
        PyValue::Bool(value) => {
            if *value {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        PyValue::Int(value) => value.to_string(),
        PyValue::Float(value) => python_float(*value),
        PyValue::Str(value) => value.to_string_lossy().into_owned(),
        PyValue::List(items) => {
            let parts: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        PyValue::Dict(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        py_repr(&PyValue::Str(key.clone())),
                        py_repr(value)
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn py_str_opt(value: Option<&PyValue>) -> String {
    value.map_or_else(|| "None".to_string(), py_str)
}

/// Python `repr(value)` for the elements of a composite `str()`.
fn py_repr(value: &PyValue) -> String {
    match value {
        PyValue::Str(text) => {
            let text = text.to_string_lossy();
            quote(&text)
        }
        other => py_str(other),
    }
}

/// CPython's `repr` quoting for a `str` (single quotes unless the value
/// contains one and no double quote).
fn quote(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// CPython `float.__repr__`, through the shared formatter.
fn python_float(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .to_string();
    }
    plane_serde::to_vec(plane_serde::Dialect::Django, &value)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| value.to_string())
}

/// Python truthiness of a raw payload value.
fn serde_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64() != Some(0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// Python truthiness.
fn py_truthy(value: Option<&PyValue>) -> bool {
    match value {
        None | Some(PyValue::Null) => false,
        Some(PyValue::Bool(value)) => *value,
        Some(PyValue::Int(value)) => value.as_i64().is_some_and(|value| value != 0),
        Some(PyValue::Float(value)) => *value != 0.0,
        Some(PyValue::Str(text)) => !text.is_empty(),
        Some(PyValue::List(items)) => !items.is_empty(),
        Some(PyValue::Dict(map)) => !map.is_empty(),
    }
}

/// Python `==` for JSON values: `1 == 1.0 == True`, NaN never equals itself,
/// and dict order is irrelevant.
fn py_eq(left: &PyValue, right: &PyValue) -> bool {
    match (left, right) {
        (PyValue::Null, PyValue::Null) => true,
        (PyValue::Bool(a), PyValue::Bool(b)) => a == b,
        (PyValue::Bool(a), PyValue::Int(b)) | (PyValue::Int(b), PyValue::Bool(a)) => {
            b.as_i64() == Some(i64::from(*a))
        }
        (PyValue::Bool(a), PyValue::Float(b)) | (PyValue::Float(b), PyValue::Bool(a)) => {
            f64::from(u8::from(*a)) == *b
        }
        (PyValue::Int(a), PyValue::Int(b)) => a == b,
        (PyValue::Int(a), PyValue::Float(b)) | (PyValue::Float(b), PyValue::Int(a)) => {
            a.as_i64().is_some_and(|value| (value as f64) == *b)
        }
        (PyValue::Float(a), PyValue::Float(b)) => a == b,
        (PyValue::Str(a), PyValue::Str(b)) => a == b,
        (PyValue::List(a), PyValue::List(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| py_eq(a, b))
        }
        (PyValue::Dict(a), PyValue::Dict(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get_wtf8(key.as_wtf8())
                        .is_some_and(|other| py_eq(value, other))
                })
        }
        _ => false,
    }
}

/// `json.loads(value) if value is not None else None` (the exact expression
/// every builder uses). A truthy non-string is `TypeError`.
fn parse_json(value: Option<&PyValue>) -> R<Option<PyValue>> {
    match value {
        None | Some(PyValue::Null) => Ok(None),
        Some(PyValue::Str(text)) => {
            let parsed = pyjson::loads(text.as_wtf8(), "utf-8")?;
            Ok(Some(parsed))
        }
        Some(other) => python(format!(
            "TypeError: the JSON object must be str, bytes or bytearray, not {}",
            python_type_name(other)
        )),
    }
}

fn python_type_name(value: &PyValue) -> &'static str {
    match value {
        PyValue::Null => "NoneType",
        PyValue::Bool(_) => "bool",
        PyValue::Int(_) => "int",
        PyValue::Float(_) => "float",
        PyValue::Str(_) => "str",
        PyValue::List(_) => "list",
        PyValue::Dict(_) => "dict",
    }
}

/// The parsed JSON object a builder calls `.get` on. `None` is Python's
/// `AttributeError: 'NoneType' object has no attribute 'get'`.
fn as_dict<'a>(value: Option<&'a PyValue>, what: &str) -> R<&'a PyDict> {
    match value {
        Some(PyValue::Dict(dict)) => Ok(dict),
        Some(other) => python(format!(
            "AttributeError: '{}' object has no attribute 'get' ({what})",
            python_type_name(other)
        )),
        None => python(format!(
            "AttributeError: 'NoneType' object has no attribute 'get' ({what})"
        )),
    }
}

/// `data.get(key)`.
fn get<'a>(dict: &'a PyDict, key: &str) -> Option<&'a PyValue> {
    dict.get(key)
}

/// `value.get(key, default)` for a parsed object.
fn get_default<'a>(dict: &'a PyDict, key: &str, default: &'a PyValue) -> &'a PyValue {
    dict.get(key).unwrap_or(default)
}

/// Python's `filter(pk=value)`: a UUIDField coercion, or an error that aborts
/// the whole call.
fn python_uuid(value: &PyValue) -> R<Uuid> {
    match value {
        PyValue::Str(text) => {
            let text = text.to_string_lossy();
            parse_uuid_text(&text)
                .ok_or_else(|| Failure::Python(format!("“{text}” is not a valid UUID.")))
        }
        PyValue::Bool(value) => Ok(Uuid::from_u128(u128::from(*value))),
        PyValue::Int(value) => {
            // `uuid.UUID(int=value)` rejects negatives and anything above
            // 2**128 - 1.
            let text = value.to_decimal_string();
            match text.parse::<u128>() {
                Ok(number) => Ok(Uuid::from_u128(number)),
                Err(_) => python("UUIDField: invalid integer UUID"),
            }
        }
        other => python(format!(
            "UUIDField: cannot coerce {}",
            python_type_name(other)
        )),
    }
}

/// `uuid.UUID(hex=...)` for a string. Rust's parser accepts the same four
/// spellings CPython's `hex` branch does (hyphenated, simple, braced, urn).
fn parse_uuid_text(text: &str) -> Option<Uuid> {
    Uuid::try_parse(text).ok()
}

/// `is_valid_uuid`: `uuid.UUID(s).version == 4`.
fn is_valid_uuid(text: &str) -> bool {
    parse_uuid_text(text).is_some_and(|uuid| uuid.get_version_num() == 4)
}

/// `extract_ids` (`:41-46`): a Python `set` of `str(x)`.
fn extract_ids(
    data: Option<&PyValue>,
    primary_key: &str,
    fallback_key: &str,
) -> R<BTreeSet<String>> {
    let Some(data) = data else {
        return Ok(BTreeSet::new());
    };
    if !py_truthy(Some(data)) {
        return Ok(BTreeSet::new());
    }
    let dict = as_dict(Some(data), "extract_ids")?;
    if let Some(value) = dict.get(primary_key) {
        return iterable_to_ids(value);
    }
    match dict.get(fallback_key) {
        Some(value) => iterable_to_ids(value),
        None => Ok(BTreeSet::new()),
    }
}

/// `{str(x) for x in values}`: a list's items, a string's characters, a
/// dict's keys; a number/None is `TypeError`.
fn iterable_to_ids(values: &PyValue) -> R<BTreeSet<String>> {
    match values {
        PyValue::List(items) => Ok(items.iter().map(py_str).collect()),
        PyValue::Str(text) => Ok(text
            .to_string_lossy()
            .chars()
            .map(|character| character.to_string())
            .collect()),
        PyValue::Dict(map) => Ok(map
            .keys()
            .map(|key| key.to_string_lossy().into_owned())
            .collect()),
        other => python(format!(
            "TypeError: '{}' object is not iterable",
            python_type_name(other)
        )),
    }
}

/// `set.difference` without Python's hash order.
fn difference(left: &BTreeSet<String>, right: &BTreeSet<String>) -> BTreeSet<String> {
    left.difference(right).cloned().collect()
}

/// A `CharField`/`TextField` value: `None` stays `None`, everything else is
/// Python `str(value)` (Django's `Field.get_db_prep_value` → `to_python`).
fn text_field(value: Option<&PyValue>) -> Option<String> {
    match value {
        None | Some(PyValue::Null) => None,
        Some(value) => Some(py_str(value)),
    }
}

/// `dict.get(key, "")`: a missing key is `""`, a present JSON `null` is
/// Python `None` (`text_field`).
fn text_field_default(dict: &PyDict, key: &str) -> Option<String> {
    match dict.get(key) {
        Some(value) => text_field(Some(value)),
        None => Some(String::new()),
    }
}

fn uuid_field(value: Option<&PyValue>) -> R<Option<Uuid>> {
    match value {
        None | Some(PyValue::Null) => Ok(None),
        Some(value) => Ok(Some(python_uuid(value)?)),
    }
}

/// `FloatField.to_python`: `None` stays `None`, `float(value)` accepts
/// numbers, booleans and numeric strings; a list/dict is a `ValidationError`.
fn convert_epoch(value: Option<&PyValue>) -> R<Option<f64>> {
    match value {
        None | Some(PyValue::Null) => Ok(None),
        Some(PyValue::Bool(value)) => Ok(Some(f64::from(u8::from(*value)))),
        Some(PyValue::Int(value)) => value
            .as_i64()
            .map(|value| Some(value as f64))
            .ok_or_else(|| Failure::Python("FloatField: integer out of range".to_string())),
        Some(PyValue::Float(value)) => Ok(Some(*value)),
        Some(PyValue::Str(text)) => {
            let text = text.to_string_lossy();
            text.trim().parse::<f64>().map(Some).or_else(|_| {
                python(format!(
                    "ValueError: could not convert string to float: {text:?}"
                ))
            })
        }
        Some(other) => python(format!(
            "TypeError: float() argument must be a string or a number, not '{}'",
            python_type_name(other)
        )),
    }
}

/// [`task_arg`], recreating kombu's UUID envelope when the queued payload
/// recorded that the producer passed a `uuid.UUID` object.
fn task_arg_typed(value: Option<&PyValue>, as_uuid: bool) -> TaskArg {
    if as_uuid
        && let Some(PyValue::Str(text)) = value
        && let Some(uuid) = parse_uuid_text(&text.to_string_lossy())
    {
        return TaskArg::Uuid(uuid);
    }
    task_arg(value)
}

/// `task_arg` for the `notifications` pass-through kwargs.
fn task_arg(value: Option<&PyValue>) -> TaskArg {
    match value {
        None | Some(PyValue::Null) => TaskArg::Null,
        Some(PyValue::Bool(value)) => TaskArg::Bool(*value),
        Some(PyValue::Int(value)) => match value.as_i64() {
            Some(value) => TaskArg::Int(value),
            None => TaskArg::Str(value.to_string()),
        },
        Some(PyValue::Float(value)) => TaskArg::Float(*value),
        Some(PyValue::Str(value)) => TaskArg::Str(value.to_string_lossy().into_owned()),
        Some(PyValue::List(items)) => TaskArg::list(items.iter().map(|item| task_arg(Some(item)))),
        Some(PyValue::Dict(map)) => TaskArg::Map(
            map.iter()
                .map(|(key, value)| (key.to_string_lossy().into_owned(), task_arg(Some(value))))
                .collect(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Database reads
// ---------------------------------------------------------------------------

struct ProjectRow {
    id: Uuid,
    workspace_id: Uuid,
}

async fn load_project(client: &Client, project_id: Uuid) -> R<Option<ProjectRow>> {
    // `Project.objects` is the `SoftDeletionManager` (`plane/db/mixins.py:52-55`).
    let row = client
        .query_opt(
            "SELECT id, workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL",
            &[&project_id],
        )
        .await
        .map_err(db)?;
    Ok(row.map(|row| ProjectRow {
        id: row.get(0),
        workspace_id: row.get(1),
    }))
}

/// The issue the task bumps (`:1578-1582`): the soft-deletion manager and the
/// `project_id` filter apply.
struct BumpIssue {
    id: Uuid,
    state_id: Option<Uuid>,
}

async fn load_issue_for_bump(
    client: &Client,
    issue_id: Uuid,
    project_id: Option<Uuid>,
) -> R<Option<BumpIssue>> {
    let row = match project_id {
        Some(project_id) => client
            .query_opt(
                "SELECT id, state_id FROM issues \
                     WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL",
                &[&issue_id, &project_id],
            )
            .await
            .map_err(db)?,
        None => client
            .query_opt(
                "SELECT id, state_id FROM issues \
                     WHERE id = $1 AND deleted_at IS NULL",
                &[&issue_id],
            )
            .await
            .map_err(db)?,
    };
    Ok(row.map(|row| BumpIssue {
        id: row.get(0),
        state_id: row.get(1),
    }))
}

/// `issue.updated_at = timezone.now(); issue.save(update_fields=["updated_at"])`
/// including `Issue.save`'s NULL-state branch (`plane/db/models/issue.py:180-255`).
async fn bump_issue(client: &Client, issue: &BumpIssue) -> R<()> {
    if issue.state_id.is_some() {
        client
            .execute(
                "UPDATE issues SET updated_at = now() WHERE id = $1",
                &[&issue.id],
            )
            .await
            .map_err(db)?;
        return Ok(());
    }
    // `_ensure_default_state`: the project's default non-triage state, else
    // its first non-triage state by `State.Meta.ordering`.
    let project = client
        .query_opt("SELECT project_id FROM issues WHERE id = $1", &[&issue.id])
        .await
        .map_err(db)?
        .map(|row| row.get::<_, Uuid>(0));
    let Some(project) = project else {
        return Ok(());
    };
    let state = client
        .query_opt(
            "SELECT id, \"group\" FROM states \
             WHERE project_id = $1 AND deleted_at IS NULL \
               AND NOT \"group\" = 'triage' AND NOT is_triage \
               AND default ORDER BY sequence ASC LIMIT 1",
            &[&project],
        )
        .await
        .map_err(db)?;
    let state = match state {
        Some(state) => Some(state),
        None => client
            .query_opt(
                "SELECT id, \"group\" FROM states \
                 WHERE project_id = $1 AND deleted_at IS NULL \
                   AND NOT \"group\" = 'triage' AND NOT is_triage \
                 ORDER BY sequence ASC LIMIT 1",
                &[&project],
            )
            .await
            .map_err(db)?,
    };
    let Some(state) = state else {
        client
            .execute(
                "UPDATE issues SET updated_at = now() WHERE id = $1",
                &[&issue.id],
            )
            .await
            .map_err(db)?;
        return Ok(());
    };
    // `_sync_completed_at`: `completed_at = now()` for a completed state,
    // `NULL` otherwise, and the dirty `state` field adds it to update_fields.
    let group: String = state.get(1);
    if group == "completed" {
        client
            .execute(
                "UPDATE issues SET updated_at = now(), completed_at = now() WHERE id = $1",
                &[&issue.id],
            )
            .await
            .map_err(db)?;
    } else {
        client
            .execute(
                "UPDATE issues SET updated_at = now(), completed_at = NULL WHERE id = $1",
                &[&issue.id],
            )
            .await
            .map_err(db)?;
    }
    Ok(())
}

/// One issue row for a builder's bump: `Issue.objects.filter(pk, project)`.
async fn bump_project_issue(
    client: &Client,
    issue_id: Option<&PyValue>,
    project_id: Uuid,
) -> R<bool> {
    let Some(issue_id) = issue_id else {
        return Ok(false);
    };
    if issue_id.is_null() {
        return Ok(false);
    }
    let uuid = python_uuid(issue_id)?;
    let found = load_issue_for_bump(client, uuid, Some(project_id)).await?;
    let Some(issue) = found else {
        return Ok(false);
    };
    // The builder's query also filters `project_id`; the `UPDATE` uses the pk.
    let belongs = client
        .query_opt(
            "SELECT 1 FROM issues WHERE id = $1 AND project_id = $2",
            &[&uuid, &project_id],
        )
        .await
        .map_err(db)?;
    if belongs.is_none() {
        return Ok(false);
    }
    bump_issue(client, &issue).await?;
    Ok(true)
}

/// The shared state/project/label lookups keep the manager's predicates.
async fn load_name(
    client: &Client,
    table: &str,
    id: Option<&PyValue>,
    project_id: Uuid,
) -> R<Option<(Uuid, String)>> {
    let Some(id) = id else {
        return Ok(None);
    };
    if id.is_null() {
        return Ok(None);
    }
    let uuid = python_uuid(id)?;
    let sql = match table {
        // `State.objects` excludes triage states (`plane/db/models/state.py:65-69`).
        "states" => "SELECT id, name FROM states WHERE id = $1 AND project_id = $2 \
             AND deleted_at IS NULL AND NOT \"group\" = 'triage' LIMIT 1"
            .to_string(),
        _ => format!(
            "SELECT id, name FROM {table} WHERE id = $1 AND project_id = $2 \
             AND deleted_at IS NULL LIMIT 1"
        ),
    };
    let row = client
        .query_opt(&sql, &[&uuid, &project_id])
        .await
        .map_err(db)?;
    Ok(row.map(|row| (row.get(0), row.get(1))))
}

/// `User.objects.filter(pk=…)` for an assignee, including the manager join
/// and role rules (`:394-403`, `:438-443`).
async fn load_service_assignee(
    client: &Client,
    user_id: &str,
    project_id: Uuid,
    workspace_id: Uuid,
    added: bool,
) -> R<Option<(Uuid, String)>> {
    let Some(uuid) = parse_uuid_text(user_id) else {
        return python(format!("“{user_id}” is not a valid UUID."));
    };
    let row = if added {
        client
            .query_opt(
                "SELECT users.id, users.display_name FROM users \
                 INNER JOIN project_members ON project_members.member_id = users.id \
                 WHERE users.id = $1 AND project_members.project_id = $2 \
                   AND project_members.role >= 15 AND project_members.is_active \
                   AND project_members.deleted_at IS NULL LIMIT 1",
                &[&uuid, &project_id],
            )
            .await
            .map_err(db)?
    } else {
        client
            .query_opt(
                "SELECT users.id, users.display_name FROM users \
                 INNER JOIN workspace_members ON workspace_members.member_id = users.id \
                 WHERE users.id = $1 AND workspace_members.workspace_id = $2 \
                   AND workspace_members.deleted_at IS NULL LIMIT 1",
                &[&uuid, &workspace_id],
            )
            .await
            .map_err(db)?
    };
    Ok(row.map(|row| (row.get(0), row.get(1))))
}
// ---------------------------------------------------------------------------
// The 27 builders
// ---------------------------------------------------------------------------

/// `create_issue_activity` (`:589-623`).
async fn create_issue_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    // `Issue.objects.get(pk=issue_id)` (`:599`). `get(pk=None)` is
    // `WHERE id IS NULL` → DoesNotExist; a malformed value is a ValidationError.
    let issue_id = match &engine.issue_id {
        None | Some(PyValue::Null) => {
            return python("Issue matching query does not exist.");
        }
        Some(value) => python_uuid(value)?,
    };
    let row = engine
        .client
        .query_opt(
            "SELECT id, created_at, created_by_id FROM issues \
             WHERE id = $1 AND deleted_at IS NULL",
            &[&issue_id],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return python("Issue matching query does not exist.");
    };
    let created_at: DateTime<Utc> = row.get(1);
    let created_by: Option<Uuid> = row.get(2);

    let mut activity = engine.activity(Some(issue_id), "created", None)?;
    activity.comment = "created the issue".to_string();
    activity.actor_id = created_by;
    engine
        .insert_immediate(engine.client, activity, created_at)
        .await?;

    // `requested_data = json.loads(requested_data) if requested_data is not None
    // else None` (`:612`); a `None` value is then `AttributeError` at `.get`.
    let parsed = parse_json(requested.as_ref())?;
    let requested_dict = as_dict(parsed.as_ref(), "create_issue_activity")?;
    if let Some(assignees) = requested_dict.get("assignee_ids")
        && !assignees.is_null()
    {
        // The call passes the *raw* `current_instance` (`:614-623`).
        track_assignees(engine, requested_dict, current.as_ref()).await?;
    }
    Ok(())
}

/// `update_issue_activity` (`:626-671`).
async fn update_issue_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let current = parse_json(current.as_ref())?;
    let requested_dict = as_dict(requested.as_ref(), "update_issue_activity")?;
    // The loop calls `.get` on the current object; a `None`/scalar is an abort.
    let current_dict = as_dict(current.as_ref(), "update_issue_activity")?;

    for (key, _value) in requested_dict.iter() {
        match key.as_str() {
            Some("name") => track_name(engine, requested_dict, current_dict)?,
            Some("parent_id" | "parent") => {
                track_parent(engine, requested_dict, current_dict).await?;
            }
            Some("priority") => track_priority(engine, requested_dict, current_dict)?,
            Some("state_id" | "state") => {
                track_state(engine, requested_dict, current_dict).await?;
            }
            Some("description_html") => {
                track_description(engine, requested_dict, current_dict).await?;
            }
            Some("target_date") => {
                track_target_date(engine, requested_dict, current_dict)?;
            }
            Some("start_date") => track_start_date(engine, requested_dict, current_dict)?,
            Some("label_ids" | "labels") => {
                track_labels(engine, requested_dict, current_dict).await?;
            }
            Some("assignee_ids" | "assignees") => {
                track_assignees(
                    engine,
                    requested_dict,
                    Some(&PyValue::Dict(current_dict.clone())),
                )
                .await?;
            }
            Some("estimate_point") => {
                track_estimate_points(engine, requested_dict, current_dict).await?;
            }
            Some("archived_at") => {
                track_archive_at(engine, requested_dict, current_dict)?;
            }
            Some("closed_to") => track_closed_to(engine, requested_dict).await?,
            // A key whose text is not valid UTF-8 cannot equal a mapper key.
            _ => {}
        }
    }
    Ok(())
}

/// `delete_issue_activity` (`:674-695`).
fn delete_issue_activity(engine: &mut Engine<'_>) -> R<()> {
    let mut activity = engine.activity(engine.issue()?, "deleted", Some("issue"))?;
    activity.comment = "deleted the issue".to_string();
    engine.push(activity);
    Ok(())
}

/// `track_name` (`:50-74`).
fn track_name(engine: &mut Engine<'_>, requested: &PyDict, current: &PyDict) -> R<()> {
    let old = get(current, "name");
    let new = get(requested, "name");
    if !py_eq_opt(old, new) {
        let mut activity = engine.activity(engine.issue()?, "updated", Some("name"))?;
        activity.old_value = text_field(old);
        activity.new_value = text_field(new);
        activity.comment = "updated the name to".to_string();
        engine.push(activity);
    }
    Ok(())
}

/// `track_description` (`:78-111`).
async fn track_description(engine: &mut Engine<'_>, requested: &PyDict, current: &PyDict) -> R<()> {
    let old = get(current, "description_html");
    let new = get(requested, "description_html");
    if py_eq_opt(old, new) {
        return Ok(());
    }
    // `IssueActivity.objects.filter(issue_id=issue_id).order_by("-created_at").first()`
    let issue_id = engine.issue()?;
    let last = match issue_id {
        Some(issue_id) => engine
            .client
            .query_opt(
                "SELECT id, field, actor_id FROM issue_activities \
                 WHERE issue_id = $1 AND deleted_at IS NULL \
                 ORDER BY created_at DESC LIMIT 1",
                &[&issue_id],
            )
            .await
            .map_err(db)?,
        None => None,
    };
    if let Some(last) = last {
        let field: Option<String> = last.get(1);
        let actor: Option<Uuid> = last.get(2);
        let actor_matches = py_str_opt(engine.actor_id.as_ref())
            == actor.map_or_else(|| "None".to_string(), |actor| actor.to_string());
        if field.as_deref() == Some("description") && actor_matches {
            let id: Uuid = last.get(0);
            engine
                .client
                .execute(
                    "UPDATE issue_activities SET created_at = now() WHERE id = $1",
                    &[&id],
                )
                .await
                .map_err(db)?;
            return Ok(());
        }
    }
    let mut activity = engine.activity(issue_id, "updated", Some("description"))?;
    activity.old_value = text_field(old);
    activity.new_value = text_field(new);
    activity.comment = "updated the description to".to_string();
    engine.push(activity);
    Ok(())
}

/// `track_parent` (`:115-167`).
async fn track_parent(engine: &mut Engine<'_>, requested: &PyDict, current: &PyDict) -> R<()> {
    // `current_instance.get("parent_id") or current_instance.get("parent")`.
    let current_parent = py_or(get(current, "parent_id"), get(current, "parent"));
    let requested_parent = py_or(get(requested, "parent_id"), get(requested, "parent"));
    if let Some(value) = current_parent
        && !value.is_null()
        && !valid_uuid_value(value)?
    {
        return Ok(());
    }
    if let Some(value) = requested_parent
        && !value.is_null()
        && !valid_uuid_value(value)?
    {
        return Ok(());
    }
    if py_eq_opt(current_parent, requested_parent) {
        return Ok(());
    }
    let old_parent = match current_parent {
        Some(value) if !value.is_null() => {
            issue_identifier(engine.client, &python_uuid(value)?, engine.project_id()).await?
        }
        _ => None,
    };
    let new_parent = match requested_parent {
        Some(value) if !value.is_null() => {
            let uuid = python_uuid(value)?;
            issue_identifier(engine.client, &uuid, engine.project_id()).await?
        }
        _ => None,
    };
    if let Some(value) = requested_parent
        && !value.is_null()
        && new_parent.is_none()
    {
        return Ok(());
    }
    let mut activity = engine.activity(engine.issue()?, "updated", Some("parent"))?;
    activity.old_value = Some(
        old_parent
            .as_ref()
            .map(|parent| parent.label.clone())
            .unwrap_or_default(),
    );
    activity.new_value = Some(
        new_parent
            .as_ref()
            .map(|parent| parent.label.clone())
            .unwrap_or_default(),
    );
    activity.comment = "updated the parent issue to".to_string();
    activity.old_identifier = old_parent.map(|parent| parent.id);
    activity.new_identifier = new_parent.map(|parent| parent.id);
    engine.push(activity);
    Ok(())
}

/// `track_priority` (`:171-195`).
fn track_priority(engine: &mut Engine<'_>, requested: &PyDict, current: &PyDict) -> R<()> {
    let old = get(current, "priority");
    let new = get(requested, "priority");
    if !py_eq_opt(old, new) {
        let mut activity = engine.activity(engine.issue()?, "updated", Some("priority"))?;
        activity.old_value = text_field(old);
        activity.new_value = text_field(new);
        activity.comment = "updated the priority to".to_string();
        engine.push(activity);
    }
    Ok(())
}

/// `track_state` (`:199-236`).
async fn track_state(engine: &mut Engine<'_>, requested: &PyDict, current: &PyDict) -> R<()> {
    let mut current_state = py_or(get(current, "state_id"), get(current, "state"));
    let mut requested_state = py_or(get(requested, "state_id"), get(requested, "state"));
    // Invalid ids are replaced with `None`, not skipped (`:212-215`).
    if let Some(value) = current_state
        && !value.is_null()
        && !valid_uuid_value(value)?
    {
        current_state = None;
    }
    if let Some(value) = requested_state
        && !value.is_null()
        && !valid_uuid_value(value)?
    {
        requested_state = None;
    }
    if py_eq_opt(current_state, requested_state) {
        return Ok(());
    }
    let new_state = load_name(
        engine.client,
        "states",
        requested_state,
        engine.project_id(),
    )
    .await?;
    let old_state = load_name(engine.client, "states", current_state, engine.project_id()).await?;
    let mut activity = engine.activity(engine.issue()?, "updated", Some("state"))?;
    activity.old_value = old_state.as_ref().map(|state| state.1.clone());
    activity.new_value = new_state.as_ref().map(|state| state.1.clone());
    activity.comment = "updated the state to".to_string();
    activity.old_identifier = old_state.map(|state| state.0);
    activity.new_identifier = new_state.map(|state| state.0);
    engine.push(activity);
    Ok(())
}

/// `track_target_date` (`:240-266`).
fn track_target_date(engine: &mut Engine<'_>, requested: &PyDict, current: &PyDict) -> R<()> {
    let old = get(current, "target_date");
    let new = get(requested, "target_date");
    if !py_eq_opt(old, new) {
        let mut activity = engine.activity(engine.issue()?, "updated", Some("target_date"))?;
        activity.old_value = Some(
            old.filter(|value| !value.is_null())
                .map_or_else(String::new, py_str),
        );
        activity.new_value = Some(
            new.filter(|value| !value.is_null())
                .map_or_else(String::new, py_str),
        );
        activity.comment = "updated the target date to".to_string();
        engine.push(activity);
    }
    Ok(())
}

/// `track_start_date` (`:270-296`).
fn track_start_date(engine: &mut Engine<'_>, requested: &PyDict, current: &PyDict) -> R<()> {
    let old = get(current, "start_date");
    let new = get(requested, "start_date");
    if !py_eq_opt(old, new) {
        let mut activity = engine.activity(engine.issue()?, "updated", Some("start_date"))?;
        activity.old_value = Some(
            old.filter(|value| !value.is_null())
                .map_or_else(String::new, py_str),
        );
        activity.new_value = Some(
            new.filter(|value| !value.is_null())
                .map_or_else(String::new, py_str),
        );
        activity.comment = "updated the start date to ".to_string();
        engine.push(activity);
    }
    Ok(())
}

/// `track_labels` (`:300-367`).
async fn track_labels(engine: &mut Engine<'_>, requested: &PyDict, current: &PyDict) -> R<()> {
    let requested_labels = extract_ids_from_dict(Some(requested), "label_ids", "labels")?;
    let current_labels = extract_ids_from_dict(Some(current), "label_ids", "labels")?;
    let added = difference(&requested_labels, &current_labels);
    let dropped = difference(&current_labels, &requested_labels);
    for label in added {
        if !is_valid_uuid(&label) {
            continue;
        }
        let Some((id, name)) = load_name(
            engine.client,
            "labels",
            Some(&PyValue::Str(PyStr::new(&label))),
            engine.project_id(),
        )
        .await?
        else {
            continue;
        };
        let mut activity = engine.activity(engine.issue()?, "updated", Some("labels"))?;
        activity.old_value = Some(String::new());
        activity.new_value = Some(name);
        activity.comment = "added label ".to_string();
        activity.new_identifier = Some(id);
        engine.push(activity);
    }
    for label in dropped {
        if !is_valid_uuid(&label) {
            continue;
        }
        let Some((id, name)) = load_name(
            engine.client,
            "labels",
            Some(&PyValue::Str(PyStr::new(&label))),
            engine.project_id(),
        )
        .await?
        else {
            continue;
        };
        let mut activity = engine.activity(engine.issue()?, "updated", Some("labels"))?;
        activity.old_value = Some(name);
        activity.new_value = Some(String::new());
        activity.comment = "removed label ".to_string();
        activity.old_identifier = Some(id);
        engine.push(activity);
    }
    Ok(())
}

/// `track_assignees` (`:371-460`): `current` is the raw `current_instance`
/// for the create builder and the parsed object for the update builder.
async fn track_assignees(
    engine: &mut Engine<'_>,
    requested: &PyDict,
    current: Option<&PyValue>,
) -> R<()> {
    let requested_assignees = extract_ids_from_dict(Some(requested), "assignee_ids", "assignees")?;
    let current_assignees = match current {
        Some(value) => extract_ids(Some(value), "assignee_ids", "assignees")?,
        None => BTreeSet::new(),
    };
    let added = difference(&requested_assignees, &current_assignees);
    let dropped = difference(&current_assignees, &requested_assignees);
    let workspace_id = engine.workspace();
    let project_id = engine.project_id();

    let mut subscribers: Vec<Uuid> = Vec::new();
    for assignee in added {
        if !is_valid_uuid(&assignee) {
            continue;
        }
        let Some((id, display_name)) =
            load_service_assignee(engine.client, &assignee, project_id, workspace_id, true).await?
        else {
            continue;
        };
        let mut activity = engine.activity(engine.issue()?, "updated", Some("assignees"))?;
        activity.old_value = Some(String::new());
        activity.new_value = Some(display_name);
        activity.comment = "added assignee ".to_string();
        activity.new_identifier = Some(id);
        engine.push(activity);
        subscribers.push(id);
    }
    // `IssueSubscriber.objects.bulk_create(..., batch_size=10,
    // ignore_conflicts=True)` (`:431`).
    if !subscribers.is_empty() {
        let Some(issue_id) = engine.issue()? else {
            return python("NOT NULL constraint: issue_subscribers.issue_id");
        };
        insert_subscribers(
            engine.client,
            &subscribers,
            project_id,
            workspace_id,
            issue_id,
        )
        .await?;
    }

    for assignee in dropped {
        if !is_valid_uuid(&assignee) {
            continue;
        }
        let Some((id, display_name)) =
            load_service_assignee(engine.client, &assignee, project_id, workspace_id, false)
                .await?
        else {
            continue;
        };
        let mut activity = engine.activity(engine.issue()?, "updated", Some("assignees"))?;
        activity.old_value = Some(display_name);
        activity.new_value = Some(String::new());
        activity.comment = "removed assignee ".to_string();
        activity.old_identifier = Some(id);
        engine.push(activity);
    }
    Ok(())
}

/// `IssueSubscriber.objects.bulk_create` (`:419-431`): the audit columns are
/// explicit and `bulk_create` keeps them.
async fn insert_subscribers(
    client: &Client,
    subscriber_ids: &[Uuid],
    project_id: Uuid,
    workspace_id: Uuid,
    issue_id: Uuid,
) -> R<()> {
    let mut sql = String::from(
        "INSERT INTO issue_subscribers \
         (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
          issue_id, subscriber_id, workspace_id, project_id) VALUES ",
    );
    let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> =
        Vec::with_capacity(subscriber_ids.len() * 10);
    for (index, subscriber) in subscriber_ids.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        let base = index * 10;
        sql.push('(');
        for column in 0..10 {
            if column > 0 {
                sql.push_str(", ");
            }
            sql.push('$');
            sql.push_str(&(base + column + 1).to_string());
        }
        sql.push(')');
        params.push(Box::new(Uuid::new_v4()));
        params.push(Box::new(Utc::now()));
        params.push(Box::new(Utc::now()));
        params.push(Box::new(Some(*subscriber)));
        params.push(Box::new(Some(*subscriber)));
        params.push(Box::new(None::<DateTime<Utc>>));
        params.push(Box::new(issue_id));
        params.push(Box::new(*subscriber));
        params.push(Box::new(workspace_id));
        params.push(Box::new(project_id));
    }
    sql.push_str(" ON CONFLICT DO NOTHING");
    let borrowed: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
        .iter()
        .map(|value| value.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    client
        .execute(&sql, borrowed.as_slice())
        .await
        .map_err(db)?;
    Ok(())
}

/// `track_estimate_points` (`:463-507`).
async fn track_estimate_points(
    engine: &mut Engine<'_>,
    requested: &PyDict,
    current: &PyDict,
) -> R<()> {
    let current_point = get(current, "estimate_point");
    let requested_point = get(requested, "estimate_point");
    if py_eq_opt(current_point, requested_point) {
        return Ok(());
    }
    let old_estimate = match current_point {
        Some(value) if !value.is_null() => {
            estimate_point(engine.client, Some(value), engine.project_id()).await?
        }
        _ => None,
    };
    let new_estimate = match requested_point {
        Some(value) if !value.is_null() => {
            estimate_point(engine.client, Some(value), engine.project_id()).await?
        }
        _ => None,
    };
    if let Some(value) = requested_point
        && !value.is_null()
        && new_estimate.is_none()
    {
        return Ok(());
    }
    // `field="estimate_" + new_estimate.estimate.type` (`:501`) runs in the
    // removal branch too, where `new_estimate` is None: an upstream abort.
    let Some(new_estimate) = new_estimate else {
        return python("AttributeError: 'NoneType' object has no attribute 'estimate'");
    };
    let mut activity = engine.activity(
        engine.issue()?,
        "updated",
        Some(&format!("estimate_{}", new_estimate.estimate_type)),
    )?;
    activity.old_identifier = match current_point {
        Some(value) if !value.is_null() => uuid_field(Some(value))?,
        _ => None,
    };
    activity.new_identifier = match requested_point {
        Some(value) if !value.is_null() => uuid_field(Some(value))?,
        _ => None,
    };
    activity.old_value = old_estimate.map(|point| point.value);
    activity.new_value = Some(new_estimate.value);
    activity.comment = "updated the estimate point to ".to_string();
    engine.push(activity);
    Ok(())
}

struct EstimatePoint {
    value: String,
    estimate_type: String,
}

async fn estimate_point(
    client: &Client,
    id: Option<&PyValue>,
    project_id: Uuid,
) -> R<Option<EstimatePoint>> {
    let Some(id) = id else {
        return Ok(None);
    };
    let uuid = python_uuid(id)?;
    let row = client
        .query_opt(
            "SELECT estimate_points.value, estimates.type FROM estimate_points \
             INNER JOIN estimates ON estimates.id = estimate_points.estimate_id \
             WHERE estimate_points.id = $1 AND estimate_points.project_id = $2 \
               AND estimate_points.deleted_at IS NULL LIMIT 1",
            &[&uuid, &project_id],
        )
        .await
        .map_err(db)?;
    Ok(row.map(|row| EstimatePoint {
        value: row.get(0),
        estimate_type: row.get(1),
    }))
}

/// `track_archive_at` (`:510-556`).
fn track_archive_at(engine: &mut Engine<'_>, requested: &PyDict, current: &PyDict) -> R<()> {
    let old = get(current, "archived_at");
    let new = get(requested, "archived_at");
    if py_eq_opt(old, new) {
        return Ok(());
    }
    let mut activity = engine.activity(engine.issue()?, "updated", Some("archived_at"))?;
    if new.is_none_or(PyValue::is_null) {
        activity.comment = "has restored the issue".to_string();
        activity.old_value = Some("archive".to_string());
        activity.new_value = Some("restore".to_string());
    } else {
        let automation = py_truthy(get(requested, "automation"));
        activity.comment = if automation {
            "Plane has archived the issue".to_string()
        } else {
            "Actor has archived the issue".to_string()
        };
        activity.new_value = Some(if automation {
            "archive".to_string()
        } else {
            "manual_archive".to_string()
        });
    }
    engine.push(activity);
    Ok(())
}

/// `track_closed_to` (`:559-586`): `State.objects.get`, so a missing state is
/// an abort rather than a skipped activity.
async fn track_closed_to(engine: &mut Engine<'_>, requested: &PyDict) -> R<()> {
    let Some(closed_to) = get(requested, "closed_to") else {
        return Ok(());
    };
    if closed_to.is_null() {
        return Ok(());
    }
    let state = load_name(
        engine.client,
        "states",
        Some(closed_to),
        engine.project_id(),
    )
    .await?;
    let Some((id, name)) = state else {
        return python("State matching query does not exist.");
    };
    let mut activity = engine.activity(engine.issue()?, "updated", Some("state"))?;
    activity.new_value = Some(name);
    activity.comment = "Plane updated the state to ".to_string();
    activity.new_identifier = Some(id);
    engine.push(activity);
    Ok(())
}

/// `create_comment_activity` (`:698-725`).
fn create_comment_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let _current = parse_json(current.as_ref())?;
    let requested = as_dict(requested.as_ref(), "create_comment_activity")?;
    let mut activity = engine.activity(engine.issue()?, "created", Some("comment"))?;
    activity.comment = "created a comment".to_string();
    activity.new_value = text_field_default(requested, "comment_html");
    let id = uuid_field(get(requested, "id"))?;
    activity.new_identifier = id;
    activity.issue_comment_id = id;
    engine.push(activity);
    Ok(())
}

/// `update_comment_activity` (`:728-759`).
fn update_comment_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let current = parse_json(current.as_ref())?;
    let requested = as_dict(requested.as_ref(), "update_comment_activity")?;
    let current = as_dict(current.as_ref(), "update_comment_activity")?;
    // Only a change to `comment_html` is recorded.
    if requested.contains_key("comment_html")
        && !py_eq_opt(get(current, "comment_html"), get(requested, "comment_html"))
    {
        let mut activity = engine.activity(engine.issue()?, "updated", Some("comment"))?;
        activity.comment = "updated a comment".to_string();
        activity.old_value = text_field_default(current, "comment_html");
        activity.old_identifier = uuid_field(get(current, "id"))?;
        activity.new_value = text_field_default(requested, "comment_html");
        activity.new_identifier = uuid_field(get(current, "id"))?;
        activity.issue_comment_id = uuid_field(get(current, "id"))?;
        engine.push(activity);
    }
    Ok(())
}

/// `delete_comment_activity` (`:762-785`).
fn delete_comment_activity(engine: &mut Engine<'_>, requested: &Option<PyValue>) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let requested = as_dict(requested.as_ref(), "delete_comment_activity")?;
    let mut activity = engine.activity(engine.issue()?, "deleted", Some("comment"))?;
    activity.comment = "deleted the comment".to_string();
    activity.issue_comment_id = uuid_field(get(requested, "comment_id"))?;
    engine.push(activity);
    Ok(())
}

/// `create_cycle_issue_activity` (`:788-854`).
async fn create_cycle_issue_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let _requested = parse_json(requested.as_ref())?;
    let current = parse_json(current.as_ref())?;
    let current = as_dict(current.as_ref(), "create_cycle_issue_activity")?;
    let empty_list = PyValue::List(Vec::new());
    let updated_records = get_default(current, "updated_cycle_issues", &empty_list);
    let created_raw = get_default(current, "created_cycle_issues", &empty_list);
    // `json.loads(current_instance.get("created_cycle_issues", []))` (`:803`):
    // the default is a list, which `json.loads` rejects.
    let created = parse_json(Some(created_raw))?;
    let created = match created.as_ref() {
        Some(PyValue::List(items)) => items.clone(),
        Some(other) => {
            return python(format!(
                "TypeError: '{}' object is not iterable",
                python_type_name(other)
            ));
        }
        None => {
            return python(
                "TypeError: the JSON object must be str, bytes or bytearray, not NoneType",
            );
        }
    };
    let updated = match updated_records {
        PyValue::List(items) => items.clone(),
        other => {
            return python(format!(
                "TypeError: '{}' object is not iterable",
                python_type_name(other)
            ));
        }
    };

    for record in &updated {
        let record = as_dict(Some(record), "cycle updated record")?;
        let old_cycle = load_cycle(
            engine.client,
            get(record, "old_cycle_id"),
            engine.project_id(),
        )
        .await?;
        let new_cycle = load_cycle(
            engine.client,
            get(record, "new_cycle_id"),
            engine.project_id(),
        )
        .await?;
        let issue_value = get(record, "issue_id");
        let found = bump_project_issue(engine.client, issue_value, engine.project_id()).await?;
        if !found {
            continue;
        }
        let mut activity = engine.activity(uuid_field(issue_value)?, "updated", Some("cycles"))?;
        activity.old_value = Some(
            old_cycle
                .as_ref()
                .map_or_else(String::new, |cycle| cycle.1.clone()),
        );
        activity.new_value = Some(
            new_cycle
                .as_ref()
                .map_or_else(String::new, |cycle| cycle.1.clone()),
        );
        activity.comment = format!(
            "updated cycle from {}\n                to {}",
            old_cycle.as_ref().map_or("", |cycle| cycle.1.as_str()),
            new_cycle.as_ref().map_or("", |cycle| cycle.1.as_str())
        );
        activity.old_identifier = old_cycle.map(|cycle| cycle.0);
        activity.new_identifier = new_cycle.map(|cycle| cycle.0);
        engine.push(activity);
    }

    for record in created {
        let record = as_dict(Some(&record), "cycle created record")?;
        let Some(fields) = record.get("fields") else {
            return python("AttributeError: 'NoneType' object has no attribute 'get'");
        };
        let fields = as_dict(Some(fields), "cycle created record fields")?;
        let cycle = load_cycle(engine.client, get(fields, "cycle"), engine.project_id()).await?;
        let issue_value = get(fields, "issue");
        let found = bump_project_issue(engine.client, issue_value, engine.project_id()).await?;
        match (cycle, found) {
            (Some(cycle), true) => {
                let mut activity =
                    engine.activity(uuid_field(issue_value)?, "created", Some("cycles"))?;
                activity.old_value = Some(String::new());
                activity.new_value = Some(cycle.1.clone());
                activity.comment = format!("added cycle {}", cycle.1);
                activity.new_identifier = Some(cycle.0);
                engine.push(activity);
            }
            _ => continue,
        }
    }
    Ok(())
}

/// `delete_cycle_issue_activity` (`:857-894`).
async fn delete_cycle_issue_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let requested = as_dict(requested.as_ref(), "delete_cycle_issue_activity")?;
    let empty = PyValue::Str(PyStr::new(""));
    let cycle_id = get_default(requested, "cycle_id", &empty);
    let cycle = load_cycle(engine.client, Some(cycle_id), engine.project_id()).await?;
    let cycle_name = py_str(get_default(
        requested,
        "cycle_name",
        &PyValue::Str(PyStr::new("")),
    ));
    let Some(issues) = requested.get("issues") else {
        return python("TypeError: 'NoneType' object is not iterable");
    };
    let PyValue::List(issues) = issues else {
        return python(format!(
            "TypeError: '{}' object is not iterable",
            python_type_name(issues)
        ));
    };
    for issue in issues {
        let found = bump_project_issue(engine.client, Some(issue), engine.project_id()).await?;
        if !found {
            continue;
        }
        let mut activity = engine.activity(uuid_field(Some(issue))?, "deleted", Some("cycles"))?;
        activity.old_value = Some(
            cycle
                .as_ref()
                .map_or_else(|| cycle_name.clone(), |cycle| cycle.1.clone()),
        );
        activity.new_value = Some(String::new());
        activity.comment = format!(
            "removed this issue from {}",
            cycle
                .as_ref()
                .map_or_else(|| cycle_name.clone(), |cycle| cycle.1.clone())
        );
        activity.old_identifier = uuid_field(Some(cycle_id))?;
        engine.push(activity);
    }
    Ok(())
}

/// `create_module_issue_activity` (`:897-928`).
async fn create_module_issue_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let requested = as_dict(requested.as_ref(), "create_module_issue_activity")?;
    let module_id = get(requested, "module_id");
    let Some(module) = load_name(engine.client, "modules", module_id, engine.project_id()).await?
    else {
        return Ok(());
    };
    let issue = engine.issue()?;
    let Some(issue_id) = issue else {
        return Ok(());
    };
    let found = bump_project_issue(
        engine.client,
        Some(&PyValue::Str(PyStr::new(&issue_id.to_string()))),
        engine.project_id(),
    )
    .await?;
    if !found {
        return Ok(());
    }
    let mut activity = engine.activity(Some(issue_id), "created", Some("modules"))?;
    activity.old_value = Some(String::new());
    activity.new_value = Some(module.1.clone());
    activity.comment = format!("added module {}", module.1);
    activity.new_identifier = match module_id {
        Some(value) if !value.is_null() => uuid_field(Some(value))?,
        _ => None,
    };
    engine.push(activity);
    Ok(())
}

/// `delete_module_issue_activity` (`:931-964`).
async fn delete_module_issue_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let current = parse_json(current.as_ref())?;
    let requested = as_dict(requested.as_ref(), "delete_module_issue_activity")?;
    let current = as_dict(current.as_ref(), "delete_module_issue_activity")?;
    let module_name = get(current, "module_name");
    let issue = engine.issue()?;
    if let Some(issue_id) = issue {
        let found = bump_project_issue(
            engine.client,
            Some(&PyValue::Str(PyStr::new(&issue_id.to_string()))),
            engine.project_id(),
        )
        .await?;
        if !found {
            return Ok(());
        }
    }
    let mut activity = engine.activity(issue, "deleted", Some("modules"))?;
    activity.old_value = text_field(module_name);
    activity.new_value = Some(String::new());
    activity.comment = format!(
        "removed this issue from {}",
        module_name.map_or_else(|| "None".to_string(), py_str)
    );
    activity.old_identifier = uuid_field(get(requested, "module_id"))?;
    engine.push(activity);
    Ok(())
}

/// `create_link_activity` (`:967-993`).
fn create_link_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let _current = parse_json(current.as_ref())?;
    let requested = as_dict(requested.as_ref(), "create_link_activity")?;
    let mut activity = engine.activity(engine.issue()?, "created", Some("link"))?;
    activity.comment = "created a link".to_string();
    activity.new_value = text_field_default(requested, "url");
    activity.new_identifier = uuid_field(get(requested, "id"))?;
    engine.push(activity);
    Ok(())
}

/// `update_link_activity` (`:996-1025`).
fn update_link_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let current = parse_json(current.as_ref())?;
    let requested = as_dict(requested.as_ref(), "update_link_activity")?;
    let current = as_dict(current.as_ref(), "update_link_activity")?;
    if !py_eq_opt(get(current, "url"), get(requested, "url")) {
        let mut activity = engine.activity(engine.issue()?, "updated", Some("link"))?;
        activity.comment = "updated a link".to_string();
        activity.old_value = text_field_default(current, "url");
        activity.old_identifier = uuid_field(get(current, "id"))?;
        activity.new_value = text_field_default(requested, "url");
        activity.new_identifier = uuid_field(get(current, "id"))?;
        engine.push(activity);
    }
    Ok(())
}

/// `delete_link_activity` (`:1028-1053`).
fn delete_link_activity(engine: &mut Engine<'_>, current: &Option<PyValue>) -> R<()> {
    let current = parse_json(current.as_ref())?;
    let current = as_dict(current.as_ref(), "delete_link_activity")?;
    let mut activity = engine.activity(engine.issue()?, "deleted", Some("link"))?;
    activity.comment = "deleted the link".to_string();
    activity.old_value = text_field_default(current, "url");
    activity.new_value = Some(String::new());
    engine.push(activity);
    Ok(())
}

/// `create_attachment_activity` (`:1056-1082`). The new value comes from
/// `current_instance`, not `requested_data` (upstream).
fn create_attachment_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let _requested = parse_json(requested.as_ref())?;
    let current = parse_json(current.as_ref())?;
    let current = as_dict(current.as_ref(), "create_attachment_activity")?;
    let mut activity = engine.activity(engine.issue()?, "created", Some("attachment"))?;
    activity.comment = "created an attachment".to_string();
    activity.new_value = text_field_default(current, "asset");
    activity.new_identifier = uuid_field(get(current, "id"))?;
    engine.push(activity);
    Ok(())
}

/// `delete_attachment_activity` (`:1085-1106`).
fn delete_attachment_activity(engine: &mut Engine<'_>) -> R<()> {
    let mut activity = engine.activity(engine.issue()?, "deleted", Some("attachment"))?;
    activity.comment = "deleted the attachment".to_string();
    engine.push(activity);
    Ok(())
}

/// `create_issue_relation_activity` (`:1316-1367`).
async fn create_issue_relation_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let current = parse_json(current.as_ref())?;
    let requested = as_dict(requested.as_ref(), "create_issue_relation_activity")?;
    if current.as_ref().is_some_and(|value| !value.is_null())
        || requested.get("issues").is_none_or(PyValue::is_null)
    {
        return Ok(());
    }
    let issue_id = engine.issue()?;
    let Some(issue_id) = issue_id else {
        return Ok(());
    };
    let current_issue = issue_label(
        engine.client,
        &issue_id,
        engine.project_id(),
        engine.workspace(),
    )
    .await?;
    let Some(current_issue) = current_issue else {
        return Ok(());
    };
    let Some(PyValue::List(related_issues)) = requested.get("issues") else {
        return python(format!(
            "TypeError: '{}' object is not iterable",
            requested.get("issues").map_or("NoneType", python_type_name)
        ));
    };
    let relation_type = get(requested, "relation_type");
    for related in related_issues {
        let uuid = python_uuid(related)?;
        // `Issue.objects.filter(pk=related_issue, workspace_id=workspace_id)`
        // (`:1334`): relations can span projects, so only the workspace is
        // enforced.
        let Some(issue) = issue_label_any_project(engine.client, &uuid, engine.workspace()).await?
        else {
            continue;
        };
        let mut activity = engine.activity(Some(issue_id), "updated", None)?;
        activity.old_value = Some(String::new());
        activity.new_value = Some(issue.label.clone());
        activity.field = relation_type.map(py_str);
        activity.comment = format!(
            "added {} relation",
            relation_type.map_or_else(|| "None".to_string(), py_str)
        );
        activity.old_identifier = Some(uuid);
        engine.push(activity);

        let inverse = relation_type.map_or_else(
            || "None".to_string(),
            |value| get_inverse_relation(&py_str(value)),
        );
        let mut activity = engine.activity(Some(uuid), "updated", None)?;
        activity.old_value = Some(String::new());
        activity.new_value = Some(current_issue.label.clone());
        activity.field = Some(inverse.clone());
        activity.comment = format!("added {inverse} relation");
        activity.old_identifier = Some(issue_id);
        engine.push(activity);
    }
    Ok(())
}

/// `delete_issue_relation_activity` (`:1370-1423`).
async fn delete_issue_relation_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let _current = parse_json(current.as_ref())?;
    let requested = as_dict(requested.as_ref(), "delete_issue_relation_activity")?;
    let issue_id = engine.issue()?;
    let related = get(requested, "related_issue");
    let Some(related_uuid) = related.map(python_uuid).transpose()? else {
        return Ok(());
    };
    let current_issue = match issue_id {
        Some(issue_id) => {
            issue_label(
                engine.client,
                &issue_id,
                engine.project_id(),
                engine.workspace(),
            )
            .await?
        }
        None => None,
    };
    let Some(current_issue) = current_issue else {
        return Ok(());
    };
    let Some(issue) =
        issue_label_any_project(engine.client, &related_uuid, engine.workspace()).await?
    else {
        return Ok(());
    };
    let relation_type = get(requested, "relation_type");
    let relation_text = relation_type.map_or_else(|| "None".to_string(), py_str);
    let mut activity = engine.activity(issue_id, "deleted", None)?;
    activity.old_value = Some(issue.label.clone());
    activity.new_value = Some(String::new());
    activity.field = relation_type.map(py_str);
    activity.comment = format!("deleted {relation_text} relation");
    activity.old_identifier = uuid_field(related)?;
    engine.push(activity);

    let inverse = if relation_text == "blocked_by" {
        "blocking".to_string()
    } else if relation_text == "blocking" {
        "blocked_by".to_string()
    } else {
        relation_text.clone()
    };
    let mut activity = engine.activity(Some(related_uuid), "deleted", None)?;
    activity.old_value = Some(current_issue.label.clone());
    activity.new_value = Some(String::new());
    activity.field = Some(inverse);
    activity.comment = format!("deleted {relation_text} relation");
    activity.old_identifier = uuid_field(related)?;
    engine.push(activity);
    Ok(())
}

/// `create_issue_reaction_activity` (`:1109-1146`).
async fn create_issue_reaction_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    if !py_truthy(requested.as_ref()) {
        return Ok(());
    }
    let requested = as_dict(requested.as_ref(), "create_issue_reaction_activity")?;
    let Some(reaction) = get(requested, "reaction").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let Some(actor) = engine.actor()? else {
        return Ok(());
    };
    let row = engine
        .client
        .query_opt(
            "SELECT id FROM issue_reactions \
             WHERE reaction = $1 AND project_id = $2 AND actor_id = $3 \
               AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
            &[&py_str(reaction), &engine.project_id(), &actor],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return Ok(());
    };
    let reaction_id: Uuid = row.get(0);
    let mut activity = engine.activity(engine.issue()?, "created", Some("reaction"))?;
    activity.new_value = Some(py_str(reaction));
    activity.comment = "added the reaction".to_string();
    activity.new_identifier = Some(reaction_id);
    engine.push(activity);
    Ok(())
}

/// `delete_issue_reaction_activity` (`:1149-1176`).
fn delete_issue_reaction_activity(engine: &mut Engine<'_>, current: &Option<PyValue>) -> R<()> {
    let current = parse_json(current.as_ref())?;
    if !py_truthy(current.as_ref()) {
        return Ok(());
    }
    let current = as_dict(current.as_ref(), "delete_issue_reaction_activity")?;
    let Some(reaction) = get(current, "reaction").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let mut activity = engine.activity(engine.issue()?, "deleted", Some("reaction"))?;
    activity.old_value = Some(py_str(reaction));
    activity.comment = "removed the reaction".to_string();
    activity.old_identifier = uuid_field(get(current, "identifier"))?;
    engine.push(activity);
    Ok(())
}

/// `create_comment_reaction_activity` (`:1179-1217`).
async fn create_comment_reaction_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    if !py_truthy(requested.as_ref()) {
        return Ok(());
    }
    let requested = as_dict(requested.as_ref(), "create_comment_reaction_activity")?;
    let Some(reaction) = get(requested, "reaction").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let Some(actor) = engine.actor()? else {
        return Ok(());
    };
    let row = engine
        .client
        .query_opt(
            "SELECT comment_reactions.id, issue_comments.id FROM comment_reactions \
             INNER JOIN issue_comments ON issue_comments.id = comment_reactions.comment_id \
             WHERE comment_reactions.reaction = $1 AND comment_reactions.project_id = $2 \
               AND comment_reactions.actor_id = $3 AND comment_reactions.deleted_at IS NULL \
             ORDER BY comment_reactions.created_at DESC LIMIT 1",
            &[&py_str(reaction), &engine.project_id(), &actor],
        )
        .await
        .map_err(db)?;
    // `comment_reaction_id, comment_id = (...).first()` (`:1191-1199`): no row
    // is a `TypeError`, not a skip.
    let Some(row) = row else {
        return python("TypeError: cannot unpack non-iterable NoneType object");
    };
    let reaction_id: Uuid = row.get(0);
    let comment_id: Uuid = row.get(1);
    let comment = engine
        .client
        .query_opt(
            "SELECT id, issue_id FROM issue_comments \
             WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL",
            &[&comment_id, &engine.project_id()],
        )
        .await
        .map_err(db)?;
    let Some(comment) = comment else {
        return python("IssueComment matching query does not exist.");
    };
    let issue_id: Uuid = comment.get(1);
    let mut activity = engine.activity(Some(issue_id), "created", Some("reaction"))?;
    activity.new_value = Some(py_str(reaction));
    activity.comment = "added the reaction".to_string();
    activity.new_identifier = Some(reaction_id);
    engine.push(activity);
    Ok(())
}

/// `delete_comment_reaction_activity` (`:1220-1253`).
async fn delete_comment_reaction_activity(
    engine: &mut Engine<'_>,
    current: &Option<PyValue>,
) -> R<()> {
    let current = parse_json(current.as_ref())?;
    if !py_truthy(current.as_ref()) {
        return Ok(());
    }
    let current = as_dict(current.as_ref(), "delete_comment_reaction_activity")?;
    let Some(reaction) = get(current, "reaction").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let comment_id = get(current, "comment_id");
    let issue_id = match comment_id {
        Some(value) if !value.is_null() => {
            let uuid = python_uuid(value)?;
            engine
                .client
                .query_opt(
                    "SELECT issue_id FROM issue_comments \
                     WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL LIMIT 1",
                    &[&uuid, &engine.project_id()],
                )
                .await
                .map_err(db)?
                .map(|row| row.get::<_, Uuid>(0))
        }
        _ => None,
    };
    let Some(issue_id) = issue_id else {
        return Ok(());
    };
    let mut activity = engine.activity(Some(issue_id), "deleted", Some("reaction"))?;
    activity.old_value = Some(py_str(reaction));
    activity.comment = "removed the reaction".to_string();
    activity.old_identifier = uuid_field(get(current, "identifier"))?;
    engine.push(activity);
    Ok(())
}

/// `create_issue_vote_activity` (`:1256-1283`).
fn create_issue_vote_activity(engine: &mut Engine<'_>, requested: &Option<PyValue>) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    if !py_truthy(requested.as_ref()) {
        return Ok(());
    }
    let requested = as_dict(requested.as_ref(), "create_issue_vote_activity")?;
    let Some(vote) = get(requested, "vote").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let mut activity = engine.activity(engine.issue()?, "updated", Some("vote"))?;
    activity.new_value = Some(py_str(vote));
    activity.comment = "added the vote".to_string();
    engine.push(activity);
    Ok(())
}

/// `delete_issue_vote_activity` (`:1286-1313`).
fn delete_issue_vote_activity(engine: &mut Engine<'_>, current: &Option<PyValue>) -> R<()> {
    let current = parse_json(current.as_ref())?;
    if !py_truthy(current.as_ref()) {
        return Ok(());
    }
    let current = as_dict(current.as_ref(), "delete_issue_vote_activity")?;
    let Some(vote) = get(current, "vote").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let mut activity = engine.activity(engine.issue()?, "deleted", Some("vote"))?;
    activity.old_value = Some(py_str(vote));
    activity.comment = "removed the vote".to_string();
    engine.push(activity);
    Ok(())
}

/// `create_draft_issue_activity` (`:1426-1447`).
fn create_draft_issue_activity(engine: &mut Engine<'_>) -> R<()> {
    let mut activity = engine.activity(engine.issue()?, "created", Some("draft"))?;
    activity.comment = "drafted the issue".to_string();
    engine.push(activity);
    Ok(())
}

/// `update_draft_issue_activity` (`:1450-1486`).
fn update_draft_issue_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let _current = parse_json(current.as_ref())?;
    let requested = as_dict(requested.as_ref(), "update_draft_issue_activity")?;
    // `requested_data.get("is_draft") is not None and ... is False`: identity,
    // not equality, so `0` takes the draft branch.
    let published = matches!(requested.get("is_draft"), Some(PyValue::Bool(false)));
    let mut activity = engine.activity(engine.issue()?, "updated", None)?;
    if published {
        activity.comment = "created the issue".to_string();
    } else {
        activity.comment = "updated the draft issue".to_string();
        activity.field = Some("draft".to_string());
    }
    engine.push(activity);
    Ok(())
}

/// `delete_draft_issue_activity` (`:1489-1509`): no `issue_id` at all.
fn delete_draft_issue_activity(engine: &mut Engine<'_>) -> R<()> {
    let mut activity = engine.activity(None, "deleted", Some("draft"))?;
    activity.comment = "deleted the draft issue".to_string();
    engine.push(activity);
    Ok(())
}

/// `create_intake_activity` (`:1512-1545`).
fn create_intake_activity(
    engine: &mut Engine<'_>,
    requested: &Option<PyValue>,
    current: &Option<PyValue>,
) -> R<()> {
    let requested = parse_json(requested.as_ref())?;
    let current = parse_json(current.as_ref())?;
    let requested = as_dict(requested.as_ref(), "create_intake_activity")?;
    let current = as_dict(current.as_ref(), "create_intake_activity")?;
    let Some(status) = get(requested, "status").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let mut activity = engine.activity(engine.issue()?, "updated", Some("intake"))?;
    activity.verb = py_str(status);
    activity.comment = "updated the intake status".to_string();
    activity.old_value = intake_status(get(current, "status")).map(str::to_owned);
    activity.new_value = intake_status(Some(status)).map(str::to_owned);
    engine.push(activity);
    Ok(())
}

/// `status_dict` (`:1524-1530`) with Python `dict` key equality (`True == 1`,
/// `1.0 == 1`).
fn intake_status(value: Option<&PyValue>) -> Option<&'static str> {
    let value = value?;
    let code = match value {
        PyValue::Null => return None,
        PyValue::Bool(value) => i64::from(u8::from(*value)),
        PyValue::Int(value) => value.as_i64()?,
        PyValue::Float(value) => {
            if value.fract() != 0.0 || !value.is_finite() {
                return None;
            }
            *value as i64
        }
        _ => return None,
    };
    match code {
        -2 => Some("Pending"),
        -1 => Some("Rejected"),
        0 => Some("Snoozed"),
        1 => Some("Accepted"),
        2 => Some("Duplicate"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Builder helpers
// ---------------------------------------------------------------------------

fn py_eq_opt(left: Option<&PyValue>, right: Option<&PyValue>) -> bool {
    match (left, right) {
        (None, None) => true,
        (None, Some(PyValue::Null)) | (Some(PyValue::Null), None) => true,
        (Some(left), Some(right)) => py_eq(left, right),
        _ => false,
    }
}

/// Python's `a or b` for two optional JSON values.
fn py_or<'a>(left: Option<&'a PyValue>, right: Option<&'a PyValue>) -> Option<&'a PyValue> {
    match left {
        Some(value) if py_truthy(Some(value)) => Some(value),
        _ => right,
    }
}

/// `is_valid_uuid(value)` where a non-string argument is `AttributeError`.
fn valid_uuid_value(value: &PyValue) -> R<bool> {
    match value {
        PyValue::Str(text) => Ok(is_valid_uuid(&text.to_string_lossy())),
        other => python(format!(
            "AttributeError: '{}' object has no attribute 'replace'",
            python_type_name(other)
        )),
    }
}

fn extract_ids_from_dict(
    data: Option<&PyDict>,
    primary_key: &str,
    fallback_key: &str,
) -> R<BTreeSet<String>> {
    match data {
        None => Ok(BTreeSet::new()),
        Some(dict) => extract_ids(
            Some(&PyValue::Dict(dict.clone())),
            primary_key,
            fallback_key,
        ),
    }
}

/// `get_inverse_relation` (`plane/utils/issue_relation_mapper.py:8-19`).
fn get_inverse_relation(relation: &str) -> String {
    match relation {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocked_by" => "blocking",
        "blocking" => "blocked_by",
        "start_before" => "start_after",
        "finish_before" => "finish_after",
        "implemented_by" => "implements",
        "implements" => "implemented_by",
        other => other,
    }
    .to_string()
}

struct LabeledIssue {
    id: Uuid,
    label: String,
}

/// `Issue.objects.filter(pk=…, project_id=…, workspace_id=…).first()` with
/// `f"{issue.project.identifier}-{issue.sequence_id}"`.
async fn issue_label(
    client: &Client,
    issue_id: &Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
) -> R<Option<LabeledIssue>> {
    let row = client
        .query_opt(
            "SELECT issues.id, projects.identifier, issues.sequence_id FROM issues \
             INNER JOIN projects ON projects.id = issues.project_id \
             WHERE issues.id = $1 AND issues.project_id = $2 AND issues.workspace_id = $3 \
               AND issues.deleted_at IS NULL LIMIT 1",
            &[issue_id, &project_id, &workspace_id],
        )
        .await
        .map_err(db)?;
    Ok(row.map(|row| LabeledIssue {
        id: row.get(0),
        label: format!("{}-{}", row.get::<_, String>(1), row.get::<_, i32>(2)),
    }))
}

/// `Issue.objects.filter(pk=…, workspace_id=…).first()` (relations can cross
/// projects).
async fn issue_label_any_project(
    client: &Client,
    issue_id: &Uuid,
    workspace_id: Uuid,
) -> R<Option<LabeledIssue>> {
    let row = client
        .query_opt(
            "SELECT issues.id, projects.identifier, issues.sequence_id FROM issues \
             INNER JOIN projects ON projects.id = issues.project_id \
             WHERE issues.id = $1 AND issues.workspace_id = $2 \
               AND issues.deleted_at IS NULL LIMIT 1",
            &[issue_id, &workspace_id],
        )
        .await
        .map_err(db)?;
    Ok(row.map(|row| LabeledIssue {
        id: row.get(0),
        label: format!("{}-{}", row.get::<_, String>(1), row.get::<_, i32>(2)),
    }))
}

/// `Issue.objects.filter(pk=…, project_id=…).first()` for `track_parent`.
async fn issue_identifier(
    client: &Client,
    issue_id: &Uuid,
    project_id: Uuid,
) -> R<Option<LabeledIssue>> {
    let row = client
        .query_opt(
            "SELECT issues.id, projects.identifier, issues.sequence_id FROM issues \
             INNER JOIN projects ON projects.id = issues.project_id \
             WHERE issues.id = $1 AND issues.project_id = $2 \
               AND issues.deleted_at IS NULL LIMIT 1",
            &[issue_id, &project_id],
        )
        .await
        .map_err(db)?;
    Ok(row.map(|row| LabeledIssue {
        id: row.get(0),
        label: format!("{}-{}", row.get::<_, String>(1), row.get::<_, i32>(2)),
    }))
}

/// `Cycle.objects.filter(pk=…, project_id=…).first()`.
async fn load_cycle(
    client: &Client,
    id: Option<&PyValue>,
    project_id: Uuid,
) -> R<Option<(Uuid, String)>> {
    load_name(client, "cycles", id, project_id).await
}
// ---------------------------------------------------------------------------
// `IssueActivitySerializer` for the `notifications` payload
// ---------------------------------------------------------------------------

/// `json.dumps(IssueActivitySerializer(rows, many=True).data,
/// cls=DjangoJSONEncoder)` (`:1639-1642`).
///
/// Field order is DRF's: the declared fields (`BaseSerializer.id`, then the
/// serializer's own `actor_detail`, `issue_detail`, `project_detail`,
/// `workspace_detail`, `source_data`), then the model fields in `_meta` order,
/// then the forward relations last. Verified against the pinned DRF with
/// `IssueActivitySerializer().fields`.
async fn serialize_activities(client: &Client, inserted: &[Inserted]) -> R<PyValue> {
    let mut items = Vec::with_capacity(inserted.len());
    let mut users: BTreeMap<Uuid, PyValue> = BTreeMap::new();
    let mut issues: BTreeMap<Uuid, PyValue> = BTreeMap::new();
    let mut projects: BTreeMap<Uuid, PyValue> = BTreeMap::new();
    let mut workspaces: BTreeMap<Uuid, PyValue> = BTreeMap::new();

    for inserted in inserted {
        let row = &inserted.row;
        let actor_detail = match row.actor_id {
            Some(actor) => {
                if let Some(cached) = users.get(&actor) {
                    cached.clone()
                } else {
                    let value = user_lite(client, actor).await?;
                    users.insert(actor, value.clone());
                    value
                }
            }
            None => PyValue::Null,
        };
        let issue_detail = match row.issue_id {
            Some(issue) => {
                if let Some(cached) = issues.get(&issue) {
                    cached.clone()
                } else {
                    let value = issue_flat(client, issue).await?;
                    issues.insert(issue, value.clone());
                    value
                }
            }
            None => PyValue::Null,
        };
        let project_detail = if let Some(cached) = projects.get(&row.project_id) {
            cached.clone()
        } else {
            let value = project_lite(client, row.project_id).await?;
            projects.insert(row.project_id, value.clone());
            value
        };
        let workspace_detail = if let Some(cached) = workspaces.get(&row.workspace_id) {
            cached.clone()
        } else {
            let value = workspace_lite(client, row.workspace_id).await?;
            workspaces.insert(row.workspace_id, value.clone());
            value
        };

        items.push(py_map(vec![
            ("id", py_string(row.id.to_string())),
            ("actor_detail", actor_detail),
            ("issue_detail", issue_detail),
            ("project_detail", project_detail),
            ("workspace_detail", workspace_detail),
            // `get_source_data` (`app/serializers/issue.py:348-357`): `Issue`
            // has no `source_data` attribute in this revision.
            ("source_data", PyValue::Null),
            ("created_at", py_string(drf_datetime(inserted.created_at))),
            ("updated_at", py_string(drf_datetime(inserted.updated_at))),
            ("deleted_at", PyValue::Null),
            ("verb", py_string(row.verb.clone())),
            ("field", py_opt_string(row.field.clone())),
            ("old_value", py_opt_string(row.old_value.clone())),
            ("new_value", py_opt_string(row.new_value.clone())),
            ("comment", py_string(row.comment.clone())),
            ("attachments", PyValue::List(Vec::new())),
            ("old_identifier", py_opt_uuid(row.old_identifier)),
            ("new_identifier", py_opt_uuid(row.new_identifier)),
            (
                "epoch",
                inserted.epoch.map_or(PyValue::Null, PyValue::Float),
            ),
            ("created_by", PyValue::Null),
            ("updated_by", PyValue::Null),
            ("project", py_string(row.project_id.to_string())),
            ("workspace", py_string(row.workspace_id.to_string())),
            ("issue", py_opt_uuid(row.issue_id)),
            ("issue_comment", py_opt_uuid(row.issue_comment_id)),
            ("actor", py_opt_uuid(row.actor_id)),
        ]));
    }
    Ok(PyValue::List(items))
}

fn py_string(text: String) -> PyValue {
    PyValue::Str(PyStr::new(&text))
}

fn py_opt_string(text: Option<String>) -> PyValue {
    text.map_or(PyValue::Null, py_string)
}

fn py_opt_uuid(value: Option<Uuid>) -> PyValue {
    value.map_or(PyValue::Null, |value| py_string(value.to_string()))
}

fn py_map(pairs: Vec<(&str, PyValue)>) -> PyValue {
    let mut dict = PyDict::with_capacity(pairs.len());
    for (key, value) in pairs {
        dict.insert(PyStr::new(key), value);
    }
    PyValue::Dict(dict)
}

/// DRF's `DateTimeField.to_representation`: Python `isoformat()` with
/// `+00:00` replaced by `Z`; microseconds appear only when non-zero.
fn drf_datetime(value: DateTime<Utc>) -> String {
    let base = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        value.year(),
        value.month(),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    );
    let micros = value.nanosecond() / 1_000;
    if micros == 0 {
        format!("{base}Z")
    } else {
        format!("{base}.{micros:06}Z")
    }
}

fn drf_date(value: NaiveDate) -> String {
    value.format("%Y-%m-%d").to_string()
}

/// `UserLiteSerializer` (`app/serializers/user.py:155-167`).
async fn user_lite(client: &Client, user_id: Uuid) -> R<PyValue> {
    let row = client
        .query_opt(
            "SELECT id, first_name, last_name, avatar, avatar_asset_id, is_bot, \
                    display_name FROM users WHERE id = $1",
            &[&user_id],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return Ok(PyValue::Null);
    };
    let avatar: String = row.get(3);
    let avatar_asset: Option<Uuid> = row.get(4);
    let avatar_url = if let Some(asset) = avatar_asset {
        static_asset_url(client, asset).await?
    } else if !avatar.is_empty() {
        py_string(avatar.clone())
    } else {
        PyValue::Null
    };
    Ok(py_map(vec![
        ("id", py_string(row.get::<_, Uuid>(0).to_string())),
        ("first_name", py_string(row.get(1))),
        ("last_name", py_string(row.get(2))),
        ("avatar", py_string(avatar)),
        ("avatar_url", avatar_url),
        ("is_bot", PyValue::Bool(row.get(5))),
        ("display_name", py_string(row.get(6))),
    ]))
}

/// `IssueFlatSerializer` (`app/serializers/issue.py:52-68`).
async fn issue_flat(client: &Client, issue_id: Uuid) -> R<PyValue> {
    let row = client
        .query_opt(
            "SELECT id, name, description_json::text, description_html, priority, \
                    start_date, target_date, sequence_id, sort_order, is_draft \
             FROM issues WHERE id = $1",
            &[&issue_id],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return Ok(PyValue::Null);
    };
    let description_json = database_json(row.get(2))?;
    let start_date: Option<NaiveDate> = row.get(5);
    let target_date: Option<NaiveDate> = row.get(6);
    Ok(py_map(vec![
        ("id", py_string(row.get::<_, Uuid>(0).to_string())),
        ("name", py_string(row.get(1))),
        ("description_json", description_json),
        ("description_html", py_string(row.get(3))),
        ("priority", py_string(row.get(4))),
        (
            "start_date",
            start_date.map_or(PyValue::Null, |date| py_string(drf_date(date))),
        ),
        (
            "target_date",
            target_date.map_or(PyValue::Null, |date| py_string(drf_date(date))),
        ),
        (
            "sequence_id",
            PyValue::from(i64::from(row.get::<_, i32>(7))),
        ),
        ("sort_order", PyValue::Float(row.get(8))),
        ("is_draft", PyValue::Bool(row.get(9))),
    ]))
}

/// `ProjectLiteSerializer` (`app/serializers/project.py:113-125`).
async fn project_lite(client: &Client, project_id: Uuid) -> R<PyValue> {
    let row = client
        .query_opt(
            "SELECT id, identifier, name, cover_image, cover_image_asset_id, \
                    logo_props::text, description FROM projects WHERE id = $1",
            &[&project_id],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return Ok(PyValue::Null);
    };
    let cover_image: Option<String> = row.get(3);
    let cover_asset: Option<Uuid> = row.get(4);
    let cover_image_url = if let Some(asset) = cover_asset {
        static_asset_url(client, asset).await?
    } else if let Some(cover) = cover_image.as_ref().filter(|value| !value.is_empty()) {
        py_string(cover.clone())
    } else {
        PyValue::Null
    };
    Ok(py_map(vec![
        ("id", py_string(row.get::<_, Uuid>(0).to_string())),
        ("identifier", py_string(row.get(1))),
        ("name", py_string(row.get(2))),
        ("cover_image", py_opt_string(cover_image)),
        ("cover_image_url", cover_image_url),
        ("logo_props", database_json(row.get(5))?),
        ("description", py_string(row.get(6))),
    ]))
}

/// `WorkspaceLiteSerializer` (`app/serializers/workspace.py:87-91`).
async fn workspace_lite(client: &Client, workspace_id: Uuid) -> R<PyValue> {
    let row = client
        .query_opt(
            "SELECT name, slug, id, logo, logo_asset_id FROM workspaces WHERE id = $1",
            &[&workspace_id],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return Ok(PyValue::Null);
    };
    let logo: Option<String> = row.get(3);
    let logo_asset: Option<Uuid> = row.get(4);
    let logo_url = if let Some(asset) = logo_asset {
        static_asset_url(client, asset).await?
    } else if let Some(logo) = logo.as_ref().filter(|value| !value.is_empty()) {
        py_string(logo.clone())
    } else {
        PyValue::Null
    };
    Ok(py_map(vec![
        ("name", py_string(row.get(0))),
        ("slug", py_string(row.get(1))),
        ("id", py_string(row.get::<_, Uuid>(2).to_string())),
        ("logo_url", logo_url),
    ]))
}

/// `FileAsset.asset_url` (`plane/db/models/asset.py:83-104`).
async fn asset_url(client: &Client, asset_id: Uuid) -> R<PyValue> {
    let row = client
        .query_opt(
            "SELECT entity_type, workspace_id, project_id, issue_id FROM file_assets \
             WHERE id = $1",
            &[&asset_id],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return Ok(PyValue::Null);
    };
    let entity_type: Option<String> = row.get(0);
    let workspace_id: Option<Uuid> = row.get(1);
    let project_id: Option<Uuid> = row.get(2);
    let issue_id: Option<Uuid> = row.get(3);
    let Some(entity_type) = entity_type else {
        return Ok(PyValue::Null);
    };
    match entity_type.as_str() {
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER" => {
            Ok(py_string(format!("/api/assets/v2/static/{asset_id}/")))
        }
        "ISSUE_ATTACHMENT" => {
            let Some(workspace_id) = workspace_id else {
                return Ok(PyValue::Null);
            };
            let Some(slug) = workspace_slug(client, workspace_id).await? else {
                return Ok(PyValue::Null);
            };
            Ok(py_string(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{}/issues/{}/attachments/{asset_id}/",
                project_id.map_or_else(|| "None".to_string(), |id| id.to_string()),
                issue_id.map_or_else(|| "None".to_string(), |id| id.to_string()),
            )))
        }
        "ISSUE_DESCRIPTION"
        | "COMMENT_DESCRIPTION"
        | "PAGE_DESCRIPTION"
        | "DRAFT_ISSUE_DESCRIPTION" => {
            let Some(workspace_id) = workspace_id else {
                return Ok(PyValue::Null);
            };
            let Some(slug) = workspace_slug(client, workspace_id).await? else {
                return Ok(PyValue::Null);
            };
            Ok(py_string(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{}/{asset_id}/",
                project_id.map_or_else(|| "None".to_string(), |id| id.to_string()),
            )))
        }
        _ => Ok(PyValue::Null),
    }
}

async fn static_asset_url(client: &Client, asset_id: Uuid) -> R<PyValue> {
    asset_url(client, asset_id).await
}

async fn workspace_slug(client: &Client, workspace_id: Uuid) -> R<Option<String>> {
    let row = client
        .query_opt(
            "SELECT slug FROM workspaces WHERE id = $1",
            &[&workspace_id],
        )
        .await
        .map_err(db)?;
    Ok(row.map(|row| row.get(0)))
}

/// A `jsonb`/`json` column as `psycopg` returns it: `json.loads(text)` keeps
/// the database's key order.
fn database_json(text: Option<String>) -> R<PyValue> {
    let Some(text) = text else {
        return Ok(PyValue::Null);
    };
    pyjson::loads(text.as_bytes(), "utf-8").map_err(Failure::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_uuid_accepts_version_four_only() {
        assert!(is_valid_uuid("11111111-1111-4111-8111-111111111111"));
        assert!(!is_valid_uuid("11111111-1111-1111-8111-111111111111"));
        assert!(!is_valid_uuid("33333333-3333-3333-3333-333333333333"));
        assert!(!is_valid_uuid("not-a-uuid"));
        assert!(is_valid_uuid("11111111111141118111111111111111"));
    }

    #[test]
    fn python_str_spells_values_the_way_django_stores_them() {
        assert_eq!(py_str(&PyValue::Null), "None");
        assert_eq!(py_str(&PyValue::Bool(true)), "True");
        assert_eq!(py_str(&PyValue::from(7)), "7");
        assert_eq!(py_str(&PyValue::Float(1.0)), "1.0");
        assert_eq!(py_str(&PyValue::Float(f64::NAN)), "nan");
        assert_eq!(
            py_str(&PyValue::List(vec![
                PyValue::from(1),
                PyValue::Str(PyStr::new("a")),
                PyValue::Bool(true),
            ])),
            "[1, 'a', True]"
        );
    }

    #[test]
    fn intake_status_uses_python_dict_key_equality() {
        assert_eq!(intake_status(Some(&PyValue::from(-2))), Some("Pending"));
        assert_eq!(intake_status(Some(&PyValue::Bool(true))), Some("Accepted"));
        assert_eq!(intake_status(Some(&PyValue::Float(1.0))), Some("Accepted"));
        assert_eq!(intake_status(Some(&PyValue::Float(1.5))), None);
        assert_eq!(intake_status(Some(&PyValue::Str(PyStr::new("1")))), None);
    }

    #[test]
    fn inverse_relation_matches_the_python_mapping() {
        assert_eq!(get_inverse_relation("blocked_by"), "blocking");
        assert_eq!(get_inverse_relation("blocking"), "blocked_by");
        assert_eq!(get_inverse_relation("start_before"), "start_after");
        assert_eq!(get_inverse_relation("relates_to"), "relates_to");
        assert_eq!(get_inverse_relation("unknown"), "unknown");
    }

    #[test]
    fn drf_datetimes_omit_a_zero_microsecond() {
        let value = DateTime::parse_from_rfc3339("2026-11-19T05:22:50Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        assert_eq!(drf_datetime(value), "2026-11-19T05:22:50Z");
        let value = DateTime::parse_from_rfc3339("2026-11-19T05:22:50.670790Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        assert_eq!(drf_datetime(value), "2026-11-19T05:22:50.670790Z");
    }

    #[test]
    fn a_uuid_hint_recreates_the_celery_envelope() {
        let id = "11111111-1111-4111-8111-111111111111";
        let value = PyValue::Str(PyStr::new(id));
        assert_eq!(
            task_arg_typed(Some(&value), true),
            TaskArg::Uuid(Uuid::parse_str(id).expect("uuid"))
        );
        assert_eq!(
            task_arg_typed(Some(&value), false),
            TaskArg::Str(id.to_string())
        );
        // A non-UUID string keeps its string type even with a hint.
        let other = PyValue::Str(PyStr::new("not-a-uuid"));
        assert_eq!(
            task_arg_typed(Some(&other), true),
            TaskArg::Str("not-a-uuid".to_string())
        );
    }

    #[test]
    fn py_eq_follows_python_number_and_dict_equality() {
        assert!(py_eq(&PyValue::from(1), &PyValue::Float(1.0)));
        assert!(py_eq(&PyValue::Bool(true), &PyValue::from(1)));
        let mut left = PyDict::new();
        left.insert(PyStr::new("a"), PyValue::from(1));
        left.insert(PyStr::new("b"), PyValue::from(2));
        let mut right = PyDict::new();
        right.insert(PyStr::new("b"), PyValue::from(2));
        right.insert(PyStr::new("a"), PyValue::from(1));
        assert!(py_eq(&PyValue::Dict(left), &PyValue::Dict(right)));
        assert!(!py_eq(&PyValue::Float(f64::NAN), &PyValue::Float(f64::NAN)));
    }
}
