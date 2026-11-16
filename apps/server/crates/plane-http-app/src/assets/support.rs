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
//! * `S3Storage.generate_presigned_url` (`plane/settings/storage.py:114-151`).
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
use plane_serializers::generated::app_asset::FileAssetSerializer;
use plane_serializers::generated::app_issue::IssueAttachmentSerializer;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, PgJson, RenderCtx};
use plane_storage::{PresignGet, RequestInfo, Storage};
use uuid::Uuid;

use super::queries;

/// `plane.db.models.project.ROLE.GUEST` (5). A project member with this role on
/// a project whose `guest_view_all_features` is off only reaches the assets of
/// issues they created (`plane/db/models/asset.py:136`).
const ROLE_GUEST: i16 = 5;

/// `HttpResponseRedirect`'s content type (`django/http/response.py:190-205`): a
/// 302 has an empty body, this header and `Content-Length: 0`.
const HTML_UTF8: &str = "text/html; charset=utf-8";

/// The caller's `request.user.id`.
pub(super) fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}

/// `view.kwargs["<name>"]` (`KeyError` → 400 `The required key does not exist.`).
pub(super) fn kwarg<'a>(ctx: &'a ViewCtx<'a>, name: &str) -> Result<&'a str, ViewError> {
    ctx.kwarg(name).ok_or(ViewError::Key)
}

/// A UUID URL kwarg. The router already enforces Django's `<uuid:…>` converter,
/// so a parse failure is unreachable; the variant keeps the Python mapping
/// (`ValidationError` → 400) if it ever is not.
pub(super) fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &str) -> Result<Uuid, ViewError> {
    let value = kwarg(ctx, name)?;
    Uuid::parse_str(value).map_err(|_| ViewError::Validation)
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
    pub(super) fn mime_type(&self) -> Option<&str> {
        self.0.get("type").and_then(serde_json::Value::as_str)
    }
}

/// The storage client, built once per process from `Settings` (see the module
/// docs for why this is not on `ViewCtx`).
fn storage(settings: &Settings) -> Result<&'static Storage, ViewError> {
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
/// The key is `view.asset` and the filename `view.attributes["name"]`; the
/// disposition and the optional `Content-Type` pin come from the caller, as in
/// the Python.
pub(super) async fn presigned_redirect(
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

/// A JSON object built in the view (`Response({...})`), in dict order.
pub(super) fn json_object(status: u16, object: &Object) -> Result<ViewResponse, ViewError> {
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
pub(super) fn issue_attachment_serializer<'a>(
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
        let rows = client
            .query(queries::COMMENT_ISSUE_IDS.sql, &[&comment_ids])
            .await?;
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
        let rows = client
            .query(queries::ISSUES_OWNED_BY_USER.sql, &[&issue_ids, &user_id])
            .await?;
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
pub(super) async fn issue_hidden_from_guest(
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
}
