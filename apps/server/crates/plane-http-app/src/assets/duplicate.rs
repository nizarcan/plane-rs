// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `DuplicateAssetEndpoint.post` (`plane/app/views/asset/v2.py:840-956`),
//! route 28.
//!
//! The handler copies one uploaded asset of the destination workspace into a
//! new `file_assets` row: the row is inserted first, the S3 object is then
//! copied synchronously, and `is_uploaded` is flipped by a queryset update
//! (which never touches `updated_at`/`updated_by`). There is no transaction
//! (`ATOMIC_REQUESTS` is off), so a failure after the INSERT leaves the row.
//!
//! ## Order of operations (`v2.py:877-956`)
//!
//! 1. `request.data` (a non-mapping body is an `AttributeError` 500 at `:878`);
//! 2. the entity-type allow-list (`FileAsset.EntityTypeContext.values`) → 400
//!    `{"error": "Invalid entity type or entity id"}` — note the missing
//!    `status` key, unlike the other asset literals;
//! 3. `Workspace.objects.get(slug=…)` → 404;
//! 4. `S3Storage(request=request)` — constructed **before** the source lookup,
//!    so a malformed `SIGNED_URL_EXPIRATION`/`AWS_REGION` is this route's 500
//!    even for a 404 source (`storage.py:28-66`);
//! 5. the source: `FileAsset.objects.filter(id=…, is_uploaded=True,
//!    workspace=workspace).first()` → 404 `{"error": "Asset not found"}`;
//! 6. `is_project_accessible_to` → 403 "You don't have access to this asset.";
//! 7. the destination project: `PROJECT_COVER` stores on `entity_id`
//!    (`v2.py:910-911`); a truthy id must exist in this workspace (404
//!    `{"error": "Project not found", "status": false}`) and have an active
//!    `ProjectMember` row for the caller (403
//!    `{"error": "You don't have access to this project.", "status": false}`).
//!    A falsy id keeps the source's own project;
//! 8. `sanitize_filename(attributes.get("name")) or "unnamed"`, then
//!    `<workspace.id>/<uuid4().hex>-<name>`;
//! 9. the INSERT (`created_by_id = request.user.id`, `updated_by` NULL,
//!    `size = source.size`, `storage_metadata = source.storage_metadata`, the
//!    entity FK from `get_entity_id_field` with `project_id` removed);
//! 10. `storage.copy_object(source.asset, destination_key)` — a `ClientError`
//!     is logged and swallowed (`storage.py:171-183`), a transport error is
//!     uncaught and becomes a 500;
//! 11. `FileAsset.objects.filter(id=…).update(is_uploaded=True)`;
//! 12. `{"asset_id": str(duplicated.id)}`, 200.
//!
//! ## The throttle
//!
//! `throttle_classes = [AssetRateThrottle]` (`v2.py:841`) replaces DRF's
//! default `AnonRateThrottle`: scope `asset_id`, key
//! `throttle_asset_<asset_id>`, 5/minute, shared by every user
//! (`plane/throttles/asset.py:8-15`, `plane/settings/common.py:142-145`). The
//! runtime applies it from `routes.json` before the handler, so the 429 body is
//! the authentication adapter's and no statement here is involved.

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_storage::RequestInfo;
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::write;

use super::entities;
use super::support;
use super::workspace_write::{
    ENTITY_TYPES, accessible, body_get, ensure_mapping, fk_uuid, insert_asset, truthy, type_error,
};
use super::write_c_queries;

/// Register the class's `POST`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.asset.v2.DuplicateAssetEndpoint")?
        .method(http::Method::POST, post)?;
    Ok(())
}

/// `DuplicateAssetEndpoint.post` (`v2.py:876-956`).
pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;
        ensure_mapping(data)?;

        // `request.data.get("project_id", None)`, `get("entity_id", None)` and
        // `get("entity_type", None)` (`v2.py:878-880`): every default is `None`.
        let project_id_arg = body_get(data, "project_id").unwrap_or(Json::Null);
        let entity_id = body_get(data, "entity_id").unwrap_or(Json::Null);
        let entity_type = body_get(data, "entity_type").unwrap_or(Json::Null);

        let Some(entity_type) = entity_type.as_str() else {
            return invalid_entity_type();
        };
        if !ENTITY_TYPES.contains(&entity_type) {
            return invalid_entity_type();
        }

        let workspace = lookup::workspace_by_slug(client, slug).await?;
        // `S3Storage(request=request)` (`v2.py:890`) happens before the source
        // lookup: the settings are validated here, not at the presign below.
        let storage = support::storage(ctx.settings)?;

        let Some(original) = load_source(client, asset_id, workspace.id).await? else {
            return asset_not_found();
        };
        if !accessible(ctx, &original).await? {
            return entities::no_access();
        }

        // `if entity_type == PROJECT_COVER: project_id = entity_id`
        // (`v2.py:910-911`).
        let project_arg = if entity_type == "PROJECT_COVER" {
            entity_id.clone()
        } else {
            project_id_arg
        };
        // `if project_id:` — Python truthiness, not "is a valid UUID": `""`,
        // `[]`, `{}`, `0` and `null` all take the source-project branch.
        let project_id = if truthy(&project_arg) {
            let Some(project_id) = fk_uuid(&project_arg)? else {
                // Unreachable: a truthy value is never one of Django's
                // empty-value forms.
                return invalid_entity_type();
            };
            if !project_exists(client, project_id, workspace.id).await? {
                return json_literal(
                    404,
                    &plane_core::json::Object::new()
                        .field("error", "Project not found")
                        .field("status", false),
                );
            }
            if !project_member_active(client, user, workspace.id, project_id).await? {
                return json_literal(
                    403,
                    &plane_core::json::Object::new()
                        .field("error", "You don't have access to this project.")
                        .field("status", false),
                );
            }
            Some(project_id)
        } else {
            original.project_id
        };

        let attributes = source_attributes(&original.attributes)?;
        let name = attributes
            .get("name")
            .and_then(Json::as_str)
            .and_then(support::sanitize_filename)
            .unwrap_or_else(|| "unnamed".to_owned());
        let destination_key = format!("{}/{}-{}", workspace.id, Uuid::new_v4().simple(), name);

        // `entity_id_fields = self.get_entity_id_field(entity_type, entity_id)`
        // then `entity_id_fields.pop("project_id", None)` (`v2.py:934-936`).
        let entity_column = duplicate_entity_column(entity_type);
        let entity_value = if entity_column.is_some() {
            fk_uuid(&entity_id)?
        } else {
            None
        };

        let now = write::now();
        let mut duplicated = FileAsset {
            created_at: now,
            updated_at: now,
            id: Uuid::new_v4(),
            attributes: copy_attributes(&attributes)?,
            asset: destination_key.clone(),
            created_by_id: Some(user),
            updated_by_id: None,
            workspace_id: Some(workspace.id),
            is_deleted: false,
            deleted_at: None,
            is_archived: false,
            comment_id: None,
            entity_type: Some(entity_type.to_owned()),
            external_id: None,
            external_source: None,
            is_uploaded: false,
            issue_id: None,
            page_id: None,
            project_id,
            size: original.size,
            storage_metadata: original.storage_metadata.clone(),
            user_id: None,
            draft_issue_id: None,
            entity_identifier: None,
        };
        // A leftover `<field>_id` keyword overrides the `workspace=` argument
        // in Django's `Model.__init__` (`django/db/models/base.py:537-576`),
        // which is why a WORKSPACE_LOGO copy with `entity_id: null` writes a
        // NULL workspace (`v2.py:937-951`).
        if let Some(column) = entity_column {
            super::workspace_write::assign_entity_fk(&mut duplicated, column, entity_value);
        }
        insert_asset(client, &duplicated).await?;

        // A `ClientError` (a missing object, a denied copy) is swallowed; a
        // transport failure is uncaught and becomes the family 500.
        storage
            .request(request_info(ctx))
            .copy_object(&original.asset, &destination_key)
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;

        let statement = client
            .prepare_cached(write_c_queries::FILE_ASSET_SET_UPLOADED.sql)
            .await?;
        client
            .execute(&statement, &[&duplicated.id])
            .await
            .map_err(super::workspace_write::integrity)?;

        let mut body = PyDict::new();
        body.insert(
            PyStr::from("asset_id"),
            Json::Str(PyStr::from(duplicated.id.to_string())),
        );
        let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &Json::Dict(body))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, rendered))
    })
}

/// `get_entity_id_field` of `DuplicateAssetEndpoint` (`v2.py:843-874`).
///
/// Unlike `ProjectAssetEndpoint`'s copy it has **no**
/// `DRAFT_ISSUE_DESCRIPTION` entry, so a draft-description copy is created with
/// no FK at all. `PROJECT_COVER` maps to `project_id`, which the caller removes
/// before the INSERT because `project_id=` is passed explicitly.
fn duplicate_entity_column(entity_type: &str) -> Option<&'static str> {
    match entity_type {
        "WORKSPACE_LOGO" => Some("workspace_id"),
        "PROJECT_COVER" => Some("project_id"),
        "USER_AVATAR" | "USER_COVER" => Some("user_id"),
        "ISSUE_ATTACHMENT" | "ISSUE_DESCRIPTION" => Some("issue_id"),
        "PAGE_DESCRIPTION" => Some("page_id"),
        "COMMENT_DESCRIPTION" => Some("comment_id"),
        _ => None,
    }
}

/// The source row: `FileAsset.objects.filter(id=…, is_uploaded=True,
/// workspace=workspace).first()` (`v2.py:892-896`).
async fn load_source(
    client: &deadpool_postgres::Client,
    asset_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<FileAsset>, ViewError> {
    let statement = client
        .prepare_cached(write_c_queries::FILE_ASSET_DUPLICATE_SOURCE.sql)
        .await?;
    let row = client
        .query_opt(&statement, &[&asset_id, &workspace_id])
        .await?;
    row.as_ref()
        .map(|row| FileAsset::from_row(row, 0))
        .transpose()
        .map_err(ViewError::from)
}

/// `Project.objects.filter(id=project_id, workspace=workspace).exists()`
/// (`v2.py:917`).
async fn project_exists(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(write_c_queries::PROJECT_EXISTS_IN_WORKSPACE.sql)
        .await?;
    Ok(client
        .query_opt(&statement, &[&project_id, &workspace_id])
        .await?
        .is_some())
}

/// `ProjectMember.objects.filter(member=…, workspace=…, project_id=…,
/// is_active=True).exists()` (`v2.py:919-924`).
async fn project_member_active(
    client: &deadpool_postgres::Client,
    user_id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(write_c_queries::PROJECT_MEMBER_ACTIVE_EXISTS.sql)
        .await?;
    Ok(client
        .query_opt(&statement, &[&user_id, &workspace_id, &project_id])
        .await?
        .is_some())
}

/// The source's `attributes` decoded as Python sees the `jsonb` column
/// (`psycopg` hands back the `json.loads` result).
fn source_attributes(text: &JsonText) -> Result<Json, ViewError> {
    plane_serde::pyjson::loads(text.as_str().as_bytes(), "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `attributes={"name": source.get("name"), "type": source.get("type"),
/// "size": source.get("size")}` (`v2.py:938-942`): three keys, in that order,
/// with the **raw** values (a missing member is `None`).
fn copy_attributes(parsed: &Json) -> Result<JsonText, ViewError> {
    let map = match parsed {
        Json::Dict(map) => map,
        // Python's `source.attributes.get(…)` on a list/str is an
        // `AttributeError` → the family 500; only the status is observable.
        _ => {
            return Err(type_error(
                "AttributeError: 'object' has no attribute 'get'",
            ));
        }
    };
    let mut attributes = PyDict::new();
    for key in ["name", "type", "size"] {
        attributes.insert(
            PyStr::from(key),
            map.get(key).cloned().unwrap_or(Json::Null),
        );
    }
    JsonText::from_json(&Json::Dict(attributes)).map_err(ViewError::from)
}

/// `Response({"error": "Invalid entity type or entity id"}, status=400)`
/// (`v2.py:882-886`). No `status` key, unlike the other asset literals.
fn invalid_entity_type() -> Result<ViewResponse, ViewError> {
    json_literal(
        400,
        &plane_core::json::Object::new().field("error", "Invalid entity type or entity id"),
    )
}

/// `Response({"error": "Asset not found"}, status=404)` (`v2.py:898-899`).
fn asset_not_found() -> Result<ViewResponse, ViewError> {
    json_literal(
        404,
        &plane_core::json::Object::new().field("error", "Asset not found"),
    )
}

/// A `Response({...})` literal, in Python dict order.
fn json_literal(status: u16, object: &plane_core::json::Object) -> Result<ViewResponse, ViewError> {
    support::json_object(status, object)
}

/// `S3Storage(request=request)`'s host/scheme inputs
/// (`plane/settings/storage.py:42-54`).
fn request_info<'a>(ctx: &'a ViewCtx<'a>) -> RequestInfo<'a> {
    RequestInfo {
        scheme: if ctx.ctx.is_secure { "https" } else { "http" },
        host: ctx.ctx.header("host").unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The duplicate's `get_entity_id_field` (`v2.py:843-874`) versus the
    /// `ProjectAssetEndpoint` one (`v2.py:587-614`): only the draft-description
    /// entry is missing, so a draft copy is unattached.
    #[test]
    fn the_entity_column_map_has_no_draft_description() {
        assert_eq!(
            duplicate_entity_column("DRAFT_ISSUE_DESCRIPTION"),
            None,
            "the duplicate helper does not map DRAFT_ISSUE_DESCRIPTION"
        );
        assert_eq!(
            super::super::workspace_write::entity_id_column("DRAFT_ISSUE_DESCRIPTION", true),
            Some("draft_issue_id"),
            "the project upload helper does"
        );
        for entity_type in ENTITY_TYPES {
            if entity_type == "DRAFT_ISSUE_DESCRIPTION" || entity_type == "DRAFT_ISSUE_ATTACHMENT" {
                assert_eq!(duplicate_entity_column(entity_type), None, "{entity_type}");
            } else {
                assert!(
                    duplicate_entity_column(entity_type).is_some(),
                    "{entity_type} must map a column"
                );
            }
        }
    }

    /// The copy's `attributes` keeps only name/type/size, in Python dict order,
    /// and takes the raw members (`v2.py:938-942`). `JsonText`'s separators are
    /// Python's `json.dumps` defaults; the column is `jsonb`, so Postgres
    /// normalises the text and only the members are observable.
    #[test]
    fn the_copy_attributes_are_the_three_source_members() {
        let text = JsonText::new(r#"{"name":"spec.pdf","type":"text/plain","size":10240,"x":1}"#);
        let parsed = source_attributes(&text).expect("jsonb");
        let copy = copy_attributes(&parsed).expect("render");
        assert_eq!(
            copy.as_str(),
            r#"{"name": "spec.pdf", "type": "text/plain", "size": 10240}"#
        );
        // A missing member is `None`, not dropped.
        let text = JsonText::new(r#"{"type":"text/plain"}"#);
        let parsed = source_attributes(&text).expect("jsonb");
        let copy = copy_attributes(&parsed).expect("render");
        assert_eq!(
            copy.as_str(),
            r#"{"name": null, "type": "text/plain", "size": null}"#
        );
    }
}
