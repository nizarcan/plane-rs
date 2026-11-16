// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `StateViewSet`'s write actions (`plane/app/views/state/base.py`).
//!
//! | Route | Python |
//! | --- | --- |
//! | `POST …/projects/{project_id}/states/` | `create` (`:56-75`) |
//! | `PATCH …/projects/{project_id}/states/{pk}/` | `partial_update` (`:77-97`) |
//! | `POST …/states/{pk}/mark-default/` | `mark_as_default` (`:99-110`) |
//! | `DELETE …/states/{pk}/` | `destroy` (`:112-135`) |
//!
//! ## create (`:56-75`)
//!
//! `@invalidate_cache(path="workspaces/:slug/states/", url_params=True,
//! user=False)` wraps the permission decorator and deletes the key **before**
//! the view runs, so the handler does it first. `StateSerializer(data=…)` →
//! 400 `serializer.errors`; then `serializer.save(project_id=…)`:
//! `State.objects.create` runs H19 (slugify + `MAX(sequence) + 15000` when the
//! project has a live non-triage state), `ProjectBaseModel.save` copies the
//! project's workspace and `BaseModel.save` stamps `created_by`; the INSERT is
//! one full row. An `IntegrityError` whose message contains `"already exists"`
//! is the 400 `{"name": "The state name is already taken"}`; any other one is
//! the family's 400 `{"error": "The payload is not valid"}`. The response is
//! `StateSerializer(instance).data` with **200**, not 201.
//!
//! ## partial_update (`:77-97`)
//!
//! No cache decorator. `self.get_queryset().get(pk=…)` (project membership,
//! `archived_at IS NULL`, `is_triage=False`, manager without triage and
//! soft-deleted rows) → 404 `{"error": "The required object does not exist."}`.
//! The serializer is `partial=True`; `default` is read-only, `order` is a
//! declared non-model field that `ModelSerializer.update` sets as an instance
//! attribute (it is rendered, never stored). `State.save` recomputes the slug,
//! leaves `sequence` alone (H19 only on create) and H01 sets `updated_by`.
//!
//! ## mark_as_default (`:99-110`)
//!
//! The state comes from the same queryset (404 otherwise), then **one**
//! `UPDATE … SET default = CASE WHEN id = <pk> THEN TRUE ELSE FALSE END WHERE
//! project_id = …` over every live non-triage state of the project (the
//! `is_triage` boolean is *not* filtered and `updated_at` is untouched). 204.
//!
//! ## destroy (`:112-135`)
//!
//! `State.objects.get(is_triage=False, pk, project_id, workspace__slug)` — no
//! membership or `archived_at` filter; the project-admin permission is the only
//! authorization. A default state is 400 `{"error": "Default state cannot be
//! deleted"}`; a state still referenced by any non-deleted issue (draft,
//! archived and triage included: the view uses `Issue.objects`, the
//! soft-delete-only manager) is 400 `{"error": "The state is not empty, only
//! empty states can be deleted"}`. `state.delete()` is H03 (a full save with
//! the slug recomputed) plus the `soft_delete_related_objects` task; 204.
//!
//! ## StateSerializer (`plane/app/serializers/state.py:10-26`)
//!
//! Fields, in this order: `id`, `project_id`, `workspace_id`, `name`, `color`,
//! `group`, `default`, `description`, `sequence`, `order`. The first three and
//! `default` are read-only; `order` is a declared `FloatField(required=False)`
//! that no column backs. `validate()` rejects `group='triage'` with
//! `{"non_field_errors": ["Cannot create triage state"]}`.

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks;
use plane_db::models::state::State;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use plane_serializers::FieldValue;
use plane_serializers::generated::app_state::StateSerializer;
use uuid::Uuid;

use crate::common::{cache, client, lookup};
use crate::states::queries;
use crate::write::fields::{CharOptions, Detail, Raw};
use crate::write::{self, Serializer};

// ---------------------------------------------------------------------------
// The body
// ---------------------------------------------------------------------------

/// The writable `StateSerializer` fields, in `self.fields` order.
#[derive(Debug, Default)]
struct Body {
    name: Option<String>,
    color: Option<String>,
    group: Option<String>,
    description: Option<String>,
    sequence: Option<f64>,
    order: Option<f64>,
}

/// `StateGroup` choice keys (`plane/db/models/state.py:13-20`).
const GROUPS: [&str; 6] = [
    "backlog",
    "unstarted",
    "started",
    "completed",
    "cancelled",
    "triage",
];

// The field parsers have to run in serializer order and each one records its
// own value and error; one struct initializer would reorder the calls.
#[allow(clippy::field_reassign_with_default)]
fn read_body(serializer: &mut Serializer<'_>, partial: bool) -> Body {
    let mut body = Body::default();

    body.name = serializer.char_field(
        "name",
        CharOptions {
            allow_null: false,
            allow_blank: false,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    if body.name.is_none() && serializer.missing_required("name") {
        serializer.push_error("name", Detail::required());
    }
    body.color = serializer.char_field(
        "color",
        CharOptions {
            allow_null: false,
            allow_blank: false,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    if body.color.is_none() && serializer.missing_required("color") {
        serializer.push_error("color", Detail::required());
    }
    body.group = serializer.field("group", false, group_choice);
    body.description = serializer.char_field(
        "description",
        CharOptions {
            allow_null: false,
            allow_blank: true,
            trim_whitespace: true,
            max_length: None,
        },
    );
    body.sequence = serializer.float_field("sequence", false);
    body.order = serializer.float_field("order", false);

    // A field with a default is applied when absent — except under
    // `partial=True`, where `validate_empty_values` raises `SkipField` first
    // (`rest_framework/fields.py:494-530`).
    if !partial && body.group.is_none() && !serializer.has_errors() {
        body.group = Some("backlog".to_owned());
    }

    // `StateSerializer.validate` (`plane/app/serializers/state.py:20-23`):
    // `ValidationError("Cannot create triage state")` is a non-field error.
    if !serializer.has_errors() && body.group.as_deref() == Some("triage") {
        serializer.push_error(
            "non_field_errors",
            Detail::new("Cannot create triage state", "invalid"),
        );
    }

    body
}

/// `ChoiceField(StateGroup.choices)`: `str(data)` against the keys.
fn group_choice(raw: Raw<'_>) -> Result<String, Detail> {
    let text = match raw {
        Raw::Text(text) => text.to_owned(),
        Raw::Json(value) => python_str(value),
    };
    if GROUPS.contains(&text.as_str()) {
        Ok(text)
    } else {
        Err(Detail::new(
            format!("\"{text}\" is not a valid choice."),
            "invalid_choice",
        ))
    }
}

/// Python `str(value)` for a JSON value (the choice error message).
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
        Json::Str(value) => value.as_str().map(str::to_owned).unwrap_or_default(),
        Json::List(items) => {
            let parts: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Json::Dict(object) => {
            let parts: Vec<String> = object
                .iter()
                .map(|(key, value)| {
                    format!(
                        "'{}': {}",
                        key.as_str().unwrap_or_default(),
                        python_repr(value)
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn python_repr(value: &Json) -> String {
    match value {
        Json::Str(text) => format!("'{}'", text.as_str().unwrap_or_default()),
        other => python_str(other),
    }
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

/// `StateViewSet.create` (`plane/app/views/state/base.py:56-75`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `@invalidate_cache` wraps the permission decorator, so it runs before
        // the view (and before validation).
        invalidate_states_cache(ctx).await?;

        let mut serializer = Serializer::new(ctx, false)?;
        let body = read_body(&mut serializer, false);
        if let Err(errors) = serializer.finish() {
            return Ok(errors.into_response());
        }
        // `ModelSerializer.create` forwards every validated key to
        // `State.objects.create`, and `State.__init__` rejects the declared
        // non-model `order` field: Django answers 500
        // (`TypeError: State() got unexpected keyword arguments: 'order'`).
        if body.order.is_some() {
            return Err(ViewError::Unknown(
                "TypeError: State() got unexpected keyword arguments: 'order'".to_owned(),
            ));
        }
        let name = body
            .name
            .clone()
            .ok_or_else(|| ViewError::Unknown("validated name vanished".to_owned()))?;
        let color = body
            .color
            .clone()
            .ok_or_else(|| ViewError::Unknown("validated color vanished".to_owned()))?;

        // `ProjectBaseModel.save`: `self.workspace = self.project.workspace`
        // (`plane/db/models/project.py:237-239`); H19 then aggregates
        // `State.objects` for the project.
        let Some(row) = client
            .query_opt(queries::STATE_PROJECT_WORKSPACE.sql, &[&project_id])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = row.get(0);
        let row = client
            .query_one(queries::STATE_SEQUENCE_MAX.sql, &[&project_id])
            .await
            .map_err(ViewError::from)?;
        let max_sequence: Option<f64> = row.get(0);

        let saved =
            hooks::state::before_save(&name, true, body.sequence.unwrap_or(65535.0), max_sequence);
        let now = write::now();
        let state = State {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            project_id,
            workspace_id,
            name,
            description: body.description.clone().unwrap_or_default(),
            color,
            slug: saved.slug,
            sequence: saved.sequence,
            group: body.group.clone().unwrap_or_else(|| "backlog".to_owned()),
            is_triage: false,
            default: false,
            external_source: None,
            external_id: None,
        };
        match insert_state(client, &state).await {
            Ok(()) => {}
            Err(error) if is_duplicate_name(&error) => return Ok(duplicate_name()),
            Err(error) => return Err(map_write_error(error)),
        }
        render_state(ctx, &state, None)
    })
}

// ---------------------------------------------------------------------------
// partial_update
// ---------------------------------------------------------------------------

/// `StateViewSet.partial_update` (`plane/app/views/state/base.py:77-97`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let state_id = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `self.get_queryset().get(pk=pk)` (`:79`).
        let Some(state) = state_in_queryset(client, slug, project_id, state_id, user).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        let mut serializer = Serializer::new(ctx, true)?;
        let body = read_body(&mut serializer, true);
        if let Err(errors) = serializer.finish() {
            return Ok(errors.into_response());
        }

        let mut updated = state;
        if let Some(name) = body.name.clone() {
            updated.name = name;
        }
        if let Some(color) = body.color.clone() {
            updated.color = color;
        }
        if let Some(group) = body.group.clone() {
            updated.group = group;
        }
        if let Some(description) = body.description.clone() {
            updated.description = description;
        }
        if let Some(sequence) = body.sequence {
            updated.sequence = sequence;
        }
        // H19: the slug is recomputed on every save; the sequence is not.
        let saved = hooks::state::before_save(&updated.name, false, updated.sequence, None);
        updated.slug = saved.slug;
        updated.updated_at = write::now();
        updated.updated_by_id = Some(user);

        match update_state(client, &updated).await {
            Ok(()) => {}
            Err(error) if is_duplicate_name(&error) => return Ok(duplicate_name()),
            Err(error) => return Err(map_write_error(error)),
        }

        render_state(ctx, &updated, body.order)
    })
}

// ---------------------------------------------------------------------------
// mark_as_default
// ---------------------------------------------------------------------------

/// `StateViewSet.mark_as_default` (`plane/app/views/state/base.py:99-110`).
pub fn mark_as_default<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let state_id = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        invalidate_states_cache(ctx).await?;

        // `state = self.get_queryset().get(pk=pk)` (`:100`).
        let Some(state) = state_in_queryset(client, slug, project_id, state_id, user).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        // One statement over every live non-triage state of the project. The
        // `is_triage` boolean is not filtered; `updated_at` is not touched.
        client
            .execute(queries::STATE_MARK_DEFAULT.sql, &[&state.id, &project_id])
            .await
            .map_err(ViewError::from)?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// destroy
// ---------------------------------------------------------------------------

/// `StateViewSet.destroy` (`plane/app/views/state/base.py:112-135`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let state_id = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        invalidate_states_cache(ctx).await?;

        // `State.objects.get(is_triage=False, pk=pk, project_id=project_id,
        // workspace__slug=slug)` (`:113`): no membership and no archived filter.
        let row = client
            .query_opt(
                queries::STATE_FOR_DELETE.sql,
                &[&state_id, &project_id, &slug],
            )
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let state = State::from_row(&row, 0)
            .map_err(|error| ViewError::Unknown(format!("decoding the state row: {error}")))?;

        if state.default {
            return Ok(write::error_body(400, "Default state cannot be deleted"));
        }
        let issue_exists = client
            .query_opt(queries::STATE_ISSUE_EXISTS.sql, &[&state_id])
            .await
            .map_err(ViewError::from)?
            .is_some();
        if issue_exists {
            return Ok(write::error_body(
                400,
                "The state is not empty, only empty states can be deleted",
            ));
        }

        // H03: `deleted_at`, then a full save (H19's slug, H01's `updated_by`).
        let now = write::now();
        let mut deleted = state;
        deleted.deleted_at = Some(now);
        deleted.slug = hooks::state::slugify(&deleted.name);
        deleted.updated_at = now;
        deleted.updated_by_id = Some(user);
        update_state(client, &deleted)
            .await
            .map_err(map_write_error)?;
        write::publish_soft_delete_cascade(ctx, "db", "state", state_id).await?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `self.get_queryset().get(pk=…)` for the write actions: the same membership,
/// archived and triage filters as `GET …/states/{pk}/`.
async fn state_in_queryset(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    state_id: Uuid,
    member_id: Uuid,
) -> Result<Option<State>, ViewError> {
    let row = client
        .query_opt(
            queries::STATE_IN_PROJECT.sql,
            &[&slug, &project_id, &member_id, &state_id],
        )
        .await
        .map_err(ViewError::from)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(State::from_row(&row, 0).map_err(|error| {
        ViewError::Unknown(format!("decoding the state row: {error}"))
    })?))
}

async fn insert_state(
    client: &deadpool_postgres::Client,
    state: &State,
) -> Result<(), tokio_postgres::Error> {
    let statement = client.prepare_cached(queries::STATE_INSERT.sql).await?;
    client
        .execute(
            &statement,
            &[
                &state.created_at,
                &state.updated_at,
                &state.created_by_id,
                &state.updated_by_id,
                &state.deleted_at,
                &state.id,
                &state.project_id,
                &state.workspace_id,
                &state.name,
                &state.description,
                &state.color,
                &state.slug,
                &state.sequence,
                &state.group,
                &state.is_triage,
                &state.default,
                &state.external_source,
                &state.external_id,
            ],
        )
        .await
        .map(|_| ())
}

async fn update_state(
    client: &deadpool_postgres::Client,
    state: &State,
) -> Result<(), tokio_postgres::Error> {
    let statement = client.prepare_cached(queries::STATE_UPDATE.sql).await?;
    client
        .execute(
            &statement,
            &[
                &state.created_at,
                &state.updated_at,
                &state.created_by_id,
                &state.updated_by_id,
                &state.deleted_at,
                &state.id,
                &state.project_id,
                &state.workspace_id,
                &state.name,
                &state.description,
                &state.color,
                &state.slug,
                &state.sequence,
                &state.group,
                &state.is_triage,
                &state.default,
                &state.external_source,
                &state.external_id,
            ],
        )
        .await
        .map(|_| ())
}

/// `IntegrityError` handling of `create`/`partial_update`
/// (`plane/app/views/state/base.py:63-74`, `:86-96`): a duplicate name is the
/// 400 `{"name": "The state name is already taken"}`; anything else is the
/// family's IntegrityError mapping.
fn is_duplicate_name(error: &tokio_postgres::Error) -> bool {
    if error.code() != Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION) {
        return false;
    }
    error
        .as_db_error()
        .and_then(|error| error.detail())
        .is_some_and(|detail| detail.contains("already exists"))
}

fn map_write_error(error: tokio_postgres::Error) -> ViewError {
    match error.code() {
        Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION)
        | Some(&tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION) => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

fn duplicate_name() -> ViewResponse {
    ViewResponse::raw(
        400,
        APPLICATION_JSON,
        plane_query::json::Json::object(vec![(
            "name",
            plane_query::json::Json::string("The state name is already taken"),
        )])
        .to_drf(),
    )
}

/// `@invalidate_cache(path="workspaces/:slug/states/", url_params=True,
/// user=False)` (`plane/utils/cache.py:54-88`): the decorator runs before the
/// view and deletes `workspaces/<slug>/states/`.
async fn invalidate_states_cache(ctx: &ViewCtx<'_>) -> Result<(), ViewError> {
    cache::invalidate_cache(ctx, Some("workspaces/:slug/states/"), true, false, false).await?;
    Ok(())
}

/// `StateSerializer(instance).data`, 200. `order` is present only when the
/// request supplied it (the serializer field is a declared non-model field).
fn render_state(
    ctx: &ViewCtx<'_>,
    state: &State,
    order: Option<f64>,
) -> Result<ViewResponse, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let serializer = StateSerializer {
        id: state.id,
        project_id: state.project_id,
        workspace_id: state.workspace_id,
        name: &state.name,
        color: &state.color,
        group: &state.group,
        default: state.default,
        description: &state.description,
        sequence: state.sequence,
        order: match order {
            Some(order) => FieldValue::value(order),
            None => FieldValue::absent(),
        },
    };
    let body =
        plane_serializers::render(&serializer, &plane_serializers::RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}
