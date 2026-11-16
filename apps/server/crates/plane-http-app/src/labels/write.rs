// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `LabelViewSet`'s write actions and `BulkCreateIssueLabelsEndpoint.post`
//! (`plane/app/views/issue/label.py`), plus the `LabelSerializer` validation
//! they share (`plane/app/serializers/issue.py:371-396`).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/issue-labels/` | `create` | `label.py:41-62` |
//! | `PUT …/issue-labels/{pk}/` | `update` (mixin) | `label.py:69` |
//! | `PATCH …/issue-labels/{pk}/` | `partial_update` | `label.py:65-92` |
//! | `DELETE …/issue-labels/{pk}/` | `destroy` | `label.py:94-96` |
//! | `POST …/bulk-create-labels/` | `post` | `label.py:99-124` |
//!
//! ## What Django does per action
//!
//! **create** invalidates `/api/workspaces/:slug/labels/` in Redis, validates
//! the serializer (parent existence, `name` required/unique per project via
//! `validate_name`), then `Label.objects.create`: H20 writes
//! `MAX(sort_order)+10000` (or 65535), `WorkspaceBaseModel.save` re-derives the
//! workspace from the project, H01 stamps `created_by`. An `IntegrityError`
//! (a race past `validate_name`) is caught and answered with the label-specific
//! 400.
//!
//! **PUT** is DRF's `UpdateModelMixin.update`: full validation, but
//! `get_serializer_context()` carries **no** `project_id`, so `validate_name`
//! checks the *workspace-level* labels instead of the project's. No cache
//! invalidation (only `partial_update` carries the decorator) and no
//! `IntegrityError` catch, so a duplicate that only the database sees is the
//! family 400.
//!
//! **PATCH** invalidates the cache, runs an exact-name pre-check *before*
//! `get_object()`, then the normalizer with `project_id` in context.
//!
//! **destroy** invalidates the cache, `get_object()`, then H03 (soft delete +
//! `soft_delete_related_objects`); `issue_labels` rows are removed by the
//! worker, not in the request.
//!
//! **bulk create** has no cache invalidation, no serializer and no
//! transaction: `Project.objects.get(pk=…)`, one `Label` per item with a random
//! colour and `created_by`/`updated_by` set, `bulk_create(ignore_conflicts=
//! True)` — so `Label.save()`/H20 never runs and every row keeps
//! `sort_order=65535` — then the *in-memory* list is serialised, including rows
//! the conflict clause skipped.

use chrono::{DateTime, Utc};
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks;
use plane_db::hooks::audit::Audit;
use plane_db::models::label::Label;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use plane_serializers::generated::app_issue::LabelSerializer;
use plane_serializers::{RenderCtx, render, render_list};
use uuid::Uuid;

use crate::common::cache;
use crate::common::{client, lookup};
use crate::labels::{not_found, queries};
use crate::write;
use crate::write::ValidationErrors;
use crate::write::fields::{self, CharOptions, Detail, Raw};

/// `@invalidate_cache(path="/api/workspaces/:slug/labels/", url_params=True,
/// user=False, multiple=True)` (`label.py:40,64,94`).
const LABELS_CACHE_PATH: &str = "/api/workspaces/:slug/labels/";

/// One request-body field as the serializer's `get_value` sees it.
enum FieldValue<'a> {
    Absent,
    Json(&'a Json),
    Text(&'a str),
}

/// `request.data.get(field)` without DRF's JSON→string reduction.
fn field<'a>(data: &'a Data, name: &str) -> FieldValue<'a> {
    match data {
        Data::Json(Json::Dict(map)) => map.get(name).map_or(FieldValue::Absent, FieldValue::Json),
        Data::Form(query) => query.get(name).map_or(FieldValue::Absent, FieldValue::Text),
        Data::Json(_) => FieldValue::Absent,
    }
}

/// The validated `LabelSerializer` data.
#[derive(Default)]
struct ValidatedLabel {
    /// `None` = absent; `Some(None)` = explicit null; `Some(Some(id))` = a row.
    parent: Option<Option<Uuid>>,
    name: Option<String>,
    color: Option<String>,
    sort_order: Option<f64>,
}

/// `LabelSerializer(data=…).is_valid()` for one of the three write actions.
///
/// `project_scope` is the serializer context's `project_id` (the URL project
/// for create/PATCH, `None` for PUT); `instance_id` is `self.instance.pk` for
/// an update.
async fn validate(
    data: &Data,
    client: &deadpool_postgres::Client,
    project_scope: Option<Uuid>,
    instance_id: Option<Uuid>,
    partial: bool,
) -> Result<Result<ValidatedLabel, ViewResponse>, ViewError> {
    // `Serializer.to_internal_value` on a non-mapping body.
    if let Data::Json(value) = data {
        match value {
            Json::Dict(_) => {}
            Json::Null => return Ok(Err(non_field_errors("No data provided", "null"))),
            other => {
                return Ok(Err(non_field_errors(
                    format!(
                        "Invalid data. Expected a dictionary, but got {}.",
                        python_type_name(other)
                    ),
                    "invalid",
                )));
            }
        }
    }

    let mut errors = ValidationErrors::default();
    let mut validated = ValidatedLabel::default();

    // `parent`: `PrimaryKeyRelatedField(queryset=Label.objects.all(),
    // allow_null=True, required=False)`.
    match field(data, "parent") {
        FieldValue::Absent => {}
        value => {
            let parsed = match parse_parent(value) {
                Ok(ParsedParent::Null) => Some(None),
                Ok(ParsedParent::Id { id, display }) => {
                    let statement = queries::LABEL_PARENT_EXISTS.prepare(client).await?;
                    if client.query_opt(&statement, &[&id]).await?.is_some() {
                        Some(Some(id))
                    } else {
                        errors.push(
                            "parent",
                            Detail::new(
                                format!("Invalid pk \"{display}\" - object does not exist."),
                                "does_not_exist",
                            ),
                        );
                        None
                    }
                }
                Err(detail) => {
                    errors.push("parent", detail);
                    None
                }
            };
            validated.parent = parsed;
        }
    }

    // `name`: `CharField(max_length=255)` + `validate_name`.
    match field(data, "name") {
        FieldValue::Absent => {
            if !partial {
                errors.push("name", Detail::required());
            }
        }
        FieldValue::Json(Json::Null) => errors.push("name", Detail::null()),
        value => {
            let options = CharOptions {
                allow_null: false,
                allow_blank: false,
                trim_whitespace: true,
                max_length: Some(255),
            };
            match fields::char(raw(value), &options) {
                Ok(name) => {
                    if name_taken(client, project_scope, instance_id, &name).await? {
                        errors.push("name", Detail::new("LABEL_NAME_ALREADY_EXISTS", "invalid"));
                    } else {
                        validated.name = Some(name);
                    }
                }
                Err(detail) => errors.push("name", detail),
            }
        }
    }

    // `color`: `CharField(max_length=255, allow_blank=True)`.
    match field(data, "color") {
        FieldValue::Absent => {}
        FieldValue::Json(Json::Null) => errors.push("color", Detail::null()),
        value => {
            let options = CharOptions {
                allow_null: false,
                allow_blank: true,
                trim_whitespace: true,
                max_length: Some(255),
            };
            match fields::char(raw(value), &options) {
                Ok(color) => validated.color = Some(color),
                Err(detail) => errors.push("color", detail),
            }
        }
    }

    // `sort_order`: `FloatField(required=False)`.
    match field(data, "sort_order") {
        FieldValue::Absent => {}
        FieldValue::Json(Json::Null) => errors.push("sort_order", Detail::null()),
        value => match fields::float(raw(value)) {
            Ok(value) => validated.sort_order = Some(value),
            Err(detail) => errors.push("sort_order", detail),
        },
    }

    if errors.is_empty() {
        Ok(Ok(validated))
    } else {
        Ok(Err(errors.into_response()))
    }
}

fn raw(value: FieldValue<'_>) -> Raw<'_> {
    match value {
        FieldValue::Text(text) => Raw::Text(text),
        FieldValue::Json(value) => Raw::Json(value),
        FieldValue::Absent => unreachable!("raw() is not called for an absent field"),
    }
}

/// `{"non_field_errors": [message]}`.
fn non_field_errors(message: impl Into<String>, code: &'static str) -> ViewResponse {
    ValidationErrors::new("non_field_errors", Detail::new(message, code)).into_response()
}

/// Python's `type(value).__name__` for the body-shape error.
fn python_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::List(_) => "list",
        Json::Dict(_) => "dict",
    }
}

enum ParsedParent {
    Null,
    Id { id: Uuid, display: String },
}

/// `PrimaryKeyRelatedField.to_internal_value` (`rest_framework/relations.py:
/// 251-262`) with no `pk_field`: Django's `UUIDField` coercion decides the
/// message.
fn parse_parent(value: FieldValue<'_>) -> Result<ParsedParent, Detail> {
    match value {
        FieldValue::Text(text) => parse_parent_text(text),
        FieldValue::Json(Json::Null) => Ok(ParsedParent::Null),
        FieldValue::Json(Json::Str(text)) => parse_parent_text(text.as_str().unwrap_or("")),
        FieldValue::Json(Json::Bool(_)) => Err(Detail::new(
            "Incorrect type. Expected pk value, received bool.",
            "incorrect_type",
        )),
        FieldValue::Json(Json::Int(value)) => {
            let Some(number) = value.as_i64() else {
                return Err(invalid_uuid(&value.to_string()));
            };
            if number < 0 {
                return Err(invalid_uuid(&number.to_string()));
            }
            let Ok(number) = u128::try_from(number) else {
                return Err(invalid_uuid(&number.to_string()));
            };
            Ok(ParsedParent::Id {
                id: Uuid::from_u128(number),
                display: number.to_string(),
            })
        }
        FieldValue::Json(other) => Err(invalid_uuid(&python_str(other))),
        FieldValue::Absent => unreachable!("parse_parent() is not called for an absent field"),
    }
}

fn parse_parent_text(text: &str) -> Result<ParsedParent, Detail> {
    match Uuid::parse_str(text) {
        Ok(id) => Ok(ParsedParent::Id {
            id,
            display: text.to_owned(),
        }),
        Err(_) => Err(invalid_uuid(text)),
    }
}

/// `ValidationError("“%(value)s” is not a valid UUID.")` from Django's
/// `UUIDField.to_python`, converted by DRF's `get_error_detail`.
fn invalid_uuid(value: &str) -> Detail {
    Detail::new(format!("“{value}” is not a valid UUID."), "invalid")
}

/// Python `str(value)`.
fn python_str(value: &Json) -> String {
    match value {
        Json::Null => "None".to_owned(),
        Json::Bool(value) => {
            if *value {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Json::Int(value) => value.to_string(),
        Json::Float(value) => plane_query::pyfloat::repr(*value),
        Json::Str(value) => value.as_str().unwrap_or_default().to_owned(),
        Json::List(items) => format!(
            "[{}]",
            items.iter().map(python_repr).collect::<Vec<_>>().join(", ")
        ),
        Json::Dict(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    python_repr_str(key.as_str().unwrap_or_default()),
                    python_repr(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Python `repr(value)` for the nested parts of a composite `str()`.
fn python_repr(value: &Json) -> String {
    match value {
        Json::Str(value) => python_repr_str(value.as_str().unwrap_or_default()),
        other => python_str(other),
    }
}

fn python_repr_str(text: &str) -> String {
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

/// `LabelSerializer.validate_name` (`plane/app/serializers/issue.py:385-396`).
async fn name_taken(
    client: &deadpool_postgres::Client,
    project_scope: Option<Uuid>,
    instance_id: Option<Uuid>,
    name: &str,
) -> Result<bool, ViewError> {
    let row = match (project_scope, instance_id) {
        (Some(project_id), Some(instance_id)) => {
            let statement = queries::LABEL_NAME_TAKEN_PATCH.prepare(client).await?;
            client
                .query_opt(&statement, &[&project_id, &name, &instance_id])
                .await?
        }
        (Some(project_id), None) => {
            let statement = queries::LABEL_NAME_TAKEN_CREATE.prepare(client).await?;
            client.query_opt(&statement, &[&project_id, &name]).await?
        }
        (None, instance_id) => {
            let statement = queries::LABEL_NAME_TAKEN_UPDATE.prepare(client).await?;
            client
                .query_opt(&statement, &[&name, &instance_id.unwrap_or_else(Uuid::nil)])
                .await?
        }
    };
    Ok(row.is_some())
}

/// Django's `IntegrityError` family (`django.db.utils.IntegrityError`): the
/// family handler's 400.
fn integrity(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// `LabelViewSet.create` (`plane/app/views/issue/label.py:41-62`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        cache::invalidate_cache(ctx, Some(LABELS_CACHE_PATH), true, false, true).await?;

        let data = ctx.data()?;
        let validated = match validate(data, client, Some(project_id), None, false).await? {
            Ok(validated) => validated,
            Err(response) => return Ok(response),
        };

        // `Label.save()`: H20 first, then `WorkspaceBaseModel.save` reads the
        // project (`Project.objects.get(pk=…)`).
        let sort_order = hooks::label::before_insert(client, Some(project_id))
            .await
            .map_err(ViewError::from)?;
        let Some(workspace_id) = project_workspace(client, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        let now = write::now();
        let label = Label {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user_id),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            workspace_id,
            project_id: Some(project_id),
            parent_id: validated.parent.flatten(),
            name: validated.name.unwrap_or_default(),
            description: String::new(),
            color: validated.color.unwrap_or_default(),
            sort_order,
            external_source: None,
            external_id: None,
        };

        let statement = queries::LABEL_INSERT.prepare(client).await?;
        if let Err(error) = client
            .execute(
                &statement,
                &[
                    &label.created_at,
                    &label.updated_at,
                    &label.created_by_id,
                    &label.updated_by_id,
                    &label.id,
                    &label.workspace_id,
                    &label.project_id,
                    &label.parent_id,
                    &label.name,
                    &label.description,
                    &label.color,
                    &label.sort_order,
                ],
            )
            .await
        {
            if error
                .as_db_error()
                .is_some_and(|db| db.code().code().starts_with("23"))
            {
                return Ok(write::error_body(
                    400,
                    "Label with the same name already exists in the project",
                ));
            }
            return Err(ViewError::from(error));
        }

        render_label(ctx, &label, 201)
    })
}

/// `UpdateModelMixin.update` — the PUT route (`label.py:69`).
pub fn update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
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

        let Some(mut label) = self_label(client, slug, project_id, pk, user_id).await? else {
            return not_found::no_match("Label");
        };

        let data = ctx.data()?;
        // `UpdateModelMixin` builds the serializer context from
        // `get_serializer_context()`, which has no `project_id`.
        let validated = match validate(data, client, None, Some(label.id), false).await? {
            Ok(validated) => validated,
            Err(response) => return Ok(response),
        };

        apply(&mut label, validated);
        label.updated_at = write::now();
        label.updated_by_id = Audit::on_update(Some(user_id));
        save_label(client, &label).await?;
        render_label(ctx, &label, 200)
    })
}

/// `LabelViewSet.partial_update` (`plane/app/views/issue/label.py:65-92`).
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

        // `partial_update`'s decorator has no `multiple=True` (unlike `create`),
        // so it deletes the literal `/api/workspaces/<slug>/labels/` key and
        // leaves the per-user cache entry in place.
        cache::invalidate_cache(ctx, Some(LABELS_CACHE_PATH), true, false, false).await?;

        // The exact-name pre-check runs before `get_object()` and uses the raw
        // body value (`label.py:47-58`).
        let data = ctx.data()?;
        match field(data, "name") {
            FieldValue::Absent => {}
            value => {
                if let Some(name) = raw_python_str(value)
                    && exact_name_taken(client, project_id, pk, &name).await?
                {
                    return Ok(write::error_body(
                        400,
                        "Label with the same name already exists in the project",
                    ));
                }
            }
        }

        let Some(mut label) = self_label(client, slug, project_id, pk, user_id).await? else {
            return not_found::no_match("Label");
        };
        let validated = match validate(data, client, Some(project_id), Some(label.id), true).await?
        {
            Ok(validated) => validated,
            Err(response) => return Ok(response),
        };

        apply(&mut label, validated);
        label.updated_at = write::now();
        label.updated_by_id = Audit::on_update(Some(user_id));
        save_label(client, &label).await?;
        render_label(ctx, &label, 200)
    })
}

/// `LabelViewSet.destroy` (`label.py:94-96`) → `BaseViewSet.destroy`.
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

        // Same missing `multiple=True` as `partial_update`.
        cache::invalidate_cache(ctx, Some(LABELS_CACHE_PATH), true, false, false).await?;
        let Some(label) = self_label(client, slug, project_id, pk, user_id).await? else {
            return not_found::no_match("Label");
        };

        write::soft_delete(
            client,
            "labels",
            "id",
            label.id,
            Audit::on_update(Some(user_id)),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "label", label.id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `BulkCreateIssueLabelsEndpoint.post` (`label.py:99-124`).
pub fn bulk_create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let data = ctx.data()?;
        let items: Vec<&Json> = match field(data, "label_data") {
            FieldValue::Absent => Vec::new(),
            FieldValue::Json(Json::List(items)) => items.iter().collect(),
            // Iterating a non-list is a `TypeError` (numbers/None) or yields
            // strings/keys that fail `label.get` (`str`/`dict`).
            FieldValue::Json(_) => {
                return Err(ViewError::Unknown(
                    "TypeError/AttributeError: label_data is not a list".to_string(),
                ));
            }
            FieldValue::Text("") => Vec::new(),
            FieldValue::Text(_) => {
                return Err(ViewError::Unknown(
                    "AttributeError: 'str' object has no attribute 'get'".to_string(),
                ));
            }
        };

        // `Project.objects.get(pk=project_id)` runs before the comprehension.
        let Some(workspace_id) = project_workspace(client, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        let statement = queries::LABEL_BULK_INSERT.prepare(client).await?;
        let mut labels = Vec::with_capacity(items.len());
        for item in items {
            let Some(item) = item.as_dict() else {
                return Err(ViewError::Unknown(
                    "AttributeError: a label_data entry is not an object".to_string(),
                ));
            };
            let name = match item.get("name") {
                None => "Migrated".to_owned(),
                Some(Json::Str(value)) => value.as_str().unwrap_or_default().to_owned(),
                Some(Json::Null) => {
                    // `name` is NOT NULL: `IntegrityError` → the family 400.
                    return Err(ViewError::Integrity);
                }
                Some(_) => {
                    return Err(ViewError::Unknown(
                        "ProgrammingError: label name is not a string".to_string(),
                    ));
                }
            };
            let description = match item.get("description") {
                None => "Migrated Issue".to_owned(),
                Some(Json::Str(value)) => value.as_str().unwrap_or_default().to_owned(),
                Some(Json::Null) => {
                    return Err(ViewError::Integrity);
                }
                Some(_) => {
                    return Err(ViewError::Unknown(
                        "ProgrammingError: label description is not a string".to_string(),
                    ));
                }
            };
            let now = write::now();
            let label = Label {
                created_at: now,
                updated_at: now,
                created_by_id: Some(user_id),
                updated_by_id: Some(user_id),
                deleted_at: None,
                id: Uuid::new_v4(),
                workspace_id,
                project_id: Some(project_id),
                parent_id: None,
                name,
                description,
                color: random_colour(),
                sort_order: 65535.0,
                external_source: None,
                external_id: None,
            };
            client
                .execute(
                    &statement,
                    &[
                        &label.created_at,
                        &label.updated_at,
                        &label.created_by_id,
                        &label.updated_by_id,
                        &label.id,
                        &label.workspace_id,
                        &label.project_id,
                        &label.name,
                        &label.description,
                        &label.color,
                        &label.sort_order,
                    ],
                )
                .await
                .map_err(integrity)?;
            labels.push(label);
        }

        let entries: Vec<plane_query::json::Json> = labels
            .iter()
            .map(|label| {
                plane_query::json::Json::object(vec![
                    ("parent", plane_query::json::Json::opt_uuid(label.parent_id)),
                    ("name", plane_query::json::Json::string(&label.name)),
                    ("color", plane_query::json::Json::string(&label.color)),
                    ("id", plane_query::json::Json::uuid(label.id)),
                    (
                        "project_id",
                        plane_query::json::Json::opt_uuid(label.project_id),
                    ),
                    (
                        "workspace_id",
                        plane_query::json::Json::uuid(label.workspace_id),
                    ),
                    (
                        "sort_order",
                        plane_query::json::Json::Float(label.sort_order),
                    ),
                ])
            })
            .collect();
        let body = plane_query::json::Json::object(vec![(
            "labels",
            plane_query::json::Json::Array(entries),
        )])
        .to_drf();
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `Project.objects.get(pk=…)`, reduced to the workspace id the label needs.
async fn project_workspace(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
) -> Result<Option<Uuid>, ViewError> {
    let statement = queries::PROJECT_FOR_LABEL.prepare(client).await?;
    Ok(client
        .query_opt(&statement, &[&project_id])
        .await?
        .map(|row| row.get(1)))
}

/// `get_object()` over `LabelViewSet.get_queryset()`.
async fn self_label(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    pk: Uuid,
    member_id: Uuid,
) -> Result<Option<Label>, ViewError> {
    let statement = queries::LABEL_IN_PROJECT.prepare(client).await?;
    match client
        .query_opt(&statement, &[&slug, &project_id, &member_id, &pk])
        .await?
    {
        Some(row) => Ok(Some(Label::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

/// `partial_update`'s exact-name pre-check.
async fn exact_name_taken(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    pk: Uuid,
    name: &str,
) -> Result<bool, ViewError> {
    let statement = queries::LABEL_NAME_EXACT_PATCH.prepare(client).await?;
    Ok(client
        .query_opt(&statement, &[&project_id, &name, &pk])
        .await?
        .is_some())
}

/// The raw body value as `str(value)` for the pre-check's `name=` filter.
/// `None` means the value is `None`, which Django turns into `name IS NULL`
/// (never a match, because the column is NOT NULL).
fn raw_python_str(value: FieldValue<'_>) -> Option<String> {
    match value {
        FieldValue::Absent => None,
        FieldValue::Text(text) => Some(text.to_owned()),
        FieldValue::Json(Json::Null) => None,
        FieldValue::Json(value) => Some(python_str(value)),
    }
}

/// `ModelSerializer.update`: assign only the validated fields.
fn apply(label: &mut Label, validated: ValidatedLabel) {
    if let Some(parent) = validated.parent {
        label.parent_id = parent;
    }
    if let Some(name) = validated.name {
        label.name = name;
    }
    if let Some(color) = validated.color {
        label.color = color;
    }
    if let Some(sort_order) = validated.sort_order {
        label.sort_order = sort_order;
    }
}

async fn save_label(client: &deadpool_postgres::Client, label: &Label) -> Result<(), ViewError> {
    let statement = queries::LABEL_UPDATE.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &label.created_at,
                &label.updated_at,
                &label.created_by_id,
                &label.updated_by_id,
                &label.deleted_at,
                &label.id,
                &label.workspace_id,
                &label.project_id,
                &label.parent_id,
                &label.name,
                &label.description,
                &label.color,
                &label.sort_order,
                &label.external_source,
                &label.external_id,
                &label.id,
            ],
        )
        .await
        .map_err(integrity)?;
    Ok(())
}

fn serializer_for(label: &Label) -> LabelSerializer<'_> {
    LabelSerializer {
        parent: label.parent_id,
        name: &label.name,
        color: &label.color,
        id: label.id,
        project_id: label.project_id,
        workspace_id: label.workspace_id,
        sort_order: label.sort_order,
    }
}

fn render_label(ctx: &ViewCtx<'_>, label: &Label, status: u16) -> Result<ViewResponse, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let body = render(&serializer_for(label), &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// `f"#{random.randint(0, 0xFFFFFF + 1):06X}"` (`label.py:111`). The upper
/// bound is inclusive, so `0x1000000` renders seven digits — ported as it is.
fn random_colour() -> String {
    let bytes = Uuid::new_v4().into_bytes();
    let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 0x0100_0001;
    format!("#{value:06X}")
}

/// A `render_list` target the bulk route does not need; kept private to avoid
/// an unused import in a future refactor.
#[allow(dead_code)]
fn _render_many(ctx: &ViewCtx<'_>, labels: &[Label]) -> Result<ViewResponse, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let serializers: Vec<LabelSerializer<'_>> = labels.iter().map(serializer_for).collect();
    let body = render_list(&serializers, &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

#[allow(dead_code)]
fn _assert_deleted_at_type(_: Option<DateTime<Utc>>) {}
