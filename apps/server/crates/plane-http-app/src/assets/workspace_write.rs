// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkspaceFileAssetEndpoint`'s write verbs
//! (`plane/app/views/asset/v2.py:243-522`), routes 18 and 19.
//!
//! | Route | Python | Handler |
//! | --- | --- | --- |
//! | `POST /api/assets/v2/workspaces/{slug}/` | `post` (`:356-439`) | [`post`] |
//! | `PATCH /api/assets/v2/workspaces/{slug}/{asset_id}/` | `patch` (`:441-472`) | [`patch`] |
//! | `DELETE /api/assets/v2/workspaces/{slug}/{asset_id}/` | `delete` (`:474-493`) | [`delete`] |
//!
//! The class also serves `GET` (`v2.rs`), and both URL shapes share one method
//! table, so three of the six verbs answer the wrong shape: `PATCH`/`DELETE`
//! on the collection and `POST` on a single asset. Python raises `TypeError`
//! when the handler is called with the wrong argument list — *after*
//! `@allow_permission` has run (`plane/app/permissions/base.py:18-100`, the
//! wrapper calls `view_func` last) — so a caller without a workspace role gets
//! the decorator's 403 and everyone else gets the family 500
//! (`plane/app/views/base.py:297-348`). The runtime enforces the role from
//! `routes.json` before the handler runs, so these three branches only have to
//! produce the 500.
//!
//! ## Order of operations (no transactions: `ATOMIC_REQUESTS` is off)
//!
//! **18 POST** (`v2.py:357-439`): `request.data` (a non-mapping body is an
//! `AttributeError` 500 at `:358`) → `sanitize_filename(name) or "unnamed"` →
//! `int(size)` (`ValueError`/`TypeError` → 500 at `:360`) → the entity-type
//! allow-list → the admin-only `WORKSPACE_LOGO` gate → the image MIME list →
//! `min(FILE_SIZE_LIMIT, size)` → `Workspace.objects.get(slug=…)` (404) → build
//! the row → `can_modify_asset` **only when `asset.project_id` is truthy**
//! (`:420`) → INSERT → presign. A failure after the INSERT leaves the row.
//!
//! **19 PATCH** (`v2.py:441-472`): get (404) → `is_project_accessible_to`
//! (403) → `can_modify_asset` (403) → `is_uploaded = True` → the
//! `get_asset_object_metadata` task when `storage_metadata` is falsy → the
//! entity save (which may soft-delete the row being confirmed: "confirm twice"
//! makes the workspace logo's previous asset *this* asset) → `request.data`
//! (a non-mapping body is an `AttributeError` **here**, after the entity save)
//! → the two-column UPDATE.
//!
//! **19 DELETE** (`v2.py:474-493`): get → accessible → may-modify → the entity
//! delete → the two-column UPDATE. `request.data` is never read.
//!
//! ## `entity_identifier` defaults to `False`, not `None` (`:362`)
//!
//! `False` reaches the FK column and `UUIDField.to_python(False)` is
//! `uuid.UUID(int=False)`, i.e. the nil UUID (`django/db/models/fields/__init__.py`,
//! `isinstance(False, int)`), so the INSERT violates the foreign key and the
//! family handler answers 400 `{"error": "The payload is not valid"}` — after
//! the type check, the workspace lookup and any `can_modify_asset` call. An
//! explicit `null` writes `NULL` and succeeds. [`fk_uuid`] ports the whole
//! `UUIDField` conversion including the empty-value rule Django's
//! `ForeignKey.get_db_prep_save` applies (`""`, `[]`, `{}` → `NULL`).
//!
//! ## Entity side effects
//!
//! The two helpers below are `WorkspaceFileAssetEndpoint.asset_delete` /
//! `entity_asset_save` / `entity_asset_delete` (`v2.py:278-354`). They
//! soft-delete the *previous* logo/cover asset and rebind the entity's FK, then
//! invalidate the three cache paths (`entities::WORKSPACE_ENTITY_INVALIDATIONS`),
//! even though only `/api/instances/` is ever cached: the harness compares the
//! Redis diff.

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_permissions::Role;
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_storage::{PresignPost, RequestInfo};
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::write;

use super::{entities, queries, support, write_a_queries, write_queries};

/// `FileAsset.EntityTypeContext.values` (`plane/db/models/asset.py:29-38`):
/// every value the entity-type allow-list accepts.
pub const ENTITY_TYPES: [&str; 10] = [
    "ISSUE_ATTACHMENT",
    "ISSUE_DESCRIPTION",
    "COMMENT_DESCRIPTION",
    "PAGE_DESCRIPTION",
    "USER_COVER",
    "USER_AVATAR",
    "WORKSPACE_LOGO",
    "PROJECT_COVER",
    "DRAFT_ISSUE_ATTACHMENT",
    "DRAFT_ISSUE_DESCRIPTION",
];

/// The upload MIME allow-list (`v2.py:383-389`, `:632-638`).
pub const IMAGE_TYPES: [&str; 5] = [
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/jpg",
    "image/gif",
];

/// `WorkspaceFileAssetEndpoint.post` (`v2.py:356-439`).
pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("asset_id").is_some() {
            // `/api/assets/v2/workspaces/{slug}/{asset_id}/` supplies an
            // `asset_id` this method does not take: `post() got an unexpected
            // keyword argument 'asset_id'` → 500.
            return Err(type_error(
                "WorkspaceFileAssetEndpoint.post() got an unexpected keyword argument 'asset_id'",
            ));
        }
        let slug = support::kwarg(ctx, "slug")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;
        ensure_mapping(data)?;

        let name = sanitized_name(body_get(data, "name").as_ref());
        let mime = body_get(data, "type");
        let size = size_arg(data, ctx.settings.limits.file_size_limit)?;
        let entity_type = body_get(data, "entity_type");
        // `request.data.get("entity_identifier", False)` — the `False` default
        // is a Django bug this route keeps (`v2.py:362`); see the module docs.
        let entity_identifier = body_get(data, "entity_identifier").unwrap_or(Json::Bool(false));

        let Some(entity_type) = entity_type.as_ref().and_then(Json::as_str) else {
            return entities::invalid_entity_type();
        };
        if !ENTITY_TYPES.contains(&entity_type) {
            return entities::invalid_entity_type();
        }

        // `WORKSPACE_LOGO` may only be uploaded by workspace admins
        // (`v2.py:371-380`): the decorator's role check is not enough.
        if entity_type == "WORKSPACE_LOGO" {
            let statement = client
                .prepare_cached(plane_permissions::queries::WORKSPACE_MEMBER_ROLE.sql)
                .await?;
            let role: Option<i16> = client
                .query_opt(&statement, &[&user, &slug])
                .await?
                .map(|row| row.get(0));
            if role != Some(Role::Admin.value()) {
                return entities::workspace_logo_needs_admin();
            }
        }

        let file_type = match mime.as_ref() {
            // `request.data.get("type", "image/jpeg")`.
            None => "image/jpeg",
            Some(Json::Str(value)) => value.as_str().unwrap_or_default(),
            // Any other JSON value is not in the allow-list either: 400, not a
            // default.
            Some(_) => return entities::invalid_file_type(),
        };
        if !IMAGE_TYPES.contains(&file_type) {
            return entities::invalid_file_type();
        }

        let size_limit = size.min(ctx.settings.limits.file_size_limit);
        let workspace = lookup::workspace_by_slug(client, slug).await?;
        let asset_key = format!("{}/{}-{}", workspace.id, Uuid::new_v4().simple(), name);

        let mut asset = new_asset(&asset_key, &name, file_type, size_limit, entity_type, user)?;
        // `**self.get_entity_id_field(entity_type=…, entity_id=entity_identifier)`
        // (`v2.py:416`). The value may override the `workspace=` kwarg because
        // Django's `Model.__init__` applies a leftover `<field>_id` kwarg last
        // (see [`fk_uuid`]).
        asset.workspace_id = Some(workspace.id);
        if let Some(column) = entity_id_column(entity_type, false) {
            let value = fk_uuid(&entity_identifier)?;
            assign_entity_fk(&mut asset, column, value);
        }

        // "A cover bound to a project at upload time needs a project of this
        // workspace that the caller administers" (`v2.py:419-424`). The check
        // is skipped when the raw `project_id` is falsy.
        if entity_id_column(entity_type, false) == Some("project_id")
            && truthy(&entity_identifier)
            && !support::can_modify_asset(client, user, &asset).await?
        {
            return entities::no_permission();
        }

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

/// `WorkspaceFileAssetEndpoint.patch` (`v2.py:441-472`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("asset_id").is_none() {
            return Err(type_error(
                "WorkspaceFileAssetEndpoint.patch() missing 1 required positional argument: \
                 'asset_id'",
            ));
        }
        let slug = support::kwarg(ctx, "slug")?;
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(asset) = load_asset(client, asset_id, slug).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if !accessible(ctx, &asset).await? {
            return entities::no_access();
        }
        if !support::can_modify_asset(client, user, &asset).await? {
            return entities::no_permission();
        }

        // `asset.is_uploaded = True` is in memory only; the UPDATE below
        // writes it beside `attributes`.
        if !storage_metadata_truthy(&asset) {
            publish_metadata_task(ctx, asset_id).await?;
        }
        entity_asset_save(ctx, client, &asset, user).await?;

        // The first `request.data` access of this method (`v2.py:469`): a
        // parse error or a non-mapping body fails **after** the entity save.
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
            .map_err(integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `WorkspaceFileAssetEndpoint.delete` (`v2.py:474-493`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("asset_id").is_none() {
            return Err(type_error(
                "WorkspaceFileAssetEndpoint.delete() missing 1 required positional argument: \
                 'asset_id'",
            ));
        }
        let slug = support::kwarg(ctx, "slug")?;
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(asset) = load_asset(client, asset_id, slug).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if !accessible(ctx, &asset).await? {
            return entities::no_access();
        }
        if !support::can_modify_asset(client, user, &asset).await? {
            return entities::no_permission();
        }

        // `asset.deleted_at = timezone.now()` happens before the entity
        // delete (`v2.py:488-489`), so the value is fixed even if the entity
        // helper rolls nothing back (there is no transaction).
        let deleted_at = write::now();
        entity_asset_delete(ctx, client, &asset, user).await?;
        let statement = client
            .prepare_cached(write_a_queries::FILE_ASSET_SET_DELETED.sql)
            .await?;
        client
            .execute(&statement, &[&asset_id, &deleted_at])
            .await
            .map_err(integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Entity side effects (`v2.py:278-354`)
// ---------------------------------------------------------------------------

/// `WorkspaceFileAssetEndpoint.entity_asset_save` (`v2.py:289-326`).
pub(super) async fn entity_asset_save(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    asset: &FileAsset,
    user: Uuid,
) -> Result<(), ViewError> {
    match asset.entity_type.as_deref() {
        Some("WORKSPACE_LOGO") => {
            // `Workspace.objects.filter(id=asset.workspace_id).first()`: a NULL
            // id matches nothing, so the query is skipped.
            let Some(workspace_id) = asset.workspace_id else {
                return Ok(());
            };
            let Some(previous) = workspace_logo(client, workspace_id).await? else {
                return Ok(());
            };
            if let Some(previous) = previous {
                entities::asset_delete(client, previous, None, write::now()).await?;
            }
            let now = write::now();
            let statement = client
                .prepare_cached(write_a_queries::WORKSPACE_SET_LOGO.sql)
                .await?;
            client
                .execute(&statement, &[&workspace_id, &asset.id, &now, &user])
                .await
                .map_err(integrity)?;
            entities::invalidate_entity(ctx, entities::WORKSPACE_ENTITY_INVALIDATIONS).await
        }
        Some("PROJECT_COVER") => {
            let Some(project_id) = asset.project_id else {
                return Ok(());
            };
            let Some(project) = project_cover(client, project_id, asset.workspace_id).await? else {
                return Ok(());
            };
            if let Some(previous) = project.cover_image_asset_id {
                entities::asset_delete(client, previous, None, write::now()).await?;
            }
            let now = write::now();
            let identifier = plane_db::hooks::project::before_update(&project.identifier);
            let statement = client
                .prepare_cached(write_a_queries::PROJECT_SET_COVER.sql)
                .await?;
            client
                .execute(
                    &statement,
                    &[&project_id, &asset.id, &now, &user, &identifier],
                )
                .await
                .map_err(integrity)?;
            // The project branch has no cache invalidation (`v2.py:312-324`).
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `WorkspaceFileAssetEndpoint.entity_asset_delete` (`v2.py:328-354`).
pub(super) async fn entity_asset_delete(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    asset: &FileAsset,
    user: Uuid,
) -> Result<(), ViewError> {
    match asset.entity_type.as_deref() {
        Some("WORKSPACE_LOGO") => {
            // `Workspace.objects.get(id=asset.workspace_id)`: a miss is a
            // `DoesNotExist` → 404, which is also what a NULL id produces
            // (`WHERE id IS NULL` matches nothing).
            let Some(workspace_id) = asset.workspace_id else {
                return Err(ViewError::ObjectDoesNotExist);
            };
            if !workspace_exists(client, workspace_id).await? {
                return Err(ViewError::ObjectDoesNotExist);
            }
            let now = write::now();
            let statement = client
                .prepare_cached(write_a_queries::WORKSPACE_CLEAR_LOGO.sql)
                .await?;
            client
                .execute(&statement, &[&workspace_id, &now, &user])
                .await
                .map_err(integrity)?;
            entities::invalidate_entity(ctx, entities::WORKSPACE_ENTITY_INVALIDATIONS).await
        }
        Some("PROJECT_COVER") => {
            let Some(project_id) = asset.project_id else {
                return Ok(());
            };
            let Some(project) = project_cover(client, project_id, asset.workspace_id).await? else {
                return Ok(());
            };
            let now = write::now();
            let identifier = plane_db::hooks::project::before_update(&project.identifier);
            let statement = client
                .prepare_cached(write_a_queries::PROJECT_CLEAR_COVER.sql)
                .await?;
            client
                .execute(&statement, &[&project_id, &now, &user, &identifier])
                .await
                .map_err(integrity)?;
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `Workspace.objects.filter(id=…).first()` → `Some(logo_asset_id)` when the
/// workspace exists, `None` when it does not.
async fn workspace_logo(
    client: &deadpool_postgres::Client,
    workspace_id: Uuid,
) -> Result<Option<Option<Uuid>>, ViewError> {
    let statement = client
        .prepare_cached(write_a_queries::WORKSPACE_LOGO_STATE.sql)
        .await?;
    match client.query_opt(&statement, &[&workspace_id]).await? {
        Some(row) => Ok(Some(row.try_get(0)?)),
        None => Ok(None),
    }
}

/// `Workspace.objects.get(id=…)`'s existence half (`v2.py:331`).
async fn workspace_exists(
    client: &deadpool_postgres::Client,
    workspace_id: Uuid,
) -> Result<bool, ViewError> {
    Ok(workspace_logo(client, workspace_id).await?.is_some())
}

/// One `projects` row as [`write_a_queries::PROJECT_COVER_STATE`] selects it.
pub(super) struct ProjectCoverState {
    pub cover_image_asset_id: Option<Uuid>,
    pub identifier: String,
}

/// `Project.objects.filter(id=…, workspace_id=…).first()` (`v2.py:314`, `:347`).
async fn project_cover(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    workspace_id: Option<Uuid>,
) -> Result<Option<ProjectCoverState>, ViewError> {
    let Some(workspace_id) = workspace_id else {
        return Ok(None);
    };
    let statement = client
        .prepare_cached(write_a_queries::PROJECT_COVER_STATE.sql)
        .await?;
    match client
        .query_opt(&statement, &[&project_id, &workspace_id])
        .await?
    {
        Some(row) => Ok(Some(ProjectCoverState {
            cover_image_asset_id: row.try_get(0)?,
            identifier: row.try_get(1)?,
        })),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Shared body helpers (`project_write.rs` reuses them: both halves port the
// same Python module and the same `request.data` idioms)
// ---------------------------------------------------------------------------

/// `request.data` as a mapping. `request.data.get` on a JSON list/scalar is an
/// `AttributeError` (family 500), never a 400.
pub fn ensure_mapping(data: &Data) -> Result<(), ViewError> {
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

/// `request.data.get(key)` as a Python value: a JSON object's member, or a
/// form's (last) string value. `None` when the key is absent.
pub fn body_get(data: &Data, key: &str) -> Option<Json> {
    match data {
        Data::Json(Json::Dict(map)) => map.get(key).cloned(),
        Data::Form(query) => query.get(key).map(|value| Json::Str(PyStr::from(value))),
        Data::Json(_) => None,
    }
}

/// `sanitize_filename(request.data.get("name")) or "unnamed"` (`v2.py:358`).
pub fn sanitized_name(value: Option<&Json>) -> String {
    value
        .and_then(Json::as_str)
        .and_then(support::sanitize_filename)
        .unwrap_or_else(|| "unnamed".to_owned())
}

/// `int(request.data.get("size", settings.FILE_SIZE_LIMIT))` (`v2.py:360`).
///
/// Python raises for a `None`, a list or a dict (`TypeError`) and for a
/// non-numeric string (`ValueError`); both are the family's 500 before any
/// other check. An integer larger than `i64` is clamped for a positive value
/// (it is above every size limit, so `min` picks the limit) and rejected for a
/// negative one (the `FloatField` conversion would raise `OverflowError`).
pub fn size_arg(data: &Data, default: i64) -> Result<i64, ViewError> {
    const OVERFLOW: &str = "OverflowError: int too large to convert to float";
    let value = body_get(data, "size");
    match &value {
        None => Ok(default),
        Some(Json::Int(value)) => {
            if value.is_negative() {
                return Err(type_error(OVERFLOW));
            }
            Ok(value.to_decimal_string().parse::<i64>().unwrap_or(i64::MAX))
        }
        Some(Json::Bool(value)) => Ok(i64::from(*value)),
        // `int(1.9)` truncates towards zero; Rust's `as` on a finite float does
        // the same and saturates instead of wrapping.
        Some(Json::Float(value)) => Ok(*value as i64),
        Some(Json::Str(value)) => {
            let Some(text) = value.as_str() else {
                return Err(type_error(OVERFLOW));
            };
            match plane_core::py::int(text) {
                Ok(value) => Ok(value),
                Err(plane_core::py::PyIntError::Invalid) => Err(type_error(&format!(
                    "ValueError: invalid literal for int() with base 10: {text:?}"
                ))),
                Err(plane_core::py::PyIntError::Overflow) => {
                    if text.trim_start().starts_with('-') {
                        Err(type_error(OVERFLOW))
                    } else {
                        Ok(i64::MAX)
                    }
                }
            }
        }
        Some(other) => Err(type_error(&format!(
            "TypeError: int() argument must be a string, a bytes-like object or a real number, \
             not '{}'",
            json_type_name(other)
        ))),
    }
}

/// The foreign-key column an entity type binds, or `None` when
/// `get_entity_id_field` returns `{}`.
///
/// `draft_aware` is the `ProjectAssetEndpoint` variant (`v2.py:587-614`), which
/// also maps `DRAFT_ISSUE_DESCRIPTION`; the workspace variant
/// (`v2.py:246-276`) does not, so a draft-description upload there writes an
/// asset with no FK at all.
pub fn entity_id_column(entity_type: &str, draft_aware: bool) -> Option<&'static str> {
    match entity_type {
        "WORKSPACE_LOGO" => Some("workspace_id"),
        "PROJECT_COVER" => Some("project_id"),
        "USER_AVATAR" | "USER_COVER" => Some("user_id"),
        "ISSUE_ATTACHMENT" | "ISSUE_DESCRIPTION" => Some("issue_id"),
        "PAGE_DESCRIPTION" => Some("page_id"),
        "COMMENT_DESCRIPTION" => Some("comment_id"),
        "DRAFT_ISSUE_DESCRIPTION" if draft_aware => Some("draft_issue_id"),
        _ => None,
    }
}

/// Assign one FK column of a not-yet-saved asset.
pub fn assign_entity_fk(asset: &mut FileAsset, column: &str, value: Option<Uuid>) {
    match column {
        "workspace_id" => asset.workspace_id = value,
        "project_id" => asset.project_id = value,
        "user_id" => asset.user_id = value,
        "issue_id" => asset.issue_id = value,
        "page_id" => asset.page_id = value,
        "comment_id" => asset.comment_id = value,
        "draft_issue_id" => asset.draft_issue_id = value,
        // Unreachable: `entity_id_column` only returns these names.
        _ => {}
    }
}

/// `UUIDField.get_db_prep_save` for a `ForeignKey`, i.e. what Django does with
/// the raw `entity_identifier` when the row is inserted.
///
/// * `None`, `""`, `[]` and `{}` are `ForeignKey.get_db_prep_save`'s empty
///   values → `NULL`;
/// * a `str` goes through `uuid.UUID(hex=…)` — the three forms
///   `Uuid::parse_str` accepts; anything else is Django's `ValidationError`
///   (`400 {"error": "Please provide valid detail"}`, `base.py:216-220`);
/// * an `int`/`bool` is `uuid.UUID(int=…)` because `isinstance(False, int)` is
///   true, so `False` is the nil UUID (and the INSERT then fails the foreign
///   key);
/// * a float, a non-empty list or a non-empty dict has no `hex` attribute →
///   `ValidationError`.
pub fn fk_uuid(value: &Json) -> Result<Option<Uuid>, ViewError> {
    match value {
        Json::Null => Ok(None),
        Json::Str(text) => {
            if text.as_str() == Some("") {
                return Ok(None);
            }
            text.as_str()
                .and_then(|text| Uuid::parse_str(text).ok())
                .map_or(Err(ViewError::Validation), |uuid| Ok(Some(uuid)))
        }
        Json::List(items) if items.is_empty() => Ok(None),
        Json::Dict(map) if map.is_empty() => Ok(None),
        Json::List(_) | Json::Dict(_) => Err(ViewError::Validation),
        Json::Bool(value) => Ok(Some(Uuid::from_u128(u128::from(*value)))),
        Json::Int(value) => {
            if value.is_negative() {
                return Err(ViewError::Validation);
            }
            // `uuid.UUID(int=n)` accepts `0 <= n < 2**128`.
            value
                .to_decimal_string()
                .parse::<u128>()
                .map_or(Err(ViewError::Validation), |value| {
                    Ok(Some(Uuid::from_u128(value)))
                })
        }
        Json::Float(_) => Err(ViewError::Validation),
    }
}

/// Python truthiness, for `if asset.project_id:` (`v2.py:420`).
pub fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Int(value) => value.to_decimal_string() != "0",
        Json::Float(value) => *value != 0.0,
        // A string with lone surrogates is still a non-empty `str`.
        Json::Str(value) => value.as_str().is_none_or(|text| !text.is_empty()),
        Json::List(items) => !items.is_empty(),
        Json::Dict(map) => !map.is_empty(),
    }
}

/// `if not asset.storage_metadata:` (`v2.py:459`, `:695`): `NULL`, `{}`, `[]`,
/// `0`, `""` and `false` are all falsy.
pub fn storage_metadata_truthy(asset: &FileAsset) -> bool {
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
        // Unreachable: the column is `jsonb`, so Postgres only hands back
        // valid JSON.
        Err(_) => true,
    }
}

/// The row a new upload builds: `FileAsset(attributes=…, asset=…, size=…,
/// workspace=…, created_by=request.user, entity_type=…)` plus the
/// `JSONField(default=dict)` `storage_metadata` and the ORM's timestamps.
pub fn new_asset(
    asset_key: &str,
    name: &str,
    file_type: &str,
    size_limit: i64,
    entity_type: &str,
    user: Uuid,
) -> Result<FileAsset, ViewError> {
    let mut attributes = PyDict::new();
    // Python dict order: `{"name": …, "type": …, "size": …}` (`v2.py:410`).
    attributes.insert(PyStr::from("name"), Json::Str(PyStr::from(name)));
    attributes.insert(PyStr::from("type"), Json::Str(PyStr::from(file_type)));
    attributes.insert(PyStr::from("size"), Json::from(size_limit));
    let now = write::now();
    Ok(FileAsset {
        created_at: now,
        updated_at: now,
        id: Uuid::new_v4(),
        attributes: JsonText::from_json(&Json::Dict(attributes))?,
        asset: asset_key.to_owned(),
        created_by_id: Some(user),
        updated_by_id: None,
        workspace_id: None,
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
        project_id: None,
        size: size_limit as f64,
        storage_metadata: Some(JsonText::new("{}")),
        user_id: None,
        draft_issue_id: None,
        entity_identifier: None,
    })
}

/// `FileAsset.objects.create(…)` / `asset.save()` (`v2.py:425`, `:658`).
///
/// A failed foreign key is Django's `IntegrityError` — the family's 400
/// `{"error": "The payload is not valid"}` (`base.py:306-310`), which is how
/// the `entity_identifier=False` default surfaces.
pub async fn insert_asset(
    client: &deadpool_postgres::Client,
    asset: &FileAsset,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(write_queries::FILE_ASSET_INSERT.sql)
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
    Ok(())
}

/// `django.db.IntegrityError` (SQLSTATE class 23) is the family's 400; every
/// other database error is an uncaught 500.
pub fn integrity(error: tokio_postgres::Error) -> ViewError {
    if error
        .code()
        .is_some_and(|code| code.code().starts_with("23"))
    {
        return ViewError::Integrity;
    }
    ViewError::from(error)
}

/// Whether a database error is a swallowed `IntegrityError`
/// (`v2.py:802-805`, `:809-813`, `:820-824`).
pub fn is_integrity(error: &tokio_postgres::Error) -> bool {
    error
        .code()
        .is_some_and(|code| code.code().starts_with("23"))
}

/// `get_asset_object_metadata.delay(asset_id=str(asset_id))`
/// (`v2.py:460`, `:696`).
pub async fn publish_metadata_task(ctx: &ViewCtx<'_>, asset_id: Uuid) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::GET_ASSET_OBJECT_METADATA;
    let message = plane_jobs::celery::tasks::get_asset_object_metadata(asset_id.to_string());
    crate::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::from_message(&message),
    )
    .await
    .map(|_| ())
}

/// `S3Storage(request=request)`'s host/scheme inputs
/// (`plane/settings/storage.py:42-54`).
pub fn request_info<'a>(ctx: &'a ViewCtx<'a>) -> RequestInfo<'a> {
    RequestInfo {
        scheme: if ctx.ctx.is_secure { "https" } else { "http" },
        host: ctx.ctx.header("host").unwrap_or_default(),
    }
}

/// `FileAsset.objects.get(id=…, workspace__slug=…)` (`v2.py:444`, `:476`).
pub(super) async fn load_asset(
    client: &deadpool_postgres::Client,
    asset_id: Uuid,
    slug: &str,
) -> Result<Option<FileAsset>, ViewError> {
    let row = client
        .query_opt(
            queries::FILE_ASSET_BY_ID_IN_WORKSPACE.sql,
            &[&asset_id, &slug],
        )
        .await?;
    row.as_ref()
        .map(|row| FileAsset::from_row(row, 0))
        .transpose()
        .map_err(ViewError::from)
}

/// `FileAsset.is_project_accessible_to(request.user)` for one row.
pub(super) async fn accessible(ctx: &ViewCtx<'_>, asset: &FileAsset) -> Result<bool, ViewError> {
    let client = client(ctx)?;
    let user_id = support::user_id(ctx)?;
    let flags = support::accessible_assets(client, user_id, std::slice::from_ref(asset)).await?;
    Ok(flags.first().copied().unwrap_or(false))
}

/// A `TypeError`/`AttributeError` the family handler renders as a 500.
pub fn type_error(message: &str) -> ViewError {
    // A `TypeError` is uncaught by DRF, so Plane's `handle_exception` maps it to
    // the family 500 (`plane/app/views/base.py:235-239`).
    ViewError::Unknown(message.to_owned())
}

/// Python's `type(value).__name__` for an `int()` type error message.
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
