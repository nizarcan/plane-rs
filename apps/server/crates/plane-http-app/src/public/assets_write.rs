// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The public asset writes: routes 255/256 (`EntityAssetEndpoint` POST/PATCH/
//! DELETE), 257 (`AssetRestoreEndpoint`) and 258 (`EntityBulkAssetEndpoint`),
//! `plane/space/views/asset.py`.
//!
//! | Route | Python |
//! | --- | --- |
//! | 255 `POST assets/v2/anchor/{anchor}/` | `post` (`:87-175`) |
//! | 255/256 `PATCH {…}/{pk}/` | `patch` (`:177-203`) |
//! | 255/256 `DELETE {…}/{pk}/` | `delete` (`:205-225`) |
//! | 257 `POST …/restore/{pk}/` | `AssetRestoreEndpoint.post` (`:231-250`) |
//! | 258 `POST …/{entity_id}/bulk/` | `EntityBulkAssetEndpoint.post` (`:256-301`) |
//!
//! Both `EntityAssetEndpoint` URL shapes share one method table, so `POST` on
//! the single-asset route and `PATCH`/`DELETE` on the collection raise
//! `TypeError` (a missing/extra positional argument) → the family 500, exactly
//! as the app's v2 asset views do.
//!
//! ## Upload
//!
//! Public uploads are comment images only: `entity_type` must be
//! `COMMENT_DESCRIPTION`, the MIME type one of the five images, and an
//! `entity_identifier` must be the caller's own `EXTERNAL` comment on this
//! board (`_is_own_board_comment`). The client's size is clamped into
//! `[1, FILE_SIZE_LIMIT]` and the **clamped** value is stored in both
//! `attributes["size"]` and `size` (`:115-161`). `int(size)` differs from the
//! app routes: `TypeError`/`ValueError` answer 400 `{"error": "Invalid size.",
//! "status": false}` here, not a 500.
//!
//! ## PATCH / DELETE / restore
//!
//! All three are `created_by=request.user`-scoped, and a miss is 404
//! `{"error": "The requested asset could not be found."}` — never a 403. PATCH
//! publishes `get_asset_object_metadata` when `storage_metadata` is falsy and
//! saves only `attributes`/`is_uploaded` (no `updated_at`); DELETE writes
//! `is_deleted`/`deleted_at`; restore clears them and looks the row up through
//! `FileAsset.all_objects`, so a soft-deleted asset is restorable.
//!
//! ## Bulk
//!
//! The comment must be the caller's own, `asset_ids` must be non-empty, and at
//! least one of the ids must resolve to a writable row (404 otherwise). The
//! rebind is one `UPDATE … SET comment_id` over the caller's
//! `COMMENT_DESCRIPTION` assets.

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_storage::{PresignPost, RequestInfo};
use uuid::Uuid;

use crate::assets::{support, write_queries as asset_queries};
use crate::common::client;
use crate::public::board;
use crate::public::queries;
use crate::write;

/// `FileAsset.EntityTypeContext.COMMENT_DESCRIPTION`
/// (`plane/db/models/asset.py:30`).
const COMMENT_DESCRIPTION: &str = "COMMENT_DESCRIPTION";

const ALLOWED_TYPES: [&str; 5] = [
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/jpg",
    "image/gif",
];

/// Register the endpoint verbs. `EntityAssetEndpoint` serves two URL shapes
/// with one method table (the Python `TypeError`s are branches inside the
/// handlers).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.space.views.asset.EntityAssetEndpoint")?
        .method(http::Method::POST, post)?
        .method(http::Method::PATCH, patch)?
        .method(http::Method::DELETE, delete)?;
    registry
        .api_view("plane.space.views.asset.AssetRestoreEndpoint")?
        .method(http::Method::POST, restore)?;
    registry
        .api_view("plane.space.views.asset.EntityBulkAssetEndpoint")?
        .method(http::Method::POST, bulk)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// EntityAssetEndpoint.post
// ---------------------------------------------------------------------------

/// `EntityAssetEndpoint.post` (`plane/space/views/asset.py:87-175`).
pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("pk").is_some() {
            return Err(type_error(
                "EntityAssetEndpoint.post() got an unexpected keyword argument 'pk'",
            ));
        }
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(board) = board::board_by_anchor(client, anchor).await? else {
            return Ok(write::error_body(404, "Project is not published"));
        };
        if !board.is_comments_enabled {
            return Ok(write::error_body(
                400,
                "Comments are not enabled for this project",
            ));
        }
        let data = ctx.data()?;
        ensure_mapping(data)?;

        let name = sanitized_name(body_get(data, "name").as_ref());
        let file_type = body_get(data, "type")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "image/jpeg".to_owned());
        let size = match public_size(data, ctx.settings.limits.file_size_limit) {
            Ok(size) => size,
            Err(response) => return Ok(response),
        };
        let entity_type = body_get(data, "entity_type")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let entity_identifier = match body_get(data, "entity_identifier") {
            Some(value) if truthy(&value) => Some(value),
            _ => None,
        };
        // `max(1, min(size, FILE_SIZE_LIMIT))` — written without
        // `i64::clamp`, which panics when the limit is below 1.
        let limit = ctx.settings.limits.file_size_limit;
        let size_limit = if size < limit { size } else { limit }.max(1);

        if entity_type != COMMENT_DESCRIPTION {
            return Ok(status_error(400, "Invalid entity type."));
        }
        if let Some(identifier) = &entity_identifier {
            let Some(comment_id) = uuid_of(identifier) else {
                return Err(ViewError::Validation);
            };
            if !board::is_own_board_comment(client, comment_id, &board, user).await? {
                return Ok(write::error_body(404, "Comment not found"));
            }
        }
        if !ALLOWED_TYPES.contains(&file_type.as_str()) {
            return Ok(status_error(
                400,
                "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files are allowed.",
            ));
        }

        let comment_id = match &entity_identifier {
            Some(value) => Some(uuid_of(value).ok_or(ViewError::Validation)?),
            None => None,
        };
        let asset_key = format!(
            "{}/{}-{}",
            board.workspace_id,
            Uuid::new_v4().simple(),
            name
        );
        let mut attributes = PyDict::new();
        attributes.insert(PyStr::new("name"), Json::Str(PyStr::from(name.as_str())));
        attributes.insert(
            PyStr::new("type"),
            Json::Str(PyStr::from(file_type.as_str())),
        );
        attributes.insert(PyStr::new("size"), Json::from(size_limit));

        let now = write::now();
        let asset = FileAsset {
            created_at: now,
            updated_at: now,
            id: Uuid::new_v4(),
            attributes: JsonText::from_json(&Json::Dict(attributes))?,
            asset: asset_key.clone(),
            created_by_id: Some(user),
            updated_by_id: None,
            workspace_id: Some(board.workspace_id),
            is_deleted: false,
            deleted_at: None,
            is_archived: false,
            comment_id,
            entity_type: Some(entity_type),
            external_id: None,
            external_source: None,
            is_uploaded: false,
            issue_id: None,
            page_id: None,
            project_id: board.project_id,
            size: size_limit as f64,
            storage_metadata: Some(JsonText::new("{}")),
            user_id: None,
            draft_issue_id: None,
            entity_identifier: None,
        };
        let statement = asset_queries::FILE_ASSET_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &asset.created_at,
                    &asset.updated_at,
                    &asset.id,
                    &asset.attributes,
                    &asset.asset,
                    &asset.created_by_id,
                    &asset.updated_by_id,
                    &asset.workspace_id,
                    &asset.is_deleted,
                    &asset.deleted_at,
                    &asset.is_archived,
                    &asset.comment_id,
                    &asset.entity_type,
                    &asset.external_id,
                    &asset.external_source,
                    &asset.is_uploaded,
                    &asset.issue_id,
                    &asset.page_id,
                    &asset.project_id,
                    &asset.size,
                    &asset.storage_metadata,
                    &asset.user_id,
                    &asset.draft_issue_id,
                    &asset.entity_identifier,
                ],
            )
            .await
            .map_err(integrity)?;

        let storage = support::storage(ctx.settings)?;
        let post = storage
            .request(request_info(ctx))
            .presign_post(PresignPost {
                key: &asset_key,
                file_type: Some(&file_type),
                file_size: size_limit,
                expiration: None,
            })
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let slug = super::workspace_slug(client, board.workspace_id).await?;
        let body = support::presign_upload_body(&asset, Some(&slug), &post)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// EntityAssetEndpoint.patch / delete
// ---------------------------------------------------------------------------

/// `EntityAssetEndpoint.patch` (`plane/space/views/asset.py:177-203`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(type_error(
                "EntityAssetEndpoint.patch() missing 1 required positional argument: 'pk'",
            ));
        };
        let pk = Uuid::parse_str(pk).map_err(|_| ViewError::Validation)?;
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(board) = board::board_by_anchor(client, anchor).await? else {
            return Ok(write::error_body(404, "Project is not published"));
        };
        let statement = queries::PUBLIC_ASSET_FOR_USER.prepare(client).await?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[&pk, &board.workspace_id, &board.project_id, &user],
            )
            .await?
        else {
            return Ok(write::error_body(
                404,
                "The requested asset could not be found.",
            ));
        };
        let mut asset = FileAsset::from_row(&row, 0)?;
        asset.is_uploaded = true;
        if !storage_metadata_truthy(&asset) {
            publish_metadata_task(ctx, asset.id).await?;
        }
        let data = ctx.data()?;
        ensure_mapping(data)?;
        if let Some(attributes) = body_get(data, "attributes") {
            asset.attributes = JsonText::from_json(&attributes)?;
        }
        let statement = queries::PUBLIC_ASSET_UPDATE_UPLOAD.prepare(client).await?;
        client
            .execute(
                &statement,
                &[&asset.attributes, &asset.is_uploaded, &asset.id],
            )
            .await
            .map_err(integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `EntityAssetEndpoint.delete` (`plane/space/views/asset.py:205-225`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(type_error(
                "EntityAssetEndpoint.delete() missing 1 required positional argument: 'pk'",
            ));
        };
        let pk = Uuid::parse_str(pk).map_err(|_| ViewError::Validation)?;
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(board) = board::published_board(client, anchor).await? else {
            return Ok(write::error_body(404, "Project is not published"));
        };
        let statement = queries::PUBLIC_ASSET_FOR_USER.prepare(client).await?;
        let Some(_row) = client
            .query_opt(
                &statement,
                &[&pk, &board.workspace_id, &board.project_id, &user],
            )
            .await?
        else {
            return Ok(write::error_body(
                404,
                "The requested asset could not be found.",
            ));
        };
        let statement = queries::PUBLIC_ASSET_DELETE.prepare(client).await?;
        client
            .execute(&statement, &[&write::now(), &pk])
            .await
            .map_err(integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `AssetRestoreEndpoint.post` (`plane/space/views/asset.py:231-250`).
pub fn restore<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let pk = support::uuid_kwarg(ctx, "pk")?;
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(board) = board::published_board(client, anchor).await? else {
            return Ok(write::error_body(404, "Project is not published"));
        };
        // `FileAsset.all_objects` — a soft-deleted row is found.
        let statement = queries::PUBLIC_ASSET_FOR_USER_ANY_STATE
            .prepare(client)
            .await?;
        let Some(_row) = client
            .query_opt(
                &statement,
                &[&pk, &board.workspace_id, &board.project_id, &user],
            )
            .await?
        else {
            return Ok(write::error_body(
                404,
                "The requested asset could not be found.",
            ));
        };
        let statement = queries::PUBLIC_ASSET_RESTORE.prepare(client).await?;
        client
            .execute(&statement, &[&pk])
            .await
            .map_err(integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `EntityBulkAssetEndpoint.post` (`plane/space/views/asset.py:256-301`).
pub fn bulk<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let entity_id = support::uuid_kwarg(ctx, "entity_id")?;
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(board) = board::published_board(client, anchor).await? else {
            return Ok(write::error_body(404, "Project is not published"));
        };
        if !board.is_comments_enabled {
            return Ok(write::error_body(
                400,
                "Comments are not enabled for this project",
            ));
        }
        if !board::is_own_board_comment(client, entity_id, &board, user).await? {
            return Ok(write::error_body(404, "Comment not found"));
        }
        let data = ctx.data()?;
        ensure_mapping(data)?;
        let asset_ids = match body_get(data, "asset_ids") {
            Some(value) if truthy(&value) => value,
            _ => return Ok(write::error_body(400, "No asset ids provided.")),
        };
        let mut ids = Vec::new();
        match &asset_ids {
            // `id__in` over a string iterates its characters, each of which
            // fails UUID parsing → Django's `ValidationError` → 400.
            Json::List(items) => {
                for item in items {
                    let Some(id) = uuid_of(item) else {
                        return Err(ViewError::Validation);
                    };
                    ids.push(id);
                }
            }
            Json::Str(text) => {
                if !text.as_str().is_some_and(str::is_empty) {
                    return Err(ViewError::Validation);
                }
            }
            Json::Dict(map) => {
                if !map.is_empty() {
                    return Err(ViewError::Validation);
                }
            }
            // An int/float/bool is not iterable → TypeError → 500.
            Json::Int(_) | Json::Float(_) | Json::Bool(_) | Json::Null => {
                return Err(ViewError::Unknown(
                    "TypeError: 'int' object is not iterable".to_owned(),
                ));
            }
        }

        let statement = queries::PUBLIC_ASSET_PICK_FOR_USER.prepare(client).await?;
        let Some(_row) = client
            .query_opt(
                &statement,
                &[&ids, &board.workspace_id, &board.project_id, &user],
            )
            .await?
        else {
            return Ok(write::error_body(
                404,
                "The requested asset could not be found.",
            ));
        };
        let statement = queries::PUBLIC_ASSET_REBIND.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &entity_id,
                    &ids,
                    &board.workspace_id,
                    &board.project_id,
                    &user,
                ],
            )
            .await
            .map_err(integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Body helpers (`request.data` idioms, without the app's 500-on-size rule)
// ---------------------------------------------------------------------------

fn ensure_mapping(data: &Data) -> Result<(), ViewError> {
    match data {
        Data::Json(Json::Dict(_)) | Data::Form(_) => Ok(()),
        Data::Json(Json::Null) => Err(type_error(
            "AttributeError: 'NoneType' object has no attribute 'get'",
        )),
        Data::Json(Json::List(_)) => Err(type_error(
            "AttributeError: 'list' object has no attribute 'get'",
        )),
        Data::Json(other) => Err(type_error(&format!(
            "AttributeError: '{}' object has no attribute 'get'",
            json_type_name(other)
        ))),
    }
}

fn body_get(data: &Data, key: &str) -> Option<Json> {
    match data {
        Data::Json(Json::Dict(map)) => map.get(key).cloned(),
        Data::Form(query) => query.get(key).map(|value| Json::Str(PyStr::from(value))),
        Data::Json(_) => None,
    }
}

fn sanitized_name(value: Option<&Json>) -> String {
    value
        .and_then(Json::as_str)
        .and_then(support::sanitize_filename)
        .unwrap_or_else(|| "unnamed".to_owned())
}

/// `int(request.data.get("size", settings.FILE_SIZE_LIMIT))` behind
/// `except (TypeError, ValueError)` (`asset.py:104-110`) — 400
/// `{"error": "Invalid size.", "status": false}`.
fn public_size(data: &Data, default: i64) -> Result<i64, ViewResponse> {
    let Some(value) = body_get(data, "size") else {
        return Ok(default);
    };
    let invalid = || status_error(400, "Invalid size.");
    match &value {
        Json::Int(number) => {
            let parsed = number.to_decimal_string().parse::<i64>();
            match parsed {
                Ok(value) => Ok(value),
                Err(_) => {
                    if number.to_decimal_string().starts_with('-') {
                        // `int()` itself succeeds; `min`/`max` then clamp, so a
                        // negative value is valid and becomes 1.
                        Ok(i64::MIN)
                    } else {
                        // A value above i64: `int()` succeeds and `min` picks
                        // the limit, so this is not an error either.
                        Ok(i64::MAX)
                    }
                }
            }
        }
        Json::Bool(flag) => Ok(i64::from(*flag)),
        Json::Float(number) => Ok(*number as i64),
        Json::Str(text) => {
            let Some(text) = text.as_str() else {
                return Err(invalid());
            };
            match plane_core::py::int(text) {
                Ok(value) => Ok(value),
                Err(plane_core::py::PyIntError::Invalid) => Err(invalid()),
                // `OverflowError` is not in the except tuple → 500.
                Err(plane_core::py::PyIntError::Overflow) => Err(status_error(
                    500,
                    "OverflowError: int too large to convert to int",
                )),
            }
        }
        Json::Null | Json::List(_) | Json::Dict(_) => Err(invalid()),
    }
}

fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(flag) => *flag,
        Json::Int(number) => number.as_i64().is_none_or(|value| value != 0),
        Json::Float(number) => *number != 0.0,
        Json::Str(text) => text.as_str().is_some_and(|value| !value.is_empty()),
        Json::List(items) => !items.is_empty(),
        Json::Dict(map) => !map.is_empty(),
    }
}

fn uuid_of(value: &Json) -> Option<Uuid> {
    value.as_str().and_then(|text| Uuid::parse_str(text).ok())
}

fn json_type_name(value: &Json) -> &'static str {
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

/// A `TypeError`/`AttributeError` the family handler renders as a 500.
fn type_error(message: &str) -> ViewError {
    ViewError::Unknown(message.to_owned())
}

/// `Response({"error": message, "status": False}, status)` — the three bodies
/// that carry `"status": false` (`asset.py:107-146`).
fn status_error(status: u16, message: &str) -> ViewResponse {
    let mut body = PyDict::new();
    body.insert(PyStr::new("error"), Json::Str(PyStr::from(message)));
    body.insert(PyStr::new("status"), Json::Bool(false));
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Drf, &Json::Dict(body))
        .unwrap_or_else(|_| Vec::new());
    ViewResponse::raw(status, APPLICATION_JSON, bytes)
}

fn storage_metadata_truthy(asset: &FileAsset) -> bool {
    let Some(text) = asset.storage_metadata.as_ref() else {
        return false;
    };
    match serde_json::from_str::<serde_json::Value>(text.as_str()) {
        Ok(serde_json::Value::Null) => false,
        Ok(serde_json::Value::Bool(value)) => value,
        Ok(serde_json::Value::Number(value)) => value.as_f64().is_some_and(|value| value != 0.0),
        Ok(serde_json::Value::String(value)) => !value.is_empty(),
        Ok(serde_json::Value::Array(items)) => !items.is_empty(),
        Ok(serde_json::Value::Object(map)) => !map.is_empty(),
        Err(_) => true,
    }
}

fn request_info<'a>(ctx: &'a ViewCtx<'a>) -> RequestInfo<'a> {
    RequestInfo {
        scheme: if ctx.ctx.is_secure { "https" } else { "http" },
        host: ctx.ctx.header("host").unwrap_or_default(),
    }
}

/// `get_asset_object_metadata.delay(asset_id=str(asset_id))` (`asset.py:196-197`).
async fn publish_metadata_task(ctx: &ViewCtx<'_>, asset_id: Uuid) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::GET_ASSET_OBJECT_METADATA;
    // The public view's call is positional (`asset.py:197`), unlike the v2
    // routes' `asset_id=` kwarg form.
    let message =
        plane_jobs::celery::tasks::get_asset_object_metadata_positional(asset_id.to_string());
    crate::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::from_message(&message),
    )
    .await
    .map(|_| ())
}

fn integrity(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}
