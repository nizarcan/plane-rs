// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Workflow states on `/api/v1/` (routes 312/313).
//!
//! Python: `plane/api/views/state.py:40-311` and
//! `plane/api/serializers/state.py:14-48`.
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | `pr/states/` | `GET` | `StateListCreateAPIEndpoint.get` (`:155-165`) |
//! | same | `POST` | `.post` (`:86-134`) |
//! | `pr/states/{state_id}/` | `GET` | `StateDetailAPIEndpoint.get` (`:211-222`) |
//! | same | `PATCH` | `.patch` (`:283-311`) |
//! | same | `DELETE` | `.delete` (`:236-260`) |
//!
//! ## What differs from the app surface
//!
//! * `StateSerializer` is `fields="__all__"` with nine read-only fields, so it
//!   accepts `is_triage`, `default`, `sequence` and `external_*` (the app's
//!   serializer has an `order` field and not these) — the generated table in
//!   `write/generated/api_state_serializer.rs` is exactly this serializer;
//! * the create answers **200**, not 201 (`:120`);
//! * `StateSerializer.save` clears the project's other default states in the
//!   same request when the body set `default` truthy (`serializers/state.py:27-33`);
//! * the PATCH loads the state with `State.objects.get(workspace__slug,
//!   project_id, pk)` — no membership and no `archived_at` filter (`:289`);
//! * the DELETE refuses a state any issue references, archived/draft included
//!   (`:251-257` — `Issue.objects` is the soft-deletion manager only).
//!
//! ## Bugs ported as they are
//!
//! * `StateSerializer.validate` rejects `group="triage"` on **both** verbs, so
//!   a PATCH cannot move a state into triage (`serializers/state.py:22-24`);
//! * the create's `except IntegrityError` re-looks-up the row by the **raw**
//!   body name (`:123-127`), so a name that only collides after trimming leaves
//!   `state = None` and `str(state.id)` raises `AttributeError` → 500;
//! * the create's 409 also fires when `external_id`/`external_source` are
//!   truthy but the *lookup* finds nothing, because `exists()` and `.first()`
//!   are separate queries; the `.first()` miss is the same 500.

use deadpool_postgres::Client;
use http::Method;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks;
use plane_db::models::state::State;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::generated::api_state::StateSerializer;
use plane_serializers::generated::api_user::UserLiteSerializer;
use plane_serializers::generated::api_workspace::WorkspaceLiteSerializer;
use plane_serializers::{Expansion, Expansions};
use std::collections::HashSet;
use uuid::Uuid;

use plane_http_app::assets::workspace_write::{body_get, truthy};
use plane_http_app::common::{client, lookup, render::RequestRender};
use plane_http_app::projects::lite;
use plane_http_app::states::queries as app_queries;
use plane_http_app::write::{self, Serializer};

use crate::common::render::expansions;
use crate::pyvalue::python_str;
use crate::states_queries as queries;

/// Register both classes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.state.StateListCreateAPIEndpoint")?
        .method(Method::GET, list)?
        .method(Method::POST, create)?;
    registry
        .api_view("plane.api.views.state.StateDetailAPIEndpoint")?
        .method(Method::GET, retrieve)?
        .method(Method::PATCH, patch)?
        .method(Method::DELETE, destroy)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// `StateListCreateAPIEndpoint.get` (`plane/api/views/state.py:155-165`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let member = member_id(ctx)?;
        let client = client(ctx)?;

        let (states, total, window, next, prev) = crate::projects::fetch_page(
            ctx,
            client,
            app_queries::STATES_BY_PROJECT.sql,
            &[&slug, &project_id, &member],
            queries::STATES_COUNT_BY_PROJECT.sql,
            &[&slug, &project_id, &member],
            1000,
            |row| State::from_row(row, 0).map_err(ViewError::from),
        )
        .await?;

        let requested = ctx.expand().unwrap_or_default();
        let prepared = RequestRender::from_ctx(ctx);
        let body = render_states(ctx, client, &states, &requested, &prepared).await?;
        Ok(crate::projects::page_envelope(
            total,
            &window,
            &next,
            &prev,
            states.len(),
            body,
        ))
    })
}

/// `StateDetailAPIEndpoint.get` (`plane/api/views/state.py:211-222`): honours
/// `fields=` and `expand=`.
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let state_id =
            lookup::uuid_kwarg("state_id", ctx.kwarg("state_id").ok_or(ViewError::Key)?)?;
        let member = member_id(ctx)?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(app_queries::STATE_IN_PROJECT.sql)
            .await?;
        let Some(row) = client
            .query_opt(&statement, &[&slug, &project_id, &member, &state_id])
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let state = State::from_row(&row, 0)?;

        let requested = ctx.expand().unwrap_or_default();
        let prepared = RequestRender::from_ctx(ctx);
        let body = render_states(
            ctx,
            client,
            std::slice::from_ref(&state),
            &requested,
            &prepared,
        )
        .await?;
        // `Response(serializer.data)`: one object, not an array.
        let array = body;
        let object = array[1..array.len() - 1].to_vec();
        Ok(ViewResponse::raw(200, APPLICATION_JSON, object))
    })
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// `StateListCreateAPIEndpoint.post` (`plane/api/views/state.py:86-134`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user = member_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        let serializer = Serializer::new(ctx, false)?;
        let validated = match validate(serializer, client).await? {
            Ok(validated) => validated,
            Err(response) => return Ok(response),
        };

        // The external-id 409 (`:95-117`).
        if let Some(response) = external_conflict(
            client,
            ctx.kwarg("slug").ok_or(ViewError::Key)?,
            project_id,
            None,
            data,
        )
        .await?
        {
            return Ok(response);
        }

        // `State.objects.create(..., project_id=…)`: H19's sequence and slug,
        // then `ProjectBaseModel.save` reads the project's workspace.
        let Some(row) = client
            .query_opt(app_queries::STATE_PROJECT_WORKSPACE.sql, &[&project_id])
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = row.get(0);
        let max_row = client
            .query_one(app_queries::STATE_SEQUENCE_MAX.sql, &[&project_id])
            .await?;
        let max_sequence: Option<f64> = max_row.get(0);

        let name = validated.name.clone().unwrap_or_default();
        let saved = hooks::state::before_save(
            &name,
            true,
            validated.sequence.unwrap_or(65535.0),
            max_sequence,
        );
        let group = validated
            .group
            .clone()
            .unwrap_or_else(|| "backlog".to_owned());
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
            description: validated.description.clone().unwrap_or_default(),
            color: validated.color.clone().unwrap_or_default(),
            slug: saved.slug,
            sequence: saved.sequence,
            group,
            is_triage: validated.is_triage.unwrap_or(false),
            default: validated.default.unwrap_or(false),
            external_source: validated.external_source.clone().flatten(),
            external_id: validated.external_id.clone().flatten(),
        };
        if let Err(error) = insert_state(client, &state).await {
            if is_integrity(&error) {
                let raw_name = body_get(data, "name")
                    .as_ref()
                    .map(python_str)
                    .unwrap_or_default();
                let lookup = client
                    .prepare_cached(queries::STATE_BY_NAME_FIRST.sql)
                    .await?;
                let Some(row) = client
                    .query_opt(
                        &lookup,
                        &[
                            &ctx.kwarg("slug").ok_or(ViewError::Key)?,
                            &project_id,
                            &raw_name,
                        ],
                    )
                    .await?
                else {
                    return Err(ViewError::Unknown(
                        "AttributeError: 'NoneType' object has no attribute 'id'".to_owned(),
                    ));
                };
                let existing = State::from_row(&row, 0)?;
                return conflict(
                    "State with the same name already exists in the project",
                    existing.id,
                );
            }
            return Err(ViewError::from(error));
        }

        // `StateSerializer.save`'s default-clearing UPDATE
        // (`serializers/state.py:31-33`).
        if validated.default == Some(true) {
            clear_other_defaults(client, project_id, state.id).await?;
        }

        render_state(ctx, &state, 200)
    })
}

/// `StateDetailAPIEndpoint.patch` (`plane/api/views/state.py:283-311`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let state_id =
            lookup::uuid_kwarg("state_id", ctx.kwarg("state_id").ok_or(ViewError::Key)?)?;
        let user = member_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        // `State.objects.get(workspace__slug, project_id, pk)` (`:289`).
        let statement = client.prepare_cached(queries::STATE_BY_ID.sql).await?;
        let Some(row) = client
            .query_opt(&statement, &[&slug, &project_id, &state_id])
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let mut state = State::from_row(&row, 0)?;

        let serializer = Serializer::new(ctx, true)?;
        let validated = match validate(serializer, client).await? {
            Ok(validated) => validated,
            Err(response) => return Ok(response),
        };

        // `request.data.get("external_id") and
        // state.external_id != str(request.data.get("external_id")) and …`
        // (`:292-308`).
        if let Some(response) =
            external_conflict(client, slug, project_id, Some(&state), data).await?
        {
            return Ok(response);
        }

        if let Some(name) = validated.name.clone() {
            state.name = name;
        }
        if let Some(color) = validated.color.clone() {
            state.color = color;
        }
        if let Some(group) = validated.group.clone() {
            state.group = group;
        }
        if let Some(description) = validated.description.clone() {
            state.description = description;
        }
        if let Some(sequence) = validated.sequence {
            state.sequence = sequence;
        }
        if let Some(is_triage) = validated.is_triage {
            state.is_triage = is_triage;
        }
        if let Some(default) = validated.default {
            state.default = default;
        }
        if let Some(external_source) = validated.external_source.clone() {
            state.external_source = external_source;
        }
        if let Some(external_id) = validated.external_id.clone() {
            state.external_id = external_id;
        }

        // `State.save()`: H19's slug on every save, H01's audit columns.
        let saved = hooks::state::before_save(&state.name, false, state.sequence, None);
        state.slug = saved.slug;
        state.updated_at = write::now();
        state.updated_by_id = Some(user);
        match update_state(client, &state).await {
            Ok(()) => {}
            Err(error) => {
                return Err(match is_integrity(&error) {
                    true => ViewError::Integrity,
                    false => ViewError::from(error),
                });
            }
        }

        if validated.default == Some(true) {
            clear_other_defaults(client, project_id, state.id).await?;
        }
        render_state(ctx, &state, 200)
    })
}

/// `StateDetailAPIEndpoint.delete` (`plane/api/views/state.py:236-260`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let state_id =
            lookup::uuid_kwarg("state_id", ctx.kwarg("state_id").ok_or(ViewError::Key)?)?;
        let user = member_id(ctx)?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(app_queries::STATE_FOR_DELETE.sql)
            .await?;
        let Some(row) = client
            .query_opt(&statement, &[&state_id, &project_id, &slug])
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let state = State::from_row(&row, 0)?;

        if state.default {
            return Ok(write::error_body(400, "Default state cannot be deleted"));
        }
        let issue_exists = client
            .query_opt(app_queries::STATE_ISSUE_EXISTS.sql, &[&state_id])
            .await?
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
            .map_err(|error| match is_integrity(&error) {
                true => ViewError::Integrity,
                false => ViewError::from(error),
            })?;
        write::publish_soft_delete_cascade(ctx, "db", "state", state_id).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// The validated `StateSerializer` fields (`fields="__all__"` minus the nine
/// read-only ones).
#[derive(Debug, Default)]
struct Body {
    name: Option<String>,
    description: Option<String>,
    color: Option<String>,
    sequence: Option<f64>,
    group: Option<String>,
    is_triage: Option<bool>,
    default: Option<bool>,
    /// `Some(None)` is an explicit `null`.
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
}

async fn validate(
    mut serializer: Serializer<'_>,
    client: &Client,
) -> Result<Result<Body, ViewResponse>, ViewError> {
    plane_http_app::write::generated::api_state_serializer::validate(&mut serializer, client)
        .await?;

    // `StateSerializer.validate` (`plane/api/serializers/state.py:22-24`):
    // `group="triage"` is a non-field error, and no field error can suppress it
    // only when the body otherwise validates — DRF's `validate()` runs after
    // `to_internal_value` raises, so a field error wins.
    let validated = match serializer.finish() {
        Ok(validated) => validated,
        Err(errors) => return Ok(Err(errors.into_response())),
    };

    let body = Body {
        name: validated.text("name").map(str::to_owned),
        description: validated.text("description").map(str::to_owned),
        color: validated.text("color").map(str::to_owned),
        sequence: validated.float("sequence"),
        group: validated.text("group").map(str::to_owned),
        is_triage: validated.bool("is_triage"),
        default: validated.bool("default"),
        external_source: validated
            .contains("external_source")
            .then(|| validated.text("external_source").map(str::to_owned)),
        external_id: validated
            .contains("external_id")
            .then(|| validated.text("external_id").map(str::to_owned)),
    };
    if body.group.as_deref() == Some("triage") {
        return Ok(Err(write::field_string_error(
            "non_field_errors",
            "Cannot create triage state",
        )));
    }
    Ok(Ok(body))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The create/patch external-id guard (`:95-117`, `:292-308`).
async fn external_conflict(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    existing: Option<&State>,
    data: &plane_drf::parsers::Data,
) -> Result<Option<ViewResponse>, ViewError> {
    let Some(external_id) = body_get(data, "external_id").filter(truthy) else {
        return Ok(None);
    };
    let id_text = python_str(&external_id);
    // A create needs both members; a patch defaults `external_source` to the
    // instance's stored value.
    let source = match body_get(data, "external_source").filter(truthy) {
        Some(source) => python_str(&source),
        None => match existing {
            Some(state) => state.external_source.clone().unwrap_or_default(),
            None => return Ok(None),
        },
    };
    match existing {
        Some(state) if state.external_id.as_deref() == Some(id_text.as_str()) => Ok(None),
        Some(state) => {
            let statement = client
                .prepare_cached(queries::STATE_EXTERNAL_EXISTS_PATCH.sql)
                .await?;
            if client
                .query_opt(&statement, &[&project_id, &slug, &source, &id_text])
                .await?
                .is_none()
            {
                return Ok(None);
            }
            conflict(
                "State with the same external id and external source already exists",
                state.id,
            )
            .map(Some)
        }
        None => {
            let statement = client
                .prepare_cached(queries::STATE_EXTERNAL_EXISTS.sql)
                .await?;
            if client
                .query_opt(&statement, &[&project_id, &slug, &source, &id_text])
                .await?
                .is_none()
            {
                return Ok(None);
            }
            let statement = client
                .prepare_cached(queries::STATE_EXTERNAL_FIRST.sql)
                .await?;
            let Some(row) = client
                .query_opt(&statement, &[&slug, &project_id, &id_text, &source])
                .await?
            else {
                return Err(ViewError::Unknown(
                    "AttributeError: 'NoneType' object has no attribute 'id'".to_owned(),
                ));
            };
            let state = State::from_row(&row, 0)?;
            conflict(
                "State with the same external id and external source already exists",
                state.id,
            )
            .map(Some)
        }
    }
}

fn conflict(message: &str, id: Uuid) -> Result<ViewResponse, ViewError> {
    let mut body = PyDict::new();
    body.insert(PyStr::new("error"), Json::Str(PyStr::from(message)));
    body.insert(PyStr::new("id"), Json::Str(PyStr::from(id.to_string())));
    let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &Json::Dict(body))
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(ViewResponse::raw(409, APPLICATION_JSON, rendered))
}

async fn insert_state(client: &Client, state: &State) -> Result<(), tokio_postgres::Error> {
    let statement = client.prepare_cached(app_queries::STATE_INSERT.sql).await?;
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

async fn update_state(client: &Client, state: &State) -> Result<(), tokio_postgres::Error> {
    let statement = client.prepare_cached(app_queries::STATE_UPDATE.sql).await?;
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

/// `State.objects.filter(project_id=…).exclude(pk=…).update(default=False)`
/// (`serializers/state.py:31-33`).
async fn clear_other_defaults(
    client: &Client,
    project_id: Uuid,
    state_id: Uuid,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(queries::STATE_CLEAR_OTHER_DEFAULTS.sql)
        .await?;
    client
        .execute(&statement, &[&project_id, &state_id])
        .await?;
    Ok(())
}

/// `StateSerializer(state)` — the v1 read serializer.
fn serializer(state: &State) -> StateSerializer<'_> {
    StateSerializer {
        id: state.id,
        created_at: state.created_at,
        updated_at: state.updated_at,
        deleted_at: state.deleted_at,
        name: &state.name,
        description: &state.description,
        color: &state.color,
        slug: &state.slug,
        sequence: state.sequence,
        group: &state.group,
        is_triage: state.is_triage,
        default: state.default,
        external_source: state.external_source.as_deref(),
        external_id: state.external_id.as_deref(),
        created_by: state.created_by_id,
        updated_by: state.updated_by_id,
        project: state.project_id,
        workspace: state.workspace_id,
    }
}

/// `StateSerializer(state).data`, 200 — the create/patch response.
fn render_state(ctx: &ViewCtx<'_>, state: &State, status: u16) -> Result<ViewResponse, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let body = plane_serializers::render(
        &serializer(state),
        &plane_serializers::RenderCtx::full(&timezone),
    )
    .map_err(ViewError::from)?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// The list/detail `fields=`/`expand=` renderer.
///
/// `StateSerializer`'s expandable fields are `created_by`, `updated_by`,
/// `project` and `workspace` (the mapper in
/// `plane/api/serializers/base.py:98-113`). Every other requested name falls
/// back to `getattr(instance, "<name>_id", None)` (`:139`).
async fn render_states(
    _ctx: &ViewCtx<'_>,
    client: &Client,
    states: &[State],
    requested: &[&str],
    prepared: &RequestRender<'_>,
) -> Result<Vec<u8>, ViewError> {
    let serializers: Vec<StateSerializer<'_>> = states.iter().map(serializer).collect();
    if requested.is_empty() {
        return plane_http_app::common::render_list(&serializers, &prepared.render_ctx());
    }

    let mut workspace_ids: HashSet<Uuid> = HashSet::new();
    let mut project_ids: HashSet<Uuid> = HashSet::new();
    let mut user_ids: HashSet<Uuid> = HashSet::new();
    for state in states {
        if requested.contains(&"workspace") {
            workspace_ids.insert(state.workspace_id);
        }
        if requested.contains(&"project") {
            project_ids.insert(state.project_id);
        }
        if requested.contains(&"created_by") {
            user_ids.extend(state.created_by_id);
        }
        if requested.contains(&"updated_by") {
            user_ids.extend(state.updated_by_id);
        }
    }
    let mut workspaces = lite::load_workspaces(client, &workspace_ids).await?;
    let users = lite::load_users(client, &user_ids).await?;
    let projects = crate::labels::load_projects(client, &project_ids).await?;
    let mut asset_ids = lite::asset_ids(
        &std::collections::HashMap::<Uuid, lite::LiteProject>::new(),
        &workspaces,
        &users,
    );
    asset_ids.extend(
        projects
            .values()
            .filter_map(|project| project.cover_image_asset_id),
    );
    let urls = lite::load_asset_urls(client, &asset_ids).await?;
    lite::apply_workspaces(&mut workspaces, &urls)?;

    let mut out = Vec::with_capacity(states.len() * 512 + 2);
    out.push(b'[');
    for (index, (state, value)) in states.iter().zip(&serializers).enumerate() {
        if index > 0 {
            out.push(b',');
        }
        let workspace = workspaces
            .get(&state.workspace_id)
            .map(|row| WorkspaceLiteSerializer {
                name: &row.name,
                slug: &row.slug,
                id: row.id,
            });
        let project = projects
            .get(&state.project_id)
            .map(crate::labels::project_serializer);
        let user = |id: Option<Uuid>| {
            id.and_then(|id| users.get(&id))
                .map(|user| UserLiteSerializer {
                    id: user.id,
                    first_name: &user.first_name,
                    last_name: &user.last_name,
                    email: user.email.as_deref(),
                    avatar: &user.avatar,
                    avatar_url: user.avatar_url.as_deref(),
                    display_name: &user.display_name,
                })
        };
        let created_by = user(state.created_by_id);
        let updated_by = user(state.updated_by_id);

        let mut resolved: Vec<Expansion<'_>> = Vec::new();
        if let Some(workspace) = &workspace {
            resolved.push(Expansion {
                name: "workspace",
                value: plane_serializers::DrfValue::nested(workspace),
            });
        }
        if let Some(project) = &project {
            resolved.push(Expansion {
                name: "project",
                value: plane_serializers::DrfValue::nested(project),
            });
        }
        for (name, user) in [("created_by", &created_by), ("updated_by", &updated_by)] {
            if let Some(user) = user {
                resolved.push(Expansion {
                    name,
                    value: plane_serializers::DrfValue::nested(user),
                });
            }
        }
        let values = expansions(requested, &resolved, |name| match name {
            "project" => Some(state.project_id),
            "workspace" => Some(state.workspace_id),
            "created_by" => state.created_by_id,
            "updated_by" => state.updated_by_id,
            _ => None,
        });
        let render_ctx = prepared
            .render_ctx()
            .with_expansions(Expansions::new(requested, &values));
        out.extend_from_slice(
            &plane_serializers::render(value, &render_ctx).map_err(ViewError::from)?,
        );
    }
    out.push(b']');
    Ok(out)
}

/// 23 = SQLSTATE 23*.
fn is_integrity(error: &tokio_postgres::Error) -> bool {
    error
        .code()
        .is_some_and(|code| code.code().starts_with("23"))
}

fn member_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}
