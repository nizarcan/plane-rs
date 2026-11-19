// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Tasks 12 and 13: `stack_email_notification` and `send_email_notification`
//! (`plane/bgtasks/email_notification_task.py`).
//!
//! | task | Python | side effects |
//! |---|---|---|
//! | 12 `stack_email_notification` | `:47-85` | reads unprocessed `EmailNotificationLog` rows, publishes one 13 per (receiver, issue), stamps `processed_at` |
//! | 13 `send_email_notification` | `:153-307` | Redis `SET NX EX 300`, reads the base URL under the issue id, renders `issue-updates.html`, sends one e-mail, stamps `sent_at` |
//!
//! # 12's grouping
//!
//! `:49-84`: rows are selected `processed_at IS NULL ORDER BY receiver` and
//! `values()` materialised. Receivers are `list(set(str(receiver_id) …))` — a
//! Python `set` of strings, whose iteration order is randomised per process
//! (`PYTHONHASHSEED`). Within a receiver the row order is the query's
//! (unspecified for equal receivers, in practice the heap order both clones
//! share). The handler keeps first appearance: one receiver group per
//! distinct id, rows in query order. A multi-receiver fixture therefore has a
//! message order Python does not guarantee; the cases use one receiver.
//!
//! `notification_data` is `{str(triggered_by_id): [data, …]}` with `data` the
//! JSON column; the issue key is `entity_identifier`, a `UUID` object in
//! Python (kombu's `uuid` envelope on the wire) and possibly `None`.
//!
//! # The queue keeps dict key order
//!
//! The `plane_jobs` payload is a `json` column, so the producer's text and key
//! order survive the queue (`plane_jobs::payload`). `create_payload` iterates
//! the ordered `notification_data`, so `data.0` and the comments follow the
//! producer's order exactly as Django's `dict.items()` does. The fixtures use
//! actor ids whose uuid order differs from the row order (`p8b-en-send-order`),
//! which would have failed while the column was `jsonb`.
//!
//! # 13's lock and its asymmetric release
//!
//! `:154-166`: the key is
//! `send_email_notif_<issue>_<receiver>_<sorted ids joined by _>`, written
//! with `SET NX EX 300` and the two bytes `true` (a raw redis-py connection,
//! not django-redis, so no pickle). Every exit releases it **except** the
//! missing-base-URL branch (`:168-171`): that one returns with the lock still
//! held for its remaining TTL. Ported as is, and observable in the Redis diff.
//!
//! # Ported Python details
//!
//! * `create_payload`'s `if not data.get("actor_id", {})…` guard
//!   (`:120-126`) reads the literal key `"actor_id"`, never the actor it just
//!   wrote, so `activity_time` is assigned on every change and the last one
//!   wins. When both values are empty the actor key may not exist and the
//!   assignment raises `KeyError`, failing the task: ported.
//! * `str(None)` is `"None"`, which is **truthy** and therefore stored as a
//!   value (`:95-96`); `create_payload` receives `str(...)`-ed log values.
//! * `issue.project.identifier`/`issue.project.workspace.slug` are lazy
//!   foreign keys; the three queries are reproduced as one join.
//! * `User.avatar_url` (`db/models/user.py:143-151`) reads `avatar_asset`
//!   (a `_base_manager` join, no soft-delete filter) and falls back to
//!   `avatar`; `None` is interpolated into an f-string as `"None"`
//!   (`:200,216`), which the template prints.
//! * `process_mention` (`:130-140`) is `BeautifulSoup(html, "html.parser")`,
//!   `find_all("mention-component")`, `mention["entity_identifier"]`
//!   (`KeyError` when absent), `User.objects.get`, and
//!   `replace_with("@<display_name>")`; the result is `str(soup)` — the bs4
//!   "minimal" serialisation `crates/plane-tasks/src/bs4.rs` already ports
//!   (D29).
//! * The subject strips C0/C1 control characters only
//!   (`:27-31`, `re.sub(r"[\x00-\x1F\x7F-\x9F]", "", …)`).
//! * `EmailNotificationLog.objects.filter(pk__in=…).update(sent_at=…)` does
//!   not bump `updated_at` (`QuerySet.update` bypasses `auto_now`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::NaiveDateTime;
use deadpool_postgres::Client;
use plane_db::models::JsonText;
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::{Message, task_name};
use plane_jobs::payload::Payload;
use plane_jobs::queue::Job;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_serde::pyjson::{self, Value as PyValue};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{TaskContext, publish_or_enqueue};

/// The per-issue half of 12's grouping: `{str(triggered_by_id): [data, …]}` in
/// first-appearance order.
type IssueActors = Vec<(String, Vec<PyValue>)>;
/// `{entity_identifier: IssueActors}` in first-appearance order.
type GroupedPayload = Vec<(Option<Uuid>, IssueActors)>;
/// `create_payload`'s result: `{actor_id: {field: {"old_value"/"new_value"}}}`
/// in payload order, with `activity_time` stored like any other key.
type ActorChanges = Vec<(String, Map<String, Value>)>;

/// 12 — `stack_email_notification`.
pub const STACK: &str = "plane.bgtasks.email_notification_task.stack_email_notification";
/// 13 — `send_email_notification`.
pub const SEND: &str = "plane.bgtasks.email_notification_task.send_email_notification";

/// Register both handlers.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    for task in [STACK, SEND] {
        let context = Arc::clone(&context);
        registry.register(task, move |job| {
            let context = Arc::clone(&context);
            Box::pin(async move { handle(&context, job).await })
        });
    }
}

async fn handle(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    match job.task.as_str() {
        STACK => stack_email_notification(context, job).await,
        SEND => send_email_notification(context, job).await,
        other => Err(HandlerError::fatal(format!(
            "{other} is not an email-notification task"
        ))),
    }
}

// ---------------------------------------------------------------------------
// 12 — stack_email_notification
// ---------------------------------------------------------------------------

/// One unprocessed `EmailNotificationLog` row, in query order.
struct LogRow {
    id: Uuid,
    receiver_id: Uuid,
    triggered_by_id: Uuid,
    entity_identifier: Option<Uuid>,
    data: PyValue,
}

/// `stack_email_notification` (`email_notification_task.py:47-85`). The task
/// has no `try`/`except`: a database or broker failure is a Celery `FAILURE`,
/// and the Rust handler reports a fatal error so the job is dead-lettered.
async fn stack_email_notification(
    context: &TaskContext,
    job: JobContext,
) -> Result<(), HandlerError> {
    let rows = load_unprocessed(&job.client).await?;
    let mut processed: Vec<Uuid> = Vec::new();
    let mut receivers: Vec<(Uuid, Vec<&LogRow>)> = Vec::new();
    for row in &rows {
        match receivers
            .iter_mut()
            .find(|(receiver, _)| *receiver == row.receiver_id)
        {
            Some((_, group)) => group.push(row),
            None => receivers.push((row.receiver_id, vec![row])),
        }
    }
    for (receiver_id, group) in &receivers {
        // `payload.setdefault(entity_identifier, {}).setdefault(str(triggered_by_id), []).append(data)`
        let mut payload: GroupedPayload = Vec::new();
        let mut ids: Vec<Uuid> = Vec::new();
        for row in group {
            let issue = match payload
                .iter_mut()
                .find(|(issue, _)| *issue == row.entity_identifier)
            {
                Some((_, actors)) => actors,
                None => {
                    payload.push((row.entity_identifier, Vec::new()));
                    &mut payload.last_mut().expect("just pushed").1
                }
            };
            let actor = row.triggered_by_id.hyphenated().to_string();
            match issue.iter_mut().find(|(id, _)| *id == actor) {
                Some((_, values)) => values.push(row.data.clone()),
                None => issue.push((actor, vec![row.data.clone()])),
            }
            processed.push(row.id);
            ids.push(row.id);
        }
        let receiver = receiver_id.hyphenated().to_string();
        for (issue_id, actors) in payload {
            let notification_data = TaskArg::map(
                actors
                    .into_iter()
                    .map(|(actor, values)| {
                        (actor, TaskArg::list(values.into_iter().map(TaskArg::json)))
                    })
                    .collect::<Vec<_>>(),
            );
            let message = send_message(issue_id, notification_data, &receiver, &ids);
            let enqueued = Job::from_message(&message);
            publish_or_enqueue(context, &**job.client, SEND, &message, enqueued).await?;
        }
    }
    stamp_processed(&job.client, &processed).await?;
    Ok(())
}

/// `send_email_notification.delay(...)` (`:75-82`).
///
/// `issue_id` is a `UUID` object in Python and `email_notification_ids` a
/// `list[UUID]`; `receiver_id` is the `str()` the outer comprehension produced.
fn send_message(
    issue_id: Option<Uuid>,
    notification_data: TaskArg,
    receiver_id: &str,
    ids: &[Uuid],
) -> Message {
    let kwargs = vec![
        (
            "issue_id",
            match issue_id {
                Some(id) => TaskArg::uuid(id),
                None => TaskArg::Null,
            },
        ),
        ("notification_data", notification_data),
        ("receiver_id", TaskArg::str(receiver_id)),
        (
            "email_notification_ids",
            TaskArg::list(ids.iter().map(|id| TaskArg::uuid(*id))),
        ),
    ];
    Message::new(task_name::SEND_EMAIL_NOTIFICATION, Vec::new(), kwargs)
}

async fn load_unprocessed(client: &Client) -> Result<Vec<LogRow>, HandlerError> {
    let rows = client
        .query(
            "SELECT id, receiver_id, triggered_by_id, entity_identifier, data \
             FROM email_notification_logs WHERE processed_at IS NULL \
             ORDER BY receiver_id",
            &[],
        )
        .await
        .map_err(|error| HandlerError::fatal(error.to_string()))?;
    rows.iter()
        .map(|row| {
            let id: Uuid = row
                .try_get(0)
                .map_err(|error| HandlerError::fatal(error.to_string()))?;
            let receiver_id: Uuid = row
                .try_get(1)
                .map_err(|error| HandlerError::fatal(error.to_string()))?;
            let triggered_by_id: Uuid = row
                .try_get(2)
                .map_err(|error| HandlerError::fatal(error.to_string()))?;
            let entity_identifier: Option<Uuid> = row
                .try_get(3)
                .map_err(|error| HandlerError::fatal(error.to_string()))?;
            let data: Option<JsonText> = row
                .try_get(4)
                .map_err(|error| HandlerError::fatal(error.to_string()))?;
            let data = match data {
                Some(text) => pyjson::loads(text.as_str().as_bytes(), "utf-8")
                    .map_err(|error| HandlerError::fatal(error.to_string()))?,
                None => PyValue::Null,
            };
            Ok(LogRow {
                id,
                receiver_id,
                triggered_by_id,
                entity_identifier,
                data,
            })
        })
        .collect()
}

/// `EmailNotificationLog.objects.filter(pk__in=processed_notifications)
/// .update(processed_at=timezone.now())` (`:84`).
async fn stamp_processed(client: &Client, ids: &[Uuid]) -> Result<(), HandlerError> {
    if ids.is_empty() {
        return Ok(());
    }
    client
        .execute(
            "UPDATE email_notification_logs SET processed_at = now() WHERE id = ANY($1)",
            &[&ids],
        )
        .await
        .map_err(|error| HandlerError::fatal(error.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 13 — send_email_notification
// ---------------------------------------------------------------------------

/// The decoded 13 payload.
struct SendArgs {
    /// The payload's string form. Python's argument is a `UUID`; it is parsed
    /// where the ORM would coerce it (inside the try), so a bad value releases
    /// the lock like any other lookup failure.
    issue_id: String,
    /// The ordered actor dict, as the producer wrote it. Its iteration order
    /// decides `data.0` and the comments' order in the e-mail.
    notification_data: PyValue,
    receiver_id: String,
    ids: Vec<String>,
}

/// `send_email_notification` (`email_notification_task.py:153-307`).
///
/// Argument decoding happens before the try in Python (`:155-158`), so a bad
/// payload is a `FAILURE`; every later failure is logged and swallowed.
async fn send_email_notification(
    context: &TaskContext,
    job: JobContext,
) -> Result<(), HandlerError> {
    let Some(args) = decode_send(&job.payload) else {
        return Err(HandlerError::fatal(
            "send_email_notification payload is not (issue_id, notification_data, receiver_id, email_notification_ids)",
        ));
    };
    let Some(redis) = &context.redis else {
        return Err(crate::missing_component("redis"));
    };
    let lock_id = lock_id(&args);
    // `acquire_lock` (`:34-37`).
    let acquired = match redis.set_bytes_nx_ex(&lock_id, b"true", 300).await {
        Ok(acquired) => acquired,
        Err(error) => {
            crate::swallow(error, SEND);
            return Ok(());
        }
    };
    if !acquired {
        // `:296-298`: a duplicate delivery logs and returns.
        return Ok(());
    }
    let outcome = run_send(context, &job.client, &args).await;
    match outcome {
        // `:169-171`: no base URL returns **without** releasing the lock.
        Ok(SendOutcome::NoBaseUrl) => Ok(()),
        Ok(SendOutcome::Sent) => {
            let _ = redis.del(&lock_id).await;
            Ok(())
        }
        Err(error) => {
            crate::swallow(error, SEND);
            let _ = redis.del(&lock_id).await;
            Ok(())
        }
    }
}

enum SendOutcome {
    NoBaseUrl,
    Sent,
}

fn decode_send(payload: &Payload) -> Option<SendArgs> {
    let Some(PyValue::Dict(kwargs)) = payload.py_at(&["kwargs"]) else {
        return None;
    };
    let text = |name: &str| -> Option<String> {
        match kwargs.get(name)? {
            PyValue::Str(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        }
    };
    let PyValue::List(items) = kwargs.get("email_notification_ids")? else {
        return None;
    };
    let ids = items
        .iter()
        .map(|item| match item {
            PyValue::Str(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Option<Vec<String>>>()?;
    Some(SendArgs {
        issue_id: text("issue_id")?,
        notification_data: kwargs.get("notification_data")?.clone(),
        receiver_id: text("receiver_id")?,
        ids,
    })
}

/// `sorted_ids = sorted(email_notification_ids); ids_str = "_".join(str(id) …)`
/// (`:154-158`) and `lock_id = f"send_email_notif_{issue_id}_…"`.
fn lock_id(args: &SendArgs) -> String {
    let mut ids: Vec<String> = args
        .ids
        .iter()
        .map(|id| uuid_text(id).unwrap_or_else(|| id.clone()))
        .collect();
    ids.sort();
    let joined = ids.join("_");
    format!(
        "send_email_notif_{}_{}_{}",
        args.issue_id, args.receiver_id, joined
    )
}

/// `str(uuid.UUID)` for a string that parses; the string itself otherwise
/// (Python only sees the canonical spelling when the value really is a UUID).
fn uuid_text(text: &str) -> Option<String> {
    Uuid::parse_str(text)
        .ok()
        .map(|id| id.hyphenated().to_string())
}

async fn run_send(
    context: &TaskContext,
    client: &Client,
    args: &SendArgs,
) -> Result<SendOutcome, String> {
    let redis = context
        .redis
        .as_ref()
        .ok_or_else(|| "redis is not configured".to_string())?;
    let issue_key = args.issue_id.clone();
    let base_api = redis
        .get_bytes(&issue_key)
        .await
        .map_err(|error| error.to_string())?
        .map(|bytes| String::from_utf8(bytes).map_err(|error| error.to_string()))
        .transpose()?;
    let Some(base_api) = base_api else {
        return Ok(SendOutcome::NoBaseUrl);
    };
    let data = create_payload(&args.notification_data)?;
    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    let receiver = load_user(client, &args.receiver_id)
        .await?
        .ok_or_else(|| "User matching query does not exist.".to_string())?;
    // `Issue.objects.get(pk=issue_id)` coerces the field; an unparseable value
    // is Django's `ValidationError`, handled by the caller's `except`.
    let issue_id = Uuid::parse_str(&args.issue_id)
        .map_err(|error| format!("Issue id {:?}: {error}", args.issue_id))?;
    let issue = load_issue(client, issue_id)
        .await?
        .ok_or_else(|| "Issue matching query does not exist.".to_string())?;

    let mut template_data: Vec<Value> = Vec::new();
    let mut comments: Vec<Value> = Vec::new();
    let mut actors_involved: BTreeSet<String> = BTreeSet::new();
    for (actor_id, mut changes) in data {
        let actor = load_user(client, &actor_id)
            .await?
            .ok_or_else(|| "User matching query does not exist.".to_string())?;
        actors_involved.insert(actor_id.clone());
        let comment = changes.remove("comment");
        let mention = changes.remove("mention");
        let actor_detail = serde_json::json!({
            "avatar_url": format!("{base_api}{}", actor.avatar_url.as_deref().unwrap_or("None")),
            "first_name": actor.first_name,
            "last_name": actor.last_name,
        });
        if let Some(comment) = comment.filter(is_python_truthy) {
            comments.push(serde_json::json!({
                "actor_comments": comment,
                "actor_detail": actor_detail,
            }));
        }
        if let Some(mut mention) = mention.filter(is_python_truthy) {
            if let Some(object) = mention.as_object_mut() {
                for key in ["new_value", "old_value"] {
                    let processed = process_html_content(client, object.get(key)).await?;
                    object.insert(
                        key.to_string(),
                        Value::Array(processed.into_iter().map(Value::String).collect()),
                    );
                }
            }
            comments.push(serde_json::json!({
                "actor_comments": mention,
                "actor_detail": actor_detail,
            }));
        }
        // `activity_time = changes.pop("activity_time")` (`:219`).
        let activity_time = changes
            .remove("activity_time")
            .and_then(|value| value.as_str().map(str::to_string))
            .ok_or_else(|| "activity_time is missing".to_string())?;
        let formatted = format_activity_time(&activity_time)?;
        if !changes.is_empty() {
            template_data.push(serde_json::json!({
                "actor_detail": actor_detail,
                "changes": changes,
                "issue_details": {
                    "name": issue.name,
                    "identifier": format!("{}-{}", issue.identifier, issue.sequence_id),
                },
                "activity_time": formatted,
            }));
        }
    }

    let identifier = format!("{}-{}", issue.identifier, issue.sequence_id);
    let issue_url = format!(
        "{base_api}/{}/projects/{}/issues/{}",
        issue.workspace_slug,
        issue.project_id.hyphenated(),
        issue.id.hyphenated()
    );
    let subject = format!("{identifier} {}", remove_unwanted_characters(&issue.name));
    let context_value = serde_json::json!({
        "data": template_data,
        "summary": "Updates were made to the issue by",
        "actors_involved": actors_involved.len(),
        "issue": {
            "issue_identifier": identifier,
            "name": issue.name,
            "issue_url": issue_url,
        },
        "receiver": {"email": receiver.email},
        "issue_url": issue_url,
        "project_url": format!(
            "{base_api}/{}/projects/{}/issues/",
            issue.workspace_slug,
            issue.project_id.hyphenated()
        ),
        "workspace": issue.workspace_slug,
        "project": issue.project_name,
        "user_preference": format!(
            "{base_api}/{}/settings/account/notifications/",
            issue.workspace_slug
        ),
        "comments": comments,
        "entity_type": "issue",
    });
    let html = plane_mail::render("emails/notifications/issue-updates.html", &context_value)
        .map_err(|error| error.to_string())?;
    let text = plane_mail::generate_plain_text_from_html(&html);
    let to = receiver
        .email
        .clone()
        .ok_or_else(|| "recipient address is None".to_string())?;
    let message =
        plane_mail::Message::with_html(subject, text, html, configuration.from.clone(), vec![to]);
    plane_mail::send(&configuration, &message)
        .await
        .map_err(|error| error.to_string())?;

    // `EmailNotificationLog.objects.filter(pk__in=…).update(sent_at=…)`
    // (`:287`). An unparseable id is Django's `ValidationError`, which the
    // caller's `except Exception` releases the lock for.
    let ids: Vec<Uuid> = args
        .ids
        .iter()
        .map(|id| Uuid::parse_str(id).map_err(|error| error.to_string()))
        .collect::<Result<_, String>>()?;
    if !ids.is_empty() {
        client
            .execute(
                "UPDATE email_notification_logs SET sent_at = now() WHERE id = ANY($1)",
                &[&ids],
            )
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(SendOutcome::Sent)
}

/// The issue and the two lazy joins `issue.project.workspace.slug` reads.
struct IssueContext {
    id: Uuid,
    name: String,
    sequence_id: i32,
    identifier: String,
    project_id: Uuid,
    project_name: String,
    workspace_slug: String,
}

async fn load_issue(client: &Client, id: Uuid) -> Result<Option<IssueContext>, String> {
    let row = client
        .query_opt(
            "SELECT i.id, i.name, i.sequence_id, p.identifier, p.id, p.name, ws.slug \
             FROM issues i JOIN projects p ON p.id = i.project_id \
             JOIN workspaces ws ON ws.id = p.workspace_id WHERE i.id = $1",
            &[&id],
        )
        .await
        .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(IssueContext {
        id: row.try_get(0).map_err(|error| error.to_string())?,
        name: row.try_get(1).map_err(|error| error.to_string())?,
        sequence_id: row.try_get(2).map_err(|error| error.to_string())?,
        identifier: row.try_get(3).map_err(|error| error.to_string())?,
        project_id: row.try_get(4).map_err(|error| error.to_string())?,
        project_name: row.try_get(5).map_err(|error| error.to_string())?,
        workspace_slug: row.try_get(6).map_err(|error| error.to_string())?,
    }))
}

/// The user fields the e-mail reads, plus `User.avatar_url`.
struct MailUser {
    email: Option<String>,
    first_name: String,
    last_name: String,
    display_name: String,
    avatar_url: Option<String>,
}

/// `User.objects.get(pk=…)` plus `avatar_url`'s `_base_manager` lookup
/// (`plane/db/models/user.py:143-151`). The related asset is read through a
/// plain join, so a soft-deleted avatar still renders, as Django's
/// `_base_manager` does.
async fn load_user(client: &Client, id: &str) -> Result<Option<MailUser>, String> {
    let Ok(id) = Uuid::parse_str(id) else {
        // Django's `get(pk="not-a-uuid")` raises `ValidationError`, which is
        // not `User.DoesNotExist`; the caller's `except Exception` handles it.
        return Err("User id is not a valid UUID".to_string());
    };
    let row = client
        .query_opt(
            "SELECT u.email, u.first_name, u.last_name, u.avatar, u.display_name, \
                    fa.id, fa.entity_type \
             FROM users u LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id \
             WHERE u.id = $1",
            &[&id],
        )
        .await
        .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let avatar: String = row.try_get(3).map_err(|error| error.to_string())?;
    let display_name: String = row.try_get(4).map_err(|error| error.to_string())?;
    let asset_id: Option<Uuid> = row.try_get(5).map_err(|error| error.to_string())?;
    let entity_type: Option<String> = row.try_get(6).map_err(|error| error.to_string())?;
    let asset_url = match (asset_id, entity_type.as_deref()) {
        (
            Some(asset_id),
            Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER"),
        ) => Some(format!("/api/assets/v2/static/{asset_id}/")),
        // A non-static asset type's URL needs its project/issue ids; an
        // avatar FK pointing at one is not a state any producer creates.
        (Some(_), _) => return Err("unsupported avatar asset type".to_string()),
        (None, _) => None,
    };
    Ok(Some(MailUser {
        email: row.try_get(0).map_err(|error| error.to_string())?,
        first_name: row.try_get(1).map_err(|error| error.to_string())?,
        last_name: row.try_get(2).map_err(|error| error.to_string())?,
        display_name,
        avatar_url: asset_url.or(if avatar.is_empty() {
            None
        } else {
            Some(avatar)
        }),
    }))
}

// ---------------------------------------------------------------------------
// create_payload and friends
// ---------------------------------------------------------------------------

/// `create_payload` (`email_notification_task.py:87-134`).
///
/// Returns the actor map in payload order. Every actor entry carries an
/// `activity_time` string and zero or more field entries shaped
/// `{"old_value": [...], "new_value": [...]}`.
fn create_payload(notification_data: &PyValue) -> Result<ActorChanges, String> {
    let PyValue::Dict(actors) = notification_data else {
        return Err("notification_data is not a dict".to_string());
    };
    let mut data: ActorChanges = Vec::new();
    // `dict.items()` order, not the sorted `serde_json` view: which actor is
    // `data.0` and the order of the comments are user-visible.
    for (actor_id, changes) in actors.iter() {
        let actor_id = actor_id.to_string_lossy().into_owned();
        let PyValue::List(changes) = changes else {
            return Err("notification_data actor value is not a list".to_string());
        };
        for change in changes {
            let PyValue::Dict(change) = change else {
                return Err("a notification change is not a dict".to_string());
            };
            // `if issue_activity:` is Python truthiness: `None`, `{}`, `[]`,
            // `""`, `0` and `False` all skip the change.
            let Some(activity) = change
                .get("issue_activity")
                .filter(|value| is_python_truthy_py(value))
            else {
                continue;
            };
            let PyValue::Dict(activity) = activity else {
                return Err("issue_activity is not a dict".to_string());
            };
            let field = activity
                .get("field")
                .map(python_str_py)
                .unwrap_or_else(|| "None".to_string());
            let old_value = python_str_py(activity.get("old_value").unwrap_or(&PyValue::Null));
            let new_value = python_str_py(activity.get("new_value").unwrap_or(&PyValue::Null));
            if !old_value.is_empty() {
                let entry = field_entry(&mut data, &actor_id, &field)?;
                let list = entry
                    .entry("old_value".to_string())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Value::Array(items) = list
                    && !items
                        .iter()
                        .any(|item| item.as_str() == Some(old_value.as_str()))
                {
                    items.push(Value::String(old_value));
                }
            }
            if !new_value.is_empty() {
                let entry = field_entry(&mut data, &actor_id, &field)?;
                let list = entry
                    .entry("new_value".to_string())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Value::Array(items) = list
                    && !items
                        .iter()
                        .any(|item| item.as_str() == Some(new_value.as_str()))
                {
                    items.push(Value::String(new_value));
                }
            }
            // The guard reads the literal `"actor_id"` key (`:120`), so it is
            // always true for a real actor; a missing actor entry is the
            // `KeyError` Python raises when both values were empty.
            let Some((_, actor_data)) = data.iter_mut().find(|(id, _)| id == &actor_id) else {
                return Err(format!("KeyError: {actor_id}"));
            };
            let activity_time = match activity.get("activity_time") {
                Some(PyValue::Str(value)) => value.to_string_lossy().into_owned(),
                _ => return Err("activity_time is not a string".to_string()),
            };
            actor_data.insert(
                "activity_time".to_string(),
                Value::String(format_activity_stamp(&activity_time)?),
            );
        }
    }
    Ok(data)
}

/// `data[actor_id].setdefault(field, {})`. A field literally called
/// `activity_time` collides with the timestamp key Python stores in the same
/// dict; there `data[actor]["activity_time"]` is a `str`, so the next change's
/// `["old_value"]` is a `TypeError` — an error, never a panic.
fn field_entry<'a>(
    data: &'a mut ActorChanges,
    actor_id: &str,
    field: &str,
) -> Result<&'a mut Map<String, Value>, String> {
    let index = match data.iter().position(|(id, _)| id == actor_id) {
        Some(index) => index,
        None => {
            let mut actor = Map::new();
            actor.insert(field.to_string(), Value::Object(Map::new()));
            data.push((actor_id.to_string(), actor));
            data.len() - 1
        }
    };
    data[index]
        .1
        .entry(field.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("field {field:?} is not a dict"))
}

/// `str(datetime.fromisoformat(value.rstrip("Z")).strftime("%Y-%m-%d %H:%M:%S"))`
/// (`:121-126`).
fn format_activity_stamp(value: &str) -> Result<String, String> {
    let trimmed = value.trim_end_matches('Z');
    let parsed = chrono::DateTime::parse_from_rfc3339(trimmed)
        .map(|datetime| datetime.naive_local())
        .or_else(|_| NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%.f"))
        .map_err(|error| format!("activity_time {value:?}: {error}"))?;
    Ok(parsed.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// `datetime.strptime(activity_time, "%Y-%m-%d %H:%M:%S").strftime("%H:%M %p")`
/// (`:219-221`).
fn format_activity_time(value: &str) -> Result<String, String> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .map(|time| time.format("%H:%M %p").to_string())
        .map_err(|error| format!("activity_time {value:?}: {error}"))
}

/// `process_html_content` (`:142-150`): `None` stays `None` in Python, which
/// the template's `{% for %}` renders like an empty list.
async fn process_html_content(
    client: &Client,
    content: Option<&Value>,
) -> Result<Vec<String>, String> {
    let Some(content) = content else {
        return Ok(Vec::new());
    };
    let items: Vec<String> = match content {
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => Ok(text.clone()),
                other => Err(format!("html content is not a string: {other}")),
            })
            .collect::<Result<_, String>>()?,
        // `for html_content in content` over a str iterates its characters.
        Value::String(text) => text.chars().map(|ch| ch.to_string()).collect(),
        _ => return Err("html content is not iterable".to_string()),
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(process_mention(client, &item).await?);
    }
    Ok(out)
}

/// `process_mention` (`:130-140`): replace every `<mention-component>` with
/// `@<display_name>` and return `str(soup)`.
async fn process_mention(client: &Client, html: &str) -> Result<String, String> {
    let soup = crate::bs4::Soup::parse(html);
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for entity in soup
        .mention_ids()
        .map_err(|_| "mention-component has no entity_identifier".to_string())?
    {
        if names.contains_key(&entity) {
            continue;
        }
        let user = load_user(client, &entity)
            .await?
            .ok_or_else(|| format!("User matching query does not exist: {entity}"))?;
        names.insert(entity, user.display_name);
    }
    let mut soup = soup;
    soup.replace_mentions(|entity| {
        names
            .get(entity)
            .cloned()
            .ok_or_else(|| format!("mention without a display name: {entity}"))
    })?;
    Ok(soup.serialize())
}

/// `remove_unwanted_characters` (`:27-31`):
/// `re.sub(r"[\x00-\x1F\x7F-\x9F]", "", text)`.
fn remove_unwanted_characters(text: &str) -> String {
    text.chars()
        .filter(|ch| !matches!(ch, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}'))
        .collect()
}

/// Python `str()` for the ordered `json.loads` tree.
fn python_str_py(value: &PyValue) -> String {
    match value {
        PyValue::Str(text) => text.to_string_lossy().into_owned(),
        PyValue::Null => "None".to_string(),
        PyValue::Bool(true) => "True".to_string(),
        PyValue::Bool(false) => "False".to_string(),
        PyValue::Int(number) => number.to_decimal_string(),
        PyValue::Float(number) => plane_serde::float_repr(*number).unwrap_or_else(|| {
            if number.is_nan() {
                "nan".to_string()
            } else if number.is_sign_negative() {
                "-inf".to_string()
            } else {
                "inf".to_string()
            }
        }),
        // `str(list)`/`str(dict)` is their repr: single quotes and Python
        // number spelling.
        other => crate::webhook::pyrepr::python_repr(other),
    }
}

/// Python truthiness for the `serde_json` values `create_payload` stores.
fn is_python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python truthiness for the ordered `json.loads` tree.
fn is_python_truthy_py(value: &PyValue) -> bool {
    match value {
        PyValue::Null => false,
        PyValue::Bool(value) => *value,
        PyValue::Int(number) => number.to_decimal_string() != "0",
        PyValue::Float(number) => *number != 0.0,
        PyValue::Str(text) => !text.is_empty(),
        PyValue::List(items) => !items.is_empty(),
        PyValue::Dict(dict) => !dict.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_payload_dedupes_and_timestamps() {
        let data = plane_serde::pyjson::loads(
            br#"{
                "b0000000-0000-4000-8000-000000000001": [
                    {"issue_activity": {"field": "name", "old_value": "Old",
                     "new_value": "New", "activity_time": "2026-01-01T09:05:00Z"}},
                    {"issue_activity": {"field": "name", "old_value": "Old",
                     "new_value": "Newer", "activity_time": "2026-01-01T09:06:00Z"}}
                ]
            }"#,
            "utf-8",
        )
        .expect("json");
        let payload = create_payload(&data).expect("payload");
        let (actor, changes) = &payload[0];
        assert_eq!(actor, "b0000000-0000-4000-8000-000000000001");
        assert_eq!(
            changes["name"]["old_value"],
            serde_json::json!(["Old"]),
            "a repeated value is appended once"
        );
        assert_eq!(
            changes["name"]["new_value"],
            serde_json::json!(["New", "Newer"])
        );
        assert_eq!(
            changes["activity_time"],
            serde_json::json!("2026-01-01 09:06:00"),
            "the last change wins"
        );
    }

    #[test]
    fn create_payload_skips_empty_activity_and_rejects_orphans() {
        let py = |text: &str| plane_serde::pyjson::loads(text.as_bytes(), "utf-8").expect("json");
        let skipped = py(r#"{"a": [{"issue_activity": null}]}"#);
        assert!(create_payload(&skipped).expect("skipped").is_empty());
        let orphan = py(
            r#"{"a": [{"issue_activity": {"field": "name", "old_value": "",
                 "new_value": "", "activity_time": "2026-01-01T00:00:00Z"}}]}"#,
        );
        assert!(
            create_payload(&orphan).is_err(),
            "KeyError when both values are empty"
        );
    }

    #[test]
    fn control_characters_are_stripped_from_the_subject() {
        assert_eq!(remove_unwanted_characters("a\u{0}b\nc\u{7f}d"), "abcd");
        assert_eq!(remove_unwanted_characters("Plane ☂"), "Plane ☂");
    }

    #[test]
    fn lock_key_is_sorted_and_canonical() {
        let args = SendArgs {
            issue_id: "b0000000-0000-4000-8000-000000000013".to_string(),
            notification_data: PyValue::Null,
            receiver_id: "b0000000-0000-4000-8000-000000000002".to_string(),
            ids: vec![
                "b0000000-0000-4000-8000-0000000000FF".to_string(),
                "b0000000-0000-4000-8000-000000000022".to_string(),
            ],
        };
        assert_eq!(
            lock_id(&args),
            "send_email_notif_b0000000-0000-4000-8000-000000000013_\
             b0000000-0000-4000-8000-000000000002_\
             b0000000-0000-4000-8000-000000000022_b0000000-0000-4000-8000-0000000000ff"
        );
    }

    #[test]
    fn python_str_matches_pythons_spellings() {
        let py = |text: &str| plane_serde::pyjson::loads(text.as_bytes(), "utf-8").expect("json");
        assert_eq!(python_str_py(&py("null")), "None");
        assert_eq!(python_str_py(&py("true")), "True");
        assert_eq!(python_str_py(&py("[1, \"a\"]")), "[1, 'a']");
    }

    #[test]
    fn activity_times_are_formatted_like_python() {
        assert_eq!(
            format_activity_time("2026-01-01 09:05:00").expect("time"),
            "09:05 AM"
        );
        assert_eq!(
            format_activity_time("2026-01-01 23:59:00").expect("time"),
            "23:59 PM"
        );
        assert_eq!(
            format_activity_stamp("2026-01-01T09:05:00.123456Z").expect("stamp"),
            "2026-01-01 09:05:00"
        );
    }
}
