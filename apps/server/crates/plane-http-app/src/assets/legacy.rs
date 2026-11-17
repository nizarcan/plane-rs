// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The legacy (`v1`) asset writes — `plans/ASSETS.md` set B, routes 13-17 of
//! `ROUTES.md`:
//!
//! | # | Route | Verb | Python |
//! | --- | --- | --- | --- |
//! | 13 | `/api/workspaces/{slug}/file-assets/` | POST | `FileAssetEndpoint.post` (`base.py:40-53`) |
//! | 13 | the same | DELETE | `FileAssetEndpoint.delete` (`base.py:55-65`) → **500** |
//! | 14 | `/api/workspaces/file-assets/{workspace_id}/{asset_key}/` | DELETE | `FileAssetEndpoint.delete` |
//! | 15 | `/api/users/file-assets/` | POST | `UserAssetsEndpoint.post` (`base.py:98-103`) |
//! | 15 | the same | DELETE | `UserAssetsEndpoint.delete` (`base.py:105-109`) → **500** |
//! | 16 | `/api/users/file-assets/{asset_key}/` | POST | `UserAssetsEndpoint.post` → **500** |
//! | 16 | the same | DELETE | `UserAssetsEndpoint.delete` |
//! | 17 | `.../file-assets/{workspace_id}/{asset_key}/restore/` | POST | `FileAssetViewSet.restore` (`base.py:71-81`) |
//!
//! ## The upload 500s
//!
//! Every *valid* upload is an unconditional 500, on all three POST handlers:
//! `FileAssetSerializer.save()` → `FileField.pre_save` → `S3Storage.save` →
//! `AttributeError: 'S3Storage' object has no attribute 'file_overwrite'`.
//! Plane's `S3Storage.__init__` (`plane/settings/storage.py:28-66`) never calls
//! `S3Boto3Storage.__init__`, so the parent's attributes are missing with the
//! pinned `django-storages` 1.14.6, before any request reaches S3. No row and no
//! task is written; the family `handle_exception` answers
//! `{"error": "Something went wrong please try again later"}`
//! (`plane/app/views/base.py:235-239`). Not ported: the workspace fetch
//! `get_upload_path` does through `instance.workspace` (`plane/db/models/
//! asset.py:20-24`) and the INSERT Django rolls into the same statement — the
//! failure happens before either is observable.
//!
//! The wrong-verb 500s are the same class: `routes.json` maps *every* verb of a
//! class onto *both* URL shapes, and the two shapes do not supply the same
//! positional arguments, so Python raises `TypeError` before the method body:
//!
//! * 13 DELETE: `delete(self, request, workspace_id, asset_key)` called with
//!   only `slug` (`base.py:55`).
//! * 15 DELETE: `delete(self, request, asset_key)` called with no kwargs
//!   (`base.py:105`).
//! * 16 POST: `post(self, request)` called with `asset_key=…` (`base.py:98`).
//!
//! Because the `TypeError` happens at call time, 16's POST never parses the
//! body: a JSON body there is a 500, not 15's 415 (`UserAssetsEndpoint` has no
//! `JSONParser`, `base.py:85`). All three run *after* the permission classes,
//! so a non-member gets 13's/15's 403/401 rather than the 500.
//!
//! Route 14's POST is the one verb that is not a 500: `as_view(http_method_names=
//! ["get", "delete", "head", "options"])` (`plane/app/urls/asset.py:31-36`)
//! makes it a 405, which the runtime derives from the route table. It is not
//! registered here.
//!
//! ## `is_deleted` vs `deleted_at`
//!
//! Both deletes and the restore write **only** `is_deleted`
//! (`save(update_fields=["is_deleted"])`, `base.py:64`, `:80`, `:108`), never
//! `deleted_at` and never `updated_at`/`updated_by`. `FileAsset.objects` filters
//! `deleted_at IS NULL` (`plane/db/mixins.py:57`), so such a row stays visible
//! to every legacy route and to `AssetCheckEndpoint`; the v2 routes
//! (`assets/v2.rs`, set A) write both columns. That asymmetry is why 17 can
//! restore a row 14 just deleted but 404s the fixture's `deleted_at` row.
//!
//! ## Cardinality
//!
//! 13/14/15/16/17 use `QuerySet.get()`, so two live rows with the same `asset`
//! key (the column is indexed, not unique) are `MultipleObjectsReturned` → the
//! family 500, while zero rows are `DoesNotExist` → 404
//! `{"error": "The required object does not exist."}`. See
//! [`super::write_b_queries`].
//!
//! ## Authorisation
//!
//! `IsAuthenticated` and `WorkspaceMemberPermission` are the route's
//! `permission_classes` and run in the dispatcher before the handler. 13/14/17
//! then call `can_modify_asset` (`super::support`), whose 403 body is
//! `{"error": "You don't have the required permissions."}` — different from the
//! route-level 403 (`{"detail": "You do not have permission to perform this
//! action."}`). 15/16 have no workspace: they authorise by `created_by` alone,
//! so another user's asset is a 404, not a 403 (`base.py:106`).
//!
//! ## The serializer shim
//!
//! `FileAssetSerializer` (`plane/app/serializers/asset.py:8-37`) is
//! `fields = "__all__"` with every field but `asset` and `attributes` in
//! `read_only_fields`. Validation therefore visits two fields, in `self.fields`
//! order: `attributes` (DRF `JSONField`, optional, not nullable) and `asset`
//! (DRF `FileField`, required). [`validate_upload`] reuses
//! [`crate::write::Serializer`] for the first and adds the second by hand;
//! `FileField` has no Rust equivalent because a multipart file is not a JSON
//! value.
//!
//! Two `FileField` branches are unreachable through HTTP and are not
//! implemented:
//!
//! * `no_name` ("No filename could be determined.") — `MultiPartParser` skips a
//!   part whose sanitised filename is empty (`django/http/multipartparser.py:
//!   275-282`, `:384-405`), so no `InMemoryUploadedFile` ever has an empty name;
//! * `max_length` (800) — `UploadedFile._set_name` truncates a filename to 255
//!   characters (`django/core/files/uploadedfile.py:45-62`), so
//!   `len(file_name) > 800` can never hold.
//!
//! The remaining `FileField` messages, all of which the scenario covers, are
//! `required` ("No file was submitted."), `null` ("This field may not be
//! null."), `invalid` ("The submitted data was not a file. Check the encoding
//! type on the form.") and `empty` ("The submitted file is empty.").

use deadpool_postgres::Client;
use http::Method;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use crate::common::client;
use crate::common::lookup;
use crate::write;
use crate::write::fields::Detail;

use super::entities;
use super::support;
use super::write_b_queries;

/// `FileField.default_error_messages['invalid']`
/// (`rest_framework/fields.py:1563`).
const NOT_A_FILE: &str = "The submitted data was not a file. Check the encoding type on the form.";

/// Register the three classes' write verbs.
///
/// `FileAssetEndpoint` answers both route 13 and route 14 with one handler per
/// verb; the handler branches on `ctx.kwarg("asset_key")` the way Python's
/// signature does.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.asset.base.FileAssetEndpoint")?
        .method(Method::POST, file_asset_post)?
        .method(Method::DELETE, file_asset_delete)?;
    registry
        .api_view("plane.app.views.asset.base.UserAssetsEndpoint")?
        .method(Method::POST, user_assets_post)?
        .method(Method::DELETE, user_assets_delete)?;
    registry
        .viewset("plane.app.views.asset.base.FileAssetViewSet")?
        .action("restore", file_asset_restore)?;
    Ok(())
}

/// `FileAssetEndpoint.post` (`base.py:40-53`) — route 13.
fn file_asset_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        // `WorkspaceMemberPermission` already checked membership; the view
        // re-reads the row because `serializer.save(workspace_id=…)` needs its
        // id. A soft-deleted workspace is a `DoesNotExist` → 404 even for an
        // active member (`Workspace.objects` is the soft-deletion manager).
        let _workspace = lookup::workspace_by_slug(client(ctx)?, slug).await?;
        if let Some(response) = validate_upload(ctx)? {
            return Ok(response);
        }
        // Validated: the save is the storage `AttributeError` (module docs).
        Err(upload_failure())
    })
}

/// `FileAssetEndpoint.delete` (`base.py:55-65`) — routes 13 and 14.
fn file_asset_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let Some(asset_key) = ctx.kwarg("asset_key") else {
            // Route 13 supplies only `slug`: `delete() missing 2 required
            // positional arguments: 'workspace_id' and 'asset_key'`.
            return Err(ViewError::Unknown(
                "TypeError: FileAssetEndpoint.delete() missing 2 required positional \
                 arguments: 'workspace_id' and 'asset_key'"
                    .to_string(),
            ));
        };
        let workspace_id = support::uuid_kwarg(ctx, "workspace_id")?;
        // `asset_key = str(workspace_id) + "/" + asset_key` (`base.py:56`).
        let key = format!("{workspace_id}/{asset_key}");
        let client = client(ctx)?;
        let user_id = support::user_id(ctx)?;
        let asset = get_by_key(client, &key).await?;
        if !support::can_modify_asset(client, user_id, &asset).await? {
            return entities::no_permission();
        }
        set_is_deleted(client, asset.id, true).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `UserAssetsEndpoint.post` (`base.py:98-103`) — route 15, and route 16's
/// unexpected-kwarg 500.
fn user_assets_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("asset_key").is_some() {
            // Route 16: `post(self, request)` called with `asset_key=…`. Raised
            // at call time, so the body is never parsed and a JSON body is this
            // 500 rather than 15's 415.
            return Err(ViewError::Unknown(
                "TypeError: UserAssetsEndpoint.post() got an unexpected keyword argument \
                 'asset_key'"
                    .to_string(),
            ));
        }
        if let Some(response) = validate_upload(ctx)? {
            return Ok(response);
        }
        // `serializer.save()` with no workspace: the same storage 500.
        Err(upload_failure())
    })
}

/// `UserAssetsEndpoint.delete` (`base.py:105-109`) — routes 15 and 16.
fn user_assets_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let Some(asset_key) = ctx.kwarg("asset_key") else {
            // Route 15: `delete(self, request, asset_key)` with no kwargs.
            return Err(ViewError::Unknown(
                "TypeError: UserAssetsEndpoint.delete() missing 1 required positional \
                 argument: 'asset_key'"
                    .to_string(),
            ));
        };
        let user_id = support::user_id(ctx)?;
        let client = client(ctx)?;
        // There is no workspace permission on this class: `created_by` is the
        // whole authorisation rule, so another user's key is a 404.
        let asset = get_by_key_for_user(client, asset_key, user_id).await?;
        set_is_deleted(client, asset.id, true).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `FileAssetViewSet.restore` (`base.py:71-81`) — route 17.
fn file_asset_restore<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let workspace_id = support::uuid_kwarg(ctx, "workspace_id")?;
        let asset_key = support::kwarg(ctx, "asset_key")?;
        let key = format!("{workspace_id}/{asset_key}");
        let client = client(ctx)?;
        let user_id = support::user_id(ctx)?;
        let asset = get_by_key(client, &key).await?;
        if !support::can_modify_asset(client, user_id, &asset).await? {
            return entities::no_permission();
        }
        // `is_deleted = False`; `deleted_at` is not touched, so this only
        // undoes a legacy delete (module docs).
        set_is_deleted(client, asset.id, false).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `FileAsset.objects.get(asset=key)` (`base.py:57`, `:73`).
///
/// # Errors
///
/// 404 for no row, the family 500 for two (`MultipleObjectsReturned`).
async fn get_by_key(client: &Client, key: &str) -> Result<FileAsset, ViewError> {
    let statement = write_b_queries::FILE_ASSET_BY_ASSET_KEY
        .prepare(client)
        .await?;
    let rows = client.query(&statement, &[&key]).await?;
    exactly_one(&rows)
}

/// `FileAsset.objects.get(asset=key, created_by=request.user)`
/// (`base.py:106`).
///
/// # Errors
///
/// 404 for no row or another user's row, the family 500 for two.
async fn get_by_key_for_user(
    client: &Client,
    key: &str,
    user_id: Uuid,
) -> Result<FileAsset, ViewError> {
    let statement = write_b_queries::FILE_ASSET_BY_ASSET_KEY_FOR_USER
        .prepare(client)
        .await?;
    let rows = client.query(&statement, &[&key, &user_id]).await?;
    exactly_one(&rows)
}

/// `QuerySet.get()`'s cardinality rules: one row or an error.
fn exactly_one(rows: &[tokio_postgres::Row]) -> Result<FileAsset, ViewError> {
    match rows {
        [] => Err(ViewError::ObjectDoesNotExist),
        [row] => FileAsset::from_row(row, 0).map_err(ViewError::from),
        // The statement's `LIMIT 2` cannot return more.
        _ => Err(ViewError::Unknown(
            "FileAsset.MultipleObjectsReturned: get() returned more than one FileAsset".to_string(),
        )),
    }
}

/// `file_asset.is_deleted = value; file_asset.save(update_fields=["is_deleted"])`.
///
/// # Errors
///
/// A database failure as the family 500.
async fn set_is_deleted(
    client: &Client,
    asset_id: Uuid,
    is_deleted: bool,
) -> Result<(), ViewError> {
    let statement = write_b_queries::FILE_ASSET_SET_IS_DELETED
        .prepare(client)
        .await?;
    client
        .execute(&statement, &[&asset_id, &is_deleted])
        .await?;
    Ok(())
}

/// The `S3Storage` `AttributeError` every valid legacy upload ends in.
fn upload_failure() -> ViewError {
    ViewError::Unknown(
        "AttributeError: 'S3Storage' object has no attribute 'file_overwrite'".to_string(),
    )
}

/// `FileAssetSerializer(data=request.data)` + `is_valid()`
/// (`plane/app/serializers/asset.py:8-37`): `None` when the body validates, or
/// the 400 `Response(serializer.errors)`.
///
/// # Errors
///
/// The body parse errors DRF raises out of `request.data`: 400 `ParseError`,
/// 415 `UnsupportedMediaType`, 413 (`TooManyFields`).
fn validate_upload<'a>(ctx: &'a ViewCtx<'a>) -> Result<Option<ViewResponse>, ViewError> {
    let mut serializer = write::Serializer::new(ctx, false)?;
    // `attributes` is first in `self.fields` (`id`, `created_at`, … come before
    // it, but they are read-only and skipped), so its error key comes first.
    let _ = serializer.json_field("attributes", false);
    if let Some(detail) = asset_error(ctx) {
        serializer.push_error("asset", detail);
    }
    match serializer.finish() {
        Ok(_) => Ok(None),
        Err(errors) => Ok(Some(errors.into_response())),
    }
}

/// DRF's `FileField` for `asset` (`plane/db/models/asset.py:50`,
/// `required=True`, `allow_null=False`): the one error the body produces, in
/// `FileField.to_internal_value` order.
///
/// `None` means the field validated. A body that is not a mapping is DRF's
/// `non_field_errors` before any field runs, so it adds nothing here.
fn asset_error(ctx: &ViewCtx<'_>) -> Option<Detail> {
    let Ok(data) = ctx.data() else {
        // The parser failure is `validate_upload`'s `Serializer::new`; this
        // path is unreachable because it runs first.
        return None;
    };
    match data {
        Data::Json(Json::Dict(object)) => match object.get("asset") {
            // `Field.validate_empty_values` (`fields.py:494-530`).
            None => Some(Detail::new("No file was submitted.", "required")),
            Some(Json::Null) => Some(Detail::null()),
            // A string, number, list or dict has no `.name`/`.size`, so
            // `to_internal_value` fails `invalid` (`fields.py:1576-1582`).
            Some(_) => Some(Detail::new(NOT_A_FILE, "invalid")),
        },
        Data::Json(_) => None,
        Data::Form(query) => {
            let part = multipart_field(
                ctx.ctx.header("content-type").unwrap_or(""),
                ctx.ctx.body.as_deref().unwrap_or(&[]),
                "asset",
            );
            match part {
                Some(Part::File { size: 0 }) => {
                    // `InMemoryUploadedFile.size == 0` and `allow_empty_file`
                    // is False (`fields.py:1586-1587`).
                    Some(Detail::new("The submitted file is empty.", "empty"))
                }
                Some(Part::File { .. }) => None,
                // A part without a filename is a text field
                // (`django/http/multipartparser.py:265-282`); so is a plain
                // form value.
                Some(Part::Text) => Some(Detail::new(NOT_A_FILE, "invalid")),
                // No part: fall back to the parsed form, which is what
                // `request.data` holds when `request.FILES` is empty.
                None if query.contains_key("asset") => Some(Detail::new(NOT_A_FILE, "invalid")),
                None => Some(Detail::new("No file was submitted.", "required")),
            }
        }
    }
}

/// One multipart part named like a field.
enum Part {
    /// A `filename` was present and survives Django's `sanitize_file_name`:
    /// `MultiPartParser` builds an `InMemoryUploadedFile` and `FileField` sees
    /// `.name`/`.size`.
    File { size: usize },
    /// The part had no `filename`: it is a POST field, i.e. a string.
    Text,
}

/// The **last** multipart part named `field`, mirroring `MultiValueDict`'s
/// last-value-wins `__getitem__` and DRF's `_full_data.update(self._files)`.
///
/// The parser in `plane-drf` deliberately discards file parts (it records only
/// their names), so the raw body is re-read here for the one field whose
/// validation needs the filename and the byte length. The body was already
/// validated, so a malformed part simply means "not found".
fn multipart_field(content_type: &str, body: &[u8], field: &str) -> Option<Part> {
    if !content_type
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("multipart/form-data")
    {
        return None;
    }
    let boundary = content_type
        .split(';')
        .find_map(|parameter| parameter.trim().strip_prefix("boundary="))
        .map(|value| value.trim().trim_matches('"'))?;
    let delimiter = format!("--{boundary}");
    let mut cursor = find(body, delimiter.as_bytes())? + delimiter.len();
    let marker = format!("\r\n{delimiter}");
    // `request.data` is `{**POST, **FILES}` (`Request._load_data_and_files`), so
    // a file part replaces every text value of the same name; among several
    // files the last wins (`MultiValueDict.__getitem__` is `list[-1]`).
    let mut file = None;
    let mut text = false;
    loop {
        let rest = &body[cursor..];
        // `--` after a delimiter closes the body (`parsers.rs` does the same).
        if rest.starts_with(b"--") {
            break;
        }
        let rest = rest.strip_prefix(b"\r\n".as_slice()).unwrap_or(rest);
        let Some(end) = find(rest, marker.as_bytes()) else {
            break;
        };
        let part = &rest[..end];
        // The next part starts after the CRLF + delimiter that ended this one.
        cursor = (body.len() - rest.len()) + end + 2 + delimiter.len();
        let Some(separator) = find(part, b"\r\n\r\n") else {
            continue;
        };
        let headers = &part[..separator];
        let content = &part[separator + 4..];
        let Some(disposition) = header(headers, "content-disposition") else {
            continue;
        };
        let parameters = parameters(disposition);
        if parameter(&parameters, "name") != Some(field) {
            continue;
        }
        // `parse_boundary_stream` makes a part a file only when its `filename`
        // parameter is non-empty (`django/http/multipartparser.py:730-733`);
        // `filename=""` is a plain field whose value is the part's bytes.
        match parameter(&parameters, "filename").filter(|name| !name.is_empty()) {
            Some(filename) => {
                if sanitize_file_name(filename).is_none() {
                    // `MultiPartParser` skips the part entirely: it is neither
                    // in `request.POST` nor in `request.FILES`.
                    continue;
                }
                file = Some(Part::File {
                    size: content.len(),
                });
            }
            None => text = true,
        }
    }
    file.or(text.then_some(Part::Text))
}

/// One header's value, case-insensitively, trimmed.
fn header<'a>(headers: &'a [u8], name: &str) -> Option<&'a str> {
    std::str::from_utf8(headers).ok()?.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        candidate
            .trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim())
    })
}

/// `parse_header_parameters`'s parameter half (`django/utils/http.py:147-190`):
/// the `key=value` pairs after the first `;`, keys lower-cased, quotes removed.
///
/// A quoted value that itself contains `;` is split (Django's `email`-based
/// parser is not); only `name` and `filename` are read, and both are decided by
/// the sanitised name's emptiness, so a `;` in a filename cannot change the
/// response.
fn parameters(header_value: &str) -> Vec<(String, String)> {
    header_value
        .split(';')
        .skip(1)
        .filter_map(|segment| {
            let (key, value) = segment.split_once('=')?;
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(value);
            Some((key.trim().to_ascii_lowercase(), value.to_string()))
        })
        .collect()
}

/// The value of a parameter, by name.
fn parameter<'a>(parameters: &'a [(String, String)], name: &str) -> Option<&'a str> {
    parameters
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Django's `MultiPartParser.sanitize_file_name`
/// (`django/http/multipartparser.py:384-405`): the basename, minus
/// non-printable characters, or `None` for `""`, `"."` and `".."` (which makes
/// the parser skip the part).
///
/// Only emptiness matters downstream: the sanitised name is what
/// `FileField.to_internal_value` checks for `no_name` and `max_length`, and the
/// `UploadedFile` name setter truncates to 255 characters, so both branches are
/// unreachable. `html.unescape` (the first line of the Python) is not
/// reproduced; it can only change the outcome for a name made entirely of HTML
/// entities, which no client sends.
fn sanitize_file_name(filename: &str) -> Option<String> {
    let filename = filename.rsplit('/').next().unwrap_or("");
    let filename = filename.rsplit('\\').next().unwrap_or("");
    let cleaned: String = filename
        .chars()
        .filter(|character| {
            // `str.isprintable()`: control, format, separator and private-use
            // characters are not. `char::is_whitespace` covers the separator
            // categories; U+0020 itself stays.
            *character == ' ' || !(character.is_control() || character.is_whitespace())
        })
        .collect();
    (!matches!(cleaned.as_str(), "" | "." | "..")).then_some(cleaned)
}

/// `bytes.find(needle)` (`slice::windows`, the bodies are small).
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sanitised_name_follows_django() {
        assert_eq!(sanitize_file_name("note.txt").as_deref(), Some("note.txt"));
        assert_eq!(sanitize_file_name("a/b.txt").as_deref(), Some("b.txt"));
        assert_eq!(sanitize_file_name("a\\b.txt").as_deref(), Some("b.txt"));
        assert_eq!(sanitize_file_name("café.txt").as_deref(), Some("café.txt"));
        // `isprintable()` drops control characters.
        assert_eq!(sanitize_file_name("a\u{0}b").as_deref(), Some("ab"));
        assert_eq!(sanitize_file_name("a\u{7f}b").as_deref(), Some("ab"));
        assert_eq!(sanitize_file_name("a\tb").as_deref(), Some("ab"));
        // …and the three names Django rejects outright.
        assert_eq!(sanitize_file_name(""), None);
        assert_eq!(sanitize_file_name("."), None);
        assert_eq!(sanitize_file_name(".."), None);
        assert_eq!(sanitize_file_name("/"), None);
        assert_eq!(sanitize_file_name("a/"), None);
        // A space is printable.
        assert_eq!(sanitize_file_name("a b").as_deref(), Some("a b"));
    }

    #[test]
    fn header_parameters_are_parsed_like_django() {
        let pairs = parameters(
            "form-data; name=\"asset\"; filename=\"a b.txt\"; filename*=UTF-8''a%20b.txt",
        );
        assert_eq!(parameter(&pairs, "name"), Some("asset"));
        assert_eq!(parameter(&pairs, "filename"), Some("a b.txt"));
        assert_eq!(parameter(&pairs, "filename*"), Some("UTF-8''a%20b.txt"));
        let bare = parameters("form-data; name=asset");
        assert_eq!(parameter(&bare, "name"), Some("asset"));
        assert_eq!(parameter(&bare, "filename"), None);
    }

    #[test]
    fn the_body_is_split_into_parts() {
        let body = concat!(
            "--B\r\nContent-Disposition: form-data; name=\"attributes\"\r\n\r\n",
            "{\"a\": 1}\r\n",
            "--B\r\nContent-Disposition: form-data; name=\"asset\"; filename=\"n.txt\"\r\n",
            "Content-Type: text/plain\r\n\r\nhello\r\n",
            "--B--\r\n"
        );
        let content_type = "multipart/form-data; boundary=B";
        match multipart_field(content_type, body.as_bytes(), "asset") {
            Some(Part::File { size }) => assert_eq!(size, "hello".len()),
            _ => panic!("the asset part must be a file"),
        }
        assert!(matches!(
            multipart_field(content_type, body.as_bytes(), "attributes"),
            Some(Part::Text)
        ));
        assert!(multipart_field(content_type, body.as_bytes(), "missing").is_none());
        // A filename Django rejects makes the parser skip the part, so the
        // field is absent rather than a text value.
        let skipped = concat!(
            "--B\r\nContent-Disposition: form-data; name=\"asset\"; filename=\"..\"\r\n\r\n",
            "x\r\n--B--\r\n"
        );
        assert!(multipart_field(content_type, skipped.as_bytes(), "asset").is_none());
        // An empty filename is a *field*, not a file
        // (`multipartparser.py:730-733`), so its bytes are a string value.
        let empty_name = concat!(
            "--B\r\nContent-Disposition: form-data; name=\"asset\"; filename=\"\"\r\n\r\n",
            "hello\r\n--B--\r\n"
        );
        assert!(matches!(
            multipart_field(content_type, empty_name.as_bytes(), "asset"),
            Some(Part::Text)
        ));
        // `{**POST, **FILES}`: a file part wins over a text part with the same
        // name, whatever their order.
        let mixed = concat!(
            "--B\r\nContent-Disposition: form-data; name=\"asset\"\r\n\r\nhello\r\n",
            "--B\r\nContent-Disposition: form-data; name=\"asset\"; filename=\"n.txt\"\r\n\r\nabc\r\n",
            "--B--\r\n"
        );
        match multipart_field(content_type, mixed.as_bytes(), "asset") {
            Some(Part::File { size }) => assert_eq!(size, 3),
            _ => panic!("the file part must win over the text part"),
        }
        // Two file parts: `MultiValueDict.__getitem__` returns the last.
        let two = concat!(
            "--B\r\nContent-Disposition: form-data; name=\"asset\"; filename=\"a.txt\"\r\n\r\nabc\r\n",
            "--B\r\nContent-Disposition: form-data; name=\"asset\"; filename=\"b.txt\"\r\n\r\ndefgh\r\n",
            "--B--\r\n"
        );
        match multipart_field(content_type, two.as_bytes(), "asset") {
            Some(Part::File { size }) => assert_eq!(size, 5),
            _ => panic!("the last file part wins"),
        }
    }
}
