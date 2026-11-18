// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The write actions of the v1 work-item sub-resources: links (319/331,
//! 320/332), comments (321/333, 322/334), attachments (325/337, 326/338) and
//! relations (339).
//!
//! Python: `plane/api/views/issue.py:1274-1451` (links), `:1551-1822`
//! (comments), `:2025-2417` (attachments) and `:2715-2801` (relations). The
//! reads live in [`crate::issue_subresources`].
//!
//! ## What each action publishes
//!
//! | action | rows | Celery |
//! | --- | --- | --- |
//! | link POST | `issue_links` INSERT | `crawl_work_item_link_title`, `issue_activity` (`link.activity.created`, the dedicated `LinkCreated` keyword order) |
//! | link PATCH | `issue_links` UPDATE | `issue_activity` (`link.activity.updated`, bare shape), `crawl_work_item_link_title` only when the URL changed |
//! | link DELETE | `issue_links` soft delete + cascade | `issue_activity` (`link.activity.deleted`, bare shape), `soft_delete_related_objects` |
//! | comment POST | `issue_comments` INSERT + `descriptions` INSERT | `issue_activity` (`comment.activity.created`, bare), `model_activity` (webhook) |
//! | comment PATCH | `issue_comments` UPDATE + `descriptions` UPDATE | `issue_activity` (`comment.activity.updated`, bare), `model_activity` |
//! | comment DELETE | `issue_comments` soft delete + cascade | `soft_delete_related_objects`, then `issue_activity` (`comment.activity.deleted`, bare) |
//! | attachment POST | `file_assets` INSERT | none |
//! | attachment PATCH | `file_assets` UPDATE | `issue_activity` (`attachment.activity.created`, view shape) only on the first upload, then `get_asset_object_metadata` when `storage_metadata` is empty |
//! | attachment DELETE | `file_assets` UPDATE (twice) | `issue_activity` (`attachment.activity.deleted`, view shape), `get_asset_object_metadata` when empty |
//! | relation POST | `issue_relations` INSERTs (`ON CONFLICT DO NOTHING`) | `issue_activity` (`issue_relation.activity.created`, view shape) |
//!
//! `model_activity` is the webhook task, so the two comment writes are the only
//! ones that reach it — the class attribute `webhook_event` on the comment and
//! issue views is never read on this surface.
//!
//! ## Differences from the app surface worth keeping in mind
//!
//! * Every pre-image is read from the **default manager** with no membership
//!   or archived-project filter (`IssueLink.objects.get(workspace__slug=…)`),
//!   so a row the list endpoint cannot see is still patchable/deletable.
//! * The PATCH serializers are the **full** ones (`IssueLinkSerializer`,
//!   `IssueCommentCreateSerializer` partial), not the create serializers:
//!   `url` is not validated on a link PATCH, and `deleted_at` is writable
//!   (`plane/api/serializers/issue.py:493-513`). Ported as it is.
//! * The link PATCH/DELETE and the two comment writes carry no
//!   `notification`/`origin` (`issue.py:1395-1402`, `:1436-1443`), while the
//!   attachment and relation activities use the dominant view shape
//!   (`:2160-2169`, `:2282-2291`, `:2797-2807`).
//! * A comment write's response omits `is_member`: the row is re-read through
//!   `IssueComment.objects.get(pk=…)`, which carries no annotation and DRF
//!   skips a non-required read-only field whose attribute is missing.

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::asset::FileAsset;
use plane_db::models::issue::{IssueComment, IssueLink, IssueRelation};
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_http_app::write::{CharOptions, Detail, Serializer};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityCall};
use plane_query::json::Json;
use plane_serializers::generated::api_issue::{
    IssueCommentCreateSerializer, IssueLinkCreateSerializer,
};
use plane_serializers::{DrfSerialize, FieldValue, RenderCtx};
use plane_storage::{PresignPost, RequestInfo};
use uuid::Uuid;

use crate::issue_subresources::{attachment_serializer, comment_fields, issue_link_serializer};
use crate::issue_subresources_queries as queries;
use crate::issues::{
    activity_base, base_host, guest_cannot_view, publish_issue_activity, publish_model_activity,
    user_id,
};

/// `IssueComment.access`'s choices (`plane/db/models/issue.py:467-471`).
const ACCESS_CHOICES: &[&str] = &["INTERNAL", "EXTERNAL"];

/// `IssueRelationCreateSerializer.RELATION_TYPE_CHOICES`
/// (`plane/api/serializers/issue.py:573-582`).
const RELATION_CHOICES: &[&str] = &[
    "blocking",
    "blocked_by",
    "duplicate",
    "relates_to",
    "start_before",
    "start_after",
    "finish_before",
    "finish_after",
];

/// `settings.ATTACHMENT_MIME_TYPES` (`plane/settings/common.py:505-593`).
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
    "audio/midi",
    "audio/x-midi",
    "audio/aac",
    "audio/flac",
    "audio/x-m4a",
    "video/mp4",
    "video/mpeg",
    "video/ogg",
    "video/webm",
    "video/quicktime",
    "video/x-msvideo",
    "video/x-ms-wmv",
    "application/zip",
    "application/x-rar",
    "application/x-rar-compressed",
    "application/x-tar",
    "application/gzip",
    "application/x-zip",
    "application/x-zip-compressed",
    "application/x-7z-compressed",
    "application/x-compressed",
    "application/x-compressed-tar",
    "application/x-compressed-tar-gz",
    "application/x-compressed-tar-bz2",
    "application/x-compressed-tar-zip",
    "application/x-compressed-tar-7z",
    "application/x-compressed-tar-rar",
    "model/gltf-binary",
    "model/gltf+json",
    "application/octet-stream",
    "font/ttf",
    "font/otf",
    "font/woff",
    "font/woff2",
    "text/css",
    "text/javascript",
    "application/json",
    "text/xml",
    "text/csv",
    "application/xml",
    "application/x-sql",
    "application/x-gzip",
];

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.issue.IssueLinkListCreateAPIEndpoint")?
        .method(http::Method::POST, link_create)?;
    registry
        .api_view("plane.api.views.issue.IssueLinkDetailAPIEndpoint")?
        .method(http::Method::PATCH, link_patch)?
        .method(http::Method::DELETE, link_delete)?;
    registry
        .api_view("plane.api.views.issue.IssueCommentListCreateAPIEndpoint")?
        .method(http::Method::POST, comment_create)?;
    registry
        .api_view("plane.api.views.issue.IssueCommentDetailAPIEndpoint")?
        .method(http::Method::PATCH, comment_patch)?
        .method(http::Method::DELETE, comment_delete)?;
    registry
        .api_view("plane.api.views.issue.IssueAttachmentListCreateAPIEndpoint")?
        .method(http::Method::POST, attachment_create)?;
    registry
        .api_view("plane.api.views.issue.IssueAttachmentDetailAPIEndpoint")?
        .method(http::Method::PATCH, attachment_patch)?
        .method(http::Method::DELETE, attachment_delete)?;
    registry
        .api_view("plane.api.views.issue.IssueRelationListCreateAPIEndpoint")?
        .method(http::Method::POST, relation_create)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn client<'a>(ctx: &'a ViewCtx<'a>) -> Result<&'a deadpool_postgres::Client, ViewError> {
    plane_http_app::common::client(ctx)
}

fn kwarg_uuid(ctx: &ViewCtx<'_>, name: &str) -> Result<Uuid, ViewError> {
    Uuid::parse_str(ctx.kwarg(name).ok_or(ViewError::Key)?).map_err(|_| ViewError::Validation)
}

fn error_body(status: u16, message: &str) -> ViewResponse {
    ViewResponse::raw(
        status,
        APPLICATION_JSON,
        Json::object(vec![("error", Json::string(message))]).to_drf(),
    )
}

/// `Response(serializer.data)` — the DRF rendering with no `fields=`/`expand=`
/// (the write views never pass them).
fn render_struct<T: DrfSerialize + ?Sized>(
    ctx: &ViewCtx<'_>,
    value: &T,
) -> Result<Vec<u8>, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    plane_serializers::render(value, &RenderCtx::full(&timezone)).map_err(ViewError::from)
}

/// `json.dumps(serializer.data, cls=DjangoJSONEncoder)` for the activity's
/// `current_instance`/`requested_data` fields.
fn django_struct<T: DrfSerialize + ?Sized>(
    ctx: &ViewCtx<'_>,
    value: &T,
) -> Result<String, ViewError> {
    let drf = render_struct(ctx, value)?;
    let parsed = plane_serde::pyjson::loads(&drf, "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, &parsed)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `request.data.get(key)` truthiness for the external-id duplicate checks.
fn present(data: &plane_drf::parsers::Data, key: &str) -> Option<String> {
    data.get(key)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// `Issue.objects.get(workspace__slug=…, project_id=…, pk=…)`: the
/// soft-deletion manager, so drafts, triage and archived work items are
/// reachable. A miss is the family 404.
async fn issue_any(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
) -> Result<plane_db::models::issue::Issue, ViewError> {
    let sql = format!(
        "SELECT {} FROM issues \
         INNER JOIN workspaces ON workspaces.id = issues.workspace_id \
         WHERE issues.deleted_at IS NULL AND workspaces.slug = $1 \
           AND issues.project_id = $2 AND issues.id = $3",
        plane_db::models::issue::Issue::COLUMNS.replace("{alias}", "issues")
    );
    let row = client
        .query_opt(&sql, &[&slug, &project_id, &issue_id])
        .await
        .map_err(ViewError::from)?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    plane_db::models::issue::Issue::from_row(&row, 0).map_err(ViewError::from)
}

/// `is_active_project_member` (`issue.py:183-185`).
async fn is_active_project_member(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    user: Uuid,
) -> Result<bool, ViewError> {
    let statement = queries::PROJECT_MEMBER_ACTIVE.prepare(client).await?;
    Ok(client
        .query_one(&statement, &[&project_id, &user])
        .await
        .map_err(ViewError::from)?
        .get(0))
}

/// `is_project_or_workspace_admin` (`issue.py:187-196`).
async fn is_project_admin(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    user: Uuid,
) -> Result<bool, ViewError> {
    let statement = queries::PROJECT_MEMBER_ADMIN.prepare(client).await?;
    let Some(row) = client
        .query_opt(&statement, &[&slug, &project_id, &user])
        .await
        .map_err(ViewError::from)?
    else {
        return Ok(false);
    };
    let role: i16 = row.try_get(0).map_err(ViewError::from)?;
    let workspace_admin: bool = row.try_get(1).map_err(ViewError::from)?;
    Ok(role == 20 || workspace_admin)
}

/// `user_has_issue_permission(…, allowed_roles=[ADMIN, MEMBER, GUEST],
/// allow_creator=…)` (`issue.py:168-180`).
async fn issue_permission(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    issue_creator: Option<Uuid>,
    user: Uuid,
    allow_creator: bool,
) -> Result<bool, ViewError> {
    if allow_creator && issue_creator == Some(user) {
        return Ok(true);
    }
    let statement = queries::PROJECT_MEMBER_ROLE_ALLOWED.prepare(client).await?;
    Ok(client
        .query_one(&statement, &[&project_id, &user])
        .await
        .map_err(ViewError::from)?
        .get(0))
}

// ---------------------------------------------------------------------------
// Links (319/331 POST, 320/332 PATCH+DELETE)
// ---------------------------------------------------------------------------

/// `IssueLinkListCreateAPIEndpoint.post` (`issue.py:1315-1345`).
pub fn link_create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        issue_any(client, &slug, project_id, issue_id).await?;

        let mut serializer = Serializer::new(ctx, false)?;
        let title = serializer.char_field(
            "title",
            CharOptions {
                allow_null: true,
                allow_blank: true,
                trim_whitespace: true,
                max_length: Some(255),
            },
        );
        let url = serializer.char_field(
            "url",
            CharOptions {
                allow_null: false,
                allow_blank: false,
                trim_whitespace: true,
                max_length: None,
            },
        );
        if serializer.missing_required("url") {
            serializer.push_error("url", Detail::required());
        } else if let Some(value) = &url
            && let Err(detail) = link_url_error(value)
        {
            serializer.push_error("url", detail);
        }
        let validated = match serializer.finish() {
            Ok(validated) => validated,
            Err(errors) => return Ok(errors.into_response()),
        };
        let url = validated.text("url").unwrap_or_default().to_owned();

        // `IssueLinkCreateSerializer.create` (`serializers/issue.py:461-464`).
        let statement = queries::LINK_URL_TAKEN.prepare(client).await?;
        let taken: bool = client
            .query_one(&statement, &[&url, &issue_id])
            .await
            .map_err(ViewError::from)?
            .get(0);
        if taken {
            return Ok(error_body(400, "URL already exists for this Issue"));
        }

        // `IssueLink.objects.create(**validated_data)` +
        // `ProjectBaseModel.save`/`BaseModel.save`.
        let statement = plane_http_app::issues::write_sub_queries::PROJECT_WORKSPACE
            .prepare(client)
            .await?;
        let Some(row) = client
            .query_opt(&statement, &[&project_id])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        let now = plane_http_app::write::now();
        let link_id = Uuid::new_v4();
        let metadata = validated
            .json("metadata")
            .cloned()
            .unwrap_or_else(|| JsonText::new("{}"));
        let statement = plane_http_app::issues::write_sub_queries::LINK_INSERT
            .prepare(client)
            .await?;
        client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &Some(user),
                    &None::<Uuid>,
                    &None::<chrono::DateTime<chrono::Utc>>,
                    &link_id,
                    &project_id,
                    &workspace_id,
                    &title,
                    &url,
                    &issue_id,
                    &metadata,
                ],
            )
            .await
            .map_err(integrity)?;

        // `IssueLink.objects.get(pk=serializer.instance.id)`: the activity and
        // the response both use the re-read row.
        let link = link_by_pk(client, &slug, project_id, issue_id, link_id).await?;

        // `crawl_work_item_link_title.delay(serializer.instance.id, url)`.
        publish_crawl(ctx, TaskArg::uuid(link_id), url.clone()).await?;

        // `issue_activity.delay(type="link.activity.created",
        // requested_data=json.dumps(IssueLinkCreateSerializer(link).data), …)`
        // — the dedicated keyword order (`issue.py:1286`).
        let created = IssueLinkCreateSerializer {
            title: link.title.as_deref(),
            url: &link.url,
            issue_id: link.issue_id,
        };
        publish_issue_activity(
            ctx,
            IssueActivityCall::link_created(activity_base(
                "link.activity.created",
                TaskArg::json_text(django_struct(ctx, &created)?),
                link.created_by_id.unwrap_or(user),
                &issue_id.to_string(),
                &project_id.to_string(),
                TaskArg::Null,
            )),
        )
        .await?;

        Ok(ViewResponse::raw(
            201,
            APPLICATION_JSON,
            render_struct(ctx, &issue_link_serializer(&link))?,
        ))
    })
}

/// `IssueLinkDetailAPIEndpoint.patch` (`issue.py:1376-1414`).
pub fn link_patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let mut link = link_by_pk(client, &slug, project_id, issue_id, pk).await?;
        let requested_data = plane_http_app::write::request_data_json_text(ctx)?;
        let previous_url = link.url.clone();
        let current_instance = django_struct(ctx, &issue_link_serializer(&link))?;

        // `IssueLinkSerializer(issue_link, data=request.data, partial=True)`:
        // the full serializer, so `url` gets no `validate_url` (`:1384`).
        let mut serializer = Serializer::new(ctx, true)?;
        let _ = serializer.datetime_field("deleted_at", true);
        let _ = serializer.char_field(
            "title",
            CharOptions {
                allow_null: true,
                allow_blank: true,
                trim_whitespace: true,
                max_length: Some(255),
            },
        );
        let _ = serializer.char_field(
            "url",
            CharOptions {
                allow_null: false,
                allow_blank: false,
                trim_whitespace: true,
                max_length: None,
            },
        );
        let _ = serializer.json_field("metadata", false);
        let validated = match serializer.finish() {
            Ok(validated) => validated,
            Err(errors) => return Ok(errors.into_response()),
        };

        if validated.contains("deleted_at") {
            link.deleted_at = validated.datetime("deleted_at");
        }
        if validated.contains("title") {
            link.title = validated.text("title").map(str::to_owned);
        }
        if validated.contains("url")
            && let Some(url) = validated.text("url")
        {
            link.url = url.to_owned();
        }
        if validated.contains("metadata") {
            link.metadata = validated
                .json("metadata")
                .cloned()
                .unwrap_or_else(|| JsonText::new("{}"));
        }
        let updated_url = link.url.clone();
        let now = plane_http_app::write::now();
        link.updated_at = now;
        link.updated_by_id = Some(user);
        let statement = plane_http_app::issues::write_sub_queries::LINK_UPDATE
            .prepare(client)
            .await?;
        client
            .execute(
                &statement,
                &[
                    &link.title,
                    &link.url,
                    &link.metadata,
                    &link.deleted_at,
                    &link.updated_at,
                    &link.updated_by_id,
                    &link.id,
                ],
            )
            .await
            .map_err(integrity)?;

        if !updated_url.is_empty() && updated_url != previous_url {
            publish_crawl(ctx, TaskArg::uuid(link.id), updated_url).await?;
        }

        publish_issue_activity(
            ctx,
            IssueActivityCall::bare(activity_base(
                "link.activity.updated",
                requested_data,
                user,
                &issue_id.to_string(),
                &project_id.to_string(),
                TaskArg::json_text(current_instance),
            )),
        )
        .await?;

        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_struct(ctx, &issue_link_serializer(&link))?,
        ))
    })
}

/// `IssueLinkDetailAPIEndpoint.delete` (`issue.py:1431-1451`).
pub fn link_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let link = link_by_pk(client, &slug, project_id, issue_id, pk).await?;
        let current_instance = django_struct(ctx, &issue_link_serializer(&link))?;

        let requested = format!("{{\"link_id\": \"{pk}\"}}");
        publish_issue_activity(
            ctx,
            IssueActivityCall::bare(activity_base(
                "link.activity.deleted",
                TaskArg::json_text(requested),
                user,
                &issue_id.to_string(),
                &project_id.to_string(),
                TaskArg::json_text(current_instance),
            )),
        )
        .await?;

        let now = plane_http_app::write::now();
        plane_db::hooks::soft_delete::delete_instance(
            client,
            "issue_links",
            "id",
            pk,
            Some(user),
            now,
        )
        .await
        .map_err(ViewError::from)?;
        plane_http_app::write::publish_soft_delete_cascade(ctx, "db", "issuelink", pk).await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `IssueLink.objects.get(workspace__slug=…, project_id=…, issue_id=…, pk=…)`
/// — the default manager with no membership or archived filter. A miss is the
/// family 404.
async fn link_by_pk(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    pk: Uuid,
) -> Result<IssueLink, ViewError> {
    let statement = queries::LINK_BY_PK.prepare(client).await?;
    let Some(row) = client
        .query_opt(&statement, &[&slug, &project_id, &issue_id, &pk])
        .await
        .map_err(ViewError::from)?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    IssueLink::from_row(&row, 0).map_err(ViewError::from)
}

/// `IssueLinkCreateSerializer.validate_url` (`serializers/issue.py:446-458`).
fn link_url_error(value: &str) -> Result<(), Detail> {
    if !plane_http_app::write::url::django_url_valid(value) {
        return Err(Detail::new("Invalid URL format.", "invalid"));
    }
    if !value.starts_with("http://") && !value.starts_with("https://") {
        return Err(Detail::new("Invalid URL scheme.", "invalid"));
    }
    Ok(())
}

/// `crawl_work_item_link_title.delay(id, url)` (`issue.py:1324`,`:1399`).
async fn publish_crawl(ctx: &ViewCtx<'_>, id: TaskArg, url: String) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::CRAWL_WORK_ITEM_LINK_TITLE;
    let message = tasks::crawl_work_item_link_title(id, url);
    plane_http_app::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::new(task),
    )
    .await?;
    Ok(())
}

/// Django's `IntegrityError` family (SQLSTATE class 23) → the family 400.
fn integrity(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

// ---------------------------------------------------------------------------
// Comments (321/333 POST, 322/334 PATCH+DELETE)
// ---------------------------------------------------------------------------

/// `IssueCommentListCreateAPIEndpoint.post` (`issue.py:1601-1655`).
pub fn comment_create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let issue = issue_any(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Err(ViewError::ObjectDoesNotExist);
        }

        let data = ctx.data()?;
        let external_id = present(data, "external_id");
        let external_source = present(data, "external_source");
        if let (Some(external_id), Some(external_source)) = (&external_id, &external_source) {
            let statement = queries::COMMENT_EXTERNAL_FIRST.prepare(client).await?;
            let row = client
                .query_opt(
                    &statement,
                    &[&project_id, &slug, &external_source, &external_id],
                )
                .await
                .map_err(ViewError::from)?;
            if let Some(row) = row {
                let id: Uuid = row.try_get(0).map_err(ViewError::from)?;
                return Ok(ViewResponse::raw(
                    409,
                    APPLICATION_JSON,
                    Json::object(vec![
                        (
                            "error",
                            Json::string(
                                "Work item comment with the same external id and external source already exists",
                            ),
                        ),
                        ("id", Json::uuid(id)),
                    ])
                    .to_drf(),
                ));
            }
        }

        let mut serializer = Serializer::new(ctx, false)?;
        let _ = serializer.json_field("comment_json", false);
        let _ = serializer.char_field(
            "comment_html",
            CharOptions {
                allow_null: false,
                allow_blank: true,
                trim_whitespace: true,
                max_length: None,
            },
        );
        let _ = serializer.choice_field("access", false, false, ACCESS_CHOICES);
        let _ = serializer.char_field(
            "external_source",
            CharOptions {
                allow_null: true,
                allow_blank: true,
                trim_whitespace: true,
                max_length: Some(255),
            },
        );
        let _ = serializer.char_field(
            "external_id",
            CharOptions {
                allow_null: true,
                allow_blank: true,
                trim_whitespace: true,
                max_length: Some(255),
            },
        );
        let validated = match serializer.finish() {
            Ok(validated) => validated,
            Err(errors) => return Ok(errors.into_response()),
        };

        let now = plane_http_app::write::now();
        let mut comment = IssueComment {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            project_id,
            workspace_id: issue.workspace_id,
            comment_stripped: String::new(),
            comment_json: validated
                .json("comment_json")
                .cloned()
                .unwrap_or_else(|| JsonText::new("{}")),
            comment_html: validated
                .text("comment_html")
                .unwrap_or("<p></p>")
                .to_owned(),
            description_id: None,
            attachments: Vec::new(),
            issue_id,
            actor_id: Some(user),
            access: validated.text("access").unwrap_or("INTERNAL").to_owned(),
            external_source: validated.text("external_source").map(str::to_owned),
            external_id: validated.text("external_id").map(str::to_owned),
            edited_at: None,
            parent_id: None,
        };
        plane_db::hooks::issue_comment::before_save(&mut comment);
        insert_comment(client, &mut comment, user).await?;
        // The second, column-only save is an update for `BaseModel.save`, so
        // the in-memory row carries `updated_by` (the column stays NULL).
        comment.updated_by_id = Some(user);

        // `issue_activity.delay(… requested_data=json.dumps(serializer.data),
        // actor_id=str(issue_comment.created_by_id), …)` (`:1611-1621`).
        let created = IssueCommentCreateSerializer {
            comment_json: Some(plane_serializers::PgJson::from_json_text(
                &comment.comment_json,
            )),
            comment_html: &comment.comment_html,
            access: &comment.access,
            external_source: comment.external_source.as_deref(),
            external_id: comment.external_id.as_deref(),
        };
        publish_issue_activity(
            ctx,
            IssueActivityCall::bare(activity_base(
                "comment.activity.created",
                TaskArg::json_text(django_struct(ctx, &created)?),
                comment.created_by_id.unwrap_or(user),
                &issue_id.to_string(),
                &project_id.to_string(),
                TaskArg::Null,
            )),
        )
        .await?;

        // `model_activity.delay(model_name="issue_comment", …)`.
        publish_model_activity(
            ctx,
            "issue_comment",
            &comment.id.to_string(),
            plane_http_app::write::request_data_arg(ctx)?,
            None,
            user,
            &slug,
            &base_host(ctx)?,
        )
        .await?;

        // `IssueComment.objects.get(pk=serializer.instance.id)`.
        let stored = comment_by_pk(client, &slug, project_id, issue_id, comment.id).await?;
        let attachments: Vec<&str> = stored.attachments.iter().map(String::as_str).collect();
        let serializer = comment_fields(&stored, FieldValue::absent(), &attachments);
        Ok(ViewResponse::raw(
            201,
            APPLICATION_JSON,
            render_struct(ctx, &serializer)?,
        ))
    })
}

/// `IssueCommentDetailAPIEndpoint.patch` (`issue.py:1702-1766`).
pub fn comment_patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let issue = issue_any(client, &slug, project_id, issue_id).await?;
        let is_admin = is_project_admin(client, &slug, project_id, user).await?;
        if !is_admin && guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let mut comment = comment_by_pk(client, &slug, project_id, issue_id, pk).await?;
        let member = is_active_project_member(client, project_id, user).await?;
        if !((comment.created_by_id == Some(user) && member) || is_admin) {
            return Ok(error_body(403, "You are not allowed to edit this comment"));
        }

        let data = ctx.data()?;
        let requested_data = plane_http_app::write::request_data_json_text(ctx)?;
        let attachments: Vec<&str> = comment.attachments.iter().map(String::as_str).collect();
        let current_instance = django_struct(
            ctx,
            &comment_fields(&comment, FieldValue::absent(), &attachments),
        )?;

        // The duplicate check (`:1731-1745`) reports the **URL object's** id.
        if let Some(external_id) = present(data, "external_id")
            && comment.external_id.as_deref() != Some(external_id.as_str())
        {
            let source =
                if data.get("external_source").is_some() || body_has_key(data, "external_source") {
                    data.get("external_source").map(str::to_owned)
                } else {
                    comment.external_source.clone()
                };
            let statement = queries::COMMENT_EXTERNAL_EXISTS.prepare(client).await?;
            let exists: bool = client
                .query_one(&statement, &[&project_id, &slug, &source, &external_id])
                .await
                .map_err(ViewError::from)?
                .get(0);
            if exists {
                return Ok(ViewResponse::raw(
                    409,
                    APPLICATION_JSON,
                    Json::object(vec![
                        (
                            "error",
                            Json::string(
                                "Work item comment with the same external id and external source already exists",
                            ),
                        ),
                        ("id", Json::uuid(comment.id)),
                    ])
                    .to_drf(),
                ));
            }
        }

        let mut serializer = Serializer::new(ctx, true)?;
        let _ = serializer.json_field("comment_json", false);
        let _ = serializer.char_field(
            "comment_html",
            CharOptions {
                allow_null: false,
                allow_blank: true,
                trim_whitespace: true,
                max_length: None,
            },
        );
        let _ = serializer.choice_field("access", false, false, ACCESS_CHOICES);
        let _ = serializer.char_field(
            "external_source",
            CharOptions {
                allow_null: true,
                allow_blank: true,
                trim_whitespace: true,
                max_length: Some(255),
            },
        );
        let _ = serializer.char_field(
            "external_id",
            CharOptions {
                allow_null: true,
                allow_blank: true,
                trim_whitespace: true,
                max_length: Some(255),
            },
        );
        let validated = match serializer.finish() {
            Ok(validated) => validated,
            Err(errors) => return Ok(errors.into_response()),
        };

        let old = comment.clone();
        if validated.contains("comment_json") {
            comment.comment_json = validated
                .json("comment_json")
                .cloned()
                .unwrap_or_else(|| JsonText::new("{}"));
        }
        if let Some(html) = validated.text("comment_html") {
            comment.comment_html = html.to_owned();
        }
        if let Some(access) = validated.text("access") {
            comment.access = access.to_owned();
        }
        if validated.contains("external_source") {
            comment.external_source = validated.text("external_source").map(str::to_owned);
        }
        if validated.contains("external_id") {
            comment.external_id = validated.text("external_id").map(str::to_owned);
        }
        plane_db::hooks::issue_comment::before_save(&mut comment);
        comment.updated_at = plane_http_app::write::now();
        comment.updated_by_id = Some(user);
        let changes = plane_db::hooks::issue_comment::description_changes(&old, &comment);
        save_comment(client, &comment, &changes).await?;

        publish_issue_activity(
            ctx,
            IssueActivityCall::bare(activity_base(
                "comment.activity.updated",
                requested_data,
                user,
                &issue_id.to_string(),
                &project_id.to_string(),
                TaskArg::json_text(current_instance.clone()),
            )),
        )
        .await?;
        publish_model_activity(
            ctx,
            "issue_comment",
            &pk.to_string(),
            plane_http_app::write::request_data_arg(ctx)?,
            Some(current_instance),
            user,
            &slug,
            &base_host(ctx)?,
        )
        .await?;

        let stored = comment_by_pk(client, &slug, project_id, issue_id, pk).await?;
        let attachments: Vec<&str> = stored.attachments.iter().map(String::as_str).collect();
        let serializer = comment_fields(&stored, FieldValue::absent(), &attachments);
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_struct(ctx, &serializer)?,
        ))
    })
}

/// `IssueCommentDetailAPIEndpoint.delete` (`issue.py:1784-1822`).
pub fn comment_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let issue = issue_any(client, &slug, project_id, issue_id).await?;
        let is_admin = is_project_admin(client, &slug, project_id, user).await?;
        if !is_admin && guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let comment = comment_by_pk(client, &slug, project_id, issue_id, pk).await?;
        let member = is_active_project_member(client, project_id, user).await?;
        if !((comment.created_by_id == Some(user) && member) || is_admin) {
            return Ok(error_body(
                403,
                "You are not allowed to delete this comment",
            ));
        }

        let attachments: Vec<&str> = comment.attachments.iter().map(String::as_str).collect();
        let current_instance = django_struct(
            ctx,
            &comment_fields(&comment, FieldValue::absent(), &attachments),
        )?;

        // `issue_comment.delete()`: soft delete + cascade task first, then the
        // activity (`:1804-1821`).
        let now = plane_http_app::write::now();
        plane_db::hooks::soft_delete::delete_instance(
            client,
            "issue_comments",
            "id",
            pk,
            Some(user),
            now,
        )
        .await
        .map_err(ViewError::from)?;
        plane_http_app::write::publish_soft_delete_cascade(ctx, "db", "issuecomment", pk).await?;

        let requested = format!("{{\"comment_id\": \"{pk}\"}}");
        publish_issue_activity(
            ctx,
            IssueActivityCall::bare(activity_base(
                "comment.activity.deleted",
                TaskArg::json_text(requested),
                user,
                &issue_id.to_string(),
                &project_id.to_string(),
                TaskArg::json_text(current_instance),
            )),
        )
        .await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `request.data` carries the key at all (an explicit `null` counts).
fn body_has_key(data: &plane_drf::parsers::Data, key: &str) -> bool {
    match data {
        plane_drf::parsers::Data::Json(value) => value.get(key).is_some(),
        plane_drf::parsers::Data::Form(query) => query.contains_key(key),
    }
}

/// `IssueComment.objects.get(workspace__slug=…, project_id=…, issue_id=…,
/// pk=…)` — the default manager, no `is_member` annotation.
async fn comment_by_pk(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    pk: Uuid,
) -> Result<IssueComment, ViewError> {
    let statement = queries::COMMENT_BY_PK.prepare(client).await?;
    let Some(row) = client
        .query_opt(&statement, &[&slug, &project_id, &issue_id, &pk])
        .await
        .map_err(ViewError::from)?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    IssueComment::from_row(&row, 0).map_err(ViewError::from)
}

/// `IssueComment.save()`'s `transaction.atomic()` block on create
/// (`plane/db/models/issue.py:500-512`), reused from the app-surface catalog.
async fn insert_comment(
    client: &deadpool_postgres::Client,
    comment: &mut IssueComment,
    user: Uuid,
) -> Result<(), ViewError> {
    client
        .batch_execute("BEGIN")
        .await
        .map_err(ViewError::from)?;
    let result = insert_comment_rows(client, comment, user).await;
    match result {
        Ok(()) => {
            client
                .batch_execute("COMMIT")
                .await
                .map_err(ViewError::from)?;
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(error)
        }
    }
}

async fn insert_comment_rows(
    client: &deadpool_postgres::Client,
    comment: &mut IssueComment,
    user: Uuid,
) -> Result<(), ViewError> {
    use plane_http_app::issues::write_comment_queries as write;
    let statement = queries::COMMENT_INSERT.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &comment.created_at,
                &comment.updated_at,
                &comment.created_by_id,
                &comment.updated_by_id,
                &comment.id,
                &comment.project_id,
                &comment.workspace_id,
                &comment.comment_stripped,
                &comment.comment_json,
                &comment.comment_html,
                &comment.attachments,
                &comment.issue_id,
                &comment.actor_id,
                &comment.access,
                &comment.external_source,
                &comment.external_id,
            ],
        )
        .await
        .map_err(integrity)?;

    let description_id = Uuid::new_v4();
    let description_now = plane_http_app::write::now();
    let statement = write::DESCRIPTION_INSERT.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &description_now,
                &description_now,
                &description_id,
                &comment.comment_json,
                &comment.comment_html,
                &plane_db::hooks::issue_comment::description_stripped(&comment.comment_html),
                &Some(user),
                &comment.project_id,
                &comment.workspace_id,
            ],
        )
        .await
        .map_err(integrity)?;
    comment.description_id = Some(description_id);
    let statement = write::COMMENT_SET_DESCRIPTION.prepare(client).await?;
    client
        .execute(&statement, &[&description_id, &comment.id])
        .await
        .map_err(integrity)?;
    Ok(())
}

/// `IssueComment.save()`'s update path: the full-row UPDATE then the
/// `Description` sync, in one transaction (`issue.py:513-531`).
async fn save_comment(
    client: &deadpool_postgres::Client,
    comment: &IssueComment,
    changes: &plane_db::hooks::issue_comment::DescriptionChanges,
) -> Result<(), ViewError> {
    use plane_http_app::issues::write_comment_queries as write;
    client
        .batch_execute("BEGIN")
        .await
        .map_err(ViewError::from)?;
    let result = async {
        let statement = write::COMMENT_UPDATE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &comment.created_at,
                    &comment.updated_at,
                    &comment.created_by_id,
                    &comment.updated_by_id,
                    &comment.deleted_at,
                    &comment.id,
                    &comment.project_id,
                    &comment.workspace_id,
                    &comment.comment_stripped,
                    &comment.comment_json,
                    &comment.comment_html,
                    &comment.description_id,
                    &comment.attachments,
                    &comment.issue_id,
                    &comment.actor_id,
                    &comment.access,
                    &comment.external_source,
                    &comment.external_id,
                    &comment.edited_at,
                    &comment.parent_id,
                    &comment.id,
                ],
            )
            .await
            .map_err(integrity)?;
        if changes.any()
            && let Some(description_id) = comment.description_id
        {
            let mut sql = String::from("UPDATE descriptions SET ");
            let mut sets: Vec<&str> = Vec::new();
            if changes.html {
                sets.push("description_html = $1");
            }
            if changes.stripped {
                sets.push(if changes.html {
                    "description_stripped = $2"
                } else {
                    "description_stripped = $1"
                });
            }
            if changes.json {
                sets.push(if changes.html && changes.stripped {
                    "description_json = $3"
                } else if changes.html || changes.stripped {
                    "description_json = $2"
                } else {
                    "description_json = $1"
                });
            }
            sql.push_str(&sets.join(", "));
            let next = sets.len() + 1;
            sql.push_str(&format!(
                ", updated_by_id = ${next}, updated_at = ${} WHERE id = ${}",
                next + 1,
                next + 2
            ));
            let mut args: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
            if changes.html {
                args.push(&comment.comment_html);
            }
            if changes.stripped {
                args.push(&comment.comment_stripped);
            }
            if changes.json {
                args.push(&comment.comment_json);
            }
            args.push(&comment.updated_by_id);
            args.push(&comment.updated_at);
            args.push(&description_id);
            let statement = client
                .prepare(sql.as_str())
                .await
                .map_err(ViewError::from)?;
            client.execute(&statement, &args).await.map_err(integrity)?;
        }
        Ok::<(), ViewError>(())
    }
    .await;
    match result {
        Ok(()) => {
            client
                .batch_execute("COMMIT")
                .await
                .map_err(ViewError::from)?;
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(error)
        }
    }
}

// ---------------------------------------------------------------------------
// Attachments (325/337 POST, 326/338 PATCH+DELETE)
// ---------------------------------------------------------------------------

/// `IssueAttachmentListCreateAPIEndpoint.post` (`issue.py:2028-2160`).
///
/// The view creates the `FileAsset` row **first**, then presigns. The row is
/// `is_uploaded=false` until the client confirms with the PATCH, and the
/// response's `attachment` object is the in-memory instance, so its
/// `attributes` keep the body's key order (`name`, `type`, `size`) rather than
/// the `jsonb` order a DB round trip would produce.
pub fn attachment_create<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Issue.objects.get(…)` then the create permission (`:2028-2046`).
        let issue = issue_any(client, &slug, project_id, issue_id).await?;
        if !issue_permission(client, project_id, issue.created_by_id, user, true).await? {
            return Ok(error_body(
                403,
                "You are not allowed to upload this attachment",
            ));
        }
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Err(ViewError::ObjectDoesNotExist);
        }

        let data = ctx.data()?;
        let name = data.get("name").and_then(sanitize_filename);
        let mime = data.get("type").filter(|value| !value.is_empty());
        let size = python_int_size(data);
        let external_id = present(data, "external_id");
        let external_source = present(data, "external_source");

        if name.is_none() || size <= 0 {
            return Ok(ViewResponse::raw(
                400,
                APPLICATION_JSON,
                Json::object(vec![
                    ("error", Json::string("Invalid request.")),
                    ("status", Json::Bool(false)),
                ])
                .to_drf(),
            ));
        }
        let name = name.unwrap_or_default();
        let size_limit = size.min(ctx.settings.limits.file_size_limit);

        let Some(mime) = mime.filter(|value| ATTACHMENT_MIME_TYPES.contains(value)) else {
            return Ok(ViewResponse::raw(
                400,
                APPLICATION_JSON,
                Json::object(vec![
                    ("error", Json::string("Invalid file type.")),
                    ("status", Json::Bool(false)),
                ])
                .to_drf(),
            ));
        };
        let mime = mime.to_owned();

        // `Workspace.objects.get(slug=slug)` (`:2089`).
        let statement = queries::WORKSPACE_ID_BY_SLUG.prepare(client).await?;
        let Some(row) = client
            .query_opt(&statement, &[&slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        let asset_key = format!("{}/{}-{name}", workspace_id, Uuid::new_v4().simple());

        if let (Some(external_id), Some(external_source)) = (&external_id, &external_source) {
            let statement = queries::FILE_ASSET_EXTERNAL_FIRST.prepare(client).await?;
            let row = client
                .query_opt(
                    &statement,
                    &[
                        &project_id,
                        &slug,
                        &external_source,
                        &external_id,
                        &issue_id,
                    ],
                )
                .await
                .map_err(ViewError::from)?;
            if let Some(row) = row {
                let id: Uuid = row.try_get(0).map_err(ViewError::from)?;
                return Ok(ViewResponse::raw(
                    409,
                    APPLICATION_JSON,
                    Json::object(vec![
                        (
                            "error",
                            Json::string(
                                "Issue with the same external id and external source already exists",
                            ),
                        ),
                        ("id", Json::uuid(id)),
                    ])
                    .to_drf(),
                ));
            }
        }

        // `FileAsset.objects.create(attributes={"name","type","size"}, …)`.
        let now = plane_http_app::write::now();
        let attributes = JsonText::new(format!(
            "{{\"name\":{},\"type\":{},\"size\":{size_limit}}}",
            json_string(&name),
            json_string(&mime),
        ));
        let asset = FileAsset {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            attributes: attributes.clone(),
            asset: asset_key.clone(),
            user_id: None,
            workspace_id: Some(workspace_id),
            draft_issue_id: None,
            project_id: Some(project_id),
            issue_id: Some(issue_id),
            comment_id: None,
            page_id: None,
            entity_type: Some("ISSUE_ATTACHMENT".to_owned()),
            entity_identifier: None,
            is_deleted: false,
            is_archived: false,
            external_id,
            external_source,
            size: size_limit as f64,
            is_uploaded: false,
            storage_metadata: Some(JsonText::new("{}")),
        };
        let statement = plane_http_app::assets::write_queries::FILE_ASSET_INSERT
            .prepare(client)
            .await?;
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

        let storage =
            plane_storage::Storage::from_process_env(&ctx.settings.storage).map_err(|error| {
                ViewError::Unknown(format!("object storage is not configured: {error}"))
            })?;
        let request = RequestInfo {
            scheme: if ctx.ctx.is_secure { "https" } else { "http" },
            host: ctx.ctx.header("host").unwrap_or("testserver"),
        };
        let post = storage
            .request(request)
            .presign_post(PresignPost {
                key: &asset_key,
                file_type: Some(&mime),
                file_size: size_limit,
                expiration: None,
            })
            .await
            .map_err(|error| {
                ViewError::Unknown(format!("generating a presigned URL failed: {error}"))
            })?;

        // The in-memory attributes keep the body's key order.
        let mut in_memory = asset.clone();
        in_memory.attributes = attributes;
        let attachment = pyjson_of(ctx, &attachment_serializer(&in_memory))?;
        let asset_url = plane_http_app::common::asset_url::file_asset_url(
            asset.id,
            asset.entity_type.as_deref(),
            Some(&slug),
            asset.project_id,
            asset.issue_id,
        )?;
        let mut body = plane_serde::pyjson::PyDict::new();
        body.insert(
            plane_serde::pyjson::PyStr::from("upload_data"),
            upload_data(&post),
        );
        body.insert(
            plane_serde::pyjson::PyStr::from("asset_id"),
            plane_serde::pyjson::Value::Str(plane_serde::pyjson::PyStr::from(asset.id.to_string())),
        );
        body.insert(plane_serde::pyjson::PyStr::from("attachment"), attachment);
        body.insert(
            plane_serde::pyjson::PyStr::from("asset_url"),
            asset_url
                .as_deref()
                .map_or(plane_serde::pyjson::Value::Null, |value| {
                    plane_serde::pyjson::Value::Str(plane_serde::pyjson::PyStr::from(value))
                }),
        );
        let bytes = plane_serde::to_vec(
            plane_serde::Dialect::Drf,
            &plane_serde::pyjson::Value::Dict(body),
        )
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

/// `IssueAttachmentDetailAPIEndpoint.patch` (`issue.py:2366-2417`).
pub fn attachment_patch<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let issue = issue_any(client, &slug, project_id, issue_id).await?;
        if !issue_permission(client, project_id, issue.created_by_id, user, true).await? {
            return Ok(error_body(
                403,
                "You are not allowed to upload this attachment",
            ));
        }
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let mut asset = file_asset_by_pk(client, &slug, project_id, issue_id, pk).await?;

        if !asset.is_uploaded {
            let current_instance = django_struct(ctx, &attachment_serializer(&asset))?;
            publish_issue_activity(
                ctx,
                IssueActivityCall::view(
                    activity_base(
                        "attachment.activity.created",
                        TaskArg::Null,
                        user,
                        ctx.kwarg("issue_id").unwrap_or_default(),
                        ctx.kwarg("project_id").unwrap_or_default(),
                        TaskArg::json_text(current_instance),
                    ),
                    base_host(ctx)?,
                ),
            )
            .await?;
            asset.is_uploaded = true;
            // `created_by` is kept: the delete permission is based on it.
        }
        if storage_metadata_empty(&asset) {
            publish_asset_metadata(ctx, asset.id).await?;
        }
        asset.updated_at = plane_http_app::write::now();
        asset.updated_by_id = Some(user);
        update_file_asset(client, &asset).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `IssueAttachmentDetailAPIEndpoint.delete` (`issue.py:2214-2296`).
pub fn attachment_delete<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let issue = issue_any(client, &slug, project_id, issue_id).await?;
        let is_admin = is_project_admin(client, &slug, project_id, user).await?;
        if !is_admin && guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let mut asset = file_asset_by_pk(client, &slug, project_id, issue_id, pk).await?;

        let member = is_active_project_member(client, project_id, user).await?;
        if !((asset.created_by_id == Some(user) && member) || is_admin) {
            return Ok(error_body(
                403,
                "You are not allowed to delete this attachment",
            ));
        }

        let now = plane_http_app::write::now();
        asset.is_deleted = true;
        asset.deleted_at = Some(now);
        asset.updated_at = now;
        asset.updated_by_id = Some(user);
        update_file_asset(client, &asset).await?;

        publish_issue_activity(
            ctx,
            IssueActivityCall::view(
                activity_base(
                    "attachment.activity.deleted",
                    TaskArg::Null,
                    user,
                    &issue_id.to_string(),
                    &project_id.to_string(),
                    TaskArg::Null,
                ),
                base_host(ctx)?,
            ),
        )
        .await?;

        if storage_metadata_empty(&asset) {
            publish_asset_metadata(ctx, asset.id).await?;
        }
        // `issue_attachment.save()` runs again, bumping `updated_at` a second
        // time; the row's final state is the second save's.
        asset.updated_at = plane_http_app::write::now();
        update_file_asset(client, &asset).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `FileAsset.objects.get(id=…, workspace__slug=…, project_id=…, issue_id=…,
/// entity_type=ISSUE_ATTACHMENT)`.
async fn file_asset_by_pk(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    pk: Uuid,
) -> Result<FileAsset, ViewError> {
    let statement = queries::FILE_ASSET_BY_PK.prepare(client).await?;
    let Some(row) = client
        .query_opt(&statement, &[&slug, &project_id, &issue_id, &pk])
        .await
        .map_err(ViewError::from)?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    FileAsset::from_row(&row, 0).map_err(ViewError::from)
}

async fn update_file_asset(
    client: &deadpool_postgres::Client,
    asset: &FileAsset,
) -> Result<(), ViewError> {
    let statement = plane_http_app::assets::write_queries::FILE_ASSET_UPDATE
        .prepare(client)
        .await?;
    client
        .execute(
            &statement,
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
        .await
        .map_err(integrity)?;
    Ok(())
}

/// `if not issue_attachment.storage_metadata:` — the model default `{}` is
/// falsy, exactly like `None` and `null`.
fn storage_metadata_empty(asset: &FileAsset) -> bool {
    asset
        .storage_metadata
        .as_ref()
        .is_none_or(|value| matches!(value.as_str().trim(), "" | "{}" | "null"))
}

/// `get_asset_object_metadata.delay(str(issue_attachment.id))`.
async fn publish_asset_metadata(ctx: &ViewCtx<'_>, id: Uuid) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::GET_ASSET_OBJECT_METADATA;
    let message = tasks::get_asset_object_metadata_positional(id.to_string());
    plane_http_app::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::new(task),
    )
    .await?;
    Ok(())
}

/// `plane/utils/path_validator.py:sanitize_filename` (`:14-51`).
fn sanitize_filename(filename: &str) -> Option<String> {
    let cleaned: String = filename
        .chars()
        .filter(|character| !character.is_ascii_control())
        .collect();
    let cleaned = cleaned.replace('\\', "/");
    let basename = cleaned.rsplit('/').next().unwrap_or("");
    let cleaned = basename
        .replace("..", "")
        .trim()
        .trim_start_matches('.')
        .trim()
        .to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// `int(request.data.get("size") or 0)` with its `TypeError`/`ValueError`/
/// `OverflowError` guard (`issue.py:2051-2055`): every failure is `0`, which
/// the caller answers as `Invalid request.`.
fn python_int_size(data: &plane_drf::parsers::Data) -> i64 {
    use plane_serde::pyjson::Value as Py;
    let value = match data {
        plane_drf::parsers::Data::Form(query) => {
            return query.get("size").map_or(0, python_int_str);
        }
        plane_drf::parsers::Data::Json(value) => value.get("size"),
    };
    match value {
        None | Some(Py::Null) => 0,
        Some(Py::Bool(flag)) => i64::from(*flag),
        Some(Py::Int(number)) => number.as_i64().unwrap_or(i64::MAX),
        Some(Py::Float(number)) => {
            if number.is_finite() {
                let truncated = number.trunc();
                if truncated >= i64::MAX as f64 {
                    i64::MAX
                } else if truncated <= i64::MIN as f64 {
                    0
                } else {
                    truncated as i64
                }
            } else {
                0
            }
        }
        Some(Py::Str(text)) => text.as_str().map_or(0, python_int_str),
        Some(Py::List(_) | Py::Dict(_)) => 0,
    }
}

/// Python `int(str)`: surrounding whitespace, an optional sign, digits with
/// single underscores between them. Anything else is `ValueError` → 0.
fn python_int_str(text: &str) -> i64 {
    let trimmed = text.trim();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    if digits.is_empty() {
        return 0;
    }
    let mut value: i128 = 0;
    let mut previous_digit = false;
    for character in digits.chars() {
        if character == '_' {
            if !previous_digit {
                return 0;
            }
            previous_digit = false;
            continue;
        }
        let Some(digit) = character.to_digit(10) else {
            return 0;
        };
        previous_digit = true;
        value = value.saturating_mul(10).saturating_add(i128::from(digit));
    }
    if !previous_digit {
        return 0;
    }
    if negative {
        value = -value;
    }
    i64::try_from(value).unwrap_or(if negative { i64::MIN } else { i64::MAX })
}

/// `json.dumps` of a single string (`ensure_ascii`), for the attributes blob.
fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The DRF rendering parsed into a `pyjson` tree, for a body assembled by hand.
fn pyjson_of<T: DrfSerialize + ?Sized>(
    ctx: &ViewCtx<'_>,
    value: &T,
) -> Result<plane_serde::pyjson::Value, ViewError> {
    let drf = render_struct(ctx, value)?;
    plane_serde::pyjson::loads(&drf, "utf-8").map_err(|error| ViewError::Unknown(error.to_string()))
}

/// boto3's `{"url": …, "fields": {…}}` in `generate_presigned_post` order
/// (`plane/settings/storage.py:68-102`; the field order is the storage crate's).
fn upload_data(post: &plane_storage::PresignedPost) -> plane_serde::pyjson::Value {
    use plane_serde::pyjson::{PyDict, PyStr, Value as Py};
    let mut fields = PyDict::new();
    for (key, value) in &post.fields {
        fields.insert(
            PyStr::from(key.as_str()),
            value
                .as_deref()
                .map_or(Py::Null, |value| Py::Str(PyStr::from(value))),
        );
    }
    let mut upload = PyDict::new();
    upload.insert(PyStr::from("url"), Py::Str(PyStr::from(post.url.as_str())));
    upload.insert(PyStr::from("fields"), Py::Dict(fields));
    Py::Dict(upload)
}

// ---------------------------------------------------------------------------
// Relations (339 POST)
// ---------------------------------------------------------------------------

/// `get_actual_relation` (`plane/utils/issue_relation_mapper.py:19-32`).
fn actual_relation(relation_type: &str) -> String {
    match relation_type {
        "blocking" => "blocked_by".to_owned(),
        "start_after" => "start_before".to_owned(),
        "finish_after" => "finish_before".to_owned(),
        other => other.to_owned(),
    }
}

/// `is_reverse = relation_type in ["blocking","start_after","finish_after"]`
/// (`issue.py:2743`).
fn is_reverse(relation_type: &str) -> bool {
    matches!(relation_type, "blocking" | "start_after" | "finish_after")
}

/// `IssueRelationListCreateAPIEndpoint.post` (`issue.py:2727-2801`).
pub fn relation_create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `IssueRelationCreateSerializer(data=request.data)`.
        let mut serializer = Serializer::new(ctx, false)?;
        let _ = serializer.choice_field("relation_type", false, false, RELATION_CHOICES);
        if serializer.missing_required("relation_type") {
            serializer.push_error("relation_type", Detail::required());
        }
        let issues = serializer.uuid_list_field("issues");
        if serializer.missing_required("issues") {
            serializer.push_error("issues", Detail::required());
        } else if let Some(values) = &issues
            && values.is_empty()
        {
            serializer.push_error("issues", plane_http_app::write::fields::list_min_length(1));
        }
        let validated = match serializer.finish() {
            Ok(validated) => validated,
            Err(errors) => return Ok(errors.into_response()),
        };
        let relation_type = validated
            .text("relation_type")
            .unwrap_or_default()
            .to_owned();
        let requested = validated.uuid_list("issues").unwrap_or_default().to_vec();

        // `Project.objects.get(pk=project_id, workspace__slug=slug)` (`:2736`).
        let statement = queries::PROJECT_IN_SLUG.prepare(client).await?;
        let Some(row) = client
            .query_opt(&statement, &[&project_id, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = row.try_get(1).map_err(ViewError::from)?;

        // `Issue.issue_objects.get(workspace__slug=…, project_id=…, pk=…)`
        // (`:2739`): triage/archived/draft work items are **not** reachable.
        let statement = plane_http_app::issues::write_sub_queries::ISSUE_LIVE_IN_PROJECT
            .prepare(client)
            .await?;
        let live: bool = client
            .query_one(&statement, &[&issue_id, &project_id, &slug])
            .await
            .map_err(ViewError::from)?
            .get(0);
        if !live {
            return Err(ViewError::ObjectDoesNotExist);
        }

        // `Issue.issue_objects.filter(workspace__slug=slug, pk__in=issues)
        // .values_list("id", flat=True)` (`:2746-2752`).
        let statement = plane_http_app::issues::write_sub_queries::RELATION_ISSUES
            .prepare(client)
            .await?;
        let rows = client
            .query(&statement, &[&slug, &requested])
            .await
            .map_err(ViewError::from)?;
        let issues: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();

        // `IssueRelation.objects.bulk_create([…], batch_size=10,
        // ignore_conflicts=True)` (`:2754-2770`). Django's `auto_now` stamps
        // each row separately, so the rows' `created_at` values ascend in list
        // order; the refetch's `-created_at` order depends on that.
        let stored_type = actual_relation(&relation_type);
        let mirrored = is_reverse(&relation_type);
        for issue in &issues {
            let (row_issue, row_related) = if mirrored {
                (*issue, issue_id)
            } else {
                (issue_id, *issue)
            };
            let now = plane_http_app::write::now();
            let statement = plane_http_app::issues::write_sub_queries::RELATION_INSERT
                .prepare(client)
                .await?;
            client
                .execute(
                    &statement,
                    &[
                        &now,
                        &now,
                        &Some(user),
                        &Some(user),
                        &Uuid::new_v4(),
                        &project_id,
                        &workspace_id,
                        &row_issue,
                        &row_related,
                        &stored_type,
                    ],
                )
                .await
                .map_err(integrity)?;
        }

        // `requested_data=json.dumps({"relation_type": relation_type,
        // "issues": issues})` (`:2772-2783`); `issues` is the queryset's list.
        let mut document = String::from("{\"relation_type\": ");
        document.push_str(&json_string(&relation_type));
        document.push_str(", \"issues\": [");
        for (index, issue) in issues.iter().enumerate() {
            if index > 0 {
                document.push_str(", ");
            }
            document.push('"');
            document.push_str(&issue.to_string());
            document.push('"');
        }
        document.push_str("]}");
        publish_issue_activity(
            ctx,
            IssueActivityCall::view(
                activity_base(
                    "issue_relation.activity.created",
                    TaskArg::json_text(document),
                    user,
                    &issue_id.to_string(),
                    &project_id.to_string(),
                    TaskArg::Null,
                ),
                base_host(ctx)?,
            ),
        )
        .await?;

        // The refetch (`:2788-2795`), in `-created_at` order.
        let (statement, issue_relation_serializer) = if mirrored {
            (
                queries::RELATION_REFETCH_REVERSE.prepare(client).await?,
                false,
            )
        } else {
            (
                queries::RELATION_REFETCH_FORWARD.prepare(client).await?,
                true,
            )
        };
        let rows = client
            .query(&statement, &[&slug, &issues, &issue_id, &stored_type])
            .await
            .map_err(ViewError::from)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let mut entries = Vec::with_capacity(rows.len());
        for row in &rows {
            entries.push(relation_reply(row, timezone, issue_relation_serializer)?);
        }
        Ok(ViewResponse::raw(
            201,
            APPLICATION_JSON,
            Json::Array(entries).to_drf(),
        ))
    })
}

/// One relation row's reply in `_readable_fields` order.
///
/// `issue_relation_serializer` selects between `IssueRelationSerializer`
/// (reads `related_issue`, `serializers/issue.py:616-654`) and
/// `RelatedIssueSerializer` (reads `issue`, `:657-700`). `state_id` and the two
/// type keys are omitted when the relation is null — DRF's `SkipField`, not
/// `null`.
fn relation_reply(
    row: &tokio_postgres::Row,
    timezone: chrono_tz::Tz,
    issue_relation_serializer: bool,
) -> Result<Json, ViewError> {
    let relation = IssueRelation::from_row(row, 0).map_err(ViewError::from)?;
    let start = IssueRelation::COLUMN_COUNT;
    let project: Uuid = row.try_get(start).map_err(ViewError::from)?;
    let sequence: i32 = row.try_get(start + 1).map_err(ViewError::from)?;
    let name: String = row.try_get(start + 2).map_err(ViewError::from)?;
    let state: Option<Uuid> = row.try_get(start + 3).map_err(ViewError::from)?;
    let priority: String = row.try_get(start + 4).map_err(ViewError::from)?;
    let type_id: Option<Uuid> = row.try_get(start + 5).map_err(ViewError::from)?;
    let is_epic: Option<bool> = row.try_get(start + 6).map_err(ViewError::from)?;
    let reply_id = if issue_relation_serializer {
        relation.related_issue_id
    } else {
        relation.issue_id
    };
    let datetime = |value: chrono::DateTime<chrono::Utc>| {
        Json::DateTimeZoned(value.with_timezone(&timezone).fixed_offset())
    };
    let mut entries: Vec<(&str, Json)> = vec![
        ("id", Json::uuid(reply_id)),
        ("project_id", Json::uuid(project)),
        ("sequence_id", Json::Int(i64::from(sequence))),
        (
            "relation_type",
            Json::string(relation.relation_type.as_str()),
        ),
        ("name", Json::string(name)),
    ];
    if !issue_relation_serializer {
        if let Some(type_id) = type_id {
            entries.push(("type_id", Json::uuid(type_id)));
        }
        if let Some(is_epic) = is_epic {
            entries.push(("is_epic", Json::Bool(is_epic)));
        }
    }
    if let Some(state) = state {
        entries.push(("state_id", Json::uuid(state)));
    }
    entries.push(("priority", Json::string(priority)));
    entries.push(("created_by", Json::opt_uuid(relation.created_by_id)));
    entries.push(("created_at", datetime(relation.created_at)));
    if issue_relation_serializer {
        entries.push(("updated_at", datetime(relation.updated_at)));
        entries.push(("updated_by", Json::opt_uuid(relation.updated_by_id)));
    } else {
        entries.push(("updated_by", Json::opt_uuid(relation.updated_by_id)));
        entries.push(("updated_at", datetime(relation.updated_at)));
    }
    Ok(Json::object(entries))
}
