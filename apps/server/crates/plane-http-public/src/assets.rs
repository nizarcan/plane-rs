// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Assets on `/api/v1/` (routes 273–278).
//!
//! Python: `plane/api/views/asset.py:50-668`.
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | `/assets/user-assets/` | `POST` | `UserAssetEndpoint.post` (`:112-176`) |
//! | `/assets/user-assets/{asset_id}/` | `PATCH` | `.patch` (`:205-222`) |
//! | same | `DELETE` | `.delete` (`:233-245`) |
//! | `/assets/user-assets/server/` | `POST` | `UserServerAssetEndpoint.post` (`:284-349`) |
//! | `/assets/user-assets/{asset_id}/server/` | `PATCH` | `.patch` (`:361-378`) |
//! | same | `DELETE` | `.delete` (`:389-402`) |
//! | `ws/assets/` | `POST` | `GenericAssetEndpoint.post` (`:514-618`) |
//! | `ws/assets/{asset_id}/` | `GET` | `.get` (`:427-483`) |
//! | same | `PATCH` | `.patch` (`:640-668`) |
//!
//! The two user-asset classes are byte-identical apart from their docstrings
//! (`asset.py:50-245` vs `:248-402`), so one handler serves both, exactly like
//! `plane-http-app`'s `assets::user_write` does for the `/api/assets/v2/`
//! pair.
//!
//! ## What differs from the app surface's `v2.py` handlers
//!
//! * `PATCH` on a user asset does **not** run the entity save
//!   (`v2.py:212-231` has none either: only the app's `UserAssetsV2Endpoint`
//!   patch *does* call `entity_asset_save`, and `api/views/asset.py:205-222`
//!   does not). It sets `is_uploaded`, publishes the metadata task when
//!   `storage_metadata` is falsy, replaces `attributes` and writes the two
//!   columns.
//! * `DELETE` does not invalidate a cache: `entity_asset_delete`
//!   (`asset.py:62-75`) only nulls the user's FK and `user.save()`s.
//! * the generic upload validates the project and the caller's project
//!   membership before it dedups (`:550-569`).
//! * `GET` answers its own 404/403/400 bodies inside one `try` whose
//!   `except Exception` renders `{"error": "Internal server error"}` — so a
//!   database failure, a dangling comment and a storage failure are all that
//!   500.
//! * `PATCH` catches only `FileAsset.DoesNotExist`: a non-mapping body is the
//!   family 500 (an `AttributeError` on `request.data.get`).
//!
//! ## Bugs ported as they are
//!
//! * the generic upload stores `entity_type=ISSUE_ATTACHMENT` without an
//!   issue, so its own `asset_url` renders `.../issues/None/attachments/…`
//!   (`plane/db/models/asset.py:93-94`);
//! * a project with `intake_view` off is not checked here (no intake logic);
//! * the user-asset upload's `int()` of `size` is unchecked, so a string
//!   value is the family 500 (`:121`).

use deadpool_postgres::Client;
use http::Method;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_core::json::Object;
use plane_db::JsonText;
use plane_db::models::asset::FileAsset;
use plane_db::models::user::User;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_storage::{PresignGet, PresignPost};
use uuid::Uuid;

use plane_http_app::assets::entities;
use plane_http_app::assets::support;
use plane_http_app::assets::user_write::save_user;
use plane_http_app::assets::workspace_write::{
    IMAGE_TYPES, body_get, ensure_mapping, fk_uuid, insert_asset, new_asset, publish_metadata_task,
    request_info, sanitized_name, size_arg, storage_metadata_truthy, truthy,
};
use plane_http_app::assets::{write_a_queries, write_c_queries};
use plane_http_app::common::{client, lookup};

use crate::assets_queries as queries;
use crate::pyvalue::python_str;

/// Register the three classes' verbs.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    // Both user-asset URL shapes share one method table, so both classes get
    // the same handler per verb; the handlers do not branch on the shape
    // because every shape supplies the same kwargs.
    registry
        .api_view("plane.api.views.asset.UserAssetEndpoint")?
        .method(Method::POST, user_post)?
        .method(Method::PATCH, user_patch)?
        .method(Method::DELETE, user_delete)?;
    registry
        .api_view("plane.api.views.asset.UserServerAssetEndpoint")?
        .method(Method::POST, user_post)?
        .method(Method::PATCH, user_patch)?
        .method(Method::DELETE, user_delete)?;
    registry
        .api_view("plane.api.views.asset.GenericAssetEndpoint")?
        .method(Method::POST, generic_post)?
        .method(Method::GET, generic_get)?
        .method(Method::PATCH, generic_patch)?;
    Ok(())
}

/// `FileAsset.EntityTypeContext` values the user-asset upload accepts
/// (`asset.py:128`, `:301`).
const USER_ENTITY_TYPES: [&str; 2] = ["USER_AVATAR", "USER_COVER"];

/// `settings.ATTACHMENT_MIME_TYPES` (`plane/settings/common.py:505-593`).
const ATTACHMENT_MIME_TYPES: &[&str] = &[
    "application/pdf",
    "application/zip",
    "application/x-zip-compressed",
    "application/x-rar-compressed",
    "application/x-tar",
    "application/gzip",
    "application/x-7z-compressed",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.presentation",
    "application/json",
    "application/xml",
    "application/octet-stream",
    "application/rtf",
    "application/x-httpd-php",
    "application/x-sh",
    "application/xhtml+xml",
    "application/x-msdownload",
    "application/x-msi",
    "application/vnd.ms-outlook",
    "application/vnd.apple.keynote",
    "application/vnd.apple.numbers",
    "application/vnd.apple.pages",
    "application/epub+zip",
    "application/vnd.google-apps.document",
    "application/vnd.google-apps.spreadsheet",
    "application/vnd.google-apps.presentation",
    "application/x-ndjson",
    "audio/mpeg",
    "audio/ogg",
    "audio/wav",
    "audio/webm",
    "audio/aac",
    "audio/flac",
    "audio/mp4",
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/svg+xml",
    "image/bmp",
    "image/tiff",
    "image/x-icon",
    "image/avif",
    "image/heic",
    "text/plain",
    "text/csv",
    "text/html",
    "text/css",
    "text/javascript",
    "text/markdown",
    "text/xml",
    "text/rtf",
    "text/x-python",
    "text/x-java-source",
    "text/x-c",
    "text/x-cpp",
    "text/x-csharp",
    "text/x-ruby",
    "text/x-go",
    "text/x-rust",
    "text/x-swift",
    "text/x-kotlin",
    "text/x-typescript",
    "text/x-scss",
    "text/x-sass",
    "text/x-less",
    "text/x-sql",
    "text/x-yaml",
    "text/x-toml",
    "text/x-ini",
    "video/mp4",
    "video/mpeg",
    "video/quicktime",
    "video/webm",
    "video/x-msvideo",
    "video/x-matroska",
    "video/ogg",
    "video/3gpp",
    "video/x-flv",
];

// ---------------------------------------------------------------------------
// 273/275 — the user-asset upload
// ---------------------------------------------------------------------------

/// `UserAssetEndpoint.post` / `UserServerAssetEndpoint.post`
/// (`api/views/asset.py:112-176`, `:284-349`).
pub fn user_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;
        ensure_mapping(data)?;

        // `name = sanitize_filename(request.data.get("name")) or "unnamed"`,
        // `type = request.data.get("type", "image/jpeg")`,
        // `size = int(request.data.get("size", FILE_SIZE_LIMIT))`,
        // `entity_type = request.data.get("entity_type", False)` (`:119-122`).
        let name = sanitized_name(body_get(data, "name").as_ref());
        let mime = body_get(data, "type");
        let size = size_arg(data, ctx.settings.limits.file_size_limit)?;
        let entity_type = body_get(data, "entity_type").unwrap_or(Json::Bool(false));

        let size_limit = size.min(ctx.settings.limits.file_size_limit);

        let Some(entity_type) = entity_type.as_str() else {
            return invalid_entity_type();
        };
        if !USER_ENTITY_TYPES.contains(&entity_type) {
            return invalid_entity_type();
        }

        let file_type = match mime.as_ref() {
            None => "image/jpeg",
            Some(Json::Str(value)) => value.as_str().unwrap_or_default(),
            // Any other JSON value is not in the allow-list either.
            Some(_) => return invalid_user_file_type(),
        };
        if !IMAGE_TYPES.contains(&file_type) {
            return invalid_user_file_type();
        }

        // `asset_key = f"{uuid.uuid4().hex}-{name}"` (`:152`).
        let asset_key = format!("{}-{}", Uuid::new_v4().simple(), name);

        let mut asset = new_asset(&asset_key, &name, file_type, size_limit, entity_type, user)?;
        // `FileAsset.objects.create(..., user=request.user, created_by=...)`.
        asset.user_id = Some(user);
        insert_asset(client, &asset).await?;

        let storage = support::storage(ctx.settings)?;
        let post = storage
            .request(request_info(ctx))
            .presign_post(PresignPost {
                key: &asset_key,
                file_type: Some(file_type),
                file_size: size_limit,
                expiration: None,
            })
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        // USER_AVATAR/USER_COVER render the static URL (`asset.py:85-91`).
        let body = support::presign_upload_body(&asset, None, &post)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `UserAssetEndpoint.patch` / `UserServerAssetEndpoint.patch`
/// (`api/views/asset.py:205-222`, `:361-378`).
pub fn user_patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let Some(asset) = load_for_user(client, asset_id, user).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        // `asset.is_uploaded = True` is in memory only; the UPDATE below
        // writes it beside `attributes`.
        if !storage_metadata_truthy(&asset) {
            publish_metadata_task(ctx, asset_id).await?;
        }

        // The first `request.data` access of this method: a non-mapping body is
        // an `AttributeError` **after** the metadata publish.
        let data = ctx.data()?;
        ensure_mapping(data)?;
        let attributes = match body_get(data, "attributes") {
            Some(Json::Null) => None,
            Some(value) => Some(JsonText::from_json(&value)?),
            None => Some(asset.attributes.clone()),
        };
        let statement = client
            .prepare_cached(write_a_queries::FILE_ASSET_SET_UPLOAD_STATE.sql)
            .await?;
        client
            .execute(&statement, &[&asset_id, &attributes])
            .await
            .map_err(plane_http_app::assets::workspace_write::integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `UserAssetEndpoint.delete` / `UserServerAssetEndpoint.delete`
/// (`api/views/asset.py:233-245`, `:389-402`).
pub fn user_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let Some(asset) = load_for_user(client, asset_id, user).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        // `asset.deleted_at = timezone.now()` is assigned before the entity
        // delete (`:240-243`), so the value is fixed even though nothing rolls
        // back.
        let deleted_at = plane_http_app::write::now();
        entity_asset_delete(client, &asset).await?;
        let statement = client
            .prepare_cached(write_a_queries::FILE_ASSET_SET_DELETED.sql)
            .await?;
        client
            .execute(&statement, &[&asset_id, &deleted_at])
            .await
            .map_err(plane_http_app::assets::workspace_write::integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `UserAssetEndpoint.entity_asset_delete` (`api/views/asset.py:62-75`): the
/// user's FK is nulled and the user row is full-saved (H30).
async fn entity_asset_delete(client: &Client, asset: &FileAsset) -> Result<(), ViewError> {
    let column = match asset.entity_type.as_deref() {
        Some("USER_AVATAR") => "avatar_asset_id",
        Some("USER_COVER") => "cover_image_asset_id",
        _ => return Ok(()),
    };
    // `User.objects.get(id=asset.user_id)`: a NULL id matches nothing.
    let Some(user_id) = asset.user_id else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let statement = client
        .prepare_cached(plane_http_app::users_me::queries::USER_BY_ID.sql)
        .await?;
    let Some(row) = client.query_opt(&statement, &[&user_id]).await? else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let mut user = User::from_row(&row, 0)?;
    if column == "avatar_asset_id" {
        user.avatar_asset_id = None;
    } else {
        user.cover_image_asset_id = None;
    }
    save_user(client, &user).await
}

/// `FileAsset.objects.get(id=asset_id, user_id=request.user.id)`.
async fn load_for_user(
    client: &Client,
    asset_id: Uuid,
    user_id: Uuid,
) -> Result<Option<FileAsset>, ViewError> {
    let statement = client
        .prepare_cached(write_c_queries::FILE_ASSET_BY_PK_FOR_USER.sql)
        .await?;
    let row = client.query_opt(&statement, &[&asset_id, &user_id]).await?;
    row.as_ref()
        .map(|row| FileAsset::from_row(row, 0))
        .transpose()
        .map_err(ViewError::from)
}

// ---------------------------------------------------------------------------
// 277/278 — the generic (issue-attachment) asset
// ---------------------------------------------------------------------------

/// The outcome of [`generic_get_body`] that the view's `try` turns into a
/// response.
enum Guard {
    /// `Workspace.DoesNotExist` (`asset.py:474-475`).
    WorkspaceMissing,
    /// `FileAsset.DoesNotExist` (`asset.py:476-477`).
    AssetMissing,
    /// The `except Exception` branch (`asset.py:478-483`).
    Other(ViewError),
}

/// `GenericAssetEndpoint.get` (`api/views/asset.py:427-483`).
pub fn generic_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        match generic_get_body(ctx).await {
            Ok(response) => Ok(response),
            Err(Guard::WorkspaceMissing) => {
                support::json_object(404, &Object::new().field("error", "Workspace not found"))
            }
            Err(Guard::AssetMissing) => {
                support::json_object(404, &Object::new().field("error", "Asset not found"))
            }
            Err(Guard::Other(_error)) => {
                support::json_object(500, &Object::new().field("error", "Internal server error"))
            }
        }
    })
}

async fn generic_get_body(ctx: &ViewCtx<'_>) -> Result<ViewResponse, Guard> {
    let slug = support::kwarg(ctx, "slug").map_err(Guard::Other)?;
    let asset_id = support::uuid_kwarg(ctx, "asset_id").map_err(Guard::Other)?;
    let user = user_id(ctx).map_err(Guard::Other)?;
    let client = client(ctx).map_err(Guard::Other)?;

    // `Workspace.objects.get(slug=slug)` (`:435`).
    let workspace_statement = client
        .prepare_cached(plane_http_app::common::queries::WORKSPACE_BY_SLUG.sql)
        .await
        .map_err(|error| Guard::Other(error.into()))?;
    let Some(workspace_row) = client
        .query_opt(&workspace_statement, &[&slug])
        .await
        .map_err(|error| Guard::Other(error.into()))?
    else {
        return Err(Guard::WorkspaceMissing);
    };
    let workspace = plane_db::models::workspace::Workspace::from_row(&workspace_row, 0)
        .map_err(|error| Guard::Other(error.into()))?;

    // `FileAsset.objects.get(id=asset_id, workspace_id=workspace.id,
    // is_deleted=False)` (`:438`).
    let asset = load_generic(client, asset_id, workspace.id)
        .await
        .map_err(Guard::Other)?
        .ok_or(Guard::AssetMissing)?;

    // `asset.is_project_accessible_to(request.user)` (`:441-445`).
    if !accessible(ctx, client, user, &asset)
        .await
        .map_err(Guard::Other)?
    {
        return entities::no_access().map_err(Guard::Other);
    }

    // `if not asset.is_uploaded:` (`:448-452`).
    if !asset.is_uploaded {
        return support::json_object(400, &Object::new().field("error", "Asset not yet uploaded"))
            .map_err(Guard::Other);
    }

    // `S3Storage(...).generate_presigned_url(..., disposition="inline",
    // content_type=asset.attributes.get("type"))` (`:456-462`).
    let attributes = attributes_of(&asset).map_err(Guard::Other)?;
    let name = attributes_member(&attributes, "name");
    let mime = attributes_member(&attributes, "type");
    let storage = support::storage(ctx.settings).map_err(Guard::Other)?;
    let signed_url = storage
        .request(request_info(ctx))
        .presign_get(PresignGet {
            key: &asset.asset,
            expiration: None,
            disposition: "inline",
            filename: name.as_deref().filter(|value| !value.is_empty()),
            content_type: mime.as_deref(),
        })
        .await
        .map_err(|error| Guard::Other(ViewError::Unknown(error.to_string())))?;

    // `{"asset_id", "asset_url", "asset_name", "asset_type"}` with the two
    // attribute members as `get(key, "")` returns them (`:464-472`).
    let mut body = PyDict::new();
    body.insert(
        PyStr::new("asset_id"),
        Json::Str(PyStr::from(asset.id.to_string())),
    );
    body.insert(PyStr::new("asset_url"), Json::Str(PyStr::from(signed_url)));
    body.insert(
        PyStr::new("asset_name"),
        attributes_get(&attributes, "name"),
    );
    body.insert(
        PyStr::new("asset_type"),
        attributes_get(&attributes, "type"),
    );
    let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &Json::Dict(body))
        .map_err(|error| Guard::Other(ViewError::Unknown(error.to_string())))?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, rendered))
}

/// `GenericAssetEndpoint.post` (`api/views/asset.py:514-618`).
pub fn generic_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;
        ensure_mapping(data)?;

        // `name`, `type`, `size = int(...)` (`:520-522`), in that order.
        let name = body_get(data, "name")
            .as_ref()
            .and_then(Json::as_str)
            .and_then(support::sanitize_filename);
        let mime = body_get(data, "type");
        let size = size_arg(data, ctx.settings.limits.file_size_limit)?;
        let project_raw = body_get(data, "project_id");
        let external_id = body_get(data, "external_id");
        let external_source = body_get(data, "external_source");

        // `if not name or not size:` → 400 (`:527-532`).
        if name.is_none() || size == 0 {
            return Ok(required_fields());
        }
        let name = name.unwrap_or_default();
        let size_limit = size.min(ctx.settings.limits.file_size_limit);

        // `if not type or type not in settings.ATTACHMENT_MIME_TYPES` (`:537-542`).
        let file_type = match mime.as_ref() {
            Some(Json::Str(value)) => value.as_str().unwrap_or_default(),
            _ => "",
        };
        if file_type.is_empty() || !ATTACHMENT_MIME_TYPES.contains(&file_type) {
            return invalid_file_type();
        }

        // `Workspace.objects.get(slug=slug)` (`:545`): a miss is the family 404.
        let workspace =
            lookup::workspace_by_slug(client, slug)
                .await
                .map_err(|error| match error {
                    plane_http_app::common::lookup::Error::NotFound => {
                        ViewError::ObjectDoesNotExist
                    }
                    plane_http_app::common::lookup::Error::InvalidUuid { .. } => {
                        ViewError::Validation
                    }
                    plane_http_app::common::lookup::Error::Db(error) => ViewError::from(error),
                })?;

        // `if not project_id:` → 400 (`:550-554`).
        let Some(project_value) = project_raw.as_ref().filter(|value| truthy(value)) else {
            return Ok(project_id_required());
        };
        // `Project.objects.filter(id=project_id, workspace_id=…)`: Django's
        // `UUIDField` coercion raises its `ValidationError` for a non-UUID.
        let Some(project_id) = fk_uuid(project_value)? else {
            return Ok(project_id_required());
        };
        let exists = client
            .prepare_cached(write_c_queries::PROJECT_EXISTS_IN_WORKSPACE.sql)
            .await?;
        if client
            .query_opt(&exists, &[&project_id, &workspace.id])
            .await?
            .is_none()
        {
            return support::json_object(
                404,
                &Object::new()
                    .field("error", "Project not found.")
                    .field("status", false),
            );
        }
        let member = client
            .prepare_cached(write_c_queries::PROJECT_MEMBER_ACTIVE_EXISTS.sql)
            .await?;
        if client
            .query_opt(&member, &[&user, &workspace.id, &project_id])
            .await?
            .is_none()
        {
            return support::json_object(
                403,
                &Object::new()
                    .field("error", "You don't have access to this project.")
                    .field("status", false),
            );
        }

        // `asset_key = f"{workspace.id}/{uuid.uuid4().hex}-{name}"` (`:572`).
        let asset_key = format!("{}/{}-{}", workspace.id, Uuid::new_v4().simple(), name);

        // The dedup (`:575-592`) only when both members are truthy.
        let external_source_text = external_source
            .as_ref()
            .filter(|value| truthy(value))
            .map(python_str);
        let external_id_text = external_id
            .as_ref()
            .filter(|value| truthy(value))
            .map(python_str);
        if let (Some(source), Some(external)) = (&external_source_text, &external_id_text) {
            let statement = client
                .prepare_cached(queries::GENERIC_ASSET_BY_EXTERNAL.sql)
                .await?;
            if let Some(row) = client
                .query_opt(&statement, &[&workspace.id, &project_id, source, external])
                .await?
            {
                let existing = FileAsset::from_row(&row, 0)?;
                let asset_url = plane_http_app::common::asset_url::file_asset_url(
                    existing.id,
                    existing.entity_type.as_deref(),
                    Some(&workspace.slug),
                    existing.project_id,
                    existing.issue_id,
                )?;
                let mut body = PyDict::new();
                body.insert(
                    PyStr::new("message"),
                    Json::Str(PyStr::from(
                        "Asset with same external id and source already exists",
                    )),
                );
                body.insert(
                    PyStr::new("asset_id"),
                    Json::Str(PyStr::from(existing.id.to_string())),
                );
                body.insert(
                    PyStr::new("asset_url"),
                    asset_url
                        .as_deref()
                        .map_or(Json::Null, |value| Json::Str(PyStr::from(value))),
                );
                let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &Json::Dict(body))?;
                return Ok(ViewResponse::raw(409, APPLICATION_JSON, rendered));
            }
        }

        // `FileAsset.objects.create(..., entity_type=ISSUE_ATTACHMENT)`
        // (`:595-605`): `created_by` is set, `user` is not.
        let mut asset = new_asset(
            &asset_key,
            &name,
            file_type,
            size_limit,
            "ISSUE_ATTACHMENT",
            user,
        )?;
        asset.workspace_id = Some(workspace.id);
        asset.project_id = Some(project_id);
        asset.external_source = external_source_text;
        asset.external_id = external_id_text;
        insert_asset(client, &asset).await?;

        let storage = support::storage(ctx.settings)?;
        let post = storage
            .request(request_info(ctx))
            .presign_post(PresignPost {
                key: &asset_key,
                file_type: Some(file_type),
                file_size: size_limit,
                expiration: None,
            })
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let body = support::presign_upload_body(&asset, Some(&workspace.slug), &post)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `GenericAssetEndpoint.patch` (`api/views/asset.py:640-668`).
pub fn generic_patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `try: FileAsset.objects.get(id, workspace__slug, is_deleted=False)`
        // (`:648`).
        let statement = client
            .prepare_cached(queries::GENERIC_ASSET_BY_SLUG.sql)
            .await?;
        let Some(row) = client.query_opt(&statement, &[&asset_id, &slug]).await? else {
            return support::json_object(404, &Object::new().field("error", "Asset not found"));
        };
        let mut asset = FileAsset::from_row(&row, 0)?;

        // `if not asset.is_project_accessible_to(request.user)` (`:651-655`).
        if !accessible(ctx, client, user, &asset).await? {
            return entities::no_access();
        }

        // `asset.is_uploaded = request.data.get("is_uploaded", asset.is_uploaded)`
        // (`:658`): the body is read here, so a non-mapping body is the
        // family 500 before the `save()`.
        let data = ctx.data()?;
        ensure_mapping(data)?;
        let requested = body_get(data, "is_uploaded");
        // `asset.is_uploaded = request.data.get("is_uploaded", asset.is_uploaded)`
        // assigns without validation; the `BooleanField.to_python` conversion
        // is `save()`'s, i.e. after the metadata publish below.
        asset.is_uploaded = match &requested {
            None => asset.is_uploaded,
            Some(value) => bool_prep_or_default(value),
        };

        // `if not asset.storage_metadata: get_asset_object_metadata.delay(...)`
        // (`:661-662`).
        if !storage_metadata_truthy(&asset) {
            publish_metadata_task(ctx, asset_id).await?;
        }
        let is_uploaded = match &requested {
            None => asset.is_uploaded,
            Some(value) => bool_prep(value)?,
        };

        // `asset.save(update_fields=["is_uploaded"])` (`:664`).
        let update = client
            .prepare_cached(queries::FILE_ASSET_SET_IS_UPLOADED.sql)
            .await?;
        client
            .execute(&update, &[&asset_id, &is_uploaded])
            .await
            .map_err(plane_http_app::assets::workspace_write::integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The route's `request.user.id`.
fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}

/// `Response({"error": "Invalid entity type.", "status": False}, 400)`
/// (`asset.py:128-132`, `:301-305`).
fn invalid_entity_type() -> Result<ViewResponse, ViewError> {
    entities::invalid_entity_type()
}

/// `Response({"error": "Invalid file type. Only JPEG and PNG files are
/// allowed.", "status": False}, 400)` (`asset.py:142-149`, `:315-322`).
fn invalid_user_file_type() -> Result<ViewResponse, ViewError> {
    support::json_object(
        400,
        &Object::new()
            .field(
                "error",
                "Invalid file type. Only JPEG and PNG files are allowed.",
            )
            .field("status", false),
    )
}

/// `Response({"error": "Invalid file type.", "status": False}, 400)`
/// (`asset.py:538-542`).
fn invalid_file_type() -> Result<ViewResponse, ViewError> {
    support::json_object(
        400,
        &Object::new()
            .field("error", "Invalid file type.")
            .field("status", false),
    )
}

/// `Response({"error": "Name and size are required fields.", "status": False},
/// 400)` (`asset.py:528-532`).
fn required_fields() -> ViewResponse {
    support::json_object(
        400,
        &Object::new()
            .field("error", "Name and size are required fields.")
            .field("status", false),
    )
    .unwrap_or_else(|_| ViewResponse::raw(400, APPLICATION_JSON, Vec::new()))
}

/// `Response({"error": "Project id is required.", "status": False}, 400)`
/// (`asset.py:550-554`).
fn project_id_required() -> ViewResponse {
    support::json_object(
        400,
        &Object::new()
            .field("error", "Project id is required.")
            .field("status", false),
    )
    .unwrap_or_else(|_| ViewResponse::raw(400, APPLICATION_JSON, Vec::new()))
}

/// `GenericAssetEndpoint.get`'s lookup.
async fn load_generic(
    client: &Client,
    asset_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<FileAsset>, ViewError> {
    let statement = client
        .prepare_cached(queries::GENERIC_ASSET_BY_ID.sql)
        .await?;
    let row = client
        .query_opt(&statement, &[&asset_id, &workspace_id])
        .await?;
    row.as_ref()
        .map(|row| FileAsset::from_row(row, 0))
        .transpose()
        .map_err(ViewError::from)
}

/// `FileAsset.is_project_accessible_to(user)` for one row.
async fn accessible(
    ctx: &ViewCtx<'_>,
    client: &Client,
    user_id: Uuid,
    asset: &FileAsset,
) -> Result<bool, ViewError> {
    let _ = ctx;
    let flags = support::accessible_assets(client, user_id, std::slice::from_ref(asset)).await?;
    Ok(flags.first().copied().unwrap_or(false))
}

/// `asset.attributes` as the Python sees it: a dict, or an `AttributeError`
/// that the `GET`'s `except Exception` renders as the 500.
fn attributes_of(asset: &FileAsset) -> Result<Json, ViewError> {
    let value = plane_serde::pyjson::loads(asset.attributes.as_str().as_bytes(), "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    if matches!(value, Json::Dict(_)) {
        Ok(value)
    } else {
        Err(ViewError::Unknown(
            "AttributeError: 'str' object has no attribute 'get'".to_owned(),
        ))
    }
}

/// `attributes.get(key)` when the member is a string.
fn attributes_member(attributes: &Json, key: &str) -> Option<String> {
    let Json::Dict(map) = attributes else {
        return None;
    };
    map.get(key).and_then(Json::as_str).map(str::to_owned)
}

/// `attributes.get(key, "")`: the raw member, or the empty string.
fn attributes_get(attributes: &Json, key: &str) -> Json {
    let Json::Dict(map) = attributes else {
        return Json::Str(PyStr::new(""));
    };
    map.get(key)
        .cloned()
        .unwrap_or_else(|| Json::Str(PyStr::new("")))
}

/// The in-memory assignment's value: Python stores whatever the body carried
/// and only `save()` converts it.
fn bool_prep_or_default(value: &Json) -> bool {
    match value {
        Json::Bool(value) => *value,
        _ => false,
    }
}

/// `BooleanField.get_prep_value` (`django/db/models/fields/__init__.py`,
/// `BooleanField.to_python`): `True`/`False` compare equal to `1`/`0`, the
/// six strings are the documented ones, `None` reaches the column as NULL
/// (a NOT NULL violation, the family's 400) and everything else is Django's
/// `ValidationError` → the family's 400 `Please provide valid detail`.
fn bool_prep(value: &Json) -> Result<bool, ViewError> {
    match value {
        Json::Null => Err(ViewError::Integrity),
        Json::Bool(value) => Ok(*value),
        Json::Int(value) => match value.to_decimal_string().as_str() {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(ViewError::Validation),
        },
        Json::Float(value) if *value == 0.0 => Ok(false),
        Json::Float(value) if *value == 1.0 => Ok(true),
        Json::Str(value) => match value.as_str() {
            Some("t" | "True" | "1") => Ok(true),
            Some("f" | "False" | "0") => Ok(false),
            _ => Err(ViewError::Validation),
        },
        _ => Err(ViewError::Validation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_attachment_mime_list_is_the_setting() {
        // Spot-check the boundaries of `settings.ATTACHMENT_MIME_TYPES`
        // (`plane/settings/common.py:505-593`).
        assert!(ATTACHMENT_MIME_TYPES.contains(&"application/pdf"));
        assert!(ATTACHMENT_MIME_TYPES.contains(&"image/svg+xml"));
        assert!(ATTACHMENT_MIME_TYPES.contains(&"video/mp4"));
        assert!(ATTACHMENT_MIME_TYPES.contains(&"text/x-rust"));
        assert!(!ATTACHMENT_MIME_TYPES.contains(&"image/jpg"));
        assert!(!ATTACHMENT_MIME_TYPES.contains(&"text/x-unknown"));
        // 89 entries in the setting; a duplicate would still compare equal, so
        // only the length and uniqueness matter.
        let unique: std::collections::BTreeSet<&str> =
            ATTACHMENT_MIME_TYPES.iter().copied().collect();
        assert_eq!(unique.len(), ATTACHMENT_MIME_TYPES.len());
    }

    #[test]
    fn bool_prep_follows_boolean_field_to_python() {
        assert_eq!(bool_prep(&Json::Bool(true)).ok(), Some(true));
        assert_eq!(bool_prep(&Json::Bool(false)).ok(), Some(false));
        assert_eq!(bool_prep(&Json::Str(PyStr::new("1"))).ok(), Some(true));
        assert_eq!(bool_prep(&Json::Str(PyStr::new("0"))).ok(), Some(false));
        assert!(matches!(
            bool_prep(&Json::Str(PyStr::new("yes"))),
            Err(ViewError::Validation)
        ));
        assert!(matches!(bool_prep(&Json::Null), Err(ViewError::Integrity)));
    }
}
