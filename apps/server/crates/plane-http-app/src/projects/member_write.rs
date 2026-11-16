// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ProjectMemberViewSet`'s write actions (`plane/app/views/project/member.py`)
//! and the project-members half of the invitation routes.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/projects/{project_id}/members/` | `create` | `member.py:71-162` |
//! | `PATCH …/projects/{project_id}/members/{pk}/` | `partial_update` | `member.py:221-297` |
//! | `DELETE …/projects/{project_id}/members/{pk}/` | `destroy` | `member.py:299-345` |
//! | `POST …/projects/{project_id}/members/leave/` | `leave` | `member.py:347-375` |
//!
//! ## What Django does per action
//!
//! **create** is a bulk path. It reads `members` (a list of
//! `{"member_id": …, "role": …}`), checks each requested member's active
//! workspace role, reactivates every membership of those members in the project
//! (`bulk_update` of `is_active`/`role`), computes each member's workspace
//! `MIN(sort_order)` **once**, appends a new `ProjectMember` **and** a
//! `ProjectUserProperty` for every entry in the request list,
//! `bulk_create(..., ignore_conflicts=True)` both, then re-reads the membership
//! rows and enqueues one `project_add_user_email` per row. H32 does not run
//! (`bulk_create` bypasses `save()`); the view's explicit property rows stand in
//! for it, with the audit columns of the request user only where the view sets
//! them — these rows carry `created_by_id` NULL.
//!
//! **partial_update** has three guard layers before the serializer: personal
//! fields (only the row's owner), the role block, and the `is_active` block. It
//! reads the raw body for `role` and casts it with Python's `int()` **before**
//! validation, so a non-numeric `role` is a 500, not a 400. Then
//! `ProjectMemberSerializer` validates the writable fields and `save()` runs the
//! full-row UPDATE plus H01.
//!
//! **destroy** and **leave** deactivate the row (`is_active = False; save()`),
//! not a soft delete: `deleted_at` stays NULL, `updated_at`/`updated_by_id`
//! move. Leave refuses the last remaining project admin.
//!
//! ## Statements
//!
//! | Action | Django |
//! | --- | --- |
//! | create | project get, one `WorkspaceMember.get` per distinct member, membership select, `bulk_update`, `MIN(sort_order)`, two `bulk_create`s, response select, one task per row |
//! | partial_update | target get, target workspace role, requester workspace role, requester membership, UPDATE |
//! | destroy | target get, requester membership, UPDATE |
//! | leave | own membership get, admin count, UPDATE |

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks::audit::Audit;
use plane_db::{JsonText, hooks};
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_project::{
    ProjectMemberRoleSerializer, ProjectMemberSerializer,
};
use plane_serializers::generated::app_user::UserLiteSerializer;
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use plane_serializers::{
    DrfError, DrfSerialize, DrfWriter, PgJson, RenderCtx, drf_datetime, drf_datetime_opt,
};
use uuid::Uuid;

use crate::common::{client, lookup, render_list};
use crate::projects::lite;
use crate::projects::member_queries as queries;
use crate::write::{self, CharOptions, Serializer};

/// `ROLE.ADMIN` / `ROLE.MEMBER` / `ROLE.GUEST` (`plane/db/models/project.py:25`).
pub(super) const ROLE_ADMIN: i16 = 20;
pub(super) const ROLE_MEMBER: i16 = 15;
pub(super) const ROLE_GUEST: i16 = 5;

/// The three `ROLE_CHOICES` values, in the serializer's choice order.
const ROLE_CHOICES: &[i16] = &[20, 15, 5];

/// The writable fields of `ProjectMemberSerializer`
/// (`plane/app/serializers/project.py:169-177`, `fields="__all__"` minus the
/// declared nested read-only relations, the auto fields and the explicit
/// `read_only_fields`). DRF visits them in this order, so the error keys are in
/// this order.
fn apply_body(serializer: &mut Serializer<'_>) {
    serializer.char_field("comment", CharOptions::nullable());
    serializer.integer_choice_field("role", ROLE_CHOICES);
    serializer.json_field("view_props", false);
    serializer.json_field("default_props", false);
    serializer.json_field("preferences", false);
    serializer.float_field("sort_order", false);
    serializer.bool_field("is_active");
}

/// `ProjectMember.objects.create(project_id=…, member=…, role=…)` (route 143):
/// H32 first, then the row itself.
///
/// `created_by` is crum's user, so the property row and the membership both
/// carry the request user.
///
/// # Errors
///
/// A database failure — H32's unique index on a re-added member is an
/// `IntegrityError` in Django.
pub(super) async fn create_member(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    workspace_id: Uuid,
    member_id: Uuid,
    role: i16,
    created_by: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    // H32 (`plane/db/models/project.py:276-292`).
    let property =
        hooks::project_member::before_insert(client, workspace_id, project_id, member_id)
            .await
            .map_err(ViewError::from)?;
    insert_user_property(
        client,
        property.project_id,
        property.workspace_id,
        property.user_id,
        property.sort_order,
        created_by,
        now,
    )
    .await?;
    insert_project_member(
        client,
        project_id,
        workspace_id,
        Some(member_id),
        Some(role),
        created_by,
        now,
    )
    .await
}

/// `ProjectMember.objects.bulk_create([...], ignore_conflicts=True)`'s row.
///
/// # Errors
///
/// A database failure (a `NULL` role or a duplicate is the caller's to map).
pub(super) async fn insert_project_member(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    workspace_id: Uuid,
    member_id: Option<Uuid>,
    role: Option<i16>,
    created_by: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    let statement = queries::PROJECT_MEMBER_INSERT.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &now,
                &now,
                &Uuid::new_v4(),
                &role,
                &created_by,
                &member_id,
                &project_id,
                &None::<Uuid>,
                &workspace_id,
                &default_props(),
                &default_props(),
                &default_preferences(),
            ],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// `ProjectUserProperty.objects.bulk_create([...], ignore_conflicts=True)`'s
/// row, and the explicit property `ProjectMember.save()` writes through H32.
///
/// # Errors
///
/// A database failure.
pub(super) async fn insert_user_property(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    sort_order: f64,
    created_by: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    let statement = queries::PROJECT_USER_PROPERTY_INSERT
        .prepare(client)
        .await?;
    client
        .execute(
            &statement,
            &[
                &now,
                &now,
                &Uuid::new_v4(),
                &JsonText::new(DEFAULT_DISPLAY_PROPERTIES),
                &created_by,
                &project_id,
                &None::<Uuid>,
                &user_id,
                &workspace_id,
                &JsonText::new(DEFAULT_DISPLAY_FILTERS),
                &JsonText::new(DEFAULT_FILTERS),
                &JsonText::new("{}"),
                &default_preferences(),
                &sort_order,
            ],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// `plane/db/models/project.py:39` (`get_default_props`).
#[must_use]
pub(super) fn default_props() -> JsonText {
    JsonText::new(
        r#"{"filters": {"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}, "display_filters": {"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}}"#,
    )
}

/// `plane/db/models/project.py:64` (`get_default_preferences`).
#[must_use]
pub(super) fn default_preferences() -> JsonText {
    JsonText::new(
        r#"{"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}}"#,
    )
}

/// `plane/db/models/issue.py:47` (`get_default_filters`).
const DEFAULT_FILTERS: &str = r#"{"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}"#;
/// `plane/db/models/issue.py:61` (`get_default_display_filters`).
const DEFAULT_DISPLAY_FILTERS: &str = r#"{"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}"#;
/// `plane/db/models/issue.py:73` (`get_default_display_properties`).
const DEFAULT_DISPLAY_PROPERTIES: &str = r#"{"assignee": true, "attachment_count": true, "created_on": true, "due_date": true, "estimate": true, "key": true, "labels": true, "link": true, "priority": true, "start_date": true, "state": true, "sub_issue_count": true, "updated_on": true}"#;

/// `Project.objects.get(pk=project_id, workspace__slug=slug)`.
///
/// # Errors
///
/// 404 when the project is missing or soft-deleted.
pub(super) async fn lookup_project(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
) -> Result<(Uuid, Uuid), ViewError> {
    let statement = queries::PROJECT_LOOKUP.prepare(client).await?;
    match client.query_opt(&statement, &[&slug, &project_id]).await? {
        Some(row) => Ok((row.get(0), row.get(1))),
        None => Err(ViewError::ObjectDoesNotExist),
    }
}

/// `WorkspaceMember.objects.get(workspace__slug=slug, member=…,
/// is_active=True).role`.
///
/// # Errors
///
/// 404 when the workspace membership is missing or inactive.
pub(super) async fn workspace_role(
    client: &deadpool_postgres::Client,
    slug: &str,
    member_id: Uuid,
) -> Result<i16, ViewError> {
    let statement = queries::WORKSPACE_MEMBER_ROLE.prepare(client).await?;
    match client.query_opt(&statement, &[&slug, &member_id]).await? {
        Some(row) => Ok(row.get(1)),
        None => Err(ViewError::ObjectDoesNotExist),
    }
}

/// Django's `IntegrityError` family (`django.db.utils.IntegrityError`, SQLSTATE
/// class 23) is the family handler's 400 `{"error": "The payload is not
/// valid"}` (`plane/app/views/base.py:200-206`); every other database error is
/// the 500.
pub(super) fn db_error(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// `write::error_body` as the `Result` the handlers return.
pub(super) fn fail(status: u16, message: &str) -> Result<ViewResponse, ViewError> {
    Ok(write::error_body(status, message))
}

/// A body value as DRF's `request.data` sees it.
pub(super) enum RawValue<'a> {
    Json(&'a Json),
    Text(&'a str),
}

/// `request.data.get(key)` without DRF's `.get` reducing a JSON value to a
/// string; `None` when the key is absent.
///
/// A JSON body that is not an object has no `.get`: Python raises
/// `AttributeError` and the family handler answers 500.
pub(super) fn body_value<'a>(data: &'a Data, key: &str) -> Result<Option<RawValue<'a>>, ViewError> {
    match data {
        Data::Json(Json::Dict(map)) => Ok(map.get(key).map(RawValue::Json)),
        Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: a non-object request body has no .get".to_string(),
        )),
        Data::Form(query) => Ok(query.get(key).map(RawValue::Text)),
    }
}

/// `"<key>" in request.data`.
pub(super) fn body_contains(data: &Data, key: &str) -> bool {
    match data {
        Data::Json(Json::Dict(map)) => map.contains_key(key),
        Data::Form(query) => query.get(key).is_some(),
        Data::Json(_) => false,
    }
}

/// Python truthiness of a JSON value (`bool(value)`).
pub(super) fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Int(value) => value.as_i64().is_some_and(|value| value != 0),
        Json::Float(value) => *value != 0.0,
        Json::Str(value) => !value.as_str().is_some_and(str::is_empty),
        Json::List(items) => !items.is_empty(),
        Json::Dict(map) => !map.is_empty(),
    }
}

/// `int(request.data["role"])` in `partial_update` (`member.py:253`).
///
/// A value Python's `int()` refuses is a 500; a value it truncates (`20.9` →
/// `20`) is truncated the same way here.
///
/// # Errors
///
/// 500 for a null, list, dict or non-numeric string.
pub(super) fn python_int(value: &Json) -> Result<i64, ViewError> {
    match value {
        Json::Int(value) => value
            .as_i64()
            .ok_or_else(|| ViewError::Unknown("int() overflow".to_string())),
        Json::Bool(value) => Ok(i64::from(*value)),
        Json::Float(value) => Ok(*value as i64),
        Json::Str(value) => value
            .as_str()
            .and_then(|text| text.trim().parse::<i64>().ok())
            .ok_or_else(|| {
                ViewError::Unknown(format!("ValueError: invalid literal for int(): {value:?}"))
            }),
        Json::Null | Json::List(_) | Json::Dict(_) => Err(ViewError::Unknown(
            "TypeError: int() argument must be a string or a number".to_string(),
        )),
    }
}

/// `json_equals_int` for the two role-list membership tests
/// (`25.0 in [5, 15]` is true in Python).
fn role_is(role: Option<&Json>, value: i64) -> bool {
    match role {
        Some(Json::Int(number)) => number.as_i64() == Some(value),
        Some(Json::Float(number)) => *number == value as f64,
        _ => false,
    }
}

/// `ProjectMember.role` as the smallint the INSERT/UPDATE binds.
///
/// A `NULL` is left to the database (Django's `IntegrityError` → the family
/// 400); a value Postgres cannot cast is a 500.
///
/// # Errors
///
/// 500 when the value cannot be a `smallint`.
pub(super) fn role_value(value: Option<&Json>) -> Result<Option<i16>, ViewError> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Int(number)) => number
            .as_i64()
            .and_then(|value| i16::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| ViewError::Unknown("smallint out of range".to_string())),
        Some(Json::Str(text)) => text
            .as_str()
            .and_then(|text| text.trim().parse::<i16>().ok())
            .map(Some)
            .ok_or_else(|| {
                ViewError::Unknown("invalid input syntax for type smallint".to_string())
            }),
        Some(Json::Float(_)) | Some(Json::Bool(_)) | Some(Json::List(_)) | Some(Json::Dict(_)) => {
            Err(ViewError::Unknown(
                "column \"role\" is of type smallint but the expression has another type"
                    .to_string(),
            ))
        }
    }
}

/// `ProjectMemberViewSet.create` (`plane/app/views/project/member.py:71-162`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let data = ctx.data()?;
        let members: Vec<&Json> = match body_value(data, "members")? {
            None => Vec::new(),
            Some(RawValue::Json(value)) if !truthy(value) => Vec::new(),
            Some(RawValue::Json(Json::List(items))) => items.iter().collect(),
            // `len()`/iteration of a non-list is either a `TypeError` (numbers)
            // or an `AttributeError` (`str`/`dict` entries have no `.get`).
            Some(RawValue::Json(_)) => {
                return Err(ViewError::Unknown(
                    "TypeError/AttributeError: members is not a list of objects".to_string(),
                ));
            }
            Some(RawValue::Text("")) => Vec::new(),
            Some(RawValue::Text(_)) => {
                return Err(ViewError::Unknown(
                    "AttributeError: 'str' object has no attribute 'get'".to_string(),
                ));
            }
        };

        // `Project.objects.get(...)` runs before the empty check
        // (`member.py:89-96`).
        let (_, workspace_id) = lookup_project(client, slug, project_id).await?;
        if members.is_empty() {
            return fail(400, "At least one member is required");
        }

        // `{member.get("member_id"): member.get("role") for member in members}`
        // — first-occurrence order, last role wins. A non-hashable key is a
        // `TypeError` (500).
        let mut member_roles: Vec<(Key, Option<&Json>)> = Vec::with_capacity(members.len());
        for entry in &members {
            let Some(entry) = entry.as_dict() else {
                return Err(ViewError::Unknown(
                    "AttributeError: a member entry is not an object".to_string(),
                ));
            };
            let key = match entry.get("member_id") {
                None => Key::Null,
                Some(value) => Key::from_json(value)?,
            };
            let role = entry.get("role");
            match member_roles
                .iter_mut()
                .find(|(existing, _)| *existing == key)
            {
                Some((_, existing_role)) => *existing_role = role,
                None => member_roles.push((key, role)),
            }
        }

        // The workspace-role guard, in the dict's order (`member.py:98-115`).
        for (key, role) in &member_roles {
            let member_id = key.uuid().map_err(|error| match error {
                KeyProblem::NotFound => ViewError::ObjectDoesNotExist,
                KeyProblem::Validation => ViewError::Validation,
                KeyProblem::Unknown(message) => ViewError::Unknown(message),
            })?;
            let workspace_role = workspace_role(client, slug, member_id).await?;
            if workspace_role == ROLE_ADMIN && (role_is(*role, 5) || role_is(*role, 15)) {
                return fail(
                    400,
                    "You cannot add a user with role lower than the workspace role",
                );
            }
            if workspace_role == ROLE_GUEST && (role_is(*role, 15) || role_is(*role, 20)) {
                return fail(
                    400,
                    "You cannot add a user with role higher than the workspace role",
                );
            }
        }

        let member_ids: Vec<Uuid> = member_roles
            .iter()
            .filter_map(|(key, _)| key.valid_uuid())
            .collect();

        // Reactivate every existing membership, then `bulk_update` the role.
        let statement = queries::PROJECT_MEMBERS_FOR_UPDATE.prepare(client).await?;
        let existing = client
            .query(&statement, &[&project_id, &member_ids])
            .await?;
        if !existing.is_empty() {
            let update = queries::PROJECT_MEMBER_SET_ROLE_ACTIVE
                .prepare(client)
                .await?;
            for row in &existing {
                let row_id: Uuid = row.get(0);
                let row_member: Uuid = row.get(1);
                // `member_roles[str(project_member.member_id)]`: a canonical
                // UUID string only; any other spelling is a `KeyError` (500).
                let key = Key::Str(row_member.to_string());
                let role = member_roles
                    .iter()
                    .find(|(existing, _)| *existing == key)
                    .map(|(_, role)| *role)
                    .ok_or_else(|| {
                        ViewError::Unknown(
                            "KeyError: membership is not in member_roles".to_string(),
                        )
                    })?;
                let role = role_value(role)?;
                client
                    .execute(&update, &[&role, &row_id])
                    .await
                    .map_err(db_error)?;
            }
        }

        // `MIN(sort_order)` per member, from the workspace
        // (`member.py:134-142`).
        let statement = queries::PROJECT_USER_PROPERTY_MIN.prepare(client).await?;
        let rows = client.query(&statement, &[&slug, &member_ids]).await?;
        let mut sort_orders = std::collections::HashMap::with_capacity(rows.len());
        for row in &rows {
            let user_id: Uuid = row.get(0);
            let min: Option<f64> = row.get(1);
            sort_orders.insert(user_id, min);
        }

        // Every entry in the request list becomes one membership and one
        // property, in request order (`member.py:144-160`).
        let mut new_members = Vec::with_capacity(members.len());
        for entry in &members {
            let entry = entry.as_dict().ok_or_else(|| {
                ViewError::Unknown("AttributeError: a member entry is not an object".to_string())
            })?;
            let key = match entry.get("member_id") {
                None => Key::Null,
                Some(value) => Key::from_json(value)?,
            };
            let role = match entry.get("role") {
                None => Some(5),
                Some(value) => role_value(Some(value))?,
            };
            new_members.push((key.valid_uuid(), role));
        }

        for (member_id, role) in &new_members {
            insert_project_member(
                client,
                project_id,
                workspace_id,
                *member_id,
                *role,
                None,
                write::now(),
            )
            .await?;
        }
        for (member_id, _) in &new_members {
            let Some(member_id) = member_id else {
                // The workspace-role guard already answered 404 for a null
                // key; a property row cannot exist without a user.
                continue;
            };
            let sort_order = sort_orders
                .get(member_id)
                .copied()
                .flatten()
                .map_or(65535.0, |min| min - 10000.0);
            insert_user_property(
                client,
                project_id,
                workspace_id,
                *member_id,
                sort_order,
                None,
                write::now(),
            )
            .await?;
        }

        // The response and the notification list (`member.py:149-159`).
        let statement = queries::PROJECT_MEMBERS_FOR_RESPONSE
            .prepare(client)
            .await?;
        let rows = client
            .query(&statement, &[&project_id, &member_ids])
            .await?;
        let current_site = plane_auth::base_host(
            ctx.settings,
            plane_auth::LoginFlags {
                is_app: true,
                ..Default::default()
            },
        )
        .map_err(|error| ViewError::Unknown(error.to_string()))?;

        let mut serializers = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get(0);
            let member_id: Option<Uuid> = row.get(1);
            let role: i16 = row.get(2);
            let row_project: Uuid = row.get(3);
            let created_at: DateTime<Utc> = row.get(4);
            serializers.push(ProjectMemberRoleSerializer {
                id,
                role: i64::from(role),
                member: member_id,
                project: row_project,
                original_role: i64::from(role),
                created_at,
            });
            let task = plane_jobs::celery::tasks::project_add_user_email(
                current_site.clone(),
                id,
                user_id,
            );
            crate::common::jobs::dispatch_task(
                ctx,
                task.task(),
                &task,
                plane_jobs::queue::Job::new(task.task()),
            )
            .await?;
        }

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = render_list(&serializers, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// A Python dict key: the raw `member_id` the request used.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    Str(String),
}

impl Key {
    fn from_json(value: &Json) -> Result<Self, ViewError> {
        match value {
            Json::Null => Ok(Self::Null),
            Json::Bool(value) => Ok(Self::Bool(*value)),
            Json::Int(value) => value
                .as_i64()
                .map(Self::Int)
                .ok_or_else(|| ViewError::Unknown("unhashable int key".to_string())),
            Json::Float(value) => Ok(Self::Float(value.to_bits())),
            Json::Str(value) => Ok(Self::Str(
                value
                    .as_str()
                    .ok_or_else(|| ViewError::Unknown("a lone-surrogate key".to_string()))?
                    .to_owned(),
            )),
            Json::List(_) | Json::Dict(_) => Err(ViewError::Unknown(
                "TypeError: unhashable type: 'list'".to_string(),
            )),
        }
    }

    /// The `WorkspaceMember.objects.get(member=…)` value.
    ///
    /// `None`/non-strings never match a row (`member` is NOT NULL), a string
    /// that is not a UUID is Django's `ValidationError` (400).
    fn uuid(&self) -> Result<Uuid, KeyProblem> {
        match self {
            Self::Null => Err(KeyProblem::NotFound),
            Self::Str(text) => Uuid::parse_str(text).map_err(|_| KeyProblem::Validation),
            Self::Bool(_) | Self::Int(_) | Self::Float(_) => Err(KeyProblem::Unknown(
                "AttributeError: a UUID field cannot be built from a number".to_string(),
            )),
        }
    }

    fn valid_uuid(&self) -> Option<Uuid> {
        match self {
            Self::Str(text) => Uuid::parse_str(text).ok(),
            _ => None,
        }
    }
}

enum KeyProblem {
    NotFound,
    Validation,
    Unknown(String),
}

/// `ProjectMemberViewSet.partial_update` (`member.py:221-297`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        let target = get_target(client, pk, project_id, slug).await?;

        // Personal fields are the owner's (`member.py:228-233`).
        if target.member_id != Some(user_id)
            && (body_contains(data, "preferences")
                || body_contains(data, "view_props")
                || body_contains(data, "default_props"))
        {
            return fail(403, "You cannot update another member's personal settings");
        }

        // The two workspace roles (`member.py:236-246`).
        let Some(target_member_id) = target.member_id else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let target_workspace_role = workspace_role(client, slug, target_member_id).await?;
        let requester_workspace_role = workspace_role(client, slug, user_id).await?;
        let is_workspace_admin = requester_workspace_role == ROLE_ADMIN;

        if user_id == target_member_id && !is_workspace_admin {
            return fail(400, "You cannot update your own role");
        }

        let statement = queries::PROJECT_MEMBER_SELF.prepare(client).await?;
        let requester = client
            .query_opt(&statement, &[&project_id, &user_id, &slug])
            .await?;
        let Some(requester) = requester else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let requester_role: i16 = requester.get(1);

        if body_contains(data, "role") {
            if requester_role < ROLE_ADMIN && !is_workspace_admin {
                return fail(403, "You do not have permission to update roles");
            }
            if target.role >= requester_role && !is_workspace_admin {
                return fail(
                    403,
                    "You cannot update the role of a member with a role equal to or higher than your own",
                );
            }
            let raw = body_value(data, "role")?
                .ok_or_else(|| ViewError::Unknown("KeyError: role".to_string()))?;
            let new_role = match raw {
                RawValue::Json(value) => python_int(value)?,
                RawValue::Text(text) => text
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| ViewError::Unknown("ValueError: int()".to_string()))?,
            };
            if new_role >= i64::from(requester_role) && !is_workspace_admin {
                return fail(
                    403,
                    "You cannot assign a role equal to or higher than your own",
                );
            }
            if target_workspace_role == ROLE_GUEST && (new_role == 15 || new_role == 20) {
                return fail(
                    400,
                    "You cannot add a user with role higher than the workspace role",
                );
            }
        }

        if body_contains(data, "is_active") {
            if requester_role < ROLE_ADMIN && !is_workspace_admin {
                return fail(403, "You do not have permission to update member status");
            }
            if target.role >= requester_role && !is_workspace_admin {
                return fail(
                    403,
                    "You cannot update the status of a member with a role equal to or higher than your own",
                );
            }
            if matches!(
                body_value(data, "is_active")?,
                Some(RawValue::Json(Json::List(_) | Json::Dict(_)))
            ) {
                return Err(ViewError::Unknown(
                    "TypeError: unhashable type in BooleanField".to_string(),
                ));
            }
        }

        let mut serializer = Serializer::new(ctx, true)?;
        apply_body(&mut serializer);
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        let mut member = target;
        if values.contains("comment") {
            member.comment = values.text("comment").map(str::to_owned);
        }
        if let Some(role) = values.integer("role") {
            member.role = i16::try_from(role)
                .map_err(|_| ViewError::Unknown("smallint out of range".to_string()))?;
        }
        if let Some(view_props) = values.json("view_props") {
            member.view_props = view_props.clone();
        }
        if let Some(default_props) = values.json("default_props") {
            member.default_props = default_props.clone();
        }
        if let Some(preferences) = values.json("preferences") {
            member.preferences = preferences.clone();
        }
        if let Some(sort_order) = values.float("sort_order") {
            member.sort_order = sort_order;
        }
        if let Some(is_active) = values.bool("is_active") {
            member.is_active = is_active;
        }

        member.updated_at = write::now();
        member.updated_by_id = Audit::on_update(Some(user_id));

        let statement = queries::PROJECT_MEMBER_SAVE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &member.created_at,
                    &member.updated_at,
                    &member.id,
                    &member.comment,
                    &member.role,
                    &member.created_by_id,
                    &member.member_id,
                    &member.project_id,
                    &member.updated_by_id,
                    &member.workspace_id,
                    &member.view_props,
                    &member.default_props,
                    &member.sort_order,
                    &member.preferences,
                    &member.is_active,
                    &member.deleted_at,
                    &member.id,
                ],
            )
            .await
            .map_err(db_error)?;

        let mut joined = get_joined(client, member.id, project_id, slug).await?;
        // `serializer.data` reads the in-memory instance that `update()` just
        // mutated, so the JSON columns keep the request body's key order; a
        // re-read would use jsonb's canonical order.
        joined.view_props = member.view_props;
        joined.default_props = member.default_props;
        joined.preferences = member.preferences;
        render_member(ctx, client, joined, target_member_id == user_id).await
    })
}

/// `ProjectMemberViewSet.destroy` (`member.py:299-345`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let statement = queries::PROJECT_MEMBER_DESTROY_TARGET
            .prepare(client)
            .await?;
        let target = client
            .query_opt(&statement, &[&slug, &project_id, &pk])
            .await?;
        let Some(target) = target else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let target_id: Uuid = target.get(0);
        let target_role: i16 = target.get(1);

        let statement = queries::PROJECT_MEMBER_SELF.prepare(client).await?;
        let requester = client
            .query_opt(&statement, &[&project_id, &user_id, &slug])
            .await?;
        let Some(requester) = requester else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let requester_id: Uuid = requester.get(0);
        let requester_role: i16 = requester.get(1);

        if target_id == requester_id {
            return fail(
                400,
                "You cannot remove yourself from the workspace. Please use leave workspace",
            );
        }
        if requester_role < target_role {
            return fail(400, "You cannot remove a user having role higher than you");
        }

        let statement = queries::PROJECT_MEMBER_DEACTIVATE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[&write::now(), &Audit::on_update(Some(user_id)), &target_id],
            )
            .await
            .map_err(db_error)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `ProjectMemberViewSet.leave` (`member.py:347-375`).
pub fn leave<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let statement = queries::PROJECT_MEMBER_SELF.prepare(client).await?;
        let member = client
            .query_opt(&statement, &[&project_id, &user_id, &slug])
            .await?;
        let Some(member) = member else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let id: Uuid = member.get(0);
        let role: i16 = member.get(1);

        if role == ROLE_ADMIN {
            let statement = queries::PROJECT_ADMIN_COUNT.prepare(client).await?;
            let row = client.query_one(&statement, &[&slug, &project_id]).await?;
            let admins: i64 = row.get(0);
            if admins <= 1 {
                return fail(
                    400,
                    "You cannot leave the project as your the only admin of the project you will have to either delete the project or create an another admin",
                );
            }
        }

        let statement = queries::PROJECT_MEMBER_DEACTIVATE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[&write::now(), &Audit::on_update(Some(user_id)), &id],
            )
            .await
            .map_err(db_error)?;
        Ok(ViewResponse::empty(204))
    })
}

/// The columns `PROJECT_MEMBER_TARGET` returns.
struct Target {
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    member_id: Option<Uuid>,
    comment: Option<String>,
    role: i16,
    view_props: JsonText,
    default_props: JsonText,
    preferences: JsonText,
    sort_order: f64,
    is_active: bool,
}

async fn get_target(
    client: &deadpool_postgres::Client,
    pk: Uuid,
    project_id: Uuid,
    slug: &str,
) -> Result<Target, ViewError> {
    let statement = queries::PROJECT_MEMBER_TARGET.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&pk, &project_id, &slug])
        .await?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Ok(Target {
        created_at: row.get(0),
        updated_at: row.get(1),
        created_by_id: row.get(2),
        updated_by_id: row.get(3),
        deleted_at: row.get(4),
        id: row.get(5),
        workspace_id: row.get(6),
        project_id: row.get(7),
        member_id: row.get(8),
        comment: row.get(9),
        role: row.get(10),
        view_props: row.get(11),
        default_props: row.get(12),
        preferences: row.get(13),
        sort_order: row.get(14),
        is_active: row.get(15),
    })
}

/// The joined row `ProjectMemberSerializer` renders.
struct Joined {
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    id: Uuid,
    comment: Option<String>,
    role: i16,
    view_props: JsonText,
    default_props: JsonText,
    preferences: JsonText,
    sort_order: f64,
    is_active: bool,
    project: lite::LiteProject,
    user: Option<lite::LiteUser>,
    workspace: lite::LiteWorkspace,
}

async fn get_joined(
    client: &deadpool_postgres::Client,
    id: Uuid,
    project_id: Uuid,
    slug: &str,
) -> Result<Joined, ViewError> {
    let statement = queries::PROJECT_MEMBER_WRITE_ONE.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&id, &project_id, &slug])
        .await?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Ok(Joined {
        created_at: row.get(0),
        updated_at: row.get(1),
        created_by_id: row.get(2),
        updated_by_id: row.get(3),
        deleted_at: row.get(4),
        id: row.get(5),
        comment: row.get(9),
        role: row.get(10),
        view_props: row.get(11),
        default_props: row.get(12),
        preferences: row.get(13),
        sort_order: row.get(14),
        is_active: row.get(15),
        project: crate::projects::joined::project_at(&row, 16)?,
        user: user_at(&row, 23)?,
        workspace: crate::projects::joined::workspace_at(&row, 32)?,
    })
}

/// The user columns when the FK is nullable (`PROJECT_MEMBER_WRITE_ONE`'s
/// `LEFT OUTER JOIN users`).
fn user_at(row: &tokio_postgres::Row, start: usize) -> Result<Option<lite::LiteUser>, ViewError> {
    let id: Option<Uuid> = row.get(start);
    let Some(id) = id else {
        return Ok(None);
    };
    Ok(Some(lite::LiteUser {
        id,
        first_name: row.get(start + 1),
        last_name: row.get(start + 2),
        avatar: row.get(start + 3),
        avatar_asset_id: row.get(start + 4),
        avatar_url: None,
        is_bot: row.get(start + 5),
        display_name: row.get(start + 6),
        email: row.get(start + 7),
        last_login_medium: row.get(start + 8),
    }))
}

/// `ProjectMemberSerializer(project_member).data`, with
/// `_without_personal_fields` when `include_personal` is false.
async fn render_member(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    member: Joined,
    include_personal: bool,
) -> Result<ViewResponse, ViewError> {
    let mut project = member.project.clone();
    let mut workspace = member.workspace.clone();
    let mut user = member.user.clone();

    let mut asset_ids = HashSet::new();
    asset_ids.extend(project.cover_image_asset_id);
    asset_ids.extend(workspace.logo_asset_id);
    if let Some(user) = &user {
        asset_ids.extend(user.avatar_asset_id);
    }
    let urls = lite::load_asset_urls(client, &asset_ids).await?;
    project.cover_image_url = lite::asset_or_text(
        project.cover_image_asset_id,
        project.cover_image.clone().unwrap_or_default(),
        &urls,
    )?;
    workspace.logo_url = lite::asset_or_text(
        workspace.logo_asset_id,
        workspace.logo.clone().unwrap_or_default(),
        &urls,
    )?;
    if let Some(user) = &mut user {
        user.avatar_url = lite::asset_or_text(user.avatar_asset_id, user.avatar.clone(), &urls)?;
    }

    let project_lite = project.lite();
    let workspace_lite = workspace.lite();
    let user_lite = user.as_ref().map(lite::LiteUser::lite);
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);

    if include_personal {
        let serializer = ProjectMemberSerializer {
            id: member.id,
            workspace: Some(&workspace_lite),
            project: Some(&project_lite),
            member: user_lite.as_ref(),
            created_at: member.created_at,
            updated_at: member.updated_at,
            deleted_at: member.deleted_at,
            comment: member.comment.as_deref(),
            role: i64::from(member.role),
            view_props: Some(PgJson::from(&member.view_props)),
            default_props: Some(PgJson::from(&member.default_props)),
            preferences: Some(PgJson::from(&member.preferences)),
            sort_order: member.sort_order,
            is_active: member.is_active,
            created_by: member.created_by_id,
            updated_by: member.updated_by_id,
        };
        let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
        return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
    }

    let serializer = ProjectMemberPublic {
        id: member.id,
        workspace: Some(&workspace_lite),
        project: Some(&project_lite),
        member: user_lite.as_ref(),
        created_at: member.created_at,
        updated_at: member.updated_at,
        deleted_at: member.deleted_at,
        comment: member.comment.as_deref(),
        role: i64::from(member.role),
        sort_order: member.sort_order,
        is_active: member.is_active,
        created_by: member.created_by_id,
        updated_by: member.updated_by_id,
    };
    let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

/// `ProjectMemberSerializer` with `_without_personal_fields` applied: the same
/// key order minus `view_props`, `default_props` and `preferences`
/// (`plane/app/views/project/member.py:27-34`).
#[derive(Debug)]
struct ProjectMemberPublic<'a> {
    id: Uuid,
    workspace: Option<&'a WorkspaceLiteSerializer<'a>>,
    project: Option<&'a ProjectLiteSerializer<'a>>,
    member: Option<&'a UserLiteSerializer<'a>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    comment: Option<&'a str>,
    role: i64,
    sort_order: f64,
    is_active: bool,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

impl DrfSerialize for ProjectMemberPublic<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &plane_serializers::DrfUuid(self.id))?;
        obj.field_nested_dyn(
            "workspace",
            ctx,
            self.workspace
                .map(|value| value as &(dyn DrfSerialize + '_)),
        )?;
        obj.field_nested_dyn(
            "project",
            ctx,
            self.project.map(|value| value as &(dyn DrfSerialize + '_)),
        )?;
        obj.field_nested_dyn(
            "member",
            ctx,
            self.member.map(|value| value as &(dyn DrfSerialize + '_)),
        )?;
        obj.field_dyn("created_at", ctx, &drf_datetime(self.created_at, ctx.tz()))?;
        obj.field_dyn("updated_at", ctx, &drf_datetime(self.updated_at, ctx.tz()))?;
        obj.field_dyn(
            "deleted_at",
            ctx,
            &drf_datetime_opt(self.deleted_at, ctx.tz()),
        )?;
        obj.field_dyn("comment", ctx, &self.comment)?;
        obj.field_dyn("role", ctx, &self.role)?;
        obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        obj.field_dyn("is_active", ctx, &self.is_active)?;
        obj.field_dyn(
            "created_by",
            ctx,
            &self.created_by.map(plane_serializers::DrfUuid),
        )?;
        obj.field_dyn(
            "updated_by",
            ctx,
            &self.updated_by.map(plane_serializers::DrfUuid),
        )?;
        obj.end();
        Ok(())
    }
}
