// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/favorite.py`: the write verbs of the caller's
//! favourites and favourite folders.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 212 `POST /api/workspaces/{slug}/user-favorites/` | `POST` | `favorite.py:38-76` | [`post`] |
//! | 212 | `PATCH` | the signature 500 | [`patch`] |
//! | 212 | `DELETE` | the signature 500 | [`delete`] |
//! | 213 `PATCH …/user-favorites/{favorite_id}/` | `PATCH` | `favorite.py:78-87` | [`patch`] |
//! | 213 | `DELETE` | `favorite.py:89-93` | [`delete`] |
//! | 213 | `POST` | the signature 500 | [`post`] |
//!
//! ## The three signature 500s
//!
//! The row's `methods` map is one class for both routes, so Django binds
//! `WorkspaceFavoriteEndpoint.patch(self, request, slug, favorite_id)` on 212
//! (no `favorite_id` kwarg → missing positional argument) and
//! `post(self, request, slug)` on 213 (an extra `favorite_id` kwarg →
//! unexpected keyword argument). Both are a `TypeError` that
//! `BaseAPIView.handle_exception` renders as
//! `500 {"error": "Something went wrong please try again later"}`
//! (`plane/app/views/base.py:296-348`), *after* `@allow_permission` ran — so a
//! guest is 403 and an anonymous caller 401, and only ADMIN/MEMBER see the 500.
//! `PUT` is not in `http_method_names` and stays the runtime's 405.
//!
//! ## Order, and no transactions
//!
//! The view has no `transaction.atomic` and `ATOMIC_REQUESTS` is unset, so
//! every statement commits on its own: a failure after the write leaves it
//! committed (`plans/FAVORITES.md` §4). The statement order is the Python's:
//!
//! * POST: `Workspace.objects.get` (`:41`) → `request.data.get` (`:44`) → the
//!   early-return lookup (`:45-55`) → serializer (`:58-59`) → the entity gate
//!   (`:61-67`) → `save()` (`:68-72`) → render from the in-memory instance;
//! * PATCH: lookup (`:80`) → `request.data` (`:82`) → validate (`:83-84`) →
//!   `save()` (`:85`) → render;
//! * DELETE: lookup (`:91`) → `delete(soft=False)` (`:92`). The body is **never**
//!   parsed: a malformed DELETE body is still a 204/404.
//!
//! ## The two quirks that are upstream, not porting choices
//!
//! * `if request.data.get("entity_identifier")` (`:44`) returns the stored row
//!   (200) **before** the serializer runs, so a repeat POST is a 200, not the
//!   duplicate 400, and every other body key is ignored. A child row matches
//!   too: the filter has no `parent__isnull` predicate.
//! * `parent` and `is_folder` are plain writable fields (`SERIALIZERS.md`
//!   1982-1984) and nothing validates that the parent is a folder, the caller's
//!   own row, in the same workspace, or a different row: `parent` is any live
//!   `UserFavorite` (`PrimaryKeyRelatedField(queryset=UserFavorite.objects)`),
//!   and the destroy collector then cascades to it
//!   (`plane/db/models/favorite.py:25-31`, [`crate::write::favorite`]).
//!
//! ## `project_id` is raw
//!
//! POST reads `request.data.get("project_id")` (`:60`), passes it to the gate
//! and to `serializer.save(project_id=…)`; `WorkspaceBaseModel.save` then
//! re-homes the row to the project's workspace
//! (`plane/db/models/workspace.py:192-195`) and H25 aggregates over that
//! workspace. A project in another workspace therefore writes a favourite
//! outside the URL workspace whenever the entity type has no gate (`folder` and
//! every unknown type). A malformed value is Django's `ValidationError` (400
//! `{"error": "Please provide valid detail"}`), a well-formed missing one the
//! family 404.

use plane_db::hooks;
use plane_db::models::favorite::UserFavorite;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use uuid::Uuid;

use super::favorite::render_one;
use super::write_favorite_queries as queries;
use crate::common::{self, jobs};
use crate::views::write_support::{Body, body_get, django_uuid, integrity, is_integrity};
use crate::write::{self, Serializer, fields};

/// `plane.app.views.workspace.favorite.WorkspaceFavoriteEndpoint`.
const VIEW: &str = "plane.app.views.workspace.favorite.WorkspaceFavoriteEndpoint";

/// Register the write verbs of routes 212 and 213.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view(VIEW)?
        .method(http::Method::POST, post)?
        .method(http::Method::PATCH, patch)?
        .method(http::Method::DELETE, delete)?;
    Ok(())
}

/// `WorkspaceFavoriteEndpoint.post` (`favorite.py:38-76`).
fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // Route 213 binds `favorite_id` to a method whose signature lacks it.
        if ctx.kwarg("favorite_id").is_some() {
            return Err(ViewError::Unknown(
                "WorkspaceFavoriteEndpoint.post() got an unexpected keyword argument 'favorite_id'"
                    .to_string(),
            ));
        }
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let user = jobs::user_id(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`:41`) — before the body is
        // parsed, so a missing workspace is the 404 even for a malformed body.
        let workspace_id = workspace_by_slug(client, &slug).await?;

        // `request.data.get("entity_identifier")` (`:44`): a body that is not a
        // mapping is an `AttributeError` 500 here.
        let data = ctx.data()?;
        let identifier = body_get(data, "entity_identifier")?;
        if truthy(&identifier) {
            // The raw `entity_identifier` goes through Django's `UUIDField`
            // (`ValidationError` → 400 "Please provide valid detail"), and the
            // raw `entity_type` through `CharField`'s `str()`.
            let identifier = django_uuid(data, "entity_identifier")?
                .ok_or_else(|| ViewError::Unknown("a truthy value coerced to NULL".to_string()))?;
            let entity_type = char_prep(body_get(data, "entity_type")?);
            if let Some(favorite) = early_return(
                client,
                workspace_id,
                user,
                entity_type.as_deref(),
                identifier,
            )
            .await?
            {
                return render_one(ctx, &favorite).await;
            }
        }

        // `UserFavoriteSerializer(data=request.data)` (`:58-59`).
        let mut serializer = Serializer::new(ctx, false)?;
        crate::write::generated::user_favorite_serializer::validate(&mut serializer, client)
            .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `project_id = request.data.get("project_id", None)` (`:60`): the raw
        // value, coerced by the ORM wherever it is used.
        let project_id = django_uuid(data, "project_id")?;
        let entity_type = values.text("entity_type").unwrap_or_default();
        let entity_identifier = values.uuid("entity_identifier");

        // `get_favorite_entities(entity_type, user, project_id).get(
        // pk=…, workspace=workspace)` (`:61-67`): the visibility gate, and its
        // 404. `folder` and unknown types have no model and no gate.
        if let Some(gate) = gate_for(entity_type) {
            run_gate(
                client,
                gate,
                project_id,
                entity_identifier,
                workspace_id,
                user,
            )
            .await?;
        }

        // `serializer.save(user_id=…, workspace=workspace, project_id=…)`:
        // `WorkspaceBaseModel.save` re-homes the row from the project, then H25
        // aggregates over the resolved workspace, then one INSERT.
        let resolved_workspace = match project_id {
            Some(id) => project_workspace(client, id).await?,
            None => workspace_id,
        };
        let sequence =
            hooks::favorite::before_insert(client, resolved_workspace, values.float("sequence"))
                .await
                .map_err(ViewError::from)?;

        let now = write::now();
        let id = Uuid::new_v4();
        let favorite = UserFavorite {
            created_at: now,
            updated_at: now,
            // H01 on create: `created_by` is the caller, `updated_by` stays
            // NULL (`plane/db/models/base.py:23-44`).
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id,
            workspace_id: resolved_workspace,
            project_id,
            user_id: user,
            entity_type: entity_type.to_owned(),
            entity_identifier,
            name: values.text("name").map(str::to_owned),
            is_folder: values.bool("is_folder").unwrap_or(false),
            sequence,
            parent_id: values.uuid("parent"),
        };
        let statement = client
            .prepare_cached(queries::FAVORITE_INSERT.sql)
            .await
            .map_err(ViewError::from)?;
        let written = client
            .execute(
                &statement,
                &[
                    &favorite.created_at,
                    &favorite.updated_at,
                    &favorite.created_by_id,
                    &favorite.id,
                    &favorite.workspace_id,
                    &favorite.project_id,
                    &favorite.user_id,
                    &favorite.entity_type,
                    &favorite.entity_identifier,
                    &favorite.name,
                    &favorite.is_folder,
                    &favorite.sequence,
                    &favorite.parent_id,
                ],
            )
            .await;
        match written {
            Ok(_) => {}
            // `except IntegrityError: return 400 {"error": "Favorite already
            // exists"}` (`:75-76`). The partial unique index over live
            // `(entity_type, entity_identifier, user)` rows is the reachable
            // one (`db/models/favorite.py:35-41`); it is **not** the family's
            // `{"error": "The payload is not valid"}`.
            Err(error) if is_integrity(&error) => {
                return Ok(write::error_body(400, "Favorite already exists"));
            }
            Err(error) => return Err(ViewError::from(error)),
        }

        render_one(ctx, &favorite).await
    })
}

/// `WorkspaceFavoriteEndpoint.patch` (`favorite.py:78-87`).
fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // Route 212 does not pass the kwarg the signature requires.
        let Some(favorite_id) = ctx.kwarg("favorite_id") else {
            return Err(ViewError::Unknown(
                "WorkspaceFavoriteEndpoint.patch() missing 1 required positional argument: \
                 'favorite_id'"
                    .to_string(),
            ));
        };
        let favorite_id = crate::common::lookup::uuid_kwarg("favorite_id", favorite_id)?;
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let user = jobs::user_id(ctx)?;

        // The lookup is first (`:80`): a miss is the family 404 before the body
        // is parsed.
        let mut favorite = lookup(client, user, &slug, favorite_id).await?;

        // `data = {key: request.data[key] for key in ("name", "sequence",
        // "parent") if key in request.data}` (`:82`) — a plain dict, so the
        // serializer reads JSON semantics even for a form body.
        let body = patch_body(ctx.data()?)?;

        // `UserFavoriteSerializer(favorite, data=data, partial=True)`.
        let mut serializer = Serializer::from_value(ctx, &body, true);
        crate::write::generated::user_favorite_serializer::validate(&mut serializer, client)
            .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `ModelSerializer.update` assigns the validated keys, then
        // `instance.save()`.
        match values.get("name") {
            Some(write::Value::Text(name)) => favorite.name = Some(name.clone()),
            Some(write::Value::Null) => favorite.name = None,
            _ => {}
        }
        if let Some(sequence) = values.float("sequence") {
            favorite.sequence = sequence;
        }
        if values.is_null("parent") {
            favorite.parent_id = None;
        } else if let Some(parent) = values.uuid("parent") {
            favorite.parent_id = Some(parent);
        }

        // `WorkspaceBaseModel.save` re-derives the workspace from the project
        // (through `_base_manager`) and H01's update branch sets `updated_by`;
        // `auto_now` sets `updated_at`.
        if let Some(project_id) = favorite.project_id {
            favorite.workspace_id = project_workspace(client, project_id).await?;
        }
        favorite.updated_at = write::now();
        favorite.updated_by_id = Some(user);

        let statement = client
            .prepare_cached(queries::FAVORITE_UPDATE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &favorite.created_at,
                    &favorite.updated_at,
                    &favorite.created_by_id,
                    &favorite.updated_by_id,
                    &favorite.deleted_at,
                    &favorite.workspace_id,
                    &favorite.project_id,
                    &favorite.user_id,
                    &favorite.entity_type,
                    &favorite.entity_identifier,
                    &favorite.name,
                    &favorite.is_folder,
                    &favorite.sequence,
                    &favorite.parent_id,
                    &favorite.id,
                ],
            )
            .await
            .map_err(integrity)?;

        render_one(ctx, &favorite).await
    })
}

/// `WorkspaceFavoriteEndpoint.delete` (`favorite.py:89-93`).
fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // Route 212 does not pass the kwarg the signature requires.
        let Some(favorite_id) = ctx.kwarg("favorite_id") else {
            return Err(ViewError::Unknown(
                "WorkspaceFavoriteEndpoint.delete() missing 1 required positional argument: \
                 'favorite_id'"
                    .to_string(),
            ));
        };
        let favorite_id = crate::common::lookup::uuid_kwarg("favorite_id", favorite_id)?;
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let user = jobs::user_id(ctx)?;

        // `UserFavorite.objects.get(user=…, workspace__slug=slug, pk=…)`
        // (`:91`) — a miss is the family 404. The body is never parsed.
        let favorite = lookup(client, user, &slug, favorite_id).await?;

        // `favorite.delete(soft=False)` (`:92`) → Django's deletion collector
        // over the self-referential `parent` CASCADE, shared with the five
        // per-entity destroys.
        write::favorite::hard_delete(client, favorite.id).await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `Workspace.objects.get(slug=slug)` (`:41`) — the live row only.
async fn workspace_by_slug(
    client: &deadpool_postgres::Client,
    slug: &str,
) -> Result<Uuid, ViewError> {
    let statement = client
        .prepare_cached(queries::WORKSPACE_ID_BY_SLUG.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&slug])
        .await
        .map_err(ViewError::from)?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(row.get(0))
}

/// The `.first()` of POST's early return (`:45-50`).
async fn early_return(
    client: &deadpool_postgres::Client,
    workspace_id: Uuid,
    user: Uuid,
    entity_type: Option<&str>,
    entity_identifier: Uuid,
) -> Result<Option<UserFavorite>, ViewError> {
    let statement = client
        .prepare_cached(queries::FAVORITE_EARLY_RETURN.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(
            &statement,
            &[&workspace_id, &user, &entity_type, &entity_identifier],
        )
        .await
        .map_err(ViewError::from)?;
    match row {
        Some(row) => Ok(Some(UserFavorite::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

/// `UserFavorite.objects.get(user=…, workspace__slug=slug, pk=…)`
/// (`favorite.py:80,91`).
async fn lookup(
    client: &deadpool_postgres::Client,
    user: Uuid,
    slug: &str,
    favorite_id: Uuid,
) -> Result<UserFavorite, ViewError> {
    let statement = client
        .prepare_cached(queries::FAVORITE_LOOKUP.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&user, &slug, &favorite_id])
        .await
        .map_err(ViewError::from)?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(UserFavorite::from_row(&row, 0)?)
}

/// `self.project.workspace`, read through `Project._base_manager`
/// (`plane/db/models/workspace.py:192-195`). A missing row is the forward FK
/// descriptor's `Project.DoesNotExist`, i.e. the family 404.
async fn project_workspace(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
) -> Result<Uuid, ViewError> {
    let statement = client
        .prepare_cached(queries::PROJECT_WORKSPACE.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await
        .map_err(ViewError::from)?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(row.get(0))
}

/// `get_entity_model_and_serializer(entity_type)` (`serializers/favorite.py:57-67`).
#[derive(Debug, Clone, Copy)]
enum Gate {
    Cycle,
    Module,
    Issue,
    Project,
    Page,
    /// `view` with a `project_id`; without one it takes the workspace-view
    /// branch.
    View,
}

/// The entity types with a model — and therefore a gate. `folder` and every
/// unknown type (`intake` in the fixture) fall through to `(None, None)` and are
/// not checked at all.
fn gate_for(entity_type: &str) -> Option<Gate> {
    match entity_type {
        "cycle" => Some(Gate::Cycle),
        "module" => Some(Gate::Module),
        "issue" => Some(Gate::Issue),
        "project" => Some(Gate::Project),
        "page" => Some(Gate::Page),
        "view" => Some(Gate::View),
        _ => None,
    }
}

/// `get_favorite_entities(...).get(pk=…, workspace=workspace)` — `DoesNotExist`
/// is the family 404, and the row itself is discarded (`:65-67`).
async fn run_gate(
    client: &deadpool_postgres::Client,
    gate: Gate,
    project_id: Option<Uuid>,
    entity_identifier: Option<Uuid>,
    workspace_id: Uuid,
    user: Uuid,
) -> Result<(), ViewError> {
    let visible: bool = match gate {
        // The `view` branch depends on whether the favourite has a project
        // (`serializers/favorite.py:79-85`).
        Gate::View if project_id.is_none() => {
            let statement = client
                .prepare_cached(queries::ENTITY_GATE_WORKSPACE_VIEW.sql)
                .await
                .map_err(ViewError::from)?;
            client
                .query_one(&statement, &[&entity_identifier, &workspace_id, &user])
                .await
                .map_err(ViewError::from)?
                .get(0)
        }
        _ => {
            let query = match gate {
                Gate::Cycle => &queries::ENTITY_GATE_CYCLE,
                Gate::Module => &queries::ENTITY_GATE_MODULE,
                Gate::Issue => &queries::ENTITY_GATE_ISSUE,
                Gate::Project => &queries::ENTITY_GATE_PROJECT,
                Gate::Page => &queries::ENTITY_GATE_PAGE,
                Gate::View => &queries::ENTITY_GATE_VIEW,
            };
            let statement = client
                .prepare_cached(query.sql)
                .await
                .map_err(ViewError::from)?;
            client
                .query_one(
                    &statement,
                    &[&project_id, &entity_identifier, &workspace_id, &user],
                )
                .await
                .map_err(ViewError::from)?
                .get(0)
        }
    };
    if visible {
        Ok(())
    } else {
        Err(ViewError::ObjectDoesNotExist)
    }
}

/// `if request.data.get("entity_identifier")` (`:44`) — Python truthiness of
/// the raw body value, before any field coercion.
fn truthy(value: &Body<'_>) -> bool {
    match value {
        Body::Missing => false,
        Body::Form(text) => !text.is_empty(),
        Body::Json(Json::Null) => false,
        Body::Json(Json::Bool(flag)) => *flag,
        Body::Json(Json::Int(int)) => int.to_decimal_string() != "0",
        Body::Json(Json::Float(float)) => *float != 0.0,
        Body::Json(Json::Str(text)) => !text.as_wtf8().is_empty(),
        Body::Json(Json::List(items)) => !items.is_empty(),
        Body::Json(Json::Dict(members)) => !members.is_empty(),
    }
}

/// `CharField.get_prep_value` (`django/db/models/fields/__init__.py:1186-1190`):
/// `None`/absent stay `None`, everything else is `str(value)`.
fn char_prep(value: Body<'_>) -> Option<String> {
    match value {
        Body::Missing | Body::Json(Json::Null) => None,
        Body::Json(value) => Some(fields::python_str(fields::Raw::Json(value))),
        Body::Form(text) => Some(fields::python_str(fields::Raw::Text(text))),
    }
}

/// `data = {key: request.data[key] for key in ("name", "sequence", "parent")
/// if key in request.data}` (`:82`).
///
/// `request.data[key]` keeps the raw value, and the dict is a plain `dict`, so
/// `UserFavoriteSerializer(data=data)` reads JSON semantics even when the
/// request body was a form. `key in request.data` on a non-mapping is Python's
/// own behaviour:
///
/// * a `list` is an element-membership test — a hit is `list[key]`'s
///   `TypeError` (500); no hit is an empty dict (a 200 no-op);
/// * a `str` is a substring test, with the same two outcomes;
/// * `int`/`float`/`bool`/`None` are not iterable at all — `TypeError` (500).
fn patch_body(data: &Data) -> Result<Json, ViewError> {
    const KEYS: [&str; 3] = ["name", "sequence", "parent"];
    let mut body = PyDict::new();
    match data {
        Data::Json(Json::Dict(members)) => {
            for key in KEYS {
                if let Some(value) = members.get(key) {
                    body.insert(PyStr::new(key), value.clone());
                }
            }
        }
        Data::Form(query) => {
            // `QueryDict.__getitem__` is its last value for the key.
            for key in KEYS {
                if let Some(value) = query.get(key) {
                    body.insert(PyStr::new(key), Json::Str(PyStr::new(value)));
                }
            }
        }
        Data::Json(Json::List(items)) => {
            if items.iter().any(|item| match item {
                Json::Str(text) => text.as_str().is_some_and(|text| KEYS.contains(&text)),
                _ => false,
            }) {
                return Err(ViewError::Unknown(
                    "TypeError: list indices must be integers or slices, not str".to_string(),
                ));
            }
        }
        Data::Json(Json::Str(text)) => {
            if let Some(text) = text.as_str()
                && KEYS.iter().any(|key| text.contains(key))
            {
                return Err(ViewError::Unknown(
                    "TypeError: string indices must be integers, not 'str'".to_string(),
                ));
            }
        }
        Data::Json(other) => {
            return Err(ViewError::Unknown(format!(
                "TypeError: argument of type '{}' is not iterable",
                python_type_name(other)
            )));
        }
    }
    Ok(Json::Dict(body))
}

/// `type(value).__name__` for the `TypeError` above.
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

#[cfg(test)]
mod tests {
    use super::*;
    use plane_drf::request::QueryDict;
    use plane_serde::pyjson;

    fn json(text: &str) -> Json {
        pyjson::loads(text.as_bytes(), "utf-8").expect("json")
    }

    fn data(text: &str) -> Data {
        Data::Json(json(text))
    }

    /// `request.data.get("entity_identifier")`'s Python truthiness
    /// (`views/workspace/favorite.py:44`), recorded from CPython.
    #[test]
    fn truthiness_follows_python() {
        let empty_object = data("{}");
        assert!(!truthy(&Body::Missing));
        assert!(!truthy(
            &body_get(&empty_object, "absent").expect("missing")
        ));
        for (text, expected) in [
            ("{\"k\": null}", false),
            ("{\"k\": false}", false),
            ("{\"k\": true}", true),
            ("{\"k\": 0}", false),
            ("{\"k\": 1}", true),
            ("{\"k\": -1}", true),
            ("{\"k\": 0.0}", false),
            ("{\"k\": 2.5}", true),
            ("{\"k\": \"\"}", false),
            ("{\"k\": \"0\"}", true),
            ("{\"k\": []}", false),
            ("{\"k\": [0]}", true),
            ("{\"k\": {}}", false),
            ("{\"k\": {\"a\": 1}}", true),
        ] {
            assert_eq!(
                truthy(&body_get(&data(text), "k").expect("body")),
                expected,
                "{text}"
            );
        }
        let empty = Data::Form(QueryDict::parse("k=", None).expect("form"));
        assert!(!truthy(&body_get(&empty, "k").expect("form")));
        let filled = Data::Form(QueryDict::parse("k=0", None).expect("form"));
        assert!(truthy(&body_get(&filled, "k").expect("form")));
    }

    /// `data = {key: request.data[key] for key in ("name", "sequence", "parent")
    /// if key in request.data}` (`favorite.py:82`).
    #[test]
    fn the_patch_body_is_the_three_keys_the_view_rebuilds() {
        let object = patch_body(&data(
            r#"{"name": "n", "sequence": 2, "parent": "p", "is_folder": true}"#,
        ))
        .expect("object");
        assert_eq!(
            object,
            json(r#"{"name": "n", "sequence": 2, "parent": "p"}"#)
        );

        let form = Data::Form(
            QueryDict::parse("name=n&sequence=&parent=&is_folder=true", None).expect("form"),
        );
        assert_eq!(
            patch_body(&form).expect("form"),
            json(r#"{"name": "n", "sequence": "", "parent": ""}"#)
        );

        // `key in request.data` on a list is element equality: a hit is the
        // `list[key]` TypeError, a miss is an empty dict.
        assert!(patch_body(&data(r#"["name"]"#)).is_err());
        assert_eq!(patch_body(&data(r#"["x"]"#)).expect("list"), json("{}"));
        // ...and on a string it is a substring test.
        assert!(patch_body(&data(r#""xparenty""#)).is_err());
        assert_eq!(patch_body(&data(r#""xy""#)).expect("string"), json("{}"));
        // `int`/`float`/`bool`/`None` are not iterable.
        for body in ["3", "1.5", "true", "null"] {
            assert!(patch_body(&data(body)).is_err(), "{body}");
        }
    }
}
