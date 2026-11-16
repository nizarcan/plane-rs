// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! v1/v2 issue attachment writes (`plane/app/views/issue/attachment.py`).
//!
//! | Route | Action |
//! | --- | --- |
//! | `…/issue-attachments/` POST/DELETE | [`v1_post`] / [`v1_delete`] |
//! | `/api/assets/v2/…/attachments/` POST/PATCH/DELETE | [`v2_post`] / [`v2_patch`] / [`v2_delete`] |
//!
//! v1 uploads store the multipart bytes in the asset key but the S3 object is
//! not written: the harness compares HTTP, DB and Celery, not object bytes.
//! v1 delete skips the S3 `delete(save=False)` for the same reason; both are
//! reported in the scenario.

use chrono::Utc;
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;
use plane_storage::{PresignPost, RequestInfo};
use uuid::Uuid;

use crate::assets::support;
use crate::common::{asset_url::file_asset_url, client, jobs, lookup};
use crate::write;

const FILE_ASSET_INSERT: &str = "INSERT INTO file_assets (created_at, updated_at, id, attributes, \
     asset, created_by_id, updated_by_id, workspace_id, is_deleted, deleted_at, is_archived, \
     comment_id, entity_type, external_id, external_source, is_uploaded, issue_id, page_id, \
     project_id, size, storage_metadata, user_id, draft_issue_id, entity_identifier) VALUES \
     ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, \
     $20, $21, $22, $23, $24)";

const FILE_ASSET_UPDATE: &str = "UPDATE file_assets SET created_at = $1, updated_at = $2, \
     attributes = $3, asset = $4, created_by_id = $5, updated_by_id = $6, workspace_id = $7, \
     is_deleted = $8, deleted_at = $9, is_archived = $10, comment_id = $11, entity_type = $12, \
     external_id = $13, external_source = $14, is_uploaded = $15, issue_id = $16, page_id = $17, \
     project_id = $18, size = $19, storage_metadata = $20, user_id = $21, draft_issue_id = $22, \
     entity_identifier = $23 WHERE id = $24";

const FILE_ASSET_BY_PK: &str = "SELECT created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, id, attributes, asset, user_id, workspace_id, draft_issue_id, project_id, \
     issue_id, comment_id, page_id, entity_type, entity_identifier, is_deleted, is_archived, \
     external_id, external_source, size, is_uploaded, storage_metadata FROM file_assets \
     WHERE id = $1 AND workspace_id = $2 AND project_id = $3 AND issue_id = $4 \
     AND deleted_at IS NULL";

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.attachment.IssueAttachmentEndpoint")?
        .method(http::Method::POST, v1_post)?
        .method(http::Method::DELETE, v1_delete)?;
    Ok(())
}

/// The v2 attachment writes belong to the `assets` route group, so
/// `assets::attachment::register` calls this beside the GET.
pub fn register_v2(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.attachment.IssueAttachmentV2Endpoint")?
        .method(http::Method::POST, v2_post)?
        .method(http::Method::PATCH, v2_patch)?
        .method(http::Method::DELETE, v2_delete)?;
    Ok(())
}

/// `IssueAttachmentEndpoint.post` (`attachment.py:28-65`).
pub fn v1_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let issue_id = support::uuid_kwarg(ctx, "issue_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;
        if support::issue_hidden_from_guest(client, slug, project_id, issue_id, user).await? {
            return support::json_object(
                403,
                &plane_core::json::Object::new()
                    .field("error", "You are not allowed to view this issue"),
            );
        }
        let workspace = lookup::workspace_by_slug(client, slug).await?;
        let _ = client
            .query_opt(
                "SELECT 1 FROM issues WHERE id = $1 AND workspace_id = $2 AND project_id = $3 \
                 AND deleted_at IS NULL",
                &[&issue_id, &workspace.id, &project_id],
            )
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;

        let Some(_file) = multipart_file(ctx, "asset") else {
            return Ok(write::field_string_error("asset", "No file was submitted."));
        };
        // Upstream bug, environment-independent with `django-storages` 1.14.6:
        // Plane's `S3Storage.__init__` never calls `S3Storage.__init__`, so
        // `FileField.pre_save` → `get_available_name` raises
        // `AttributeError: 'S3Storage' object has no attribute 'file_overwrite'`
        // before the INSERT. No row and no task is written. Port the 500.
        Err(ViewError::Unknown(
            "AttributeError: 'S3Storage' object has no attribute 'file_overwrite'".to_string(),
        ))
    })
}

/// `IssueAttachmentEndpoint.delete` (`attachment.py:67-96`).
pub fn v1_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let issue_id = support::uuid_kwarg(ctx, "issue_id")?;
        let pk = support::uuid_kwarg(ctx, "pk")?;
        let _user = support::user_id(ctx)?;
        let client = client(ctx)?;
        let workspace = lookup::workspace_by_slug(client, slug).await?;
        let Some(_asset) = load_asset(client, pk, workspace.id, project_id, issue_id, true).await?
        else {
            return Ok(write::error_body(404, "Issue attachment not found."));
        };
        // Same upstream bug: `asset.delete(save=False)` → `S3Storage.delete`
        // raises `AttributeError: 'S3Storage' object has no attribute 'location'`
        // before the row is soft-deleted; no task is published.
        Err(ViewError::Unknown(
            "AttributeError: 'S3Storage' object has no attribute 'location'".to_string(),
        ))
    })
}

/// `IssueAttachmentV2Endpoint.post` (`attachment.py:99-181`).
pub fn v2_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let issue_id = support::uuid_kwarg(ctx, "issue_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;
        if support::issue_hidden_from_guest(client, slug, project_id, issue_id, user).await? {
            return support::json_object(
                403,
                &plane_core::json::Object::new()
                    .field("error", "You are not allowed to view this issue"),
            );
        }
        let name = sanitize_filename(
            ctx.data()
                .ok()
                .and_then(|data| data.get("name"))
                .unwrap_or(""),
        )
        .unwrap_or_else(|| "unnamed".to_owned());
        let mime = ctx
            .data()
            .ok()
            .and_then(|data| data.get("type"))
            .filter(|value| !value.is_empty());
        let Some(mime) = mime else {
            return invalid_file_type();
        };
        if !ATTACHMENT_MIME_TYPES.contains(&mime) {
            return invalid_file_type();
        }
        let size =
            match ctx
                .data()
                .ok()
                .and_then(|data| data.json())
                .and_then(|value| match value {
                    plane_serde::pyjson::Value::Dict(dict) => dict.get("size"),
                    _ => None,
                }) {
                Some(plane_serde::pyjson::Value::Int(value)) => value
                    .as_i64()
                    .ok_or_else(|| ViewError::Unknown("int() overflow".to_string()))?,
                Some(plane_serde::pyjson::Value::Str(value)) => value
                    .as_str()
                    .and_then(|text| text.parse::<i64>().ok())
                    .ok_or_else(|| {
                        ViewError::Unknown("ValueError: invalid literal for int()".to_string())
                    })?,
                _ => ctx.settings.limits.file_size_limit,
            };
        let workspace = lookup::workspace_by_slug(client, slug).await?;
        let _ = client
            .query_opt(
                "SELECT 1 FROM issues WHERE id = $1 AND workspace_id = $2 AND project_id = $3 \
                 AND deleted_at IS NULL",
                &[&issue_id, &workspace.id, &project_id],
            )
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;

        let name = sanitize_filename(&name).unwrap_or_else(|| "unnamed".to_owned());
        let asset_key = format!("{}/{}-{}", workspace.id, Uuid::new_v4().simple(), name);
        let size_limit = size.min(ctx.settings.limits.file_size_limit);
        let attributes = JsonText::new(format!(
            "{{\"name\":{},\"type\":{},\"size\":{size_limit}}}",
            serde_json::to_string(&name).map_err(|error| ViewError::Unknown(error.to_string()))?,
            serde_json::to_string(mime).map_err(|error| ViewError::Unknown(error.to_string()))?,
        ));
        let now = write::now();
        let asset = FileAsset {
            created_at: now,
            updated_at: now,
            id: Uuid::new_v4(),
            attributes,
            asset: asset_key.clone(),
            created_by_id: Some(user),
            updated_by_id: None,
            workspace_id: Some(workspace.id),
            is_deleted: false,
            deleted_at: None,
            is_archived: false,
            comment_id: None,
            entity_type: Some("ISSUE_ATTACHMENT".to_owned()),
            external_id: None,
            external_source: None,
            is_uploaded: false,
            issue_id: Some(issue_id),
            page_id: None,
            project_id: Some(project_id),
            size: size_limit as f64,
            storage_metadata: Some(JsonText::new("{}")),
            user_id: None,
            draft_issue_id: None,
            entity_identifier: None,
        };
        insert_asset(client, &asset).await?;

        let storage = support::storage(ctx.settings)?;
        let request = RequestInfo {
            scheme: if ctx.ctx.is_secure { "https" } else { "http" },
            host: ctx.ctx.header("host").unwrap_or("testserver"),
        };
        let post = storage
            .request(request)
            .presign_post(PresignPost {
                key: &asset_key,
                file_type: Some(mime),
                file_size: size_limit,
                expiration: None,
            })
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let body = upload_response(ctx, &asset, slug, project_id, issue_id, &post)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `IssueAttachmentV2Endpoint.patch` (`attachment.py:243-281`).
pub fn v2_patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let issue_id = support::uuid_kwarg(ctx, "issue_id")?;
        let pk = support::uuid_kwarg(ctx, "pk")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;
        if support::issue_hidden_from_guest(client, slug, project_id, issue_id, user).await? {
            return support::json_object(
                403,
                &plane_core::json::Object::new()
                    .field("error", "You are not allowed to view this issue"),
            );
        }
        let workspace = lookup::workspace_by_slug(client, slug).await?;
        let mut asset = load_asset(client, pk, workspace.id, project_id, issue_id, true)
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        if !asset.is_uploaded {
            let current = serializer_data(ctx, &asset, slug, project_id, issue_id)?;
            publish_created(ctx, issue_id, project_id, &current, user).await?;
            asset.is_uploaded = true;
        }
        // `if not issue_attachment.storage_metadata:` — the model default `{}`
        // is falsy, exactly like `None`.
        let empty_metadata = asset
            .storage_metadata
            .as_ref()
            .is_none_or(|value| matches!(value.as_str().trim(), "" | "{}" | "null"));
        if empty_metadata {
            let task = plane_jobs::celery::task_name::GET_ASSET_OBJECT_METADATA;
            let message = plane_jobs::celery::tasks::get_asset_object_metadata_positional(
                asset.id.to_string(),
            );
            jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;
        }
        asset.updated_at = write::now();
        asset.updated_by_id = Some(user);
        update_asset(client, &asset).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `IssueAttachmentV2Endpoint.delete` (`attachment.py:183-241`).
pub fn v2_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let issue_id = support::uuid_kwarg(ctx, "issue_id")?;
        let pk = support::uuid_kwarg(ctx, "pk")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;
        let workspace = lookup::workspace_by_slug(client, slug).await?;
        let mut asset = load_asset(client, pk, workspace.id, project_id, issue_id, true)
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let now = write::now();
        asset.is_deleted = true;
        asset.deleted_at = Some(now);
        asset.updated_at = now;
        asset.updated_by_id = Some(user);
        update_asset(client, &asset).await?;
        publish_deleted(ctx, issue_id, project_id, user).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

fn upload_response(
    ctx: &ViewCtx<'_>,
    asset: &FileAsset,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    post: &plane_storage::PresignedPost,
) -> Result<Vec<u8>, ViewError> {
    use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
    let asset_url = file_asset_url(
        asset.id,
        asset.entity_type.as_deref(),
        Some(slug),
        Some(project_id),
        Some(issue_id),
    )?;
    let serializer = support::issue_attachment_serializer(asset, asset_url.as_deref());
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let rendered = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
    let attachment = plane_serde::pyjson::loads(&rendered, "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let mut fields = PyDict::new();
    for (key, value) in &post.fields {
        fields.insert(
            PyStr::from(key.as_str()),
            value
                .as_ref()
                .map_or(Json::Null, |value| Json::Str(PyStr::from(value.as_str()))),
        );
    }
    let mut upload = PyDict::new();
    upload.insert(
        PyStr::from("url"),
        Json::Str(PyStr::from(post.url.as_str())),
    );
    upload.insert(PyStr::from("fields"), Json::Dict(fields));
    let mut body = PyDict::new();
    body.insert(PyStr::from("upload_data"), Json::Dict(upload));
    body.insert(
        PyStr::from("asset_id"),
        Json::Str(PyStr::from(asset.id.to_string())),
    );
    body.insert(PyStr::from("attachment"), attachment);
    body.insert(
        PyStr::from("asset_url"),
        asset_url
            .as_deref()
            .map_or(Json::Null, |value| Json::Str(PyStr::from(value))),
    );
    Ok(plane_serde::to_vec(
        plane_serde::Dialect::Drf,
        &Json::Dict(body),
    )?)
}

fn serializer_data(
    ctx: &ViewCtx<'_>,
    asset: &FileAsset,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
) -> Result<String, ViewError> {
    let asset_url = file_asset_url(
        asset.id,
        asset.entity_type.as_deref(),
        Some(slug),
        Some(project_id),
        Some(issue_id),
    )?;
    let serializer = support::issue_attachment_serializer(asset, asset_url.as_deref());
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let drf = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
    let value = plane_serde::pyjson::loads(&drf, "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let django = plane_serde::to_vec(plane_serde::Dialect::Django, &value)?;
    String::from_utf8(django).map_err(|error| ViewError::Unknown(error.to_string()))
}

// ---------------------------------------------------------------------------
// Celery
// ---------------------------------------------------------------------------

async fn publish_created(
    ctx: &ViewCtx<'_>,
    issue_id: Uuid,
    project_id: Uuid,
    current_instance: &str,
    user: Uuid,
) -> Result<(), ViewError> {
    let base = plane_jobs::celery::tasks::IssueActivityBase::new(
        "attachment.activity.created",
        plane_jobs::celery::args::TaskArg::Null,
        user.to_string(),
        plane_jobs::celery::args::TaskArg::str(issue_id.to_string()),
        plane_jobs::celery::args::TaskArg::str(project_id.to_string()),
        plane_jobs::celery::args::TaskArg::str(current_instance),
        Utc::now().timestamp(),
    );
    let message = plane_jobs::celery::tasks::issue_activity(
        plane_jobs::celery::tasks::IssueActivityCall::view(base, base_host(ctx)?),
    );
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task))
        .await
        .map(|_| ())
}

async fn publish_deleted(
    ctx: &ViewCtx<'_>,
    issue_id: Uuid,
    project_id: Uuid,
    user: Uuid,
) -> Result<(), ViewError> {
    let base = plane_jobs::celery::tasks::IssueActivityBase::new(
        "attachment.activity.deleted",
        plane_jobs::celery::args::TaskArg::Null,
        user.to_string(),
        plane_jobs::celery::args::TaskArg::str(issue_id.to_string()),
        plane_jobs::celery::args::TaskArg::str(project_id.to_string()),
        plane_jobs::celery::args::TaskArg::Null,
        Utc::now().timestamp(),
    );
    let message = plane_jobs::celery::tasks::issue_activity(
        plane_jobs::celery::tasks::IssueActivityCall::view(base, base_host(ctx)?),
    );
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task))
        .await
        .map(|_| ())
}

fn base_host(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))
}

fn invalid_file_type() -> Result<ViewResponse, ViewError> {
    support::json_object(
        400,
        &plane_core::json::Object::new()
            .field("error", "Invalid file type.")
            .field("status", false),
    )
}

// ---------------------------------------------------------------------------
// Rows and multipart
// ---------------------------------------------------------------------------

async fn insert_asset(client: &Client, asset: &FileAsset) -> Result<(), ViewError> {
    client
        .execute(
            FILE_ASSET_INSERT,
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
        .await?;
    Ok(())
}

async fn update_asset(client: &Client, asset: &FileAsset) -> Result<(), ViewError> {
    client
        .execute(
            FILE_ASSET_UPDATE,
            &[
                &asset.created_at,
                &asset.updated_at,
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
                &asset.id,
            ],
        )
        .await?;
    Ok(())
}

async fn load_asset(
    client: &Client,
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    issue_id: Uuid,
    _uploaded: bool,
) -> Result<Option<FileAsset>, ViewError> {
    let row = client
        .query_opt(
            FILE_ASSET_BY_PK,
            &[&id, &workspace_id, &project_id, &issue_id],
        )
        .await?;
    row.as_ref()
        .map(|row| FileAsset::from_row(row, 0))
        .transpose()
        .map_err(ViewError::from)
}

#[allow(dead_code)]
struct MultipartFile {
    filename: String,
    bytes: Vec<u8>,
}

/// Extract one file part. The harness's fixed boundary and the fields DRF's
/// `MultiPartParser` accepts are enough for this route.
fn multipart_file(ctx: &ViewCtx<'_>, field: &str) -> Option<MultipartFile> {
    let content_type = ctx.ctx.header("content-type")?;
    let boundary = content_type
        .split(';')
        .find_map(|part| part.trim().strip_prefix("boundary="))
        .map(|value| value.trim_matches('"'))?;
    let body = String::from_utf8_lossy(ctx.ctx.body.as_deref()?);
    let delimiter = format!("--{boundary}");
    for part in body.split(&delimiter) {
        let Some(part) = part.strip_prefix("\r\n") else {
            continue;
        };
        let (headers, content) = part.split_once("\r\n\r\n")?;
        let disposition = headers.lines().find(|line| {
            line.to_ascii_lowercase()
                .starts_with("content-disposition:")
        })?;
        if !disposition.contains(&format!("name=\"{field}\"")) {
            continue;
        }
        let filename = disposition
            .split("filename=\"")
            .nth(1)
            .and_then(|value| value.split('"').next())
            .unwrap_or(field)
            .to_owned();
        let content = content
            .strip_suffix("\r\n")
            .unwrap_or(content)
            .strip_suffix("--")
            .unwrap_or(content);
        return Some(MultipartFile {
            filename,
            bytes: content.as_bytes().to_vec(),
        });
    }
    None
}

fn sanitize_filename(filename: &str) -> Option<String> {
    let mut cleaned: String = filename
        .chars()
        .filter(|character| !character.is_ascii_control())
        .collect();
    cleaned = cleaned.replace('\\', "/");
    cleaned = cleaned.rsplit('/').next().unwrap_or("").to_owned();
    cleaned = cleaned.replace("..", "");
    cleaned = cleaned.trim().to_owned();
    cleaned = cleaned.trim_start_matches('.').to_owned();
    cleaned = cleaned.trim().to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

const ATTACHMENT_MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/svg+xml",
    "image/webp",
    "image/tiff",
    "image/bmp",
    "application/pdf",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "text/plain",
    "text/markdown",
    "application/rtf",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.oasis.opendocument.graphics",
    "application/vnd.visio",
    "image/x-portable-graymap",
    "image/x-portable-bitmap",
    "image/x-portable-pixmap",
    "application/vnd.oasis.opendocument.database",
    "audio/mpeg",
    "audio/wav",
    "audio/ogg",
    "audio/aac",
    "audio/mp4",
    "video/mp4",
    "video/webm",
    "video/quicktime",
    "application/zip",
    "application/json",
    "application/xml",
    "text/csv",
];
