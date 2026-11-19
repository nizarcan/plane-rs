// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/webhook_task.py` — the webhook chain, tasks 40-43.
//!
//! | task | Python | side effects |
//! |---|---|---|
//! | 43 `model_activity` | `:479-521` | read-only diff, publishes 42 per changed key |
//! | 42 `webhook_activity` | `:393-475` | fan-out over active `Webhook` rows, serialises the entity, publishes 41 |
//! | 41 `webhook_send_task` | `:242-389` | pinned POST, HMAC header, one `WebhookLog` per attempt, retry 0-600 s × 5, deactivates after the budget |
//! | 40 `send_webhook_deactivation_email` | `:172-232` | one e-mail |
//!
//! # The delivery
//!
//! `pinned_fetch("POST", webhook.url, ..., headers=headers, json=payload,
//! timeout=30)` (`:317-325`) never follows a redirect, resolves and pins the
//! address, and raises `ValueError` for an invalid/blocked URL versus
//! `requests.RequestException` for a transport failure. Those two land in
//! different `except` branches with different `WebhookLog` rows, and only the
//! `RequestException` one retries or deactivates the webhook.
//!
//! The body is `requests`' `json=payload` encoding: Python `json.dumps` with
//! `", "`/`": "` separators and `ensure_ascii=True` ([`plane_serde::Dialect::
//! Django`]). The HMAC (`X-Plane-Signature`) is over the same bytes.
//!
//! # The log row
//!
//! `save_webhook_log` (`:93-122`) stringifies every column. `request_headers`
//! and `request_body` are **Python `repr`s** of a dict, not JSON (TASKS.md
//! §8.4); `response_headers` is `str(requests.headers)`; `request_method` is
//! the *mapped verb* (`create`/`update`/`delete`/…), not the HTTP method.
//!
//! # Retries
//!
//! `@shared_task(bind=True, autoretry_for=(requests.RequestException,),
//! retry_backoff=600, max_retries=5, retry_jitter=True)` (`:235-241`): Celery's
//! `retry_backoff=600` is the *factor* and `retry_backoff_max` defaults to
//! 600, so the countdown is `random(0, 600)`. `self.request.retries` is the
//! 0-based attempt and the sixth attempt (`retries == 5`) deactivates the
//! webhook, publishes the deactivation e-mail and returns normally.
//!
//! # Ownership
//!
//! The four tasks move together (`plans/CUTOVER.md` §2): 43's producers are
//! Rust HTTP routes, 42 is published by 43 (and two Rust routes), 41 only by
//! 42, and 40 only by 41.

pub mod entity;
pub(crate) mod pyrepr;

use std::sync::Arc;

use chrono::Utc;
use deadpool_postgres::Client;
use hmac::{Hmac, Mac};
use plane_db::models::webhook::Webhook;
use plane_db::models::{Workspace, with_alias};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks;
use plane_jobs::payload::Payload;
use plane_jobs::queue::Job;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_net::pyurl::repr_str;
use plane_net::{Method, OutboundRequest, SsrfPolicy, preset};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use serde_json::Value;
use sha2::Sha256;
use uuid::Uuid;

use crate::webhook::entity::{EntityError, reorder_event_data};
use crate::webhook::pyrepr::python_repr;
use crate::{TaskContext, kwargs, publish_or_enqueue, string_value, swallow, uuid_value};

/// 43 — `model_activity`.
pub const MODEL_ACTIVITY: &str = "plane.bgtasks.webhook_task.model_activity";
/// 42 — `webhook_activity`.
pub const WEBHOOK_ACTIVITY: &str = "plane.bgtasks.webhook_task.webhook_activity";
/// 41 — `webhook_send_task`.
pub const WEBHOOK_SEND: &str = "plane.bgtasks.webhook_task.webhook_send_task";
/// 40 — `send_webhook_deactivation_email`.
pub const DEACTIVATION_EMAIL: &str = "plane.bgtasks.webhook_task.send_webhook_deactivation_email";

/// Register all four handlers.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    for task in [
        MODEL_ACTIVITY,
        WEBHOOK_ACTIVITY,
        WEBHOOK_SEND,
        DEACTIVATION_EMAIL,
    ] {
        let context = Arc::clone(&context);
        registry.register(task, move |job| {
            let context = Arc::clone(&context);
            Box::pin(async move { handle(&context, job).await })
        });
    }
}

async fn handle(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    match job.task.as_str() {
        MODEL_ACTIVITY => handle_model_activity(context, job).await,
        WEBHOOK_ACTIVITY => handle_webhook_activity(context, job).await,
        WEBHOOK_SEND => handle_webhook_send(context, job).await,
        DEACTIVATION_EMAIL => handle_deactivation_email(context, job).await,
        other => Err(HandlerError::fatal(format!(
            "{other} is not a webhook task"
        ))),
    }
}

// ---------------------------------------------------------------------------
// 43 — model_activity
// ---------------------------------------------------------------------------

/// `model_activity` (`webhook_task.py:479-521`). The task has **no**
/// `try`/`except`, so a bad payload or an invalid `current_instance` JSON
/// string is a Celery `FAILURE`; the handler returns [`HandlerError::fatal`]
/// for the same cases.
async fn handle_model_activity(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    let Some(kw) = kwargs(&job.payload) else {
        return Err(HandlerError::fatal("model_activity has no kwargs"));
    };
    let model_name = string_value(kw.get("model_name"))
        .ok_or_else(|| HandlerError::fatal("model_activity model_name is not a string"))?;
    let model_id = string_value(kw.get("model_id"))
        .ok_or_else(|| HandlerError::fatal("model_activity model_id is not a string"))?;
    let requested = kw
        .get("requested_data")
        .cloned()
        .ok_or_else(|| HandlerError::fatal("model_activity has no requested_data"))?;
    let actor_id = uuid_value(kw.get("actor_id"))
        .ok_or_else(|| HandlerError::fatal("model_activity actor_id is not a UUID"))?;
    let slug = string_value(kw.get("slug"))
        .ok_or_else(|| HandlerError::fatal("model_activity slug is not a string"))?;
    let origin = string_value(kw.get("origin"));

    if let Some(instance) = kw.get("current_instance").filter(|value| !value.is_null()) {
        let text = instance.as_str().ok_or_else(|| {
            HandlerError::fatal("model_activity current_instance is not a string")
        })?;
        let current: Json = plane_serde::pyjson::loads(text.as_bytes(), "utf-8")
            .map_err(|error| HandlerError::fatal(format!("invalid current_instance: {error}")))?;
        let Json::Dict(current) = &current else {
            // `key in <non-dict>` then `.get` is an `AttributeError`, unless
            // `requested_data` is empty (nothing to compare).
            if requested.as_object().is_some_and(serde_json::Map::is_empty) {
                return Ok(());
            }
            return Err(HandlerError::fatal(
                "model_activity current_instance is not an object",
            ));
        };
        let requested = requested
            .as_object()
            .ok_or_else(|| HandlerError::fatal("model_activity requested_data is not an object"))?;
        for (key, requested_value) in requested {
            let Some(current_value) = current.get(key) else {
                continue;
            };
            let requested_value = json_from_serde(requested_value);
            if !python_equal(current_value, &requested_value) {
                publish_activity(
                    context,
                    &job.client,
                    ActivityPublish {
                        event: &model_name,
                        verb: "updated",
                        field: Some(key),
                        old_value: current_value.clone(),
                        new_value: requested_value,
                        actor_id,
                        slug: &slug,
                        current_site: origin.as_deref(),
                        event_id: &model_id,
                    },
                )
                .await?;
            }
        }
        return Ok(());
    }

    publish_activity(
        context,
        &job.client,
        ActivityPublish {
            event: &model_name,
            verb: "created",
            field: None,
            old_value: Json::Null,
            new_value: Json::Null,
            actor_id,
            slug: &slug,
            current_site: origin.as_deref(),
            event_id: &model_id,
        },
    )
    .await
}

/// One `webhook_activity.delay(...)` from `model_activity`.
struct ActivityPublish<'a> {
    event: &'a str,
    verb: &'a str,
    field: Option<&'a str>,
    old_value: Json,
    new_value: Json,
    actor_id: Uuid,
    slug: &'a str,
    current_site: Option<&'a str>,
    event_id: &'a str,
}

async fn publish_activity(
    context: &TaskContext,
    client: &Client,
    publish: ActivityPublish<'_>,
) -> Result<(), HandlerError> {
    // `event_id=model_id`: a `str` for every `model_activity` site
    // (`TASKS.md:1136-1140`), so the Celery body carries a plain string.
    let event_id = match Uuid::parse_str(publish.event_id) {
        Ok(id) => TaskArg::uuid(id),
        Err(_) => TaskArg::str(publish.event_id),
    };
    let message = tasks::webhook_activity(tasks::WebhookActivityCall {
        event: publish.event.to_owned(),
        verb: publish.verb.to_owned(),
        field: publish.field.map(str::to_owned),
        old_value: TaskArg::json(publish.old_value.clone()),
        new_value: TaskArg::json(publish.new_value),
        actor_id: publish.actor_id,
        slug: publish.slug.to_owned(),
        current_site: publish.current_site.unwrap_or_default().to_owned(),
        event_id,
        old_identifier: None,
        new_identifier: None,
    });
    // The `Job` is the same `(args, kwargs)` Python builds; `from_message`
    // keeps the switch transport-only.
    let job = Job::from_message(&message);
    publish_or_enqueue(context, &***client, WEBHOOK_ACTIVITY, &message, job).await
}

/// Python `!=` between two `json.loads` values: `1 == 1.0 == True`, a dict
/// compares by keys and values, a list element-wise.
fn python_equal(left: &Json, right: &Json) -> bool {
    match (left, right) {
        (Json::Null, Json::Null) => true,
        (Json::Bool(a), Json::Bool(b)) => a == b,
        (Json::Str(a), Json::Str(b)) => a == b,
        (Json::List(a), Json::List(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| python_equal(a, b))
        }
        (Json::Dict(a), Json::Dict(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key.as_str().unwrap_or_default())
                        .is_some_and(|other| python_equal(value, other))
                })
        }
        _ => match (number(left), number(right)) {
            // `bool` is an `int` in Python, so `True == 1`.
            (Some(a), Some(b)) => a == b,
            _ => false,
        },
    }
}

fn number(value: &Json) -> Option<f64> {
    match value {
        Json::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        Json::Int(value) => value.as_i64().map(|value| value as f64),
        Json::Float(value) => Some(*value),
        _ => None,
    }
}

/// The `serde_json` payload value as the `json.loads` tree.
fn json_from_serde(value: &Value) -> Json {
    let text = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
    plane_serde::pyjson::loads(&text, "utf-8").unwrap_or(Json::Null)
}

// ---------------------------------------------------------------------------
// 42 — webhook_activity
// ---------------------------------------------------------------------------

/// The 42 payload.
struct ActivityArgs {
    event: String,
    verb: String,
    field: Option<String>,
    old_value: Value,
    new_value: Value,
    actor_id: String,
    slug: String,
    current_site: Option<String>,
    event_id: String,
    old_identifier: Option<String>,
    new_identifier: Option<String>,
}

fn decode_activity(payload: &Value) -> Option<ActivityArgs> {
    let kw = kwargs(payload)?;
    let id = |value: Option<&Value>| -> Option<String> {
        match value? {
            Value::String(text) => Some(text.clone()),
            Value::Null => None,
            _ => None,
        }
    };
    Some(ActivityArgs {
        event: string_value(kw.get("event"))?,
        verb: string_value(kw.get("verb"))?,
        field: kw.get("field").and_then(|value| string_value(Some(value))),
        old_value: kw.get("old_value").cloned().unwrap_or(Value::Null),
        new_value: kw.get("new_value").cloned().unwrap_or(Value::Null),
        actor_id: id(kw.get("actor_id"))?,
        slug: string_value(kw.get("slug"))?,
        current_site: id(kw.get("current_site")),
        event_id: id(kw.get("event_id"))?,
        old_identifier: id(kw.get("old_identifier")),
        new_identifier: id(kw.get("new_identifier")),
    })
}

/// `webhook_activity` (`webhook_task.py:393-475`). Every exception is logged
/// and swallowed (`:468-475`), including a missing entity or actor.
async fn handle_webhook_activity(
    context: &TaskContext,
    job: JobContext,
) -> Result<(), HandlerError> {
    let Some(args) = decode_activity(&job.payload) else {
        swallow("invalid webhook_activity payload", WEBHOOK_ACTIVITY);
        return Ok(());
    };
    if let Err(error) = run_webhook_activity(context, &job.client, &args).await {
        swallow(error, WEBHOOK_ACTIVITY);
    }
    Ok(())
}

async fn run_webhook_activity(
    context: &TaskContext,
    client: &Client,
    args: &ActivityArgs,
) -> Result<(), String> {
    let webhooks = load_webhooks(client, &args.slug, &args.event)
        .await
        .map_err(|error| error.to_string())?;
    if webhooks.is_empty() {
        // `get_model_data` is only reached inside the loop, so an empty fan-out
        // never validates the entity or the actor.
        return Ok(());
    }
    let actor = entity::model_data(client, "user", &args.actor_id)
        .await
        .map_err(|error| error.to_string())?;
    let mut actor = actor;
    actor = reorder_event_data("user", &actor);
    let event_data = if args.verb == "deleted" {
        Json::Dict(
            [(
                PyStr::new("id"),
                match Uuid::parse_str(&args.event_id) {
                    Ok(id) => Json::from(id.hyphenated().to_string()),
                    Err(_) => Json::from(args.event_id.clone()),
                },
            )]
            .into_iter()
            .collect(),
        )
    } else {
        let data = entity::model_data(client, &args.event, &args.event_id)
            .await
            .map_err(|error| error.to_string())?;
        reorder_event_data(&args.event, &data)
    };
    let mut activity = PyDict::new();
    activity.insert(
        PyStr::new("field"),
        args.field.as_deref().map_or(Json::Null, Json::from),
    );
    activity.insert(PyStr::new("new_value"), json_from_serde(&args.new_value));
    activity.insert(PyStr::new("old_value"), json_from_serde(&args.old_value));
    activity.insert(PyStr::new("actor"), actor);
    activity.insert(
        PyStr::new("old_identifier"),
        args.old_identifier
            .as_deref()
            .map_or(Json::Null, Json::from),
    );
    activity.insert(
        PyStr::new("new_identifier"),
        args.new_identifier
            .as_deref()
            .map_or(Json::Null, Json::from),
    );
    let activity = Json::Dict(activity);

    for webhook in &webhooks {
        // `webhook_send_task.delay(...)` for each webhook, in the queryset's
        // order (`Meta.ordering = ["-created_at"]`).
        let message = tasks::webhook_send_task(tasks::WebhookSendTaskCall {
            webhook_id: webhook.id,
            slug: args.slug.clone(),
            event: args.event.clone(),
            event_data: TaskArg::json(event_data.clone()),
            action: args.verb.clone(),
            current_site: args.current_site.clone().unwrap_or_default(),
            activity: TaskArg::json(activity.clone()),
        });
        let job = Job::from_message(&message);
        publish_or_enqueue(context, &***client, WEBHOOK_SEND, &message, job)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// `Webhook.objects.filter(workspace__slug=slug, is_active=True)` plus the
/// event's flag (`webhook_task.py:433-448`).
async fn load_webhooks(
    client: &Client,
    slug: &str,
    event: &str,
) -> Result<Vec<Webhook>, EntityError> {
    let flag = match event {
        "project" => Some("project"),
        "issue" => Some("issue"),
        "module" | "module_issue" => Some("module"),
        "cycle" | "cycle_issue" => Some("cycle"),
        "issue_comment" => Some("issue_comment"),
        _ => None,
    };
    let columns = with_alias(Webhook::COLUMNS, "w");
    let flag_filter = flag.map_or(String::new(), |flag| format!(" AND w.{flag}"));
    let rows = client
        .query(
            &format!(
                "SELECT {columns} FROM webhooks w \
                 JOIN workspaces ws ON ws.id = w.workspace_id \
                 WHERE ws.slug = $1 AND ws.deleted_at IS NULL \
                   AND w.deleted_at IS NULL AND w.is_active{flag_filter} \
                 ORDER BY w.created_at DESC"
            ),
            &[&slug],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    rows.iter()
        .map(|row| {
            Webhook::from_row(row, 0).map_err(|error| EntityError::Invalid(error.to_string()))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 41 — webhook_send_task
// ---------------------------------------------------------------------------

/// The 41 payload. `event_data` and `activity` are the ordered Python values
/// the producer wrote, so the body keeps the serializer's dict order.
struct SendArgs {
    webhook_id: Uuid,
    slug: String,
    event: String,
    event_data: Json,
    action: String,
    current_site: Option<String>,
    activity: Json,
}

fn decode_send(payload: &Payload) -> Option<SendArgs> {
    let Some(Json::Dict(kw)) = payload.py_at(&["kwargs"]) else {
        return None;
    };
    let text = |name: &str| -> Option<String> {
        match kw.get(name)? {
            Json::Str(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        }
    };
    let optional = |name: &str| -> Option<Option<String>> {
        match kw.get(name) {
            None | Some(Json::Null) => Some(None),
            Some(Json::Str(value)) => Some(Some(value.to_string_lossy().into_owned())),
            _ => None,
        }
    };
    Some(SendArgs {
        webhook_id: py_uuid(kw.get("webhook_id"))?,
        slug: text("slug")?,
        event: text("event")?,
        event_data: kw.get("event_data").cloned().unwrap_or(Json::Null),
        action: text("action")?,
        current_site: optional("current_site")?,
        activity: kw.get("activity").cloned().unwrap_or(Json::Null),
    })
}

/// A UUID string in the ordered pyjson tree.
fn py_uuid(value: Option<&Json>) -> Option<Uuid> {
    match value? {
        Json::Str(text) => Uuid::parse_str(&text.to_string_lossy()).ok(),
        _ => None,
    }
}

/// `webhook_send_task` (`webhook_task.py:242-389`).
async fn handle_webhook_send(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    let Some(args) = decode_send(&job.payload) else {
        // The first `try` covers argument decoding: any failure logs and
        // returns without a log row.
        swallow("invalid webhook_send_task payload", WEBHOOK_SEND);
        return Ok(());
    };
    let webhook = match load_webhook(&job.client, args.webhook_id, &args.slug).await {
        Ok(Some(webhook)) => webhook,
        Ok(None) => {
            swallow(
                format!(
                    "Webhook matching query does not exist for {} / {}",
                    args.webhook_id, args.slug
                ),
                WEBHOOK_SEND,
            );
            return Ok(());
        }
        Err(error) => {
            swallow(error, WEBHOOK_SEND);
            return Ok(());
        }
    };
    let Some(delivery) = build_delivery(&webhook, &args) else {
        swallow("webhook payload could not be encoded", WEBHOOK_SEND);
        return Ok(());
    };

    let retries = job.attempts.saturating_sub(1);
    match deliver(context, &webhook, &delivery).await {
        Ok(response) => {
            log_attempt(
                &job.client,
                &webhook,
                &delivery,
                &format!("{}", response.status),
                &response.headers.python_dict_repr(),
                &response.text(),
                retries,
                &args.event,
            )
            .await;
            Ok(())
        }
        Err(FetchFailure::Request(error)) => {
            log_attempt(
                &job.client,
                &webhook,
                &delivery,
                "500",
                "",
                &error,
                retries,
                &args.event,
            )
            .await;
            if retries >= MAX_RETRIES {
                if let Err(failure) =
                    deactivate(context, &job.client, &webhook, &args, &error).await
                {
                    swallow(failure, WEBHOOK_SEND);
                }
                return Ok(());
            }
            // `autoretry_for=(requests.RequestException,)` with
            // `retry_backoff=600, retry_jitter=True` (`:235-241`).
            Err(HandlerError::retryable(error))
        }
        Err(FetchFailure::Value(error)) => {
            // SSRF validation failure: a 400 log row, no retry, no
            // deactivation (`:368-385`).
            log_attempt(
                &job.client,
                &webhook,
                &delivery,
                "400",
                "",
                &format!("Webhook URL rejected: {error}"),
                retries,
                &args.event,
            )
            .await;
            Ok(())
        }
        Err(FetchFailure::Other(error)) => {
            swallow(error, WEBHOOK_SEND);
            Ok(())
        }
    }
}

/// `max_retries=5` (`webhook_task.py:239`).
const MAX_RETRIES: i32 = 5;

/// One prepared delivery: the ordered headers and the exact body bytes.
struct Delivery {
    /// `headers` in the dict's insertion order, for `str(headers)`.
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// `payload` as the `json.loads` tree, for `str(request_body)`.
    payload: Json,
    /// The mapped verb, stored as `request_method` (`webhook_task.py:330`).
    action: String,
}

fn build_delivery(webhook: &Webhook, args: &SendArgs) -> Option<Delivery> {
    let mut headers = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("User-Agent".to_string(), "Autopilot".to_string()),
        (
            "X-Plane-Delivery".to_string(),
            Uuid::new_v4().hyphenated().to_string(),
        ),
        ("X-Plane-Event".to_string(), args.event.clone()),
    ];
    // `event_data`/`activity` arrive in the producer's order: the queue's
    // `json` payload keeps it (`plane_jobs::payload`), and `webhook_activity`
    // built them in serializer order. No per-serializer reordering is needed
    // (the old `reorder_event_data`/`reorder_activity` workaround is gone).
    let event_data = args.event_data.clone();
    let activity = args.activity.clone();
    let action = match args.action.as_str() {
        "POST" => "create",
        "PATCH" | "PUT" => "update",
        "DELETE" => "delete",
        other => other,
    }
    .to_string();

    let mut payload = PyDict::with_capacity(7);
    payload.insert(PyStr::new("event"), Json::from(args.event.clone()));
    payload.insert(PyStr::new("action"), Json::from(action.clone()));
    payload.insert(
        PyStr::new("webhook_id"),
        Json::from(webhook.id.hyphenated().to_string()),
    );
    payload.insert(
        PyStr::new("workspace_id"),
        Json::from(webhook.workspace_id.hyphenated().to_string()),
    );
    payload.insert(PyStr::new("workspace_slug"), Json::from(args.slug.clone()));
    payload.insert(PyStr::new("data"), event_data);
    payload.insert(PyStr::new("activity"), activity);
    let payload = Json::Dict(payload);

    let body = plane_serde::to_vec(plane_serde::Dialect::Django, &payload).ok()?;
    if !webhook.secret_key.is_empty() {
        let mut mac = Hmac::<Sha256>::new_from_slice(webhook.secret_key.as_bytes()).ok()?;
        mac.update(&body);
        let signature = mac.finalize().into_bytes();
        let hex = signature
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        headers.push(("X-Plane-Signature".to_string(), hex));
    }
    Some(Delivery {
        headers,
        body,
        payload,
        action,
    })
}

enum FetchFailure {
    /// `requests.RequestException`.
    Request(String),
    /// `ValueError`.
    Value(String),
    /// Anything else (TLS configuration, header encoding): the task's generic
    /// `except Exception`, which writes no log row.
    Other(String),
}

async fn deliver(
    context: &TaskContext,
    webhook: &Webhook,
    delivery: &Delivery,
) -> Result<plane_net::PinnedResponse, FetchFailure> {
    let mut request = OutboundRequest::new();
    for (name, value) in &delivery.headers {
        request = request.header(name.clone(), value.clone());
    }
    request = request.body(delivery.body.clone());
    let policy = webhook_policy(&context.settings);
    match context
        .http
        .fetch_once(
            &preset::WEBHOOK,
            Method::POST,
            &policy,
            &webhook.url,
            &request,
        )
        .await
    {
        Ok(response) => Ok(response),
        Err(error) if error.is_value_error() => Err(FetchFailure::Value(error.message())),
        Err(error) if error.is_request_exception() => Err(FetchFailure::Request(error.message())),
        Err(error) => Err(FetchFailure::Other(error.message())),
    }
}

/// `WEBHOOK_ALLOWED_IPS`/`WEBHOOK_ALLOWED_HOSTS` (`plane/settings/common.py`),
/// with the harness's loopback escape compiled in only with the `difftest`
/// feature (D26).
fn webhook_policy(settings: &plane_core::config::Settings) -> SsrfPolicy {
    #[allow(unused_mut)]
    let mut policy = SsrfPolicy::new(
        settings.webhook.allowed_ips.clone(),
        settings.webhook.allowed_hosts.clone(),
    );
    #[cfg(feature = "difftest")]
    if std::env::var("PLANE_RS_DIFFTEST_ALLOW_LOOPBACK").as_deref() == Ok("1") {
        policy = SsrfPolicy::new(
            {
                let mut networks = policy.allowed_ips().to_vec();
                networks.push("127.0.0.0/8".parse().expect("loopback v4 network"));
                networks.push("::1/128".parse().expect("loopback v6 network"));
                networks
            },
            settings.webhook.allowed_hosts.clone(),
        );
    }
    policy
}

async fn load_webhook(
    client: &Client,
    id: Uuid,
    slug: &str,
) -> Result<Option<Webhook>, EntityError> {
    let columns = with_alias(Webhook::COLUMNS, "w");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns} FROM webhooks w \
                 JOIN workspaces ws ON ws.id = w.workspace_id \
                 WHERE w.id = $1 AND ws.slug = $2 \
                   AND w.deleted_at IS NULL AND ws.deleted_at IS NULL"
            ),
            &[&id, &slug],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    row.map(|row| {
        Webhook::from_row(&row, 0).map_err(|error| EntityError::Invalid(error.to_string()))
    })
    .transpose()
}

/// `save_webhook_log(...)` (`webhook_task.py:93-122`).
#[allow(clippy::too_many_arguments)]
async fn log_attempt(
    client: &Client,
    webhook: &Webhook,
    delivery: &Delivery,
    response_status: &str,
    response_headers: &str,
    response_body: &str,
    retry_count: i32,
    event: &str,
) {
    let request_headers = dict_repr(&delivery.headers);
    let request_body = python_repr(&delivery.payload);
    let id = Uuid::new_v4();
    let now = Utc::now();
    let retry_count = i64::from(retry_count);
    if let Err(error) = client
        .execute(
            "INSERT INTO webhook_logs \
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
              workspace_id, webhook, event_type, request_method, request_headers, \
              request_body, response_status, response_headers, response_body, retry_count) \
             VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, $8, $9, $10, $11, \
                     $12, $13::bigint::smallint)",
            &[
                &id,
                &now,
                &now,
                &webhook.workspace_id,
                &webhook.id,
                &event,
                &delivery.action,
                &request_headers,
                &request_body,
                &response_status,
                &response_headers,
                &response_body,
                &retry_count,
            ],
        )
        .await
    {
        // `except Exception` around the `create` (`:117-122`).
        swallow(error, WEBHOOK_SEND);
    }
}

/// `str(headers)`: a Python dict repr preserving insertion order.
fn dict_repr(entries: &[(String, String)]) -> String {
    let mut out = String::from("{");
    for (index, (name, value)) in entries.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&repr_str(name));
        out.push_str(": ");
        out.push_str(&repr_str(value));
    }
    out.push('}');
    out
}

/// `Webhook.objects.filter(pk=...).update(is_active=False)` then
/// `send_webhook_deactivation_email.delay(...)` (`:356-364`).
async fn deactivate(
    context: &TaskContext,
    client: &Client,
    webhook: &Webhook,
    args: &SendArgs,
    reason: &str,
) -> Result<(), HandlerError> {
    client
        .execute(
            "UPDATE webhooks SET is_active = FALSE WHERE id = $1",
            &[&webhook.id],
        )
        .await
        .map_err(|error| HandlerError::fatal(error.to_string()))?;
    let message = tasks::send_webhook_deactivation_email(
        webhook.id,
        webhook.created_by_id,
        reason.to_owned(),
        args.current_site.clone().unwrap_or_default(),
    );
    let job = Job::from_message(&message);
    publish_or_enqueue(context, &***client, DEACTIVATION_EMAIL, &message, job).await
}

// ---------------------------------------------------------------------------
// 40 — send_webhook_deactivation_email
// ---------------------------------------------------------------------------

/// `send_webhook_deactivation_email` (`webhook_task.py:172-232`). The whole
/// body is inside `try`/`except Exception`, so a missing user or webhook is a
/// log line and a normal return.
async fn handle_deactivation_email(
    context: &TaskContext,
    job: JobContext,
) -> Result<(), HandlerError> {
    if let Err(error) = run_deactivation_email(context, &job.client, &job.payload).await {
        swallow(error, DEACTIVATION_EMAIL);
    }
    Ok(())
}

async fn run_deactivation_email(
    context: &TaskContext,
    client: &Client,
    payload: &Value,
) -> Result<(), String> {
    let Some(kw) = kwargs(payload) else {
        return Err("send_webhook_deactivation_email has no kwargs".to_string());
    };
    let webhook_id =
        uuid_value(kw.get("webhook_id")).ok_or_else(|| "webhook_id is not a UUID".to_string())?;
    let receiver_id = kw
        .get("receiver_id")
        .filter(|value| !value.is_null())
        .map(|value| uuid_value(Some(value)).ok_or_else(|| "receiver_id is not a UUID".to_string()))
        .transpose()?;
    let current_site = string_value(kw.get("current_site")).unwrap_or_default();

    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    let (email, workspace_slug) = receiver_and_slug(client, receiver_id, webhook_id).await?;
    let webhook = load_webhook_by_id(client, webhook_id).await?;
    let message = format!(
        "Webhook {} has been deactivated due to failed requests.",
        webhook.url
    );
    let webhook_url = format!(
        "{current_site}/{workspace_slug}/settings/webhooks/{}",
        webhook.id.hyphenated()
    );
    let html = plane_mail::render(
        "emails/notifications/webhook-deactivate.html",
        &serde_json::json!({
            "email": email,
            "message": message,
            "webhook_url": webhook_url,
        }),
    )
    .map_err(|error| error.to_string())?;
    let text = plane_mail::generate_plain_text_from_html(&html);
    let email_message = plane_mail::Message::with_html(
        "Webhook Deactivated",
        text,
        html,
        configuration.from.clone(),
        vec![email],
    );
    plane_mail::send(&configuration, &email_message)
        .await
        .map_err(|error| error.to_string())
}

/// `User.objects.get(pk=receiver_id)` and `webhook.workspace.slug`.
async fn receiver_and_slug(
    client: &Client,
    receiver_id: Option<Uuid>,
    webhook_id: Uuid,
) -> Result<(String, String), String> {
    let Some(receiver_id) = receiver_id else {
        return Err("User matching query does not exist.".to_string());
    };
    let row = client
        .query_opt(
            "SELECT u.email FROM users u WHERE u.id = $1",
            &[&receiver_id],
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "User matching query does not exist.".to_string())?;
    let email: Option<String> = row.get(0);
    let Some(email) = email else {
        return Err("User matching query does not exist.".to_string());
    };
    let columns = with_alias(Workspace::COLUMNS, "ws");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns} FROM workspaces ws \
                 JOIN webhooks w ON w.workspace_id = ws.id \
                 WHERE w.id = $1 AND ws.deleted_at IS NULL"
            ),
            &[&webhook_id],
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Workspace matching query does not exist.".to_string())?;
    let workspace = Workspace::from_row(&row, 0).map_err(|error| error.to_string())?;
    Ok((email, workspace.slug))
}

async fn load_webhook_by_id(client: &Client, id: Uuid) -> Result<Webhook, String> {
    let columns = with_alias(Webhook::COLUMNS, "w");
    let row = client
        .query_opt(
            &format!("SELECT {columns} FROM webhooks w WHERE w.id = $1 AND w.deleted_at IS NULL"),
            &[&id],
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Webhook matching query does not exist.".to_string())?;
    Webhook::from_row(&row, 0).map_err(|error| error.to_string())
}

/// Kept for the tests: the `X-Plane-Signature` value for a body.
#[cfg(test)]
fn signature(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_equality_matches_python() {
        let number = |text: &str| json_from_serde(&serde_json::json!(text));
        let _ = number;
        let one = json_from_serde(&serde_json::json!(1));
        let one_float = json_from_serde(&serde_json::json!(1.0));
        let truth = json_from_serde(&serde_json::json!(true));
        assert!(python_equal(&one, &one_float));
        assert!(python_equal(&truth, &one));
        assert!(!python_equal(
            &json_from_serde(&serde_json::json!("1")),
            &one
        ));
        assert!(python_equal(
            &json_from_serde(&serde_json::json!({"a": [1, null]})),
            &json_from_serde(&serde_json::json!({"a": [1.0, null]}))
        ));
    }

    #[test]
    fn signature_is_python_hmac_sha256() {
        // `hmac.new(b"secret", b'{"a": 1}', hashlib.sha256).hexdigest()`.
        assert_eq!(
            signature("secret", b"{\"a\": 1}"),
            "9efa14a18410a57a432fbf0ce38d4bc33339036a64e57b8c86da0bb1e122d972"
        );
    }

    #[test]
    fn response_headers_are_a_python_repr() {
        assert_eq!(dict_repr(&[]), "{}");
        assert_eq!(
            dict_repr(&[("Content-Type".to_string(), "application/json".to_string())]),
            "{'Content-Type': 'application/json'}"
        );
    }
}
