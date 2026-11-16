// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Three per-user project settings writes:
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | `…/projects/{project_id}/project-views/` | `POST` | `ProjectUserViewsEndpoint.post` (`plane/app/views/project/base.py:475-496`) | [`project_views`] — registered by `views::register`, because the route's group is `views` |
//! | `…/projects/{project_id}/user-properties/` | `PATCH` | `ProjectUserDisplayPropertyEndpoint.patch` (`plane/app/views/issue/base.py:745-766`) | [`user_properties`] — registered by `issues::register`, because the route's group is `issues` |
//! | `…/projects/{project_id}/preferences/member/{member_id}/` | `PATCH` | `ProjectMemberPreferenceEndpoint.patch` (`plane/app/views/project/member.py:416-432`) | [`preferences`] |
//!
//! ## project-views
//!
//! No serializer: the view reads the membership row and assigns
//! `request.data.get("<key>", <current>)` for four columns, then
//! `project_member.save()`. The three JSON columns are **replaced wholesale**
//! (`dict.get`'s default only applies when the key is absent), `sort_order` goes
//! through the ORM's float coercion, and a body value of `null` is written as
//! SQL `NULL` — which the `NOT NULL` columns refuse, so Django answers the
//! family's `400 {"error": "The payload is not valid"}` from the
//! `IntegrityError`. `204` on success.
//!
//! ## user-properties
//!
//! `get` or `create` the `ProjectUserProperty` row for the caller (create:
//! `WorkspaceBaseModel.save` copies `workspace` from the project; the five JSON
//! columns get the model defaults), then
//! `ProjectUserPropertySerializer(instance, data, partial=True)`:
//! `serializer.is_valid(raise_exception=True)` (400 with `serializer.errors`),
//! `serializer.save()` (a full-row `UPDATE`), then `200 serializer.data` from
//! the in-memory instance.
//!
//! ## preferences
//!
//! A member may only edit their own row (`403 {"error": "You cannot access
//! another member's preferences."}`). The serializer is built with
//! `{"preferences": request.data}`, i.e. the **whole body** becomes the patch,
//! and `validate_preferences` merges it into the stored dict with Python's
//! shallow `dict.update` (`plane/app/serializers/project.py:185-189`; the
//! position-preserving merge is `plane_db::hooks::json_merge`). The response is
//! `{"preferences": <merged>}`.
//!
//! ## Statements
//!
//! | Action | Django | Here |
//! | --- | ---: | ---: |
//! | project-views | 2 (membership, UPDATE) | 2 |
//! | user-properties | 2–3 (get, [workspace], INSERT/UPDATE) | 2–3 |
//! | preferences | 2 (membership, UPDATE) | 2 |

use chrono::{DateTime, Utc};
use plane_core::error::APPLICATION_JSON;
use plane_db::{JsonText, hooks};
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_issue::ProjectUserPropertySerializer;
use plane_serializers::{PgJson, RenderCtx};
use uuid::Uuid;

use crate::common::client;
use crate::projects::write_queries as queries;
use crate::projects::write_support;
use crate::write::{self, Serializer};

/// Register the three `POST`/`PATCH` methods.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.project.member.ProjectMemberPreferenceEndpoint")?
        .method(http::Method::PATCH, preferences)?;
    Ok(())
}

/// `ProjectUserViewsEndpoint.post` (`base.py:475-496`).
pub fn project_views<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, user) = write_support::target(ctx)?;
        let client = client(ctx)?;
        let _ = slug;

        let row = client
            .query_opt(
                queries::PROJECT_MEMBER_FOR_VIEW_PROPS.sql,
                &[&user, &project_id],
            )
            .await?;
        let Some(row) = row else {
            return Ok(write::error_body(403, "Forbidden"));
        };
        let member_id: Uuid = row.try_get(0)?;
        let view_props: JsonText = row.try_get(1)?;
        let default_props: JsonText = row.try_get(2)?;
        let preferences: JsonText = row.try_get(3)?;
        let sort_order: f64 = row.try_get(4)?;

        let data = ctx.data()?;
        // `request.data.get(key, current)` — present-but-null is SQL NULL.
        let view_props = replace_or_keep(data, "view_props", &view_props)?;
        let default_props = replace_or_keep(data, "default_props", &default_props)?;
        let preferences = replace_or_keep(data, "preferences", &preferences)?;
        let sort_order = float_or_keep(data, "sort_order", sort_order)?;

        client
            .execute(
                queries::PROJECT_MEMBER_SAVE_VIEW_PROPS.sql,
                &[
                    &view_props,
                    &default_props,
                    &preferences,
                    &sort_order,
                    &write::now(),
                    &Some(user),
                    &member_id,
                ],
            )
            .await
            .map_err(write_support::integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `request.data.get(key, current)` for a JSON column: `None` is SQL `NULL`.
fn replace_or_keep(
    data: &plane_drf::parsers::Data,
    key: &str,
    current: &JsonText,
) -> Result<Option<JsonText>, ViewError> {
    match data.json().and_then(|value| value.get(key)) {
        None => Ok(Some(current.clone())),
        Some(plane_serde::pyjson::Value::Null) => Ok(None),
        Some(value) => Ok(Some(JsonText::from_json(value).map_err(ViewError::from)?)),
    }
}

/// `request.data.get(key, current)` for `sort_order`: Python's `float()` on the
/// value the body carried, with `None` meaning SQL `NULL` (a `NOT NULL`
/// violation → the family's 400).
fn float_or_keep(
    data: &plane_drf::parsers::Data,
    key: &str,
    current: f64,
) -> Result<Option<f64>, ViewError> {
    use plane_serde::pyjson::Value;
    match data.json().and_then(|value| value.get(key)) {
        None => Ok(Some(current)),
        Some(Value::Null) => Ok(None),
        Some(Value::Int(value)) => Ok(Some(value.to_string().parse::<f64>().unwrap_or(f64::NAN))),
        Some(Value::Float(value)) => Ok(Some(*value)),
        Some(Value::Bool(value)) => Ok(Some(if *value { 1.0 } else { 0.0 })),
        Some(Value::Str(value)) => value
            .as_str()
            .ok_or(ViewError::Validation)?
            .trim()
            .parse::<f64>()
            .map(Some)
            .map_err(|_| {
                ViewError::Unknown("ValueError: could not convert string to float".to_owned())
            }),
        Some(_) => Err(ViewError::Unknown(
            "TypeError: float() argument must be a string or a real number".to_owned(),
        )),
    }
}

/// One stored `project_user_properties` row.
struct PropertyRow {
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    filters: JsonText,
    display_filters: JsonText,
    display_properties: JsonText,
    rich_filters: JsonText,
    preferences: JsonText,
    sort_order: f64,
}

impl PropertyRow {
    fn from_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            created_at: row.try_get(0)?,
            updated_at: row.try_get(1)?,
            created_by_id: row.try_get(2)?,
            updated_by_id: row.try_get(3)?,
            deleted_at: row.try_get(4)?,
            id: row.try_get(5)?,
            project_id: row.try_get(6)?,
            workspace_id: row.try_get(7)?,
            user_id: row.try_get(8)?,
            filters: row.try_get(9)?,
            display_filters: row.try_get(10)?,
            display_properties: row.try_get(11)?,
            rich_filters: row.try_get(12)?,
            preferences: row.try_get(13)?,
            sort_order: row.try_get(14)?,
        })
    }

    fn serializer(&self) -> ProjectUserPropertySerializer<'_> {
        ProjectUserPropertySerializer {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            filters: Some(PgJson::from(&self.filters)),
            display_filters: Some(PgJson::from(&self.display_filters)),
            display_properties: Some(PgJson::from(&self.display_properties)),
            rich_filters: Some(PgJson::from(&self.rich_filters)),
            preferences: Some(PgJson::from(&self.preferences)),
            sort_order: self.sort_order,
            created_by: self.created_by_id,
            updated_by: self.updated_by_id,
            project: self.project_id,
            workspace: self.workspace_id,
            user: self.user_id,
        }
    }
}

/// `ProjectUserDisplayPropertyEndpoint.patch` (`issue/base.py:745-766`).
pub fn user_properties<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, user) = write_support::target(ctx)?;
        let client = client(ctx)?;

        let existing = client
            .query_opt(
                queries::PROJECT_USER_PROPERTY_BY_USER.sql,
                &[&user, &project_id],
            )
            .await?;
        let mut property = match existing {
            Some(row) => PropertyRow::from_row(&row)?,
            None => {
                // `ProjectUserProperty.objects.create(user=…, project_id=…)`:
                // `WorkspaceBaseModel.save` reads `project.workspace` and the
                // five JSON columns take their model defaults.
                let workspace_id: Uuid = client
                    .query_one(queries::PROJECT_WORKSPACE_BY_ID.sql, &[&project_id, &slug])
                    .await?
                    .try_get(0)?;
                let now = write::now();
                create_property(client, project_id, workspace_id, user, now).await?
            }
        };

        let mut serializer = Serializer::new(ctx, true)?;
        write::generated::project_user_property_serializer::validate(&mut serializer, client)
            .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        if values.contains("deleted_at") {
            property.deleted_at = values.datetime("deleted_at");
        }
        for (name, slot) in [
            ("filters", &mut property.filters),
            ("display_filters", &mut property.display_filters),
            ("display_properties", &mut property.display_properties),
            ("rich_filters", &mut property.rich_filters),
            ("preferences", &mut property.preferences),
        ] {
            if let Some(value) = values.json(name) {
                *slot = value.clone();
            }
        }
        if let Some(sort_order) = values.float("sort_order") {
            property.sort_order = sort_order;
        }
        // `ModelSerializer.update` assigns every validated key; `BaseModel.save`
        // only overwrites `updated_by` on an update, so a body `created_by`
        // (including an explicit null) wins.
        if values.contains("created_by") {
            property.created_by_id = values.uuid("created_by");
        }
        property.updated_at = write::now();
        property.updated_by_id = Some(user);

        client
            .execute(
                queries::PROJECT_USER_PROPERTY_UPDATE.sql,
                &[
                    &property.created_at,
                    &property.updated_at,
                    &property.created_by_id,
                    &property.updated_by_id,
                    &property.deleted_at,
                    &property.project_id,
                    &property.workspace_id,
                    &property.user_id,
                    &property.filters,
                    &property.display_filters,
                    &property.display_properties,
                    &property.rich_filters,
                    &property.preferences,
                    &property.sort_order,
                    &property.id,
                ],
            )
            .await
            .map_err(write_support::integrity)?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&property.serializer(), &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// The `ProjectUserProperty.objects.create` statement, with the model's default
/// callables (`plane/db/models/issue.py:47-88` and
/// `plane/db/models/project.py:64-65`).
async fn create_property(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    workspace_id: Uuid,
    user: Uuid,
    now: DateTime<Utc>,
) -> Result<PropertyRow, ViewError> {
    let defaults = |json: &str| JsonText::new(json);
    let filters = defaults(
        r#"{"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}"#,
    );
    let display_filters = defaults(
        r#"{"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}"#,
    );
    let display_properties = defaults(
        r#"{"assignee": true, "attachment_count": true, "created_on": true, "due_date": true, "estimate": true, "key": true, "labels": true, "link": true, "priority": true, "start_date": true, "state": true, "sub_issue_count": true, "updated_on": true}"#,
    );
    let rich_filters = JsonText::new("{}");
    let preferences = defaults(
        r#"{"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}}"#,
    );
    let id = Uuid::new_v4();
    client
        .execute(
            queries::PROJECT_USER_PROPERTY_INSERT.sql,
            &[
                &now,
                &now,
                &Some(user),
                &id,
                &project_id,
                &workspace_id,
                &user,
                &filters,
                &display_filters,
                &display_properties,
                &rich_filters,
                &preferences,
                &65535.0_f64,
            ],
        )
        .await
        .map_err(write_support::integrity)?;
    // `serializer.instance` is the in-memory row, exactly like the Python.
    let _ = client;
    Ok(PropertyRow {
        created_at: now,
        updated_at: now,
        created_by_id: Some(user),
        updated_by_id: None,
        deleted_at: None,
        id,
        project_id,
        workspace_id,
        user_id: user,
        filters,
        display_filters,
        display_properties,
        rich_filters,
        preferences,
        sort_order: 65535.0,
    })
}

/// `ProjectMemberPreferenceEndpoint.patch` (`member.py:416-432`).
pub fn preferences<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, user) = write_support::target(ctx)?;
        let member_id = write_support::member_id(ctx)?;
        if member_id != user {
            return Ok(write::error_body(
                403,
                "You cannot access another member's preferences.",
            ));
        }
        let client = client(ctx)?;

        let row = client
            .query_opt(
                queries::PROJECT_MEMBER_BY_MEMBER_ID.sql,
                &[&project_id, &member_id, &slug],
            )
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let row_id: Uuid = row.try_get(5)?;
        let stored: JsonText = row.try_get(13)?;

        // `{"preferences": request.data}`: the whole body is the patch, and
        // `dict.update` on a non-mapping is a `TypeError` → 500.
        let patch = write::json_object(ctx.data()?)?;
        let merged = hooks::json_merge::merge(&stored, &patch).map_err(ViewError::from)?;

        client
            .execute(
                queries::PROJECT_MEMBER_SAVE_PREFERENCES.sql,
                &[&merged, &write::now(), &Some(user), &row_id],
            )
            .await
            .map_err(write_support::integrity)?;

        // `{"preferences": serializer.data["preferences"]}` — the merged dict,
        // in the position-preserving order.
        let value = plane_serde::pyjson::loads(merged.as_str().as_bytes(), "utf-8")
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let body = plane_serde::to_vec(
            plane_serde::Dialect::Drf,
            &PreferenceEnvelope { preferences: value },
        )?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `{"preferences": <merged>}`, in insertion order.
#[derive(serde::Serialize)]
struct PreferenceEnvelope {
    preferences: plane_serde::pyjson::Value,
}
