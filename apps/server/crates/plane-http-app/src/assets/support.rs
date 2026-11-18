// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Shared logic of the `assets` handlers: the model rule the Python keeps on
//! `FileAsset`, the serializer fields two routes derive from it, and the S3
//! presign call site.
//!
//! Ported from `apps/api`:
//!
//! * `FileAsset.is_project_accessible_to` (`plane/db/models/asset.py:106-146`);
//! * `issue_hidden_from_guest` (`plane/app/permissions/base.py:106-141`);
//! * `S3Storage.generate_presigned_url` (`plane/settings/storage.py:114-151`);
//! * `can_modify_asset` (`plane/app/views/asset/v2.py:31-67`) and its two role
//!   lookups — the shared authorisation rule of every asset write;
//! * `sanitize_filename` (`plane/utils/path_validator.py:14-51`);
//! * the presigned-POST response bodies (`v2.py:203-210`, `:432-439`,
//!   `:674-681` and the four-key attachment variant).
//!
//! `FileAsset.asset_url` and `request.build_absolute_uri` are generic model and
//! Django rules, so they live in [`crate::common::asset_url`] and
//! [`crate::common::request`]; this module only wires them to the asset rows.
//!
//! ## Object storage
//!
//! Django builds `S3Storage(request=request)` inside the view, and with
//! `USE_MINIO=1` the endpoint is `request.scheme://request.get_host()`
//! (`storage.py:42-54`) — the browser-visible host, not `AWS_S3_ENDPOINT_URL`.
//! The harness pins `Host: plane-fixture.test` for exactly this reason: the
//! `Location` header is signed for that host.
//!
//! `crates/plane-storage` is byte-compatible with boto3 (DECISIONS.md D6) and
//! is the same code the golden corpus pins, so the call site only has to hand
//! it the inputs the Python wrapper passes.
//!
//! `plane-http-app`'s `ViewCtx` has no storage handle, so the client is built
//! once per process from `ViewCtx::settings` in a `OnceLock` here. A `ViewCtx`
//! accessor would be better: it is process state built from `Settings`, exactly
//! like `ViewCtx::cache()` and `ViewCtx::celery()`, and every future storage
//! route would otherwise repeat this.

use std::collections::HashMap;
use std::sync::OnceLock;

use deadpool_postgres::Client;
use plane_core::config::Settings;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_core::json::Object;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::view::{ViewCtx, ViewResponse};
use plane_permissions::Role;
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::generated::app_asset::FileAssetSerializer;
use plane_serializers::generated::app_issue::IssueAttachmentSerializer;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, PgJson, RenderCtx};
use plane_storage::{PresignGet, PresignedPost, RequestInfo, Storage};
use uuid::Uuid;

use crate::common::asset_url::file_asset_url;

use super::queries;

/// `plane.db.models.project.ROLE.GUEST` (5). A project member with this role on
/// a project whose `guest_view_all_features` is off only reaches the assets of
/// issues they created (`plane/db/models/asset.py:136`).
const ROLE_GUEST: i16 = 5;

/// `HttpResponseRedirect`'s content type (`django/http/response.py:190-205`): a
/// 302 has an empty body, this header and `Content-Length: 0`.
const HTML_UTF8: &str = "text/html; charset=utf-8";

/// The caller's `request.user.id`.
pub(crate) fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}

/// `view.kwargs["<name>"]` (`KeyError` → 400 `The required key does not exist.`).
pub(crate) fn kwarg<'a>(ctx: &'a ViewCtx<'a>, name: &str) -> Result<&'a str, ViewError> {
    ctx.kwarg(name).ok_or(ViewError::Key)
}

/// A UUID URL kwarg. The router already enforces Django's `<uuid:…>` converter,
/// so a parse failure is unreachable; the variant keeps the Python mapping
/// (`ValidationError` → 400) if it ever is not.
pub(crate) fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &str) -> Result<Uuid, ViewError> {
    let value = kwarg(ctx, name)?;
    Uuid::parse_str(value).map_err(|_| ViewError::Validation)
}

/// `sanitize_filename` (`plane/utils/path_validator.py:14-51`), the upload
/// name sanitiser every asset write calls:
///
/// ```python
/// if not filename or not isinstance(filename, str):
///     return None
/// filename = "".join(char for char in filename if not (ord(char) < 32 or ord(char) == 127))
/// filename = filename.replace("\\", "/")
/// filename = os.path.basename(filename)
/// filename = filename.replace("..", "")
/// filename = filename.strip()
/// filename = filename.lstrip(".")
/// filename = filename.strip()
/// if not filename:
///     return None
/// return filename
/// ```
///
/// The control-character predicate is `ord(char) < 32 or ord(char) == 127`,
/// i.e. `char::is_ascii_control`, not `char::is_control` (which would also drop
/// U+0085 and friends). `os.path.basename` is POSIX: the substring after the
/// last `/`, or `""` when the string ends in `/`. The two `strip()` calls are
/// `str.isspace()`-based, which is the same set as `char::is_whitespace` once
/// the control characters are gone.
///
/// The Python accepts `None` and non-strings and returns `None`; callers pass
/// the already-extracted body string, so `None` is the "not a string" answer.
pub(crate) fn sanitize_filename(filename: &str) -> Option<String> {
    let cleaned: String = filename
        .chars()
        .filter(|character| !character.is_ascii_control())
        .collect();
    // `filename.replace("\\", "/")` before the basename, so a Windows-style
    // path is stripped on POSIX too.
    let cleaned = cleaned.replace('\\', "/");
    // `os.path.basename`: the Python splits on "/" only.
    let basename = cleaned.rsplit('/').next().unwrap_or("");
    let cleaned = basename
        .replace("..", "")
        .trim()
        .trim_start_matches('.')
        .trim()
        .to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// `can_modify_asset(user, asset)` (`plane/app/views/asset/v2.py:31-67`): the
/// whole authorisation rule of the asset writes, shared by the v1 and v2
/// route sets.
///
/// Two role lookups, in the Python's order:
///
/// 1. `WorkspaceMember.objects.filter(member=user, workspace_id=…,
///    is_active=True).values_list("role", flat=True).first()` — the exact
///    statement of `plane_permissions::queries::WORKSPACE_MEMBER_ROLE_BY_ID`
///    (the model orders `-created_at`, which is that statement's `ORDER BY`).
/// 2. only when `asset.project_id` is set:
///    `ProjectMember.objects.filter(member=user, workspace_id=…,
///    project_id=…, is_active=True).values_list("role").first()` — the
///    `workspace_id` variant of `PROJECT_MEMBER_ROLE`.
///
/// Both `.first()` calls are `Option<i16>`: no row is `None`, and `None` never
/// equals a role.
///
/// The decision itself is [`decide`], a pure function, so every branch is
/// unit-tested without a database.
///
/// # Errors
///
/// A database failure from either lookup.
#[allow(
    dead_code,
    reason = "the assets write sets (plans/ASSETS.md A/B/C) call it; P0 lands it first"
)]
pub(crate) async fn can_modify_asset(
    client: &Client,
    user_id: Uuid,
    asset: &FileAsset,
) -> Result<bool, ViewError> {
    // `filter(workspace_id=None)` is `IS NULL`, which matches nothing because
    // the column is NOT NULL; skip the round trip for a workspace-less asset
    // (a user avatar/cover, which 20/21 never authorise).
    let workspace_role = match asset.workspace_id {
        Some(workspace_id) => {
            let statement = client
                .prepare_cached(plane_permissions::queries::WORKSPACE_MEMBER_ROLE_BY_ID.sql)
                .await?;
            let row = client
                .query_opt(&statement, &[&workspace_id, &user_id])
                .await?;
            row.map(|row| row.get(0))
        }
        None => None,
    };

    let mut project_role = None;
    if let Some(project_id) = asset.project_id {
        let statement = client
            .prepare_cached(queries::PROJECT_MEMBER_ROLE_BY_IDS.sql)
            .await?;
        let row = client
            .query_opt(&statement, &[&user_id, &project_id, &asset.workspace_id])
            .await?;
        project_role = row.map(|row| row.get(0));
    }

    Ok(decide(
        workspace_role,
        asset.project_id.is_some(),
        project_role,
        asset.entity_type.as_deref(),
        asset.created_by_id == Some(user_id),
    ))
}

/// The pure half of [`can_modify_asset`], line by line (`v2.py:45-67`).
///
/// `has_project` is `asset.project_id is not None`: when it is true the Python
/// **replaces** `role` with the project lookup even when that returns `None`,
/// so a workspace role only survives on a workspace-level asset. `project_role`
/// is only meaningful then.
fn decide(
    workspace_role: Option<i16>,
    has_project: bool,
    project_role: Option<i16>,
    entity_type: Option<&str>,
    is_uploader: bool,
) -> bool {
    let admin = Some(Role::Admin.value());
    if entity_type == Some("WORKSPACE_LOGO") {
        return workspace_role == admin;
    }

    let mut role = workspace_role;
    if has_project {
        role = project_role;
        // Workspace admins act as admins in the projects they belong to, as in
        // `allow_permission`; the promotion needs a project membership.
        if role.is_some() && workspace_role == admin {
            role = admin;
        }
        if entity_type == Some("PROJECT_COVER") {
            return role == admin;
        }
    }

    if is_uploader && role.is_some() {
        return true;
    }
    if entity_type == Some("ISSUE_ATTACHMENT") {
        return role == admin;
    }
    matches!(role, Some(value) if value == Role::Admin.value() || value == Role::Member.value())
}

/// The `attributes` members the asset views read.
///
/// Python sees the `jsonb` column as a `dict` (psycopg decodes it); only `name`
/// (the `Content-Disposition` filename) and `type` (the inline MIME type) are
/// ever read, so this decodes them with `serde_json` instead of building a
/// `plane_serde` value tree.
pub(super) struct Attributes(serde_json::Map<String, serde_json::Value>);

impl Attributes {
    pub(super) fn parse(text: &str) -> Self {
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(serde_json::Value::Object(map)) => Self(map),
            // A non-object decodes like an empty dict: `isinstance` checks in
            // the Python turn every member into "absent".
            _ => Self(serde_json::Map::new()),
        }
    }

    /// `attributes.get("name")`, as the storage helper accepts it: only a
    /// non-empty `str` is used, anything else becomes a random name
    /// (`plane/settings/storage.py:104-112`).
    pub(super) fn name(&self) -> Option<&str> {
        self.0
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.is_empty())
    }

    /// `attributes.get("type")`.
    pub(crate) fn mime_type(&self) -> Option<&str> {
        self.0.get("type").and_then(serde_json::Value::as_str)
    }
}

/// The storage client, built once per process from `Settings` (see the module
/// docs for why this is not on `ViewCtx`).
pub(crate) fn storage(settings: &Settings) -> Result<&'static Storage, ViewError> {
    static STORAGE: OnceLock<Result<Storage, String>> = OnceLock::new();
    match STORAGE.get_or_init(|| {
        Storage::from_process_env(&settings.storage).map_err(|error| error.to_string())
    }) {
        Ok(storage) => Ok(storage),
        // Django validates `AWS_REGION` / `SIGNED_URL_EXPIRATION` when the view
        // constructs `S3Storage`, so a bad setting is a 500 on this route and
        // not a startup failure (`plane/settings/storage.py:40,50-66`).
        Err(message) => Err(ViewError::Unknown(format!(
            "object storage is not configured: {message}"
        ))),
    }
}

/// `S3Storage(request=request).generate_presigned_url(…)` followed by
/// `HttpResponseRedirect`.
///
/// `pub(crate)` since the public asset `GET`
/// (`plane/space/views/asset.py:76-85`) uses the identical presign with
/// `disposition="inline"` and the asset's own `attributes["type"]`.
///
/// The key is `view.asset` and the filename `view.attributes["name"]`; the
/// disposition and the optional `Content-Type` pin come from the caller, as in
/// the Python.
pub(crate) async fn presigned_redirect(
    ctx: &ViewCtx<'_>,
    asset: &FileAsset,
    disposition: &str,
    content_type: Option<&str>,
) -> Result<ViewResponse, ViewError> {
    let attributes = Attributes::parse(asset.attributes.as_str());
    let storage = storage(ctx.settings)?;
    let request = RequestInfo {
        scheme: if ctx.ctx.is_secure { "https" } else { "http" },
        host: ctx.ctx.header("host").unwrap_or_default(),
    };
    let signed_url = storage
        .request(request)
        .presign_get(PresignGet {
            key: &asset.asset,
            expiration: None,
            disposition,
            filename: attributes.name(),
            content_type,
        })
        .await
        .map_err(|error| {
            ViewError::Unknown(format!("generating a presigned URL failed: {error}"))
        })?;
    Ok(redirect(&signed_url))
}

/// `django.http.HttpResponseRedirect(signed_url)`.
pub(super) fn redirect(location: &str) -> ViewResponse {
    ViewResponse::raw(302, HTML_UTF8, Vec::new()).with_header("Location", location)
}

/// `asset.attributes.get("type")`, the `content_type` pin of a presigned
/// `generate_presigned_url` call.
pub(crate) fn attribute_type(asset: &FileAsset) -> Option<String> {
    Attributes::parse(asset.attributes.as_str())
        .mime_type()
        .map(str::to_owned)
}

/// `S3Storage.generate_presigned_post(...)`'s return value as the JSON object
/// the views embed (`plane/settings/storage.py:68-102`): `{"url": …, "fields":
/// {…}}`, with boto3's field order preserved and an absent value rendered as
/// `null`.
///
/// The `fields` order is part of the response bytes: `policy` and
/// `x-amz-signature` follow the client-side fields
/// (`crates/plane-storage/src/storage.rs:465-471`), and the harness compares
/// the order.
pub(crate) fn upload_data(post: &PresignedPost) -> Json {
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
    Json::Dict(upload)
}

/// The 3-key presigned-upload body of the v2 upload routes
/// (`plane/app/views/asset/v2.py:203-210`, `:432-439`, `:674-681`):
///
/// ```python
/// return Response(
///     {
///         "upload_data": presigned_url,
///         "asset_id": str(asset.id),
///         "asset_url": asset.asset_url,
///     },
///     status=status.HTTP_200_OK,
/// )
/// ```
///
/// `workspace_slug` is `asset.workspace.slug` (the instance is created with the
/// workspace already assigned, so no query runs); `asset_url` is
/// [`file_asset_url`]. The keys are in Python dict order, not sorted.
///
/// The issue-attachment response adds a fourth `attachment` key *before*
/// `asset_url` (`attachment.py:170-178`), so it builds its body from
/// [`upload_data`] instead.
///
/// # Errors
///
/// A render failure, or the `AttributeError` `file_asset_url` reports for a
/// non-static asset without a workspace.
#[allow(
    dead_code,
    reason = "the assets write sets (plans/ASSETS.md A/B) call it; P0 lands it first"
)]
pub(crate) fn presign_upload_body(
    asset: &FileAsset,
    workspace_slug: Option<&str>,
    post: &PresignedPost,
) -> Result<Vec<u8>, ViewError> {
    let asset_url = file_asset_url(
        asset.id,
        asset.entity_type.as_deref(),
        workspace_slug,
        asset.project_id,
        asset.issue_id,
    )?;
    let mut body = PyDict::new();
    body.insert(PyStr::from("upload_data"), upload_data(post));
    body.insert(
        PyStr::from("asset_id"),
        Json::Str(PyStr::from(asset.id.to_string())),
    );
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

/// A JSON object built in the view (`Response({...})`), in dict order.
pub(crate) fn json_object(status: u16, object: &Object) -> Result<ViewResponse, ViewError> {
    let body = plane_serde::to_vec(plane_serde::Dialect::Drf, object)
        .map_err(|error| ViewError::Unknown(format!("rendering the response failed: {error}")))?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// `{"data": [...], "status": true}` (`plane/app/views/asset/base.py:33`).
#[derive(Debug)]
pub(super) struct DataEnvelope<'a, T: DrfSerialize> {
    pub(super) data: &'a [T],
}

impl<T: DrfSerialize> DrfSerialize for DataEnvelope<'_, T> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field_nested_list("data", ctx, self.data)?;
        object.field("status", &true)?;
        object.end();
        Ok(())
    }
}

/// `FileAssetSerializer(asset, context={"request": request})`'s fields.
///
/// `asset_url` is the absolute URI DRF's `FileField` writes; the caller owns it
/// so it outlives the serializer.
pub(super) fn file_asset_serializer<'a>(
    asset: &'a FileAsset,
    asset_url: &'a str,
) -> FileAssetSerializer<'a> {
    FileAssetSerializer {
        id: asset.id,
        created_at: asset.created_at,
        updated_at: asset.updated_at,
        deleted_at: asset.deleted_at,
        attributes: Some(PgJson::from(&asset.attributes)),
        asset: asset_url,
        entity_type: asset.entity_type.as_deref(),
        entity_identifier: asset.entity_identifier.as_deref(),
        is_deleted: asset.is_deleted,
        is_archived: asset.is_archived,
        external_id: asset.external_id.as_deref(),
        external_source: asset.external_source.as_deref(),
        size: asset.size,
        is_uploaded: asset.is_uploaded,
        storage_metadata: asset.storage_metadata.as_ref().map(PgJson::from),
        created_by: asset.created_by_id,
        updated_by: asset.updated_by_id,
        user: asset.user_id,
        workspace: asset.workspace_id,
        draft_issue: asset.draft_issue_id,
        project: asset.project_id,
        issue: asset.issue_id,
        comment: asset.comment_id,
        page: asset.page_id,
    }
}

/// `IssueAttachmentSerializer(asset)` **without** a request context
/// (`plane/app/views/issue/attachment.py:240`): DRF's `FileField` therefore
/// renders `asset` as the storage's `url()` result, and `S3Storage.url()`
/// returns the name unchanged (`plane/settings/storage.py:23-24`).
pub(crate) fn issue_attachment_serializer<'a>(
    asset: &'a FileAsset,
    asset_url: Option<&'a str>,
) -> IssueAttachmentSerializer<'a> {
    IssueAttachmentSerializer {
        id: asset.id,
        asset_url,
        created_at: asset.created_at,
        updated_at: asset.updated_at,
        deleted_at: asset.deleted_at,
        attributes: Some(PgJson::from(&asset.attributes)),
        asset: &asset.asset,
        entity_type: asset.entity_type.as_deref(),
        entity_identifier: asset.entity_identifier.as_deref(),
        is_deleted: asset.is_deleted,
        is_archived: asset.is_archived,
        external_id: asset.external_id.as_deref(),
        external_source: asset.external_source.as_deref(),
        size: asset.size,
        is_uploaded: asset.is_uploaded,
        storage_metadata: asset.storage_metadata.as_ref().map(PgJson::from),
        created_by: asset.created_by_id,
        updated_by: asset.updated_by_id,
        user: asset.user_id,
        workspace: asset.workspace_id,
        draft_issue: asset.draft_issue_id,
        project: asset.project_id,
        issue: asset.issue_id,
        comment: asset.comment_id,
        page: asset.page_id,
    }
}

/// The three outcomes of `is_project_accessible_to`'s restricted-guest branch.
enum Restricted {
    /// Not a restricted guest, no project, or no issue binding: reachable.
    Reachable,
    /// The asset belongs to this issue, whose ownership decides.
    Issue(Uuid),
    /// The asset belongs to a comment; the comment's issue decides.
    Comment(Uuid),
}

/// `FileAsset.is_project_accessible_to(user)` for a whole result set
/// (`plane/db/models/asset.py:106-146`).
///
/// Django calls this per asset inside a list comprehension
/// (`plane/app/views/asset/base.py:30`), so its membership and issue lookups are
/// N+1. Here the membership lookup is one statement for the whole page and the
/// issue-ownership lookup one more, while the single-asset routes keep the same
/// two round trips Django makes.
pub(super) async fn accessible_assets(
    client: &Client,
    user_id: Uuid,
    assets: &[FileAsset],
) -> Result<Vec<bool>, ViewError> {
    let mut accessible = vec![true; assets.len()];
    let mut restricted: Vec<(usize, Restricted)> = Vec::new();

    let scoped: Vec<usize> = (0..assets.len())
        .filter(|&index| assets[index].project_id.is_some())
        .collect();
    if scoped.is_empty() {
        return Ok(accessible);
    }

    let mut project_ids: Vec<Uuid> = scoped
        .iter()
        .filter_map(|&index| assets[index].project_id)
        .collect();
    project_ids.sort_unstable();
    project_ids.dedup();
    let mut workspace_ids: Vec<Uuid> = scoped
        .iter()
        .filter_map(|&index| assets[index].workspace_id)
        .collect();
    workspace_ids.sort_unstable();
    workspace_ids.dedup();

    let rows = client
        .query(
            queries::PROJECT_MEMBERSHIPS_FOR_USER.sql,
            &[&user_id, &project_ids, &workspace_ids],
        )
        .await?;
    // `.first()` on the ORM query: the newest row wins for a duplicated
    // membership, which `ORDER BY created_at DESC` provides.
    let mut memberships: HashMap<Uuid, (i16, bool)> = HashMap::with_capacity(rows.len());
    for row in &rows {
        memberships
            .entry(row.try_get(0)?)
            .or_insert((row.try_get::<_, i16>(1)?, row.try_get::<_, bool>(2)?));
    }

    for &index in &scoped {
        let asset = &assets[index];
        let Some(project_id) = asset.project_id else {
            continue;
        };
        let Some(&(role, guest_view_all_features)) = memberships.get(&project_id) else {
            accessible[index] = false;
            continue;
        };
        if role != ROLE_GUEST || guest_view_all_features {
            continue;
        }
        // `self.issue_id or (self.comment.issue_id if self.comment_id else None)`
        if let Some(issue_id) = asset.issue_id {
            restricted.push((index, Restricted::Issue(issue_id)));
        } else if let Some(comment_id) = asset.comment_id {
            restricted.push((index, Restricted::Comment(comment_id)));
        }
        // Neither id: `issue_id is None` → the asset is reachable.
    }

    // `self.comment.issue_id` for the comment-bound assets, one statement for
    // the whole set.
    let mut comment_ids: Vec<Uuid> = restricted
        .iter()
        .filter_map(|(_, kind)| match kind {
            Restricted::Comment(comment_id) => Some(*comment_id),
            _ => None,
        })
        .collect();
    if !comment_ids.is_empty() {
        comment_ids.sort_unstable();
        comment_ids.dedup();
        let statement = queries::COMMENT_ISSUE_IDS.prepare(client).await?;
        let rows = client.query(&statement, &[&comment_ids]).await?;
        let comments: HashMap<Uuid, Option<Uuid>> = rows
            .iter()
            .map(|row| {
                Ok((
                    row.try_get::<_, Uuid>(0)?,
                    row.try_get::<_, Option<Uuid>>(1)?,
                ))
            })
            .collect::<Result<_, tokio_postgres::Error>>()?;
        for entry in &mut restricted {
            if let Restricted::Comment(comment_id) = entry.1 {
                // `self.comment` raises `IssueComment.DoesNotExist` for a
                // dangling id, which `handle_exception` renders as a 404.
                match comments.get(&comment_id) {
                    Some(Some(issue_id)) => entry.1 = Restricted::Issue(*issue_id),
                    // A comment whose `issue_id` is NULL: `issue_id is None` →
                    // reachable.
                    Some(None) => entry.1 = Restricted::Reachable,
                    None => return Err(ViewError::ObjectDoesNotExist),
                }
            }
        }
    }

    let mut issue_ids: Vec<Uuid> = restricted
        .iter()
        .filter_map(|(_, kind)| match kind {
            Restricted::Issue(issue_id) => Some(*issue_id),
            _ => None,
        })
        .collect();
    if !issue_ids.is_empty() {
        issue_ids.sort_unstable();
        issue_ids.dedup();
        let statement = queries::ISSUES_OWNED_BY_USER.prepare(client).await?;
        let rows = client.query(&statement, &[&issue_ids, &user_id]).await?;
        let owned: Vec<(Uuid, Uuid)> = rows
            .iter()
            .map(|row| Ok((row.try_get::<_, Uuid>(0)?, row.try_get::<_, Uuid>(1)?)))
            .collect::<Result<_, tokio_postgres::Error>>()?;
        for (index, kind) in restricted {
            let Restricted::Issue(issue_id) = kind else {
                continue;
            };
            accessible[index] = owned
                .iter()
                .any(|&(id, project)| Some(project) == assets[index].project_id && id == issue_id);
        }
    }

    Ok(accessible)
}

/// `issue_hidden_from_guest(request, slug, project_id, issue_id)`
/// (`plane/app/permissions/base.py:106-141`).
pub(crate) async fn issue_hidden_from_guest(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ViewError> {
    let restricted = client
        .query_one(
            queries::PROJECT_IS_RESTRICTED_GUEST.sql,
            &[&slug, &project_id, &user_id],
        )
        .await?
        .try_get::<_, bool>(0)?;
    if !restricted {
        return Ok(false);
    }
    let owned = client
        .query_one(
            queries::ISSUE_OWNED_BY_USER_IN_WORKSPACE.sql,
            &[&issue_id, &slug, &project_id, &user_id],
        )
        .await?
        .try_get::<_, bool>(0)?;
    Ok(!owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_db::JsonText;

    const ADMIN: i16 = 20;
    const MEMBER: i16 = 15;
    const GUEST: i16 = 5;

    fn asset(entity_type: Option<&str>) -> FileAsset {
        FileAsset {
            created_at: chrono::DateTime::UNIX_EPOCH,
            updated_at: chrono::DateTime::UNIX_EPOCH,
            id: Uuid::nil(),
            attributes: JsonText::new("{}"),
            asset: "ws/uuid-name.png".to_owned(),
            created_by_id: None,
            updated_by_id: None,
            workspace_id: Some(Uuid::nil()),
            is_deleted: false,
            deleted_at: None,
            is_archived: false,
            comment_id: None,
            entity_type: entity_type.map(str::to_owned),
            external_id: None,
            external_source: None,
            is_uploaded: false,
            issue_id: None,
            page_id: None,
            project_id: None,
            size: 0.0,
            storage_metadata: Some(JsonText::new("{}")),
            user_id: None,
            draft_issue_id: None,
            entity_identifier: None,
        }
    }

    #[test]
    fn attributes_read_only_non_empty_string_members() {
        let attributes =
            Attributes::parse(r#"{"name":"spec.pdf","type":"application/pdf","size":10}"#);
        assert_eq!(attributes.name(), Some("spec.pdf"));
        assert_eq!(attributes.mime_type(), Some("application/pdf"));
        // `_get_content_disposition` rejects anything but a non-empty `str`.
        assert_eq!(Attributes::parse(r#"{"name":""}"#).name(), None);
        assert_eq!(Attributes::parse(r#"{"name":null}"#).name(), None);
        assert_eq!(Attributes::parse(r#"{"name":7}"#).name(), None);
        assert_eq!(Attributes::parse("[]").name(), None);
        assert_eq!(Attributes::parse("not json").mime_type(), None);
    }

    // -- can_modify_asset (`plane/app/views/asset/v2.py:31-67`) ------------

    /// The branch matrix from the plan's §2b and §4, one assert per Python
    /// line. `has_project` is `asset.project_id is not None`.
    #[test]
    fn can_modify_asset_follows_the_python_branch_order() {
        // WORKSPACE_LOGO: workspace ADMIN only, even for the uploader.
        assert!(decide(
            Some(ADMIN),
            false,
            None,
            Some("WORKSPACE_LOGO"),
            false
        ));
        assert!(decide(
            Some(ADMIN),
            false,
            None,
            Some("WORKSPACE_LOGO"),
            true
        ));
        assert!(!decide(
            Some(MEMBER),
            false,
            None,
            Some("WORKSPACE_LOGO"),
            true
        ));
        assert!(!decide(
            Some(GUEST),
            false,
            None,
            Some("WORKSPACE_LOGO"),
            false
        ));
        assert!(!decide(None, false, None, Some("WORKSPACE_LOGO"), true));

        // No project: the workspace role decides.
        assert!(decide(Some(ADMIN), false, None, None, false));
        assert!(decide(Some(MEMBER), false, None, None, false));
        assert!(!decide(Some(GUEST), false, None, None, false));
        assert!(!decide(None, false, None, None, false));
        // The uploader branch needs a role.
        assert!(decide(Some(MEMBER), false, None, None, true));
        assert!(!decide(None, false, None, None, true));

        // PROJECT_COVER: the project role decides, and the uploader check is
        // unreachable (the cover branch returns first).
        assert!(decide(
            Some(ADMIN),
            true,
            Some(ADMIN),
            Some("PROJECT_COVER"),
            false
        ));
        // The workspace-admin promotion also makes a cover writable, and it
        // outranks the plain project role.
        assert!(decide(
            Some(ADMIN),
            true,
            Some(MEMBER),
            Some("PROJECT_COVER"),
            true
        ));
        assert!(!decide(
            Some(MEMBER),
            true,
            Some(MEMBER),
            Some("PROJECT_COVER"),
            true
        ));
        assert!(!decide(
            Some(ADMIN),
            true,
            None,
            Some("PROJECT_COVER"),
            true
        ));
        // A workspace admin who is a project member is promoted to project
        // admin; the promotion needs the membership row.
        assert!(decide(
            Some(ADMIN),
            true,
            Some(MEMBER),
            Some("PROJECT_COVER"),
            false
        ));
        assert!(!decide(
            Some(ADMIN),
            true,
            None,
            Some("PROJECT_COVER"),
            false
        ));
        // A workspace MEMBER who is a project ADMIN (the fixture's beta) passes.
        assert!(decide(
            Some(MEMBER),
            true,
            Some(ADMIN),
            Some("PROJECT_COVER"),
            false
        ));

        // ISSUE_ATTACHMENT: project ADMIN, or the uploader with a role.
        assert!(decide(
            Some(MEMBER),
            true,
            Some(ADMIN),
            Some("ISSUE_ATTACHMENT"),
            false
        ));
        assert!(!decide(
            Some(MEMBER),
            true,
            Some(MEMBER),
            Some("ISSUE_ATTACHMENT"),
            false
        ));
        assert!(decide(
            Some(MEMBER),
            true,
            Some(MEMBER),
            Some("ISSUE_ATTACHMENT"),
            true
        ));
        assert!(!decide(
            Some(MEMBER),
            true,
            None,
            Some("ISSUE_ATTACHMENT"),
            true
        ));
        // The workspace admin promotion also applies to an attachment.
        assert!(decide(
            Some(ADMIN),
            true,
            Some(MEMBER),
            Some("ISSUE_ATTACHMENT"),
            false
        ));

        // Any other entity type: MEMBER/ADMIN, or the uploader with a role.
        assert!(decide(
            Some(MEMBER),
            true,
            Some(MEMBER),
            Some("ISSUE_DESCRIPTION"),
            false
        ));
        assert!(!decide(
            Some(MEMBER),
            true,
            Some(GUEST),
            Some("ISSUE_DESCRIPTION"),
            false
        ));
        assert!(decide(
            Some(GUEST),
            true,
            Some(GUEST),
            Some("ISSUE_DESCRIPTION"),
            true
        ));
        assert!(!decide(
            Some(GUEST),
            true,
            None,
            Some("ISSUE_DESCRIPTION"),
            true
        ));
        // A workspace role does not survive a project-bound asset: the project
        // lookup replaces it, `None` included.
        assert!(!decide(Some(ADMIN), true, None, None, false));
        assert!(!decide(Some(MEMBER), true, None, None, false));
    }

    #[test]
    fn a_workspace_less_asset_never_matches_a_project_rule() {
        // `asset.workspace_id = None` is what the user avatar/cover routes
        // produce; they never call `can_modify_asset`, but the rule still
        // answers False for everything that needs a role.
        assert!(!decide(None, false, None, Some("USER_AVATAR"), false));
        assert!(!decide(None, false, None, Some("USER_COVER"), true));
    }

    // -- sanitize_filename (`plane/utils/path_validator.py:14-51`) ---------

    /// Expected values recorded from the real
    /// `plane.utils.path_validator.sanitize_filename` (CPython 3.12.13, the
    /// venv), one row per branch of the function.
    #[test]
    fn sanitize_filename_matches_the_python() {
        let cases: &[(&str, Option<&str>)] = &[
            ("spec.pdf", Some("spec.pdf")),
            ("  spec.pdf  ", Some("spec.pdf")),
            ("a/b.txt", Some("b.txt")),
            ("/a/b.txt", Some("b.txt")),
            ("a/", None),
            ("/", None),
            ("////", None),
            ("../etc/passwd", Some("passwd")),
            ("foo/bar/..", None),
            ("..hidden", Some("hidden")),
            (".hidden", Some("hidden")),
            ("....", None),
            ("...", None),
            (".", None),
            (" .env", Some("env")),
            ("a..b", Some("ab")),
            ("a...b", Some("a.b")),
            ("a .. b ", Some("a  b")),
            ("file.tar.gz", Some("file.tar.gz")),
            ("café.txt", Some("café.txt")),
            ("\u{3000}name\u{3000}", Some("name")),
            ("\u{a0}name\u{a0}", Some("name")),
            ("a\u{0}b", Some("ab")),
            ("\u{1b}[31mred\u{1b}[0m", Some("[31mred[0m")),
            ("del\u{7f}char", Some("delchar")),
            ("back\\\\slash\\name.txt", Some("name.txt")),
            ("a\\b\\c.txt", Some("c.txt")),
            ("a/b\\c.txt", Some("c.txt")),
            ("\u{200b}name", Some("\u{200b}name")),
            ("a\tb", Some("ab")),
            ("a\nb", Some("ab")),
            ("", None),
            (" ", None),
            ("  ..  ", None),
            ("..\u{3000}name", Some("name")),
            ("CON", Some("CON")),
            ("a/b/../c", Some("c")),
            ("x/./y", Some("y")),
            ("\\..\\x", Some("x")),
            ("a/b/", None),
        ];
        for (input, expected) in cases {
            assert_eq!(
                sanitize_filename(input).as_deref(),
                *expected,
                "input {input:?}"
            );
        }
    }

    // -- the 3-key presign body (`v2.py:203-210`, `:432-439`, `:674-681`) --

    fn presigned_post() -> PresignedPost {
        PresignedPost {
            url: "http://plane-fixture.test/uploads".to_owned(),
            fields: vec![
                ("Content-Type".to_owned(), Some("image/png".to_owned())),
                (
                    "key".to_owned(),
                    Some("a91c3d80-5ed0-40f3-92a8-a28c015f4388/deadbeef-logo.png".to_owned()),
                ),
                ("policy".to_owned(), Some("cG9saWN5".to_owned())),
                ("x-amz-signature".to_owned(), Some("0f1e2d3c".to_owned())),
                ("x-amz-missing".to_owned(), None),
            ],
        }
    }

    /// Python order (`json.dumps` over the view's dict literal), compact:
    /// `upload_data` → `asset_id` → `asset_url`, and inside `upload_data`
    /// `url` → `fields`, with boto3's field order kept.
    #[test]
    fn the_presign_body_has_the_three_keys_in_python_order() {
        let mut asset = asset(Some("WORKSPACE_LOGO"));
        asset.id = Uuid::parse_str("e704fbad-88e7-4a04-ad34-ee44588b924e").unwrap();
        let body = presign_upload_body(&asset, Some("fix-workspace"), &presigned_post())
            .expect("a static asset needs no workspace slug");
        assert_eq!(
            String::from_utf8(body).expect("utf-8"),
            concat!(
                r#"{"upload_data":{"url":"http://plane-fixture.test/uploads","fields":{"#,
                r#""Content-Type":"image/png","#,
                r#""key":"a91c3d80-5ed0-40f3-92a8-a28c015f4388/deadbeef-logo.png","#,
                r#""policy":"cG9saWN5","#,
                r#""x-amz-signature":"0f1e2d3c","#,
                r#""x-amz-missing":null}},"#,
                r#""asset_id":"e704fbad-88e7-4a04-ad34-ee44588b924e","#,
                r#""asset_url":"/api/assets/v2/static/e704fbad-88e7-4a04-ad34-ee44588b924e/"}"#,
            )
        );
    }

    /// A non-static asset's `asset_url` reads `self.workspace.slug`; the
    /// argument is that slug, and `None` is the Python `AttributeError` (a
    /// family 500), not `"null"`.
    #[test]
    fn a_project_asset_uses_the_workspace_slug() {
        let mut asset = asset(Some("ISSUE_ATTACHMENT"));
        asset.id = Uuid::parse_str("80bc6632-0e4a-4d3e-9c8f-ec0b9bf351e9").unwrap();
        asset.project_id = Some(Uuid::parse_str("b0063033-54f9-49d8-b5f3-6b7a1551c6a6").unwrap());
        asset.issue_id = Some(Uuid::parse_str("d432015f-5a0f-4904-a2eb-d8bfea948a29").unwrap());
        let body = presign_upload_body(&asset, Some("fix-workspace"), &presigned_post())
            .expect("the slug is supplied");
        let body = String::from_utf8(body).expect("utf-8");
        assert!(
            body.ends_with(concat!(
                r#""asset_url":"/api/assets/v2/workspaces/fix-workspace/projects/"#,
                r#"b0063033-54f9-49d8-b5f3-6b7a1551c6a6/issues/"#,
                r#"d432015f-5a0f-4904-a2eb-d8bfea948a29/attachments/"#,
                r#"80bc6632-0e4a-4d3e-9c8f-ec0b9bf351e9/"}"#
            )),
            "{body}"
        );
        assert!(presign_upload_body(&asset, None, &presigned_post()).is_err());
    }

    /// The issue-attachment response reuses [`upload_data`] for its fourth-key
    /// body, so the nested object is the same value.
    #[test]
    fn upload_data_is_the_nested_mapping_alone() {
        let value = plane_serde::to_vec(plane_serde::Dialect::Drf, &upload_data(&presigned_post()))
            .expect("render");
        assert_eq!(
            String::from_utf8(value).expect("utf-8"),
            concat!(
                r#"{"url":"http://plane-fixture.test/uploads","fields":{"#,
                r#""Content-Type":"image/png","#,
                r#""key":"a91c3d80-5ed0-40f3-92a8-a28c015f4388/deadbeef-logo.png","#,
                r#""policy":"cG9saWN5","#,
                r#""x-amz-signature":"0f1e2d3c","#,
                r#""x-amz-missing":null}}"#,
            )
        );
    }
}
