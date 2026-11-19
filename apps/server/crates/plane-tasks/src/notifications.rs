// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/notification_task.py` — the `notifications` task.
//!
//! `issue_activity` re-publishes this task for every activity with
//! `notification=True` (`issue_activities_task.py:1632-1645`), passing the
//! origin activity's `requested_data`/`current_instance`, the ids and the DRF
//! `IssueActivitySerializer` output as a JSON string. The body
//! (`notification_task.py:190-674`) then writes:
//!
//! * `IssueMention` rows for the mentions added to the description and soft
//!   deletes the removed ones (`:37-50`, `:662-667`),
//! * `IssueSubscriber` rows for mentioned active project members
//!   (`:84-111`, `:455-459`) and, when `subscriber` is truthy, for the actor
//!   (`:292-299`),
//! * one `Notification` per existing subscriber per activity, plus one per new
//!   description/comment mention (`:363-406`, `:465-520`, `:522-659`),
//! * `EmailNotificationLog` rows gated by `UserNotificationPreference`
//!   (`:407-450`, `:479-519`, `:572-609`, `:621-658`).
//!
//! # Ported Python details that are easy to "fix" by accident
//!
//! * **`issue_activity.get("actor_id")` is always `None`**: the serializer's
//!   field is `actor` (`app/serializers/issue.py:343-361`), so every
//!   `data["issue_activity"]["actor"]` is the string `"None"` despite the
//!   `triggered_by_id` being real. The port reads the same key.
//! * **`issue_detail.id != issue_id` is always true for a UUID argument.**
//!   `issue_detail.id` is a string; automation passes `issue.id` (a
//!   `uuid.UUID`) to `issue_activity`, which forwards it here. The typed
//!   argument loses to the string, so automation produces no per-subscriber
//!   activity notifications (`:323`).
//! * **The new-mention e-mail goes to the wrong user.** `receiver_id=subscriber`
//!   inside the `new_mentions` loop (`:576`, `:625`) is the loop variable of
//!   the *subscriber* loop above — the last existing subscriber — or the
//!   function's `subscriber` parameter (a bool) when that loop ran zero times.
//!   A bool becomes `uuid(int=0|1)` through `UUIDField.to_python`
//!   (`django/db/models/fields/__init__.py:2760-2770`) and then fails the
//!   `users` foreign key, aborting the task before any notification row is
//!   written. The port keeps both behaviours.
//! * **`old_identifier`/`new_identifier` of a last-description notification**
//!   come from the *last iterated activity*, not from `last_activity`
//!   (`:559-567`): the variable leaks out of the activity loops. When
//!   `issue_activities_created` is empty and that branch is reached, Python
//!   raises `UnboundLocalError`; the port reproduces the abort.
//! * **Every `preference` lookup is `get()`**: a user with no live
//!   `UserNotificationPreference` (or two) aborts the task, and in-app
//!   notifications are *not* gated by preferences at all.
//! * **The `issue_completed` branch only runs when `state_change` is false**,
//!   and `property_change` is the final catch-all (`:332-349`).
//! * **Mentions are HTML**: `extract_mentions` parses `description_html` with
//!   BeautifulSoup's `html.parser`; a missing `entity_identifier`,
//!   non-string input or malformed JSON makes the whole extraction return
//!   `[]` (`:115-142`), while a non-UUID mention in a comment raises
//!   `ValueError` at `UUID(mention)` (`:264`) and aborts.
//! * **Partial effects commit.** The body is a chain of autocommitted
//!   statements outside a transaction: the actor's subscriber row can survive
//!   an abort at `Project.objects.get` (`:301`), mention rows survive a failed
//!   notification insert, and so on. Every query below runs on the job's
//!   connection without a wrapping transaction.
//!
//! # Typed arguments
//!
//! `Job::from_message` stores the Python value of every argument and records
//! `uuid_kwargs` for the ones that were `uuid.UUID` objects. `issue_id` can be
//! either spelling (views pass `str(issue.id)`, automation passes `issue.id`);
//! the distinction is observable in the `!=` above and is carried through
//! [`UuidArg`].

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Datelike, Timelike, Utc};
use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_serde::pyjson::{self, PyDict, PyStr, Value as PyValue};
use tokio_postgres::types::{IsNull, ToSql, Type};
use uuid::Uuid;

use crate::issue_activity::{
    parse_uuid_text, py_eq, py_str, py_truthy, python_type_name, serde_to_py,
};
use crate::{TaskContext, swallow};

/// `plane.bgtasks.notification_task.notifications`.
pub const TASK: &str = "plane.bgtasks.notification_task.notifications";

/// `notification_task.py:206-220`: the activity types that return immediately.
const SKIPPED_TYPES: &[&str] = &[
    "cycle.activity.created",
    "cycle.activity.deleted",
    "module.activity.created",
    "module.activity.deleted",
    "issue_reaction.activity.created",
    "issue_reaction.activity.deleted",
    "comment_reaction.activity.created",
    "comment_reaction.activity.deleted",
    "issue_vote.activity.created",
    "issue_vote.activity.deleted",
    "issue_draft.activity.created",
    "issue_draft.activity.updated",
    "issue_draft.activity.deleted",
];

/// The eight required keyword arguments (`notification_task.py:191-200`).
const KEYWORDS: &[&str] = &[
    "type",
    "issue_id",
    "project_id",
    "actor_id",
    "subscriber",
    "issue_activities_created",
    "requested_data",
    "current_instance",
];

/// Register the handler.
pub fn register(registry: &mut TaskRegistry, _context: Arc<TaskContext>) {
    registry.register(TASK, handle);
}

/// Every failure Django's `except Exception` at `:672-674` swallows.
#[derive(Debug)]
enum Failure {
    /// A Python-level exception, rendered into the log line.
    Python(String),
    /// A database error (constraint, type, foreign key, …).
    Db(tokio_postgres::Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Python(message) => formatter.write_str(message),
            Self::Db(error) => match error.as_db_error() {
                Some(db) => write!(formatter, "{}: {}", db.severity(), db.message()),
                None => write!(formatter, "{error}"),
            },
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

type R<T> = Result<T, Failure>;

fn python<T>(message: impl Into<String>) -> R<T> {
    Err(Failure::Python(message.into()))
}

/// The handler entry point.
///
/// A missing/extra keyword is a Celery `FAILURE` (the arguments are bound
/// before the body's `try`), so it dead-letters the job. Everything inside the
/// body is logged and swallowed (`:672-674`).
async fn handle(job: JobContext) -> Result<(), HandlerError> {
    let call = match Call::decode(&job.payload) {
        Ok(call) => call,
        Err(message) => return Err(HandlerError::fatal(message)),
    };
    if let Err(error) = run(&job.client, &call).await {
        swallow(error, TASK);
    }
    Ok(())
}

/// A `uuid.UUID` argument or its `str` spelling.
#[derive(Clone, Debug)]
struct UuidArg {
    raw: PyValue,
    /// `Job::from_message`'s `uuid_kwargs` hint: the producer passed a
    /// `uuid.UUID` object, not `str(...)`.
    typed: bool,
}

impl UuidArg {
    /// Python `value != other` against a JSON value from the activity payload.
    fn differs_from(&self, other: &PyValue) -> bool {
        if self.typed {
            // `str != uuid.UUID` is always true.
            return true;
        }
        !py_eq(&self.raw, other)
    }

    /// Python `value == text` for a `str` on the other side.
    fn equals_str(&self, other: &str) -> bool {
        !self.typed && self.raw.as_str() == Some(other)
    }

    /// Django `UUIDField.to_python` for a filter/prepared value
    /// (`django/db/models/fields/__init__.py:2760-2770`): `None` is SQL NULL,
    /// a `bool` is `isinstance(int)`, a negative or oversized int is invalid.
    fn field_uuid(&self) -> R<Option<Uuid>> {
        field_uuid(&self.raw)
    }

    /// `uuid.UUID(value)` as used directly by `notification_task.py:309`:
    /// only a string spelling is accepted here.
    fn positional_uuid(&self) -> R<Uuid> {
        match &self.raw {
            PyValue::Str(text) => parse_uuid_text(&text.to_string_lossy()).ok_or_else(|| {
                Failure::Python(format!(
                    "ValueError: badly formed hexadecimal UUID string: {:?}",
                    text.to_string_lossy()
                ))
            }),
            other => python(format!(
                "AttributeError: '{}' object has no attribute 'replace'",
                python_type_name(other)
            )),
        }
    }
}

fn field_uuid(value: &PyValue) -> R<Option<Uuid>> {
    match value {
        PyValue::Null => Ok(None),
        PyValue::Bool(value) => Ok(Some(Uuid::from_u128(u128::from(*value)))),
        PyValue::Int(value) => {
            let text = value.to_decimal_string();
            text.parse::<u128>()
                .map(|number| Some(Uuid::from_u128(number)))
                .map_err(|_| {
                    Failure::Python(format!(
                        "ValidationError: “{text}” is not a valid UUID (UUIDField)"
                    ))
                })
        }
        PyValue::Str(text) => {
            let text = text.to_string_lossy();
            parse_uuid_text(&text).map(Some).ok_or_else(|| {
                Failure::Python(format!("ValidationError: “{text}” is not a valid UUID."))
            })
        }
        other => python(format!(
            "ValidationError: “{}” is not a valid UUID (UUIDField cannot coerce {})",
            py_str(other),
            python_type_name(other)
        )),
    }
}

/// The decoded, argument-bound call.
struct Call {
    activity_type: PyValue,
    issue_id: UuidArg,
    actor_id: UuidArg,
    project_id: UuidArg,
    subscriber: PyValue,
    activities_raw: PyValue,
    requested_data: PyValue,
    current_instance: PyValue,
}

impl Call {
    /// Bind the eight keywords. A failure here is outside `try` in Django.
    fn decode(payload: &serde_json::Value) -> Result<Self, String> {
        let kwargs = crate::kwargs(payload).ok_or_else(|| {
            "notifications() payload has no kwargs object (TypeError)".to_string()
        })?;
        if let Some(extra) = kwargs.keys().find(|key| !KEYWORDS.contains(&key.as_str())) {
            return Err(format!(
                "notifications() got an unexpected keyword argument {extra:?}"
            ));
        }
        let required = |name: &str| -> Result<PyValue, String> {
            kwargs
                .get(name)
                .map(serde_to_py)
                .ok_or_else(|| format!("notifications() missing required argument: {name:?}"))
        };
        let uuid_kwargs: BTreeSet<String> = payload
            .get("uuid_kwargs")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let uuid_arg = |name: &str| -> Result<UuidArg, String> {
            Ok(UuidArg {
                raw: required(name)?,
                typed: uuid_kwargs.contains(name),
            })
        };
        Ok(Self {
            activity_type: required("type")?,
            issue_id: uuid_arg("issue_id")?,
            actor_id: uuid_arg("actor_id")?,
            project_id: uuid_arg("project_id")?,
            subscriber: required("subscriber")?,
            activities_raw: required("issue_activities_created")?,
            requested_data: required("requested_data")?,
            current_instance: required("current_instance")?,
        })
    }

    /// `if type not in [...]` (`:205-219`). Only a `str` can equal a member,
    /// so any other JSON type falls through.
    fn is_skipped(&self) -> bool {
        self.activity_type
            .as_str()
            .is_some_and(|kind| SKIPPED_TYPES.contains(&kind))
    }
}

/// `json.loads(issue_activities_created) if issue_activities_created is not
/// None else None` (`:202-204`).
fn parse_activities(raw: &PyValue) -> R<Option<PyValue>> {
    match raw {
        PyValue::Null => Ok(None),
        PyValue::Str(text) => Ok(Some(pyjson::loads(text.as_wtf8(), "utf-8")?)),
        other => python(format!(
            "TypeError: the JSON object must be str, bytes or bytearray, not {}",
            python_type_name(other)
        )),
    }
}

/// `for issue_activity in issue_activities_created` (`:249`). `None`, a dict,
/// a string or a number abort before any write.
fn activity_list(activities: Option<&PyValue>) -> R<&[PyValue]> {
    match activities {
        Some(PyValue::List(items)) => Ok(items),
        Some(other) => python(format!(
            "TypeError: '{}' object is not iterable",
            python_type_name(other)
        )),
        None => python("TypeError: 'NoneType' object is not iterable"),
    }
}

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

// ---------------------------------------------------------------------------
// The task body
// ---------------------------------------------------------------------------

async fn run(client: &Client, call: &Call) -> R<()> {
    // `json.loads(issue_activities_created)` happens before the type filter.
    let activities = parse_activities(&call.activities_raw)?;
    if call.is_skipped() {
        return Ok(());
    }

    // `project_members = ProjectMember.objects.filter(...)` (`:231-233`); the
    // first evaluation is the `{str(member) …}` set at `:237`.
    let project_members = load_project_members(client, &call.project_id).await?;
    let member_ids: BTreeSet<Uuid> = project_members.iter().flatten().copied().collect();
    let member_strings: BTreeSet<String> = project_members
        .iter()
        .map(|member| match member {
            Some(id) => id.to_string(),
            None => "None".to_string(),
        })
        .collect();

    // `get_new_mentions` / `get_removed_mentions` (`:53-80`).
    let older = extract_mentions(&call.current_instance);
    let newer = extract_mentions(&call.requested_data);
    let new_mentions: Vec<String> = newer
        .iter()
        .filter(|mention| !older.contains(mention))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|mention| member_strings.contains(mention))
        .collect();
    let removed_mentions: Vec<String> = older
        .iter()
        .filter(|mention| !newer.contains(mention))
        .cloned()
        .collect();

    let requested_mentions = extract_mentions(&call.requested_data);
    let mut description_subscribers = mention_subscribers(
        client,
        &call.project_id,
        &call.issue_id,
        &requested_mentions,
    )
    .await?;

    // Comment mentions (`:240-269`).
    let mut comment_mentions: Vec<String> = Vec::new();
    let mut all_comment_mentions: Vec<String> = Vec::new();
    let mut last_seen_activity: Option<&PyValue> = None;
    let list = activity_list(activities.as_ref())?;
    for activity in list {
        let dict = as_dict(Some(activity), "issue_activity")?;
        last_seen_activity = Some(activity);
        if py_truthy(dict.get("issue_comment")) {
            let new_value = dict.get("new_value");
            all_comment_mentions.extend(extract_comment_mentions(new_value));
            let old_value = dict.get("old_value");
            let new_comment_mentions = match old_value {
                None | Some(PyValue::Null) => extract_comment_mentions(new_value),
                Some(old) => {
                    let older = extract_comment_mentions(Some(old));
                    extract_comment_mentions(new_value)
                        .into_iter()
                        .filter(|mention| !older.contains(mention))
                        .collect()
                }
            };
            comment_mentions.extend(new_comment_mentions);
            // `[m for m in comment_mentions if UUID(m) in set(project_members)]`
            // (`:263-265`): a non-UUID mention raises and aborts the task.
            let mut filtered = Vec::with_capacity(comment_mentions.len());
            for mention in &comment_mentions {
                let id = parse_mention_uuid(mention)?;
                if member_ids.contains(&id) {
                    filtered.push(mention.clone());
                }
            }
            comment_mentions = filtered;
        }
    }
    let mut comment_mention_subscribers = mention_subscribers(
        client,
        &call.project_id,
        &call.issue_id,
        &all_comment_mentions,
    )
    .await?;

    // `issue_subscribers` (`:280-288`).
    let excluded = build_excluded_ids(&new_mentions, &comment_mentions, &call.actor_id)?;
    let mut issue_subscribers = load_issue_subscribers(
        client,
        &call.project_id,
        &call.issue_id,
        &member_ids,
        &excluded,
    )
    .await?;

    let issue = load_issue(client, &call.issue_id).await?;

    // `if subscriber: get_or_create(...)` (`:292-299`); every failure is
    // swallowed by its own `try`.
    if py_truthy(Some(&call.subscriber)) {
        let _ = get_or_create_actor_subscriber(client, call).await;
    }

    // `project = Project.objects.get(pk=project_id)` (`:301`).
    let project = load_task_project(client, &call.project_id).await?;

    let issue_assignees =
        load_issue_assignees(client, &call.project_id, &call.issue_id, &member_ids).await?;

    // `set(issue_subscribers) - {uuid.UUID(actor_id)}` (`:309`).
    let actor_uuid = call.actor_id.positional_uuid()?;
    issue_subscribers.retain(|subscriber| *subscriber != actor_uuid);

    // The `issue` model object is only dereferenced when a row is built; a
    // missing/soft-deleted issue with nothing to do must not fail the task.
    // Django caches `issue.project`/`issue.state` on first access, so the load
    // is placed as late as the first build that needs it.
    let mut subscriber_details_needed = false;
    if !issue_subscribers.is_empty() {
        for activity in list {
            if activity_needs_issue_details(activity, &call.issue_id)? {
                subscriber_details_needed = true;
                break;
            }
        }
    }
    let mut issue_details = if subscriber_details_needed {
        Some(IssueDetails::load(client, issue.as_ref()).await?)
    } else {
        None
    };

    let mut notifications: Vec<NotificationRow> = Vec::new();
    let mut emails: Vec<EmailRow> = Vec::new();
    // Python's `subscriber` starts as the parameter and is rebound by the
    // loop below (`:311`); the new-mention e-mail reads whichever value is
    // there (`:576`, `:625`).
    let mut subscriber_var = call.subscriber.clone();

    // Existing subscribers (`:311-450`).
    for subscriber in &issue_subscribers {
        subscriber_var = PyValue::Str(PyStr::new(&subscriber.to_string()));
        let sender = issue_sender(issue.as_ref(), &issue_assignees, *subscriber)?;
        let preference =
            load_preference(client, &PyValue::Str(PyStr::new(&subscriber.to_string()))).await?;
        for activity in list {
            let dict = as_dict(Some(activity), "issue_activity")?;
            // `issue_activity.get("issue_detail").get("id") != issue_id`
            // (`:323`).
            let detail = as_dict(dict.get("issue_detail"), "issue_detail")?;
            let matches = match detail.get("id") {
                Some(id) => !call.issue_id.differs_from(id),
                None => !call.issue_id.differs_from(&PyValue::Null),
            };
            if !matches {
                continue;
            }
            let field = dict.get("field");
            if field == Some(&PyValue::Str(PyStr::new("description"))) {
                continue;
            }
            let details = issue_details.as_ref().expect("a built row needs the issue");
            let send_email = send_email(
                client,
                &call.project_id,
                field,
                &preference,
                dict.get("new_identifier"),
            )
            .await?;
            let issue_comment = if py_truthy(dict.get("issue_comment")) {
                load_issue_comment(
                    client,
                    dict.get("issue_comment"),
                    &call.project_id,
                    &call.issue_id,
                    project.workspace_id,
                )
                .await?
            } else {
                None
            };
            let comment_text = issue_comment.unwrap_or_default();
            notifications.push(NotificationRow {
                workspace_id: project.workspace_id,
                project_id: project.id,
                data: json_text(&subscriber_notification_data(details, dict, &comment_text))?,
                entity_identifier: call.issue_id.field_uuid()?,
                title: text_field(dict.get("comment")),
                message: None,
                sender,
                triggered_by_id: actor_uuid,
                receiver_id: *subscriber,
            });
            if send_email {
                emails.push(EmailRow {
                    receiver: PyValue::Str(PyStr::new(&subscriber.to_string())),
                    triggered_by_id: actor_uuid,
                    entity_identifier: call.issue_id.field_uuid()?,
                    data: json_text(&subscriber_email_data(details, dict, &comment_text))?,
                });
            }
        }
    }

    // `bulk_create(mention_subscribers + comment_mention_subscribers,
    // ignore_conflicts=True)` (`:455-459`).
    let mut subscriber_rows = std::mem::take(&mut description_subscribers);
    subscriber_rows.append(&mut comment_mention_subscribers);
    insert_issue_subscribers(client, &subscriber_rows).await?;

    // `last_activity` (`:461`) and `actor = User.objects.get(...)` (`:463`).
    let last_activity = load_last_activity(client, &call.issue_id).await?;
    let actor = load_actor(client, &call.actor_id).await?;

    let mention_details_needed = comment_mentions
        .iter()
        .any(|mention| !call.actor_id.equals_str(mention))
        || new_mentions
            .iter()
            .any(|mention| !call.actor_id.equals_str(mention));
    if issue_details.is_none() && mention_details_needed {
        issue_details = Some(IssueDetails::load(client, issue.as_ref()).await?);
    }

    // Comment mentions (`:465-520`).
    for mention_id in &comment_mentions {
        if call.actor_id.equals_str(mention_id) {
            continue;
        }
        let details = issue_details
            .as_ref()
            .expect("comment mentions need the issue");
        let preference = load_preference(client, &PyValue::Str(PyStr::new(mention_id))).await?;
        for activity in list {
            let dict = as_dict(Some(activity), "issue_activity")?;
            notifications.push(mention_notification(
                &project,
                mention_id,
                actor_uuid,
                call,
                &format!(
                    "{} has mentioned you in a comment in issue {}",
                    actor.display_name, details.name
                ),
                &mention_notification_data(details, dict),
            )?);
            if preference.mention {
                emails.push(EmailRow {
                    receiver: PyValue::Str(PyStr::new(mention_id)),
                    triggered_by_id: actor_uuid,
                    entity_identifier: call.issue_id.field_uuid()?,
                    data: json_text(&mention_email_data(
                        details,
                        dict,
                        dict.get("created_at").cloned().unwrap_or(PyValue::Null),
                        true,
                    ))?,
                });
            }
        }
    }

    // New description mentions (`:522-659`).
    for mention_id in &new_mentions {
        if call.actor_id.equals_str(mention_id) {
            continue;
        }
        let details = issue_details.as_ref().expect("new mentions need the issue");
        let preference = load_preference(client, &PyValue::Str(PyStr::new(mention_id))).await?;
        let last_is_description = last_activity.as_ref().is_some_and(|activity| {
            activity.field.as_deref() == Some("description")
                && call.actor_id.equals_str(&activity.actor_id.to_string())
        });
        if last_is_description {
            let last = last_activity.as_ref().expect("checked");
            let stale = last_seen_activity.ok_or_else(|| {
                Failure::Python(
                    "UnboundLocalError: cannot access local variable 'issue_activity' \
                     where it is not associated with a value"
                        .to_string(),
                )
            })?;
            let stale = as_dict(Some(stale), "issue_activity")?;
            notifications.push(NotificationRow {
                workspace_id: project.workspace_id,
                project_id: project.id,
                data: json_text(&last_description_notification_data(details, last, stale))?,
                entity_identifier: call.issue_id.field_uuid()?,
                title: Some(String::new()),
                message: Some(json_text(&PyValue::Str(PyStr::new(&format!(
                    "You have been mentioned in the issue {}",
                    details.name
                ))))?),
                sender: "in_app:issue_activities:mentioned",
                triggered_by_id: actor_uuid,
                receiver_id: parse_mention_uuid(mention_id)?,
            });
            if preference.mention {
                emails.push(EmailRow {
                    receiver: subscriber_var.clone(),
                    triggered_by_id: actor_uuid,
                    entity_identifier: call.issue_id.field_uuid()?,
                    data: json_text(&last_description_email_data(details, last, stale))?,
                });
            }
        } else {
            for activity in list {
                let dict = as_dict(Some(activity), "issue_activity")?;
                notifications.push(mention_notification(
                    &project,
                    mention_id,
                    actor_uuid,
                    call,
                    &format!("You have been mentioned in the issue {}", details.name),
                    &mention_notification_data(details, dict),
                )?);
                if preference.mention {
                    emails.push(EmailRow {
                        receiver: subscriber_var.clone(),
                        triggered_by_id: actor_uuid,
                        entity_identifier: call.issue_id.field_uuid()?,
                        data: json_text(&mention_email_data(
                            details,
                            dict,
                            dict.get("created_at").cloned().unwrap_or(PyValue::Null),
                            false,
                        ))?,
                    });
                }
            }
        }
    }

    // `update_mentions_for_issue` (`:37-50`, `:662-667`).
    update_mentions(
        client,
        issue.as_ref(),
        &project,
        &new_mentions,
        &removed_mentions,
        &call.issue_id,
    )
    .await?;

    // `Notification.objects.bulk_create(..., batch_size=100)` (`:669`).
    insert_notifications(client, &notifications).await?;
    // `EmailNotificationLog.objects.bulk_create(..., ignore_conflicts=True)`
    // (`:670`).
    insert_email_logs(client, &emails).await?;
    Ok(())
}

/// `uuid.UUID(mention_id)` for a mention that passed the membership filter.
fn parse_mention_uuid(mention: &str) -> R<Uuid> {
    parse_uuid_text(mention).ok_or_else(|| {
        Failure::Python(format!(
            "ValueError: badly formed hexadecimal UUID string: {mention:?}"
        ))
    })
}

// ---------------------------------------------------------------------------
// HTML mention extraction (`:115-142`)
// ---------------------------------------------------------------------------

/// `extract_mentions` (`:115-129`): `json.loads`, `description_html`, then
/// every `<mention-component entity_name="user_mention">`'s
/// `entity_identifier`, deduplicated. Every exception returns `[]`.
fn extract_mentions(value: &PyValue) -> Vec<String> {
    let PyValue::Str(text) = value else {
        return Vec::new();
    };
    let Ok(data) = pyjson::loads(text.as_wtf8(), "utf-8") else {
        return Vec::new();
    };
    let Some(PyValue::Str(html)) = data.get("description_html") else {
        return Vec::new();
    };
    let html = html.to_string_lossy();
    match crate::bs4::Soup::parse(&html).matching_attribute_values(
        "mention-component",
        "entity_name",
        "user_mention",
        "entity_identifier",
    ) {
        Ok(values) => values
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// `extract_comment_mentions` (`:133-142`): the same over a raw HTML string.
fn extract_comment_mentions(value: Option<&PyValue>) -> Vec<String> {
    let Some(PyValue::Str(html)) = value else {
        return Vec::new();
    };
    let html = html.to_string_lossy();
    match crate::bs4::Soup::parse(&html).matching_attribute_values(
        "mention-component",
        "entity_name",
        "user_mention",
        "entity_identifier",
    ) {
        Ok(values) => values
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        Err(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// `ProjectMember.objects.filter(project_id=…, is_active=True).values_list(
/// "member_id", flat=True)` (`:231-233`).
async fn load_project_members(client: &Client, project_id: &UuidArg) -> R<Vec<Option<Uuid>>> {
    let value = project_id.field_uuid()?;
    let rows = match value {
        Some(id) => {
            client
                .query(
                    "SELECT member_id FROM project_members \
                     WHERE project_id = $1 AND is_active AND deleted_at IS NULL",
                    &[&id],
                )
                .await?
        }
        None => {
            client
                .query(
                    "SELECT member_id FROM project_members \
                     WHERE project_id IS NULL AND is_active AND deleted_at IS NULL",
                    &[],
                )
                .await?
        }
    };
    Ok(rows
        .iter()
        .map(|row| row.get::<_, Option<Uuid>>(0))
        .collect())
}

/// `extract_mentions_as_subscribers` (`:84-111`): the four existence checks,
/// then `Project.objects.get` per qualifying mention.
async fn mention_subscribers(
    client: &Client,
    project_id: &UuidArg,
    issue_id: &UuidArg,
    mentions: &[String],
) -> R<Vec<SubscriberRow>> {
    let mut rows = Vec::new();
    let mut project_workspace: Option<Uuid> = None;
    let project = project_id.field_uuid()?;
    let issue = issue_id.field_uuid()?;
    for mention in mentions {
        let mention_uuid = parse_mention_uuid(mention)?;
        let exists = subscriber_exists(client, project, issue, mention_uuid).await?;
        if exists {
            continue;
        }
        // `IssueAssignee.objects.filter(project_id=…, issue_id=…,
        // assignee_id=…)` (`:95-97`).
        let assigned = match (project, issue) {
            (Some(project), Some(issue)) => client
                .query_opt(
                    "SELECT 1 FROM issue_assignees WHERE project_id = $1 AND issue_id = $2 \
                     AND assignee_id = $3 AND deleted_at IS NULL LIMIT 1",
                    &[&project, &issue, &mention_uuid],
                )
                .await?
                .is_some(),
            _ => false,
        };
        if assigned {
            continue;
        }
        // `Issue.objects.filter(project_id=project_id, pk=issue_id,
        // created_by_id=mention_id)` (`:98`).
        let created_by_mention = match (project, issue) {
            (Some(project), Some(issue)) => client
                .query_opt(
                    "SELECT 1 FROM issues WHERE project_id = $1 AND id = $2 \
                     AND created_by_id = $3 AND deleted_at IS NULL LIMIT 1",
                    &[&project, &issue, &mention_uuid],
                )
                .await?
                .is_some(),
            _ => false,
        };
        if created_by_mention {
            continue;
        }
        // `ProjectMember.objects.filter(project_id=…, member_id=…,
        // is_active=True)` (`:99`).
        let active_member = match project {
            Some(project) => client
                .query_opt(
                    "SELECT 1 FROM project_members WHERE project_id = $1 \
                     AND member_id = $2 AND is_active AND deleted_at IS NULL LIMIT 1",
                    &[&project, &mention_uuid],
                )
                .await?
                .is_some(),
            None => false,
        };
        if !active_member {
            continue;
        }
        // `project = Project.objects.get(pk=project_id)` (`:101`), repeated per
        // qualifying mention exactly like Django.
        let Some(project_id) = project else {
            return python("Project matching query does not exist.");
        };
        if project_workspace.is_none() {
            let row = client
                .query_opt(
                    "SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL",
                    &[&project_id],
                )
                .await?;
            let Some(row) = row else {
                return python("Project matching query does not exist.");
            };
            project_workspace = Some(row.get(0));
        }
        rows.push(SubscriberRow {
            project_id,
            issue_id: issue,
            subscriber_id: mention_uuid,
            workspace_id: project_workspace.expect("set above"),
        });
    }
    Ok(rows)
}

async fn subscriber_exists(
    client: &Client,
    project: Option<Uuid>,
    issue: Option<Uuid>,
    subscriber: Uuid,
) -> R<bool> {
    let exists =
        match (project, issue) {
            (Some(project), Some(issue)) => client
                .query_opt(
                    "SELECT 1 FROM issue_subscribers WHERE issue_id = $1 AND subscriber_id = $2 \
                 AND project_id = $3 AND deleted_at IS NULL LIMIT 1",
                    &[&issue, &subscriber, &project],
                )
                .await?,
            (Some(project), None) => {
                client
                    .query_opt(
                        "SELECT 1 FROM issue_subscribers WHERE issue_id IS NULL \
                 AND subscriber_id = $1 AND project_id = $2 AND deleted_at IS NULL LIMIT 1",
                        &[&subscriber, &project],
                    )
                    .await?
            }
            (None, Some(issue)) => {
                client
                    .query_opt(
                        "SELECT 1 FROM issue_subscribers WHERE issue_id = $1 \
                 AND subscriber_id = $2 AND project_id IS NULL AND deleted_at IS NULL LIMIT 1",
                        &[&issue, &subscriber],
                    )
                    .await?
            }
            (None, None) => {
                client
                    .query_opt(
                        "SELECT 1 FROM issue_subscribers WHERE issue_id IS NULL \
                 AND subscriber_id = $1 AND project_id IS NULL AND deleted_at IS NULL LIMIT 1",
                        &[&subscriber],
                    )
                    .await?
            }
        };
    Ok(exists.is_some())
}

/// `IssueSubscriber.objects.filter(project_id=…, issue_id=…,
/// subscriber__in=Subquery(project_members)).exclude(subscriber_id__in=…)`
/// (`:280-288`).
async fn load_issue_subscribers(
    client: &Client,
    project_id: &UuidArg,
    issue_id: &UuidArg,
    members: &BTreeSet<Uuid>,
    excluded: &[Uuid],
) -> R<Vec<Uuid>> {
    let project = project_id.field_uuid()?;
    let issue = issue_id.field_uuid()?;
    let member_ids: Vec<Uuid> = members.iter().copied().collect();
    let params: Vec<&(dyn ToSql + Sync)> = vec![&project, &issue, &member_ids, &excluded];
    let sql = if excluded.is_empty() {
        "SELECT subscriber_id FROM issue_subscribers \
         WHERE project_id = $1 AND issue_id = $2 AND deleted_at IS NULL \
           AND subscriber_id = ANY($3::uuid[])"
    } else {
        "SELECT subscriber_id FROM issue_subscribers \
         WHERE project_id = $1 AND issue_id = $2 AND deleted_at IS NULL \
           AND subscriber_id = ANY($3::uuid[]) \
           AND subscriber_id <> ALL($4::uuid[])"
    };
    let rows = client.query(sql, &params).await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

/// `Issue.objects.filter(pk=issue_id).first()` (`:290`).
async fn load_issue(client: &Client, issue_id: &UuidArg) -> R<Option<IssueRow>> {
    let Some(value) = issue_id.field_uuid()? else {
        return Ok(None);
    };
    let row = client
        .query_opt(
            "SELECT id, name, sequence_id, project_id, state_id, created_by_id \
             FROM issues WHERE id = $1 AND deleted_at IS NULL",
            &[&value],
        )
        .await?;
    Ok(row.map(|row| IssueRow {
        id: row.get(0),
        name: row.get(1),
        sequence_id: row.get(2),
        project_id: row.get(3),
        state_id: row.get(4),
        created_by_id: row.get(5),
    }))
}

/// `Project.objects.get(pk=project_id)` (`:301`), default manager.
async fn load_task_project(client: &Client, project_id: &UuidArg) -> R<ProjectRow> {
    let Some(value) = project_id.field_uuid()? else {
        return python("Project matching query does not exist.");
    };
    let row = client
        .query_opt(
            "SELECT id, workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL",
            &[&value],
        )
        .await?;
    let Some(row) = row else {
        return python("Project matching query does not exist.");
    };
    Ok(ProjectRow {
        id: row.get(0),
        workspace_id: row.get(1),
    })
}

/// `IssueAssignee.objects.filter(issue_id=…, project_id=…,
/// assignee__in=Subquery(project_members)).values_list("assignee", flat=True)`
/// (`:303-307`).
async fn load_issue_assignees(
    client: &Client,
    project_id: &UuidArg,
    issue_id: &UuidArg,
    members: &BTreeSet<Uuid>,
) -> R<Vec<Uuid>> {
    let project = project_id.field_uuid()?;
    let issue = issue_id.field_uuid()?;
    let member_ids: Vec<Uuid> = members.iter().copied().collect();
    let rows = match (project, issue) {
        (Some(project), Some(issue)) => {
            client
                .query(
                    "SELECT assignee_id FROM issue_assignees \
                     WHERE issue_id = $1 AND project_id = $2 AND deleted_at IS NULL \
                       AND assignee_id = ANY($3::uuid[])",
                    &[&issue, &project, &member_ids],
                )
                .await?
        }
        _ => Vec::new(),
    };
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

/// `UserNotificationPreference.objects.get(user_id=…)` (`:319`, `:467`,
/// `:524`).
async fn load_preference(client: &Client, user: &PyValue) -> R<Preference> {
    let Some(user_id) = field_uuid(user)? else {
        return python("UserNotificationPreference matching query does not exist.");
    };
    let rows = client
        .query(
            "SELECT property_change, state_change, comment, mention, issue_completed \
             FROM user_notification_preferences \
             WHERE user_id = $1 AND deleted_at IS NULL",
            &[&user_id],
        )
        .await?;
    match rows.len() {
        1 => Ok(Preference {
            property_change: rows[0].get(0),
            state_change: rows[0].get(1),
            comment: rows[0].get(2),
            mention: rows[0].get(3),
            issue_completed: rows[0].get(4),
        }),
        0 => python("UserNotificationPreference matching query does not exist."),
        _ => python("UserNotificationPreference multiple objects returned"),
    }
}

/// `State.objects.filter(project_id=…, pk=…, group="completed").exists()`
/// (`:337-341`), only evaluated by the `issue_completed` branch.
async fn state_is_completed(
    client: &Client,
    project_id: &UuidArg,
    state: Option<&PyValue>,
) -> R<bool> {
    let project = project_id.field_uuid()?;
    let state = match state {
        Some(value) => field_uuid(value)?,
        None => None,
    };
    let (Some(project), Some(state)) = (project, state) else {
        return Ok(false);
    };
    Ok(client
        .query_opt(
            "SELECT 1 FROM states WHERE project_id = $1 AND id = $2 \
               AND \"group\" = 'completed' AND deleted_at IS NULL LIMIT 1",
            &[&project, &state],
        )
        .await?
        .is_some())
}

/// `IssueComment.objects.filter(id=…, issue_id=…, project_id=…,
/// workspace_id=…).first()` (`:352-361`).
async fn load_issue_comment(
    client: &Client,
    comment: Option<&PyValue>,
    project_id: &UuidArg,
    issue_id: &UuidArg,
    workspace_id: Uuid,
) -> R<Option<String>> {
    let comment = match comment {
        Some(value) => field_uuid(value)?,
        None => None,
    };
    let project = project_id.field_uuid()?;
    let issue = issue_id.field_uuid()?;
    let (Some(comment), Some(project), Some(issue)) = (comment, project, issue) else {
        return Ok(None);
    };
    let row = client
        .query_opt(
            "SELECT comment_stripped FROM issue_comments \
             WHERE id = $1 AND issue_id = $2 AND project_id = $3 \
               AND workspace_id = $4 AND deleted_at IS NULL LIMIT 1",
            &[&comment, &issue, &project, &workspace_id],
        )
        .await?;
    Ok(row.map(|row| row.get(0)))
}

/// `IssueActivity.objects.filter(issue_id=…).order_by("-created_at").first()`
/// (`:461`).
async fn load_last_activity(client: &Client, issue_id: &UuidArg) -> R<Option<LastActivity>> {
    let Some(value) = issue_id.field_uuid()? else {
        return Ok(None);
    };
    let row = client
        .query_opt(
            "SELECT id, verb, field, actor_id, new_value, old_value, created_at \
             FROM issue_activities WHERE issue_id = $1 AND deleted_at IS NULL \
             ORDER BY created_at DESC LIMIT 1",
            &[&value],
        )
        .await?;
    Ok(row.map(|row| LastActivity {
        id: row.get(0),
        verb: row.get(1),
        field: row.get(2),
        actor_id: row.get(3),
        new_value: row.get(4),
        old_value: row.get(5),
        created_at: row.get(6),
    }))
}

/// `User.objects.get(pk=actor_id)` (`:463`).
async fn load_actor(client: &Client, actor_id: &UuidArg) -> R<ActorRow> {
    let Some(value) = actor_id.field_uuid()? else {
        return python("User matching query does not exist.");
    };
    let row = client
        .query_opt("SELECT display_name FROM users WHERE id = $1", &[&value])
        .await?;
    let Some(row) = row else {
        return python("User matching query does not exist.");
    };
    Ok(ActorRow {
        display_name: row.get(0),
    })
}

/// `IssueSubscriber.objects.get_or_create(...)`'s create branch
/// (`:295-297`); `ProjectBaseModel.save` fills `workspace` from the project.
async fn get_or_create_actor_subscriber(client: &Client, call: &Call) -> R<()> {
    let project = call.project_id.field_uuid()?;
    let issue = call.issue_id.field_uuid()?;
    let actor = call.actor_id.field_uuid()?;
    let (Some(project), Some(issue), Some(actor)) = (project, issue, actor) else {
        return Ok(());
    };
    let exists = client
        .query_opt(
            "SELECT 1 FROM issue_subscribers WHERE project_id = $1 AND issue_id = $2 \
             AND subscriber_id = $3 AND deleted_at IS NULL LIMIT 1",
            &[&project, &issue, &actor],
        )
        .await?
        .is_some();
    if exists {
        return Ok(());
    }
    // `self.project.workspace` uses the forward descriptor's base manager, so
    // a soft-deleted project still resolves; `save` writes every column.
    let workspace = client
        .query_opt(
            "SELECT workspace_id FROM projects WHERE id = $1",
            &[&project],
        )
        .await?;
    let Some(workspace) = workspace else {
        return python("Project matching query does not exist.");
    };
    let workspace: Uuid = workspace.get(0);
    let now = Utc::now();
    client
        .execute(
            "INSERT INTO issue_subscribers \
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
              issue_id, project_id, subscriber_id, workspace_id) \
             VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7)",
            &[
                &Uuid::new_v4(),
                &now,
                &now,
                &issue,
                &project,
                &actor,
                &workspace,
            ],
        )
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Data builders
// ---------------------------------------------------------------------------

/// The exact filters the subscriber loop applies before it builds a row:
/// `issue_detail.get("id") != issue_id` (`:323`) and the description skip
/// (`:327-328`). Used only to decide when `IssueDetails::load` is needed.
fn activity_needs_issue_details(activity: &PyValue, issue_id: &UuidArg) -> R<bool> {
    let dict = as_dict(Some(activity), "issue_activity")?;
    let detail = as_dict(dict.get("issue_detail"), "issue_detail")?;
    let matches = match detail.get("id") {
        Some(id) => !issue_id.differs_from(id),
        None => !issue_id.differs_from(&PyValue::Null),
    };
    if !matches {
        return Ok(false);
    }
    Ok(dict.get("field") != Some(&PyValue::Str(PyStr::new("description"))))
}

fn issue_sender(issue: Option<&IssueRow>, assignees: &[Uuid], subscriber: Uuid) -> R<&'static str> {
    let Some(issue) = issue else {
        return python("AttributeError: 'NoneType' object has no attribute 'created_by_id'");
    };
    if let Some(created_by) = issue.created_by_id {
        if created_by == subscriber {
            return Ok("in_app:issue_activities:created");
        }
        if assignees.contains(&subscriber) && !assignees.contains(&created_by) {
            return Ok("in_app:issue_activities:assigned");
        }
        return Ok("in_app:issue_activities:subscribed");
    }
    if assignees.contains(&subscriber) {
        Ok("in_app:issue_activities:assigned")
    } else {
        Ok("in_app:issue_activities:subscribed")
    }
}

/// `:331-349`.
async fn send_email(
    client: &Client,
    project_id: &UuidArg,
    field: Option<&PyValue>,
    preference: &Preference,
    new_identifier: Option<&PyValue>,
) -> R<bool> {
    let is_state = field.and_then(PyValue::as_str) == Some("state");
    if is_state && preference.state_change {
        return Ok(true);
    }
    if is_state
        && preference.issue_completed
        && state_is_completed(client, project_id, new_identifier).await?
    {
        return Ok(true);
    }
    if field.and_then(PyValue::as_str) == Some("comment") && preference.comment {
        return Ok(true);
    }
    Ok(preference.property_change)
}

fn subscriber_notification_data(issue: &IssueDetails, activity: &PyDict, comment: &str) -> PyValue {
    py_map(vec![
        ("issue", issue_json(issue, false)),
        (
            "issue_activity",
            py_map(vec![
                ("id", str_of(activity.get("id"))),
                ("verb", str_of(activity.get("verb"))),
                ("field", str_of(activity.get("field"))),
                ("actor", str_of(activity.get("actor_id"))),
                ("new_value", str_of(activity.get("new_value"))),
                ("old_value", str_of(activity.get("old_value"))),
                ("issue_comment", PyValue::Str(PyStr::new(comment))),
                ("old_identifier", opt_str_of(activity.get("old_identifier"))),
                ("new_identifier", opt_str_of(activity.get("new_identifier"))),
            ]),
        ),
    ])
}

fn subscriber_email_data(issue: &IssueDetails, activity: &PyDict, comment: &str) -> PyValue {
    py_map(vec![
        ("issue", issue_json(issue, true)),
        (
            "issue_activity",
            py_map(vec![
                ("id", str_of(activity.get("id"))),
                ("verb", str_of(activity.get("verb"))),
                ("field", str_of(activity.get("field"))),
                ("actor", str_of(activity.get("actor_id"))),
                ("new_value", str_of(activity.get("new_value"))),
                ("old_value", str_of(activity.get("old_value"))),
                ("issue_comment", PyValue::Str(PyStr::new(comment))),
                ("old_identifier", opt_str_of(activity.get("old_identifier"))),
                ("new_identifier", opt_str_of(activity.get("new_identifier"))),
                (
                    "activity_time",
                    activity.get("created_at").cloned().unwrap_or(PyValue::Null),
                ),
            ]),
        ),
    ])
}

fn mention_notification_data(issue: &IssueDetails, activity: &PyDict) -> PyValue {
    py_map(vec![
        ("issue", issue_json(issue, false)),
        (
            "issue_activity",
            py_map(vec![
                ("id", str_of(activity.get("id"))),
                ("verb", str_of(activity.get("verb"))),
                ("field", str_of(activity.get("field"))),
                ("actor", str_of(activity.get("actor_id"))),
                ("new_value", str_of(activity.get("new_value"))),
                ("old_value", str_of(activity.get("old_value"))),
                ("old_identifier", opt_str_of(activity.get("old_identifier"))),
                ("new_identifier", opt_str_of(activity.get("new_identifier"))),
            ]),
        ),
    ])
}

fn mention_email_data(
    issue: &IssueDetails,
    activity: &PyDict,
    activity_time: PyValue,
    with_project: bool,
) -> PyValue {
    py_map(vec![
        ("issue", issue_json(issue, with_project)),
        (
            "issue_activity",
            py_map(vec![
                ("id", str_of(activity.get("id"))),
                ("verb", str_of(activity.get("verb"))),
                ("field", PyValue::Str(PyStr::new("mention"))),
                ("actor", str_of(activity.get("actor_id"))),
                ("new_value", str_of(activity.get("new_value"))),
                ("old_value", str_of(activity.get("old_value"))),
                ("old_identifier", opt_str_of(activity.get("old_identifier"))),
                ("new_identifier", opt_str_of(activity.get("new_identifier"))),
                ("activity_time", activity_time),
            ]),
        ),
    ])
}

fn last_description_notification_data(
    issue: &IssueDetails,
    last: &LastActivity,
    stale: &PyDict,
) -> PyValue {
    py_map(vec![
        ("issue", issue_json(issue, true)),
        (
            "issue_activity",
            py_map(vec![
                ("id", PyValue::Str(PyStr::new(&last.id.to_string()))),
                ("verb", PyValue::Str(PyStr::new(&last.verb))),
                ("field", opt_text(last.field.as_deref())),
                (
                    "actor",
                    PyValue::Str(PyStr::new(&last.actor_id.to_string())),
                ),
                ("new_value", opt_text(last.new_value.as_deref())),
                ("old_value", opt_text(last.old_value.as_deref())),
                ("old_identifier", opt_str_of(stale.get("old_identifier"))),
                ("new_identifier", opt_str_of(stale.get("new_identifier"))),
            ]),
        ),
    ])
}

fn last_description_email_data(
    issue: &IssueDetails,
    last: &LastActivity,
    stale: &PyDict,
) -> PyValue {
    py_map(vec![
        ("issue", issue_json(issue, false)),
        (
            "issue_activity",
            py_map(vec![
                ("id", PyValue::Str(PyStr::new(&last.id.to_string()))),
                ("verb", PyValue::Str(PyStr::new(&last.verb))),
                ("field", PyValue::Str(PyStr::new("mention"))),
                (
                    "actor",
                    PyValue::Str(PyStr::new(&last.actor_id.to_string())),
                ),
                ("new_value", opt_text(last.new_value.as_deref())),
                ("old_value", opt_text(last.old_value.as_deref())),
                ("old_identifier", opt_str_of(stale.get("old_identifier"))),
                ("new_identifier", opt_str_of(stale.get("new_identifier"))),
                (
                    "activity_time",
                    PyValue::Str(PyStr::new(&python_datetime(last.created_at))),
                ),
            ]),
        ),
    ])
}

/// One `Notification` for a mention (`create_mention_notification`, `:157-187`).
fn mention_notification(
    project: &ProjectRow,
    mention: &str,
    actor: Uuid,
    call: &Call,
    message: &str,
    data: &PyValue,
) -> R<NotificationRow> {
    Ok(NotificationRow {
        workspace_id: project.workspace_id,
        project_id: project.id,
        data: json_text(data)?,
        entity_identifier: call.issue_id.field_uuid()?,
        title: Some(String::new()),
        message: Some(json_text(&PyValue::Str(PyStr::new(message)))?),
        sender: "in_app:issue_activities:mentioned",
        triggered_by_id: actor,
        receiver_id: parse_mention_uuid(mention)?,
    })
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// `update_mentions_for_issue` (`:37-50`).
async fn update_mentions(
    client: &Client,
    issue: Option<&IssueRow>,
    project: &ProjectRow,
    new_mentions: &[String],
    removed_mentions: &[String],
    issue_arg: &UuidArg,
) -> R<()> {
    if !new_mentions.is_empty() && issue.is_none() {
        // `IssueMention(issue=None)` is a NOT NULL violation at insert.
        return python(
            "IntegrityError: null value in column \"issue_id\" of relation \
             \"issue_mentions\" violates not-null constraint",
        );
    }
    let mut rows = Vec::new();
    for mention in new_mentions {
        rows.push(MentionRow {
            issue_id: issue.map(|issue| issue.id),
            project_id: project.id,
            workspace_id: project.workspace_id,
            mention_id: parse_mention_uuid(mention)?,
        });
    }
    insert_issue_mentions(client, &rows).await?;

    let mut removed = Vec::new();
    for mention in removed_mentions {
        removed.push(parse_mention_uuid(mention)?);
    }
    if removed.is_empty() {
        return Ok(());
    }
    let issue_filter = match issue {
        Some(issue) => Some(issue.id),
        None => issue_arg.field_uuid()?,
    };
    match issue_filter {
        Some(issue) => {
            client
                .execute(
                    "UPDATE issue_mentions SET deleted_at = now() \
                     WHERE issue_id = $1 AND mention_id = ANY($2::uuid[]) \
                       AND deleted_at IS NULL",
                    &[&issue, &removed],
                )
                .await?;
        }
        None => {
            client
                .execute(
                    "UPDATE issue_mentions SET deleted_at = now() \
                     WHERE issue_id IS NULL AND mention_id = ANY($1::uuid[]) \
                       AND deleted_at IS NULL",
                    &[&removed],
                )
                .await?;
        }
    }
    Ok(())
}

async fn insert_issue_mentions(client: &Client, rows: &[MentionRow]) -> R<()> {
    for chunk in rows.chunks(100) {
        let now = Utc::now();
        let ids: Vec<Uuid> = chunk.iter().map(|_| Uuid::new_v4()).collect();
        let issue_ids: Vec<Option<Uuid>> = chunk.iter().map(|row| row.issue_id).collect();
        let mention_ids: Vec<Uuid> = chunk.iter().map(|row| row.mention_id).collect();
        let project_ids: Vec<Uuid> = chunk.iter().map(|row| row.project_id).collect();
        let workspace_ids: Vec<Uuid> = chunk.iter().map(|row| row.workspace_id).collect();
        let created: Vec<Option<Uuid>> = vec![None; chunk.len()];
        let deleted: Vec<Option<DateTime<Utc>>> = vec![None; chunk.len()];
        let nows = vec![now; chunk.len()];
        client
            .execute(
                "INSERT INTO issue_mentions \
                 (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
                  issue_id, mention_id, project_id, workspace_id) \
                 SELECT * FROM UNNEST(\
                   $1::uuid[], $2::timestamptz[], $3::timestamptz[], \
                   $4::uuid[], $5::uuid[], $6::timestamptz[], \
                   $7::uuid[], $8::uuid[], $9::uuid[], $10::uuid[])",
                &[
                    &ids,
                    &nows,
                    &nows,
                    &created,
                    &created,
                    &deleted,
                    &issue_ids,
                    &mention_ids,
                    &project_ids,
                    &workspace_ids,
                ],
            )
            .await?;
    }
    Ok(())
}

async fn insert_issue_subscribers(client: &Client, rows: &[SubscriberRow]) -> R<()> {
    for chunk in rows.chunks(100) {
        let now = Utc::now();
        let ids: Vec<Uuid> = chunk.iter().map(|_| Uuid::new_v4()).collect();
        let issue_ids: Vec<Option<Uuid>> = chunk.iter().map(|row| row.issue_id).collect();
        let project_ids: Vec<Uuid> = chunk.iter().map(|row| row.project_id).collect();
        let subscriber_ids: Vec<Uuid> = chunk.iter().map(|row| row.subscriber_id).collect();
        let workspace_ids: Vec<Uuid> = chunk.iter().map(|row| row.workspace_id).collect();
        let created: Vec<Option<Uuid>> = vec![None; chunk.len()];
        let deleted: Vec<Option<DateTime<Utc>>> = vec![None; chunk.len()];
        let nows = vec![now; chunk.len()];
        client
            .execute(
                "INSERT INTO issue_subscribers \
                 (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
                  issue_id, project_id, subscriber_id, workspace_id) \
                 SELECT * FROM UNNEST(\
                   $1::uuid[], $2::timestamptz[], $3::timestamptz[], \
                   $4::uuid[], $5::uuid[], $6::timestamptz[], \
                   $7::uuid[], $8::uuid[], $9::uuid[], $10::uuid[]) \
                 ON CONFLICT DO NOTHING",
                &[
                    &ids,
                    &nows,
                    &nows,
                    &created,
                    &created,
                    &deleted,
                    &issue_ids,
                    &project_ids,
                    &subscriber_ids,
                    &workspace_ids,
                ],
            )
            .await?;
    }
    Ok(())
}

async fn insert_notifications(client: &Client, rows: &[NotificationRow]) -> R<()> {
    const COLUMNS: usize = 11;
    for chunk in rows.chunks(100) {
        let now = Utc::now();
        let mut sql = String::from(
            "INSERT INTO notifications \
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
              workspace_id, project_id, data, entity_identifier, entity_name, title, \
              message, message_html, message_stripped, sender, triggered_by_id, \
              receiver_id, read_at, snoozed_till, archived_at) VALUES ",
        );
        let mut params: Vec<Param> = Vec::with_capacity(chunk.len() * COLUMNS);
        for (index, row) in chunk.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            let base = index * COLUMNS;
            sql.push_str(&format!(
                "(${}, ${}, ${}, NULL, NULL, NULL, ${}, ${}, ${}::text::jsonb, ${}, 'issue', \
                 ${}, ${}::text::jsonb, '<p></p>', NULL, ${}, ${}, ${}, NULL, NULL, NULL)",
                base + 1,
                base + 2,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
                base + 7,
                base + 8,
                base + 9,
                base + 10,
                base + 11,
            ));
            params.push(Param::Uuid(Uuid::new_v4()));
            params.push(Param::Timestamp(now));
            params.push(Param::Uuid(row.workspace_id));
            params.push(Param::Uuid(row.project_id));
            params.push(Param::Text(row.data.clone()));
            params.push(Param::OptUuid(row.entity_identifier));
            params.push(Param::OptText(row.title.clone()));
            params.push(Param::OptText(row.message.clone()));
            params.push(Param::Text(row.sender.to_string()));
            params.push(Param::Uuid(row.triggered_by_id));
            params.push(Param::Uuid(row.receiver_id));
        }
        let refs: Vec<&(dyn ToSql + Sync)> = params
            .iter()
            .map(|value| value as &(dyn ToSql + Sync))
            .collect();
        client.execute(&sql, &refs).await?;
    }
    Ok(())
}

async fn insert_email_logs(client: &Client, rows: &[EmailRow]) -> R<()> {
    for chunk in rows.chunks(100) {
        let now = Utc::now();
        let mut sql = String::from(
            "INSERT INTO email_notification_logs \
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
              receiver_id, triggered_by_id, entity_identifier, entity_name, data, \
              processed_at, sent_at, entity, old_value, new_value) VALUES ",
        );
        let mut params: Vec<Param> = Vec::with_capacity(chunk.len() * 8);
        for (index, row) in chunk.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            let base = index * 6;
            sql.push_str(&format!(
                "(${}, ${}, ${}, NULL, NULL, NULL, ${}, ${}, ${}, 'issue', \
                 ${}::text::jsonb, NULL, NULL, '', NULL, NULL)",
                base + 1,
                base + 2,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
            ));
            params.push(Param::Uuid(Uuid::new_v4()));
            params.push(Param::Timestamp(now));
            params.push(Param::OptUuid(field_uuid(&row.receiver)?));
            params.push(Param::OptUuid(Some(row.triggered_by_id)));
            params.push(Param::OptUuid(row.entity_identifier));
            params.push(Param::Text(row.data.clone()));
        }
        let refs: Vec<&(dyn ToSql + Sync)> = params
            .iter()
            .map(|value| value as &(dyn ToSql + Sync))
            .collect();
        client
            .execute(&format!("{sql} ON CONFLICT DO NOTHING"), &refs)
            .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rows and value helpers
// ---------------------------------------------------------------------------

struct ProjectRow {
    id: Uuid,
    workspace_id: Uuid,
}

struct IssueRow {
    id: Uuid,
    name: String,
    sequence_id: i32,
    project_id: Uuid,
    state_id: Option<Uuid>,
    created_by_id: Option<Uuid>,
}

struct ActorRow {
    display_name: String,
}

struct LastActivity {
    id: Uuid,
    verb: String,
    field: Option<String>,
    actor_id: Uuid,
    new_value: Option<String>,
    old_value: Option<String>,
    created_at: DateTime<Utc>,
}

struct Preference {
    property_change: bool,
    state_change: bool,
    comment: bool,
    mention: bool,
    issue_completed: bool,
}

struct SubscriberRow {
    project_id: Uuid,
    issue_id: Option<Uuid>,
    subscriber_id: Uuid,
    workspace_id: Uuid,
}

struct MentionRow {
    issue_id: Option<Uuid>,
    project_id: Uuid,
    workspace_id: Uuid,
    mention_id: Uuid,
}

struct NotificationRow {
    workspace_id: Uuid,
    project_id: Uuid,
    data: String,
    entity_identifier: Option<Uuid>,
    title: Option<String>,
    message: Option<String>,
    sender: &'static str,
    triggered_by_id: Uuid,
    receiver_id: Uuid,
}

struct EmailRow {
    receiver: PyValue,
    triggered_by_id: Uuid,
    entity_identifier: Option<Uuid>,
    data: String,
}

/// One dynamically typed bind for the multi-row inserts, which build their
/// parameter list at run time.
#[derive(Debug)]
enum Param {
    Uuid(Uuid),
    OptUuid(Option<Uuid>),
    Text(String),
    OptText(Option<String>),
    Timestamp(DateTime<Utc>),
}

impl ToSql for Param {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut tokio_postgres::types::private::BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self {
            Self::Uuid(value) => value.to_sql(ty, out),
            Self::OptUuid(value) => value.to_sql(ty, out),
            Self::Text(value) => value.to_sql(ty, out),
            Self::OptText(value) => value.to_sql(ty, out),
            Self::Timestamp(value) => value.to_sql(ty, out),
        }
    }

    fn accepts(ty: &Type) -> bool {
        <Uuid as ToSql>::accepts(ty)
            || <String as ToSql>::accepts(ty)
            || <DateTime<Utc> as ToSql>::accepts(ty)
    }

    tokio_postgres::types::to_sql_checked!();
}

/// The `issue` object's fields every notification reads. Django caches the
/// related `project`/`state` objects on the instance after the first access.
struct IssueDetails {
    name: String,
    id: Uuid,
    sequence_id: i32,
    project_id: Uuid,
    project_identifier: String,
    workspace_slug: String,
    state_name: String,
    state_group: String,
}

impl IssueDetails {
    async fn load(client: &Client, issue: Option<&IssueRow>) -> R<Self> {
        let Some(issue) = issue else {
            return python("AttributeError: 'NoneType' object has no attribute 'name'");
        };
        let project = client
            .query_opt(
                "SELECT identifier, workspace_id FROM projects WHERE id = $1",
                &[&issue.project_id],
            )
            .await?;
        let Some(project) = project else {
            return python("AttributeError: 'NoneType' object has no attribute 'identifier'");
        };
        let identifier: String = project.get(0);
        let workspace_id: Uuid = project.get(1);
        let slug = client
            .query_opt(
                "SELECT slug FROM workspaces WHERE id = $1",
                &[&workspace_id],
            )
            .await?;
        let Some(slug) = slug else {
            return python("AttributeError: 'NoneType' object has no attribute 'slug'");
        };
        let state = match issue.state_id {
            Some(state_id) => {
                client
                    .query_opt(
                        "SELECT name, \"group\" FROM states WHERE id = $1",
                        &[&state_id],
                    )
                    .await?
            }
            None => None,
        };
        let Some(state) = state else {
            return python("AttributeError: 'NoneType' object has no attribute 'name'");
        };
        Ok(Self {
            name: issue.name.clone(),
            id: issue.id,
            sequence_id: issue.sequence_id,
            project_id: issue.project_id,
            project_identifier: identifier,
            workspace_slug: slug.get(0),
            state_name: state.get(0),
            state_group: state.get(1),
        })
    }
}

/// `{"id", "name", "identifier", "sequence_id", "state_name", "state_group"}`
/// plus, for the e-mail variants, `project_id` and `workspace_slug`
/// (`notification_task.py:374-382`, `:416-425`, `:488-497`, …).
fn issue_json(issue: &IssueDetails, with_project: bool) -> PyValue {
    let mut dict = PyDict::with_capacity(if with_project { 8 } else { 6 });
    dict.insert(
        PyStr::new("id"),
        PyValue::Str(PyStr::new(&issue.id.to_string())),
    );
    dict.insert(PyStr::new("name"), PyValue::Str(PyStr::new(&issue.name)));
    dict.insert(
        PyStr::new("identifier"),
        PyValue::Str(PyStr::new(&issue.project_identifier)),
    );
    dict.insert(
        PyStr::new("sequence_id"),
        PyValue::from(i64::from(issue.sequence_id)),
    );
    dict.insert(
        PyStr::new("state_name"),
        PyValue::Str(PyStr::new(&issue.state_name)),
    );
    dict.insert(
        PyStr::new("state_group"),
        PyValue::Str(PyStr::new(&issue.state_group)),
    );
    if with_project {
        dict.insert(
            PyStr::new("project_id"),
            PyValue::Str(PyStr::new(&issue.project_id.to_string())),
        );
        dict.insert(
            PyStr::new("workspace_slug"),
            PyValue::Str(PyStr::new(&issue.workspace_slug)),
        );
    }
    PyValue::Dict(dict)
}

fn py_map(pairs: Vec<(&str, PyValue)>) -> PyValue {
    let mut dict = PyDict::with_capacity(pairs.len());
    for (key, value) in pairs {
        dict.insert(PyStr::new(key), value);
    }
    PyValue::Dict(dict)
}

/// `str(x)`; a missing key is Python `None` → `"None"`.
fn str_of(value: Option<&PyValue>) -> PyValue {
    match value {
        Some(value) => PyValue::Str(PyStr::new(&py_str(value))),
        None => PyValue::Str(PyStr::new("None")),
    }
}

/// `str(x) if x else None`.
fn opt_str_of(value: Option<&PyValue>) -> PyValue {
    match value {
        Some(value) if py_truthy(Some(value)) => PyValue::Str(PyStr::new(&py_str(value))),
        _ => PyValue::Null,
    }
}

/// `str(x)` for an already-string model value (`None` → `"None"`).
fn opt_text(value: Option<&str>) -> PyValue {
    match value {
        Some(value) => PyValue::Str(PyStr::new(value)),
        None => PyValue::Str(PyStr::new("None")),
    }
}

/// A text column's `get_prep_value`: `str(value)`, or NULL.
fn text_field(value: Option<&PyValue>) -> Option<String> {
    match value {
        None | Some(PyValue::Null) => None,
        Some(value) => Some(py_str(value)),
    }
}

/// `json.dumps(value, cls=DjangoJSONEncoder)`.
fn json_text(value: &PyValue) -> R<String> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)
        .map_err(|error| Failure::Python(format!("json.dumps: {error}")))?;
    String::from_utf8(bytes).map_err(|error| Failure::Python(format!("json.dumps: {error}")))
}

/// Python `str(datetime)`: a space separator and `+00:00`; microseconds only
/// when non-zero.
fn python_datetime(value: DateTime<Utc>) -> String {
    let base = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        value.month(),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    );
    let micros = value.nanosecond() / 1_000;
    if micros == 0 {
        format!("{base}+00:00")
    } else {
        format!("{base}.{micros:06}+00:00")
    }
}

fn build_excluded_ids(
    new_mentions: &[String],
    comment_mentions: &[String],
    actor: &UuidArg,
) -> R<Vec<Uuid>> {
    let mut out = Vec::new();
    for mention in new_mentions.iter().chain(comment_mentions) {
        out.push(parse_mention_uuid(mention)?);
    }
    if let Some(actor) = actor.field_uuid()? {
        out.push(actor);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn py(text: &str) -> PyValue {
        PyValue::Str(PyStr::new(text))
    }

    #[test]
    fn the_skip_list_is_the_thirteen_types_from_python() {
        assert_eq!(SKIPPED_TYPES.len(), 13);
        for kind in [
            "cycle.activity.created",
            "cycle.activity.deleted",
            "module.activity.created",
            "module.activity.deleted",
            "issue_reaction.activity.created",
            "issue_reaction.activity.deleted",
            "comment_reaction.activity.created",
            "comment_reaction.activity.deleted",
            "issue_vote.activity.created",
            "issue_vote.activity.deleted",
            "issue_draft.activity.created",
            "issue_draft.activity.updated",
            "issue_draft.activity.deleted",
        ] {
            assert!(SKIPPED_TYPES.contains(&kind), "{kind} is skipped");
        }
        assert!(!SKIPPED_TYPES.contains(&"issue.activity.updated"));
    }

    #[test]
    fn mentions_come_from_mention_components_only() {
        let value = py(
            r#"{"description_html":"<p><mention-component entity_name=\"user_mention\" entity_identifier=\"11111111-1111-4111-8111-111111111111\"/></p>"}"#,
        );
        assert_eq!(
            extract_mentions(&value),
            vec!["11111111-1111-4111-8111-111111111111"]
        );
        let other = py(
            r#"{"description_html":"<mention-component entity_name=\"other\" entity_identifier=\"x\"/>"}"#,
        );
        assert!(extract_mentions(&other).is_empty());
    }

    #[test]
    fn a_missing_attribute_makes_the_whole_extraction_empty() {
        let value = py(
            r#"{"description_html":"<mention-component entity_name=\"user_mention\" entity_identifier=\"a\"/><mention-component entity_name=\"user_mention\"/>"}"#,
        );
        assert!(extract_mentions(&value).is_empty());
    }

    #[test]
    fn malformed_json_and_non_string_values_extract_nothing() {
        assert!(extract_mentions(&py("{")).is_empty());
        assert!(extract_mentions(&PyValue::Null).is_empty());
        assert!(extract_mentions(&PyValue::from(5_i64)).is_empty());
        assert!(extract_mentions(&py(r#"{"description_html": 5}"#)).is_empty());
    }

    #[test]
    fn typed_uuid_arguments_never_equal_the_string_detail_id() {
        let typed = UuidArg {
            raw: py("11111111-1111-4111-8111-111111111111"),
            typed: true,
        };
        assert!(typed.differs_from(&py("11111111-1111-4111-8111-111111111111")));
        let untyped = UuidArg {
            raw: py("11111111-1111-4111-8111-111111111111"),
            typed: false,
        };
        assert!(!untyped.differs_from(&py("11111111-1111-4111-8111-111111111111")));
        assert!(untyped.differs_from(&PyValue::Null));
    }

    #[test]
    fn python_datetime_keeps_the_space_and_offset() {
        let value = "2026-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(python_datetime(value), "2026-01-01 00:00:00+00:00");
        let value = "2026-01-01T00:00:00.123456Z"
            .parse::<DateTime<Utc>>()
            .unwrap();
        assert_eq!(python_datetime(value), "2026-01-01 00:00:00.123456+00:00");
    }

    #[test]
    fn uuid_field_coercion_accepts_bools_and_rejects_floats() {
        assert_eq!(
            field_uuid(&PyValue::Bool(true)).unwrap(),
            Some(Uuid::from_u128(1))
        );
        assert_eq!(field_uuid(&PyValue::Null).unwrap(), None);
        assert!(field_uuid(&PyValue::Float(1.5)).is_err());
        assert!(field_uuid(&py("nope")).is_err());
    }
}
