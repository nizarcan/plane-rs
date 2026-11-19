// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `get_model_data` (`plane/bgtasks/webhook_task.py:125-168`): one entity as the
//! webhook payload's `data`.
//!
//! The task serialises with `plane.api.serializers`' `*Serializer(instance)`
//! **without a request**, i.e. no timezone activation and no annotations. Two
//! consequences are ported as they are:
//!
//! * annotation fields (`ProjectSerializer.total_members`, `CycleSerializer.
//!   total_issues`, `ModuleSerializer.total_issues`, `IssueCommentSerializer.
//!   is_member`, `CycleIssueSerializer.sub_issues_count`, …) have no attribute
//!   on a bare instance, so DRF raises `SkipField` and the key is **absent**
//!   (`rest_framework/fields.py:get_attribute`: `not self.required` →
//!   `SkipField`). They are never rendered as `null`.
//! * `IssueExpandSerializer.cycle`/`.module` read `issue_cycle.cycle` and
//!   `issue_module.module` (`plane/api/serializers/issue.py:852-853`), but
//!   `issue_cycle` is a reverse **manager**, not an object
//!   (`db/models/cycle.py:109`), so both fields raise `AttributeError` and are
//!   absent from every issue webhook. `db/models/issue.py` has no
//!   `issue_cycle` property.
//!
//! A UUID renders as its hyphenated string, a datetime as `isoformat()` in UTC
//! with `Z` (DRF's `DateTimeField`, `rest_framework/fields.py:1190`), a date as
//! `%Y-%m-%d`, a `JSONField` as the value Postgres stored.
//!
//! `issue` is the only event `get_model_data` prefetches for: `label_issue`
//! (ordered `-created_at`) and `issue_assignee` (same), both expanded into
//! `LabelLiteSerializer` / `UserLiteSerializer` objects because the call passes
//! `context={"expand": ["labels", "assignees"]}`.

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_db::JsonText;
use plane_db::models::{Issue, IssueComment, Project, User, with_alias};
use plane_serde::pyjson::{PyDict, PyStr, Value};
use uuid::Uuid;

/// Why an entity could not be serialised.
#[derive(Debug, Clone)]
pub enum EntityError {
    /// `ObjectDoesNotExist`: the default manager hides the row (soft-deleted)
    /// or it never existed.
    NotFound,
    /// Anything else DRF raises while serialising: a `ValidationError` for a
    /// malformed id, or a `TypeError` from a value the JSON encoder refuses.
    Invalid(String),
}

impl std::fmt::Display for EntityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("no row"),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

/// `get_model_data(event, event_id)`.
pub async fn model_data(
    client: &Client,
    event: &str,
    event_id: &str,
) -> Result<Value, EntityError> {
    match event {
        "project" => project(client, event_id).await,
        "issue" => issue(client, event_id, true).await,
        "cycle" => cycle(client, event_id).await,
        "module" => module(client, event_id).await,
        "cycle_issue" => cycle_issue(client, event_id).await,
        "module_issue" => module_issue(client, event_id).await,
        "issue_comment" => issue_comment(client, event_id).await,
        "user" => user(client, event_id).await,
        "intake_issue" => intake_issue(client, event_id).await,
        other => Err(EntityError::Invalid(format!(
            "Model not found for event: {other}"
        ))),
    }
}

/// The UUID an id column accepts. `UUIDField.get_prep_value` raises
/// `ValidationError`, which is not `ObjectDoesNotExist`.
fn parse_id(id: &str) -> Result<Uuid, EntityError> {
    Uuid::parse_str(id)
        .map_err(|_| EntityError::Invalid(format!("\u{2018}{id}\u{2019} is not a valid UUID.")))
}

// ---------------------------------------------------------------------------
// Key order
// ---------------------------------------------------------------------------

/// The serializer's `_readable_fields` order.
///
/// `webhook_activity` (the producer) applies this before publishing, because
/// `model_data` returns Python dicts whose field order is not the serializer's
/// (`_readable_fields` is). The queue now keeps that order (`json` payload,
/// `plane_jobs::payload`), so `webhook_send_task` no longer re-applies it; the
/// function stays as the producer's order definition.
fn reorder(value: &Value, fields: &[&str]) -> Value {
    let Value::Dict(dict) = value else {
        return value.clone();
    };
    let mut out = PyDict::with_capacity(dict.len());
    for field in fields {
        if let Some(item) = dict.get(field) {
            out.insert(PyStr::new(field), reorder_field(field, item));
        }
    }
    // An unknown key would be a producer/port mismatch; keep it rather than
    // silently dropping data, after the known fields as Python's dict would.
    for (key, item) in dict.iter() {
        if !fields
            .iter()
            .any(|field| *field == key.as_str().unwrap_or(""))
        {
            out.insert(key.clone(), item.clone());
        }
    }
    Value::Dict(out)
}

fn reorder_field(field: &str, value: &Value) -> Value {
    match field {
        "labels" | "assignees" | "members" => match value {
            Value::List(items) if field == "labels" => Value::List(
                items
                    .iter()
                    .map(|item| reorder(item, LABEL_FIELDS))
                    .collect(),
            ),
            Value::List(items) if field == "assignees" => Value::List(
                items
                    .iter()
                    .map(|item| reorder(item, USER_FIELDS))
                    .collect(),
            ),
            other => other.clone(),
        },
        "actor" if value.is_null() => Value::Null,
        "actor" => reorder(value, USER_FIELDS),
        "issue_detail" => reorder(value, ISSUE_FIELDS),
        other => {
            let _ = other;
            value.clone()
        }
    }
}

/// `ProjectSerializer::FIELDS` (`plane-serializers/src/generated/api_project.rs`).
pub const PROJECT_FIELDS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "network",
    "identifier",
    "emoji",
    "icon_prop",
    "module_view",
    "cycle_view",
    "issue_views_view",
    "page_view",
    "intake_view",
    "is_time_tracking_enabled",
    "is_issue_type_enabled",
    "guest_view_all_features",
    "cover_image",
    "archive_in",
    "close_in",
    "logo_props",
    "archived_at",
    "timezone",
    "external_source",
    "external_id",
    "created_by",
    "updated_by",
    "workspace",
    "default_assignee",
    "project_lead",
    "cover_image_asset",
    "estimate",
    "default_state",
    // `cover_image_url` is declared before `created_at` in the serializer; the
    // generated order puts the annotation fields first. See
    // `ProjectSerializer::FIELDS` for the exact list; it is inserted below by
    // `project_body_fields`.
];

/// The project body's field order with the optional `cover_image_url` in its
/// serializer position.
#[must_use]
pub fn project_fields(has_cover_image_url: bool) -> Vec<&'static str> {
    let mut fields = vec![
        "id",
        "total_members",
        "total_cycles",
        "total_modules",
        "is_member",
        "sort_order",
        "member_role",
        "is_deployed",
    ];
    if has_cover_image_url {
        fields.push("cover_image_url");
    }
    fields.extend_from_slice(&PROJECT_FIELDS[1..]);
    fields
}

/// `IssueExpandSerializer::FIELDS`.
pub const ISSUE_FIELDS: &[&str] = &[
    "id",
    "cycle",
    "module",
    "labels",
    "assignees",
    "state",
    "description",
    "created_at",
    "updated_at",
    "deleted_at",
    "point",
    "name",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "parent",
    "estimate_point",
    "type",
];

/// `CycleSerializer::FIELDS`, annotation fields included for completeness (a
/// webhook value never carries them).
pub const CYCLE_FIELDS: &[&str] = &[
    "id",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "total_estimates",
    "completed_estimates",
    "started_estimates",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "start_date",
    "end_date",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "archived_at",
    "logo_props",
    "timezone",
    "version",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "owned_by",
];

/// `ModuleSerializer::FIELDS` (the model fields then `members`).
pub const MODULE_FIELDS: &[&str] = &[
    "id",
    "total_issues",
    "cancelled_issues",
    "completed_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "archived_at",
    "logo_props",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "lead",
    "members",
];

/// `CycleIssueSerializer::FIELDS`.
pub const CYCLE_ISSUE_FIELDS: &[&str] = &[
    "id",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
    "cycle",
];

/// `ModuleIssueSerializer::FIELDS`.
pub const MODULE_ISSUE_FIELDS: &[&str] = &[
    "id",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "deleted_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "module",
    "issue",
];

/// `IssueCommentSerializer::FIELDS`.
pub const ISSUE_COMMENT_FIELDS: &[&str] = &[
    "id",
    "is_member",
    "created_at",
    "updated_at",
    "deleted_at",
    "comment_html",
    "attachments",
    "access",
    "external_source",
    "external_id",
    "edited_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "description",
    "issue",
    "actor",
    "parent",
];

/// `UserLiteSerializer::FIELDS`.
pub const USER_FIELDS: &[&str] = &[
    "id",
    "first_name",
    "last_name",
    "email",
    "avatar",
    "avatar_url",
    "display_name",
];

/// `LabelLiteSerializer::FIELDS`.
pub const LABEL_FIELDS: &[&str] = &["id", "name", "color"];

/// `StateLiteSerializer::FIELDS`.
pub const STATE_FIELDS: &[&str] = &["id", "name", "color", "group"];

/// `IntakeIssueSerializer::FIELDS`.
pub const INTAKE_ISSUE_FIELDS: &[&str] = &[
    "id",
    "issue_detail",
    "inbox",
    "created_at",
    "updated_at",
    "deleted_at",
    "status",
    "snoozed_till",
    "source",
    "source_email",
    "external_source",
    "external_id",
    "extra",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "intake",
    "issue",
    "duplicate_to",
];

/// Rebuild `event_data`'s key order for one event type.
#[must_use]
pub fn reorder_event_data(event: &str, value: &Value) -> Value {
    match event {
        "project" => reorder(
            value,
            &project_fields(value.get("cover_image_url").is_some()),
        ),
        "issue" => reorder(value, ISSUE_FIELDS),
        "cycle" => reorder(value, CYCLE_FIELDS),
        "module" => reorder(value, MODULE_FIELDS),
        "cycle_issue" => reorder(value, CYCLE_ISSUE_FIELDS),
        "module_issue" => reorder(value, MODULE_ISSUE_FIELDS),
        "issue_comment" => reorder(value, ISSUE_COMMENT_FIELDS),
        "user" => reorder(value, USER_FIELDS),
        "intake_issue" => reorder(value, INTAKE_ISSUE_FIELDS),
        _ => value.clone(),
    }
}

/// Rebuild an activity dict's key order: `field`, `new_value`, `old_value`,
/// `actor`, `old_identifier`, `new_identifier` (`webhook_task.py:458-465`).
#[must_use]
pub fn reorder_activity(value: &Value) -> Value {
    reorder(
        value,
        &[
            "field",
            "new_value",
            "old_value",
            "actor",
            "old_identifier",
            "new_identifier",
        ],
    )
}

// ---------------------------------------------------------------------------
// Value helpers
// ---------------------------------------------------------------------------

fn text(value: &str) -> Value {
    Value::Str(PyStr::new(value))
}

fn owned(value: String) -> Value {
    Value::Str(PyStr::from_string(value))
}

fn owned_optional(value: Option<String>) -> Value {
    value.map_or(Value::Null, owned)
}

fn uuid(value: Uuid) -> Value {
    owned(value.hyphenated().to_string())
}

fn uuid_optional(value: Option<Uuid>) -> Value {
    value.map_or(Value::Null, uuid)
}

/// DRF `DateTimeField.to_representation` without an activated timezone: UTC,
/// `isoformat()`, `+00:00` → `Z`.
fn datetime(value: DateTime<Utc>) -> Value {
    let base = value.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = value.timestamp_subsec_micros();
    if micros == 0 {
        owned(format!("{base}Z"))
    } else {
        owned(format!("{base}.{micros:06}Z"))
    }
}

fn datetime_optional(value: Option<DateTime<Utc>>) -> Value {
    value.map_or(Value::Null, datetime)
}

fn date(value: NaiveDate) -> Value {
    owned(value.format("%Y-%m-%d").to_string())
}

fn date_optional(value: Option<NaiveDate>) -> Value {
    value.map_or(Value::Null, date)
}

/// A `JSONField`: DRF returns the Python object `json.loads` would build from
/// the stored text.
fn json(value: &JsonText) -> Value {
    plane_serde::pyjson::loads(value.as_str().as_bytes(), "utf-8").unwrap_or(Value::Null)
}

fn json_optional(value: Option<&JsonText>) -> Value {
    value.map_or(Value::Null, json)
}

fn int(value: i32) -> Value {
    Value::from(i64::from(value))
}

fn big_int(value: i64) -> Value {
    Value::from(value)
}

fn float(value: f64) -> Value {
    Value::from(value)
}

fn boolean(value: bool) -> Value {
    Value::Bool(value)
}

/// Build one object in `_readable_fields` order.
struct Object {
    entries: PyDict,
}

impl Object {
    fn new() -> Self {
        Self {
            entries: PyDict::new(),
        }
    }

    fn set(mut self, key: &str, value: Value) -> Self {
        self.entries.insert(PyStr::new(key), value);
        self
    }

    fn build(self) -> Value {
        Value::Dict(self.entries)
    }
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

/// `ProjectSerializer` (`plane/api/serializers/project.py:210-240`). The eight
/// annotation fields are absent; `cover_image_url` is present unless the asset
/// row is hidden by the default manager, in which case the property raises
/// `RelatedObjectDoesNotExist` and DRF skips the field.
async fn project(client: &Client, id: &str) -> Result<Value, EntityError> {
    let id = parse_id(id)?;
    let columns = with_alias(Project::COLUMNS, "p");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns}, fa.id FROM projects p \
                 LEFT JOIN file_assets fa ON fa.id = p.cover_image_asset_id \
                     AND fa.deleted_at IS NULL \
                 WHERE p.id = $1 AND p.deleted_at IS NULL"
            ),
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?
        .ok_or(EntityError::NotFound)?;
    let project =
        Project::from_row(&row, 0).map_err(|error| EntityError::Invalid(error.to_string()))?;
    let asset_present: Option<Uuid> = row.get(Project::COLUMN_COUNT);
    let cover_image_url: Option<Value> = if project.cover_image_asset_id.is_some() {
        asset_present.map(|asset_id| owned(format!("/api/assets/v2/static/{asset_id}/")))
    } else {
        Some(owned_optional(project.cover_image.clone()))
    };

    let mut object = Object::new()
        .set("id", uuid(project.id))
        .set("created_at", datetime(project.created_at))
        .set("updated_at", datetime(project.updated_at))
        .set("deleted_at", datetime_optional(project.deleted_at))
        .set("name", text(&project.name))
        .set("description", text(&project.description))
        .set(
            "description_text",
            json_optional(project.description_text.as_ref()),
        )
        .set(
            "description_html",
            json_optional(project.description_html.as_ref()),
        )
        .set("network", big_int(i64::from(project.network)))
        .set("identifier", text(&project.identifier))
        .set("emoji", owned_optional(project.emoji.clone()))
        .set("icon_prop", json_optional(project.icon_prop.as_ref()))
        .set("module_view", boolean(project.module_view))
        .set("cycle_view", boolean(project.cycle_view))
        .set("issue_views_view", boolean(project.issue_views_view))
        .set("page_view", boolean(project.page_view))
        .set("intake_view", boolean(project.intake_view))
        .set(
            "is_time_tracking_enabled",
            boolean(project.is_time_tracking_enabled),
        )
        .set(
            "is_issue_type_enabled",
            boolean(project.is_issue_type_enabled),
        )
        .set(
            "guest_view_all_features",
            boolean(project.guest_view_all_features),
        )
        .set("cover_image", owned_optional(project.cover_image.clone()))
        .set("archive_in", int(project.archive_in))
        .set("close_in", int(project.close_in))
        .set("logo_props", json(&project.logo_props))
        .set("archived_at", datetime_optional(project.archived_at))
        .set("timezone", text(&project.timezone))
        .set(
            "external_source",
            owned_optional(project.external_source.clone()),
        )
        .set("external_id", owned_optional(project.external_id.clone()))
        .set("created_by", uuid_optional(project.created_by_id))
        .set("updated_by", uuid_optional(project.updated_by_id))
        .set("workspace", uuid(project.workspace_id))
        .set(
            "default_assignee",
            uuid_optional(project.default_assignee_id),
        )
        .set("project_lead", uuid_optional(project.project_lead_id))
        .set(
            "cover_image_asset",
            uuid_optional(project.cover_image_asset_id),
        )
        .set("estimate", uuid_optional(project.estimate_id))
        .set("default_state", uuid_optional(project.default_state_id));
    if let Some(url) = cover_image_url {
        object = object.set("cover_image_url", url);
    }
    Ok(object.build())
}

// ---------------------------------------------------------------------------
// User
// ---------------------------------------------------------------------------

/// `UserLiteSerializer` (`plane/api/serializers/user.py:13-40`).
///
/// `avatar_url` is `User.avatar_url` (`db/models/user.py:143-151`): the avatar
/// asset's URL, else the stored avatar URL, else `None`. An asset id whose row
/// the default manager hides raises `RelatedObjectDoesNotExist` from the
/// property, so DRF skips the field.
async fn user(client: &Client, id: &str) -> Result<Value, EntityError> {
    let id = parse_id(id)?;
    let columns = with_alias(User::COLUMNS, "u");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns}, fa.id FROM users u \
                 LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id \
                     AND fa.deleted_at IS NULL \
                 WHERE u.id = $1"
            ),
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?
        .ok_or(EntityError::NotFound)?;
    let user = User::from_row(&row, 0).map_err(|error| EntityError::Invalid(error.to_string()))?;
    let asset_present: Option<Uuid> = row.get(User::COLUMN_COUNT);
    let avatar_url: Option<Value> = if user.avatar_asset_id.is_some() {
        asset_present.map(|asset_id| owned(format!("/api/assets/v2/static/{asset_id}/")))
    } else if user.avatar.is_empty() {
        Some(Value::Null)
    } else {
        Some(text(&user.avatar))
    };
    let mut object = Object::new()
        .set("id", uuid(user.id))
        .set("first_name", text(&user.first_name))
        .set("last_name", text(&user.last_name))
        .set("email", owned_optional(user.email.clone()))
        .set("avatar", text(&user.avatar))
        .set("display_name", text(&user.display_name));
    if let Some(url) = avatar_url {
        object = object.set("avatar_url", url);
    }
    Ok(object.build())
}

// ---------------------------------------------------------------------------
// Issue
// ---------------------------------------------------------------------------

/// `IssueExpandSerializer` with `context={"expand": ["labels", "assignees"]}`
/// (`plane/api/serializers/issue.py:844-885`).
async fn issue(client: &Client, id: &str, expand: bool) -> Result<Value, EntityError> {
    let id = parse_id(id)?;
    let columns = with_alias(Issue::COLUMNS, "i");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns}, st.id, st.name, st.color, st.\"group\" \
                 FROM issues i \
                 LEFT JOIN states st ON st.id = i.state_id AND st.deleted_at IS NULL \
                 WHERE i.id = $1 AND i.deleted_at IS NULL"
            ),
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?
        .ok_or(EntityError::NotFound)?;
    let issue =
        Issue::from_row(&row, 0).map_err(|error| EntityError::Invalid(error.to_string()))?;
    let start = Issue::COLUMN_COUNT;
    let state_id: Option<Uuid> = row.get(start);
    let state_name: Option<String> = row.get(start + 1);
    let state_color: Option<String> = row.get(start + 2);
    let state_group: Option<String> = row.get(start + 3);
    // `description_binary` is not a DRF-mapped field: it becomes a
    // `ReadOnlyField` over `bytes`, which the JSON encoder refuses, so the
    // serialisation (and therefore the webhook) fails.
    if issue.description_binary.is_some() {
        return Err(EntityError::Invalid(
            "Object of type bytes is not JSON serializable".to_string(),
        ));
    }

    let labels = if expand {
        issue_labels(client, issue.id).await?
    } else {
        issue_label_ids(client, issue.id).await?
    };
    let assignees = if expand {
        issue_assignees(client, issue.id).await?
    } else {
        issue_assignee_ids(client, issue.id).await?
    };
    let state = match (state_id, state_name, state_color, state_group) {
        (Some(state_id), Some(name), Some(color), Some(group)) => Some(
            Object::new()
                .set("id", uuid(state_id))
                .set("name", owned(name))
                .set("color", owned(color))
                .set("group", owned(group))
                .build(),
        ),
        _ => None,
    };

    let mut object = Object::new()
        .set("id", uuid(issue.id))
        .set("labels", Value::List(labels))
        .set("assignees", Value::List(assignees));
    if let Some(state) = state {
        object = object.set("state", state);
    }
    object = object
        .set("description", json(&issue.description_json))
        .set("created_at", datetime(issue.created_at))
        .set("updated_at", datetime(issue.updated_at))
        .set("deleted_at", datetime_optional(issue.deleted_at))
        .set(
            "point",
            issue
                .point
                .map_or(Value::Null, |point| big_int(i64::from(point))),
        )
        .set("name", text(&issue.name))
        .set("description_json", json(&issue.description_json))
        .set("description_html", text(&issue.description_html))
        .set(
            "description_stripped",
            owned_optional(issue.description_stripped.clone()),
        )
        // A `ReadOnlyField` over the nullable `BinaryField`: `None` renders as
        // `null`, and a non-null value is refused by the JSON encoder above.
        .set("description_binary", Value::Null)
        .set("priority", text(&issue.priority))
        .set("start_date", date_optional(issue.start_date))
        .set("target_date", date_optional(issue.target_date))
        .set("sequence_id", big_int(i64::from(issue.sequence_id)))
        .set("sort_order", float(issue.sort_order))
        .set("completed_at", datetime_optional(issue.completed_at))
        .set("archived_at", date_optional(issue.archived_at))
        .set("is_draft", boolean(issue.is_draft))
        .set(
            "external_source",
            owned_optional(issue.external_source.clone()),
        )
        .set("external_id", owned_optional(issue.external_id.clone()))
        .set("created_by", uuid_optional(issue.created_by_id))
        .set("updated_by", uuid_optional(issue.updated_by_id))
        .set("project", uuid(issue.project_id))
        .set("workspace", uuid(issue.workspace_id))
        .set("parent", uuid_optional(issue.parent_id))
        .set("estimate_point", uuid_optional(issue.estimate_point_id))
        .set("type", uuid_optional(issue.type_id));
    Ok(object.build())
}

/// [`issue_labels`] without the expansion: `[il.label_id for il in ...]`.
async fn issue_label_ids(client: &Client, issue_id: Uuid) -> Result<Vec<Value>, EntityError> {
    let rows = client
        .query(
            "SELECT label_id FROM issue_labels \
             WHERE issue_id = $1 AND deleted_at IS NULL \
             ORDER BY created_at DESC",
            &[&issue_id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    Ok(rows.iter().map(|row| uuid(row.get(0))).collect())
}

/// [`issue_assignees`] without the expansion: `[ia.assignee_id for ia in ...]`.
async fn issue_assignee_ids(client: &Client, issue_id: Uuid) -> Result<Vec<Value>, EntityError> {
    let rows = client
        .query(
            "SELECT assignee_id FROM issue_assignees \
             WHERE issue_id = $1 AND deleted_at IS NULL \
             ORDER BY created_at DESC",
            &[&issue_id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    Ok(rows.iter().map(|row| uuid(row.get(0))).collect())
}

/// `[il.label for il in issue.label_issue.all()]` through
/// `LabelLiteSerializer` (id, name, color).
async fn issue_labels(client: &Client, issue_id: Uuid) -> Result<Vec<Value>, EntityError> {
    let rows = client
        .query(
            "SELECT l.id, l.name, l.color FROM issue_labels il \
             JOIN labels l ON l.id = il.label_id \
             WHERE il.issue_id = $1 AND il.deleted_at IS NULL \
             ORDER BY il.created_at DESC",
            &[&issue_id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    Ok(rows
        .iter()
        .map(|row| {
            Object::new()
                .set("id", uuid(row.get(0)))
                .set("name", owned(row.get(1)))
                .set("color", owned(row.get(2)))
                .build()
        })
        .collect())
}

/// `[ia.assignee for ia in issue.issue_assignee.all()]` through
/// `UserLiteSerializer`.
async fn issue_assignees(client: &Client, issue_id: Uuid) -> Result<Vec<Value>, EntityError> {
    let rows = client
        .query(
            &format!(
                "SELECT {columns}, fa.id FROM issue_assignees ia \
                 JOIN users u ON u.id = ia.assignee_id \
                 LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id \
                     AND fa.deleted_at IS NULL \
                 WHERE ia.issue_id = $1 AND ia.deleted_at IS NULL \
                 ORDER BY ia.created_at DESC",
                columns = with_alias(User::COLUMNS, "u")
            ),
            &[&issue_id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    let mut assignees = Vec::with_capacity(rows.len());
    for row in &rows {
        let user =
            User::from_row(row, 0).map_err(|error| EntityError::Invalid(error.to_string()))?;
        let asset_present: Option<Uuid> = row.get(User::COLUMN_COUNT);
        let avatar_url = if user.avatar_asset_id.is_some() {
            asset_present.map(|asset_id| owned(format!("/api/assets/v2/static/{asset_id}/")))
        } else if user.avatar.is_empty() {
            Some(Value::Null)
        } else {
            Some(text(&user.avatar))
        };
        let mut object = Object::new()
            .set("id", uuid(user.id))
            .set("first_name", text(&user.first_name))
            .set("last_name", text(&user.last_name))
            .set("email", owned_optional(user.email.clone()))
            .set("avatar", text(&user.avatar))
            .set("display_name", text(&user.display_name));
        if let Some(url) = avatar_url {
            object = object.set("avatar_url", url);
        }
        assignees.push(object.build());
    }
    Ok(assignees)
}

// ---------------------------------------------------------------------------
// Cycle and Module
// ---------------------------------------------------------------------------

/// `CycleSerializer` (`plane/api/serializers/cycle.py:136-200`). Its nine
/// annotation fields are absent.
async fn cycle(client: &Client, id: &str) -> Result<Value, EntityError> {
    let id = parse_id(id)?;
    let columns = with_alias(plane_db::models::Cycle::COLUMNS, "c");
    let row = client
        .query_opt(
            &format!("SELECT {columns} FROM cycles c WHERE c.id = $1 AND c.deleted_at IS NULL"),
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?
        .ok_or(EntityError::NotFound)?;
    let cycle = plane_db::models::Cycle::from_row(&row, 0)
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    Ok(Object::new()
        .set("id", uuid(cycle.id))
        .set("created_at", datetime(cycle.created_at))
        .set("updated_at", datetime(cycle.updated_at))
        .set("deleted_at", datetime_optional(cycle.deleted_at))
        .set("name", text(&cycle.name))
        .set("description", text(&cycle.description))
        .set("start_date", datetime_optional(cycle.start_date))
        .set("end_date", datetime_optional(cycle.end_date))
        .set("view_props", json(&cycle.view_props))
        .set("sort_order", float(cycle.sort_order))
        .set(
            "external_source",
            owned_optional(cycle.external_source.clone()),
        )
        .set("external_id", owned_optional(cycle.external_id.clone()))
        .set("progress_snapshot", json(&cycle.progress_snapshot))
        .set("archived_at", datetime_optional(cycle.archived_at))
        .set("logo_props", json(&cycle.logo_props))
        .set("timezone", text(&cycle.timezone))
        .set("version", big_int(i64::from(cycle.version)))
        .set("created_by", uuid_optional(cycle.created_by_id))
        .set("updated_by", uuid_optional(cycle.updated_by_id))
        .set("project", uuid(cycle.project_id))
        .set("workspace", uuid(cycle.workspace_id))
        .set("owned_by", uuid(cycle.owned_by_id))
        .build())
}

/// `ModuleSerializer` (`plane/api/serializers/module.py:169-206`): the model's
/// fields plus `members` (the m2m ids) appended by `to_representation`.
async fn module(client: &Client, id: &str) -> Result<Value, EntityError> {
    let id = parse_id(id)?;
    let columns = with_alias(plane_db::models::Module::COLUMNS, "m");
    let row = client
        .query_opt(
            &format!("SELECT {columns} FROM modules m WHERE m.id = $1 AND m.deleted_at IS NULL"),
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?
        .ok_or(EntityError::NotFound)?;
    let module = plane_db::models::Module::from_row(&row, 0)
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    let members = client
        .query(
            "SELECT member_id FROM module_members WHERE module_id = $1",
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    let members: Vec<Value> = members.iter().map(|row| uuid(row.get(0))).collect();
    Ok(Object::new()
        .set("id", uuid(module.id))
        .set("created_at", datetime(module.created_at))
        .set("updated_at", datetime(module.updated_at))
        .set("deleted_at", datetime_optional(module.deleted_at))
        .set("name", text(&module.name))
        .set("description", text(&module.description))
        .set(
            "description_text",
            json_optional(module.description_text.as_ref()),
        )
        .set(
            "description_html",
            json_optional(module.description_html.as_ref()),
        )
        .set("start_date", date_optional(module.start_date))
        .set("target_date", date_optional(module.target_date))
        .set("status", text(&module.status))
        .set("view_props", json(&module.view_props))
        .set("sort_order", float(module.sort_order))
        .set(
            "external_source",
            owned_optional(module.external_source.clone()),
        )
        .set("external_id", owned_optional(module.external_id.clone()))
        .set("archived_at", datetime_optional(module.archived_at))
        .set("logo_props", json(&module.logo_props))
        .set("created_by", uuid_optional(module.created_by_id))
        .set("updated_by", uuid_optional(module.updated_by_id))
        .set("project", uuid(module.project_id))
        .set("workspace", uuid(module.workspace_id))
        .set("lead", uuid_optional(module.lead_id))
        .set("members", Value::List(members))
        .build())
}

/// `CycleIssueSerializer`; `sub_issues_count` is an annotation and absent.
async fn cycle_issue(client: &Client, id: &str) -> Result<Value, EntityError> {
    let id = parse_id(id)?;
    let columns = with_alias(plane_db::models::CycleIssue::COLUMNS, "ci");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns} FROM cycle_issues ci \
                 WHERE ci.id = $1 AND ci.deleted_at IS NULL"
            ),
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?
        .ok_or(EntityError::NotFound)?;
    let link = plane_db::models::CycleIssue::from_row(&row, 0)
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    Ok(Object::new()
        .set("id", uuid(link.id))
        .set("created_at", datetime(link.created_at))
        .set("updated_at", datetime(link.updated_at))
        .set("deleted_at", datetime_optional(link.deleted_at))
        .set("created_by", uuid_optional(link.created_by_id))
        .set("updated_by", uuid_optional(link.updated_by_id))
        .set("project", uuid(link.project_id))
        .set("workspace", uuid(link.workspace_id))
        .set("issue", uuid(link.issue_id))
        .set("cycle", uuid(link.cycle_id))
        .build())
}

/// `ModuleIssueSerializer`; `sub_issues_count` is absent.
async fn module_issue(client: &Client, id: &str) -> Result<Value, EntityError> {
    let id = parse_id(id)?;
    let columns = with_alias(plane_db::models::ModuleIssue::COLUMNS, "mi");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns} FROM module_issues mi \
                 WHERE mi.id = $1 AND mi.deleted_at IS NULL"
            ),
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?
        .ok_or(EntityError::NotFound)?;
    let link = plane_db::models::ModuleIssue::from_row(&row, 0)
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    Ok(Object::new()
        .set("id", uuid(link.id))
        .set("created_at", datetime(link.created_at))
        .set("updated_at", datetime(link.updated_at))
        .set("deleted_at", datetime_optional(link.deleted_at))
        .set("created_by", uuid_optional(link.created_by_id))
        .set("updated_by", uuid_optional(link.updated_by_id))
        .set("project", uuid(link.project_id))
        .set("workspace", uuid(link.workspace_id))
        .set("module", uuid(link.module_id))
        .set("issue", uuid(link.issue_id))
        .build())
}

/// `IssueCommentSerializer` (`plane/api/serializers/issue.py:756-778`); its
/// `is_member` annotation and the two excluded fields are absent.
async fn issue_comment(client: &Client, id: &str) -> Result<Value, EntityError> {
    let id = parse_id(id)?;
    let columns = with_alias(IssueComment::COLUMNS, "ic");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns} FROM issue_comments ic \
                 WHERE ic.id = $1 AND ic.deleted_at IS NULL"
            ),
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?
        .ok_or(EntityError::NotFound)?;
    let comment =
        IssueComment::from_row(&row, 0).map_err(|error| EntityError::Invalid(error.to_string()))?;
    Ok(Object::new()
        .set("id", uuid(comment.id))
        .set("created_at", datetime(comment.created_at))
        .set("updated_at", datetime(comment.updated_at))
        .set("deleted_at", datetime_optional(comment.deleted_at))
        .set("comment_html", text(&comment.comment_html))
        .set(
            "attachments",
            Value::List(comment.attachments.iter().map(|item| text(item)).collect()),
        )
        .set("access", text(&comment.access))
        .set(
            "external_source",
            owned_optional(comment.external_source.clone()),
        )
        .set("external_id", owned_optional(comment.external_id.clone()))
        .set("edited_at", datetime_optional(comment.edited_at))
        .set("created_by", uuid_optional(comment.created_by_id))
        .set("updated_by", uuid_optional(comment.updated_by_id))
        .set("project", uuid(comment.project_id))
        .set("workspace", uuid(comment.workspace_id))
        .set("description", uuid_optional(comment.description_id))
        .set("issue", uuid(comment.issue_id))
        .set("actor", uuid_optional(comment.actor_id))
        .set("parent", uuid_optional(comment.parent_id))
        .build())
}

/// `IntakeIssueSerializer` (`plane/api/serializers/intake.py:58-81`) with its
/// `issue_detail` expansion (`IssueExpandSerializer`) and `inbox`
/// (`source="intake.id"`).
async fn intake_issue(client: &Client, id: &str) -> Result<Value, EntityError> {
    let id = parse_id(id)?;
    let columns = with_alias(plane_db::models::IntakeIssue::COLUMNS, "ii");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns}, i.id FROM intake_issues ii \
                 JOIN intakes i ON i.id = ii.intake_id AND i.deleted_at IS NULL \
                 WHERE ii.id = $1 AND ii.deleted_at IS NULL"
            ),
            &[&id],
        )
        .await
        .map_err(|error| EntityError::Invalid(error.to_string()))?
        .ok_or(EntityError::NotFound)?;
    let intake = plane_db::models::IntakeIssue::from_row(&row, 0)
        .map_err(|error| EntityError::Invalid(error.to_string()))?;
    let inbox: Uuid = row.get(plane_db::models::IntakeIssue::COLUMN_COUNT);
    // `IntakeIssueSerializer.issue_detail` is built without a context, so the
    // nested serializer's `get_labels`/`get_assignees` see no `expand` and
    // return the raw id lists (`plane/api/serializers/intake.py:66`).
    let issue_detail = issue(client, &intake.issue_id.to_string(), false).await?;
    Ok(Object::new()
        .set("id", uuid(intake.id))
        .set("issue_detail", issue_detail)
        .set("inbox", uuid(inbox))
        .set("created_at", datetime(intake.created_at))
        .set("updated_at", datetime(intake.updated_at))
        .set("deleted_at", datetime_optional(intake.deleted_at))
        .set("status", big_int(i64::from(intake.status)))
        .set("snoozed_till", datetime_optional(intake.snoozed_till))
        .set("source", owned_optional(intake.source.clone()))
        .set("source_email", owned_optional(intake.source_email.clone()))
        .set(
            "external_source",
            owned_optional(intake.external_source.clone()),
        )
        .set("external_id", owned_optional(intake.external_id.clone()))
        .set("extra", json(&intake.extra))
        .set("created_by", uuid_optional(intake.created_by_id))
        .set("updated_by", uuid_optional(intake.updated_by_id))
        .set("project", uuid(intake.project_id))
        .set("workspace", uuid(intake.workspace_id))
        .set("intake", uuid(intake.intake_id))
        .set("issue", uuid(intake.issue_id))
        .set("duplicate_to", uuid_optional(intake.duplicate_to_id))
        .build())
}
