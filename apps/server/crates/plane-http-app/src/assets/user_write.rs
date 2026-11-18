// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `UserAssetsV2Endpoint`'s write verbs and `AssetRestoreEndpoint.post`
//! (`plane/app/views/asset/v2.py:70-241`, `:567-581`), routes 20, 21 and 22.
//!
//! | Route | Python | Handler |
//! | --- | --- | --- |
//! | `POST /api/assets/v2/user-assets/` | `post` (`:151-210`) | [`post`] |
//! | `PATCH /api/assets/v2/user-assets/{asset_id}/` | `patch` (`:212-231`) | [`patch`] |
//! | `DELETE /api/assets/v2/user-assets/{asset_id}/` | `delete` (`:233-240`) | [`delete`] |
//! | `POST /api/assets/v2/workspaces/{slug}/restore/{asset_id}/` | `AssetRestoreEndpoint.post` (`:571-581`) | [`restore`] |
//!
//! Both user-asset URL shapes share one method table, so `PATCH`/`DELETE` on
//! the collection raise `TypeError` (`missing 1 required positional argument:
//! 'asset_id'`) and `POST` on a single asset raises `TypeError` (`got an
//! unexpected keyword argument 'asset_id'`). The runtime enforces only
//! `IsAuthenticated` here (`routes.json` order 20/21), so these branches are the
//! whole answer: the family 500 (`plane/app/views/base.py:297-348`).
//!
//! ## Order of operations (no transactions: `ATOMIC_REQUESTS` is off)
//!
//! **20 POST** (`v2.py:151-210`): `request.data` (a non-mapping body is an
//! `AttributeError` 500 at `:153`) → `sanitize_filename(name) or "unnamed"` →
//! `int(size)` (500 on garbage, `:155`) → the entity-type allow-list (only
//! `USER_AVATAR`/`USER_COVER`) → the image MIME list → `min(size, FILE_SIZE_LIMIT)`
//! → INSERT → presign. No workspace is involved, so the key is
//! `<uuid4().hex>-<name>` and `asset_url` is the static route.
//!
//! **21 PATCH** (`v2.py:212-231`): get (`user_id=request.user.id`, so 404) →
//! `is_uploaded = True` in memory → the `get_asset_object_metadata` task when
//! `storage_metadata` is falsy → the entity save (which soft-deletes the
//! *previous* avatar/cover and rewrites `users`) → `request.data` (a
//! non-mapping body is an `AttributeError` **here**, after the entity save) →
//! the two-column UPDATE (`is_uploaded`, `attributes`; `updated_at` and
//! `updated_by` are not written).
//!
//! **21 DELETE** (`v2.py:233-240`): get → `is_deleted`/`deleted_at` in memory →
//! the entity delete (which nulls the user's FK and rewrites `users`) → the
//! two-column UPDATE.
//!
//! **22 POST** (`v2.py:571-581`): `FileAsset.all_objects.get(id, workspace__slug)`
//! (404; soft-deleted rows are found) → `can_modify_asset` (403) → the
//! two-column UPDATE that clears `is_deleted`/`deleted_at`.
//!
//! ## The user save is H30
//!
//! `user.save()` (`v2.py:93,111,127,140`) runs `User.save`
//! (`plane/db/models/user.py:169-187`, [`plane_db::hooks::user`]): the email is
//! lower-cased and stripped, `display_name` is derived when empty,
//! `is_superuser` forces `is_staff`, and a non-NULL `token_updated_at` rotates
//! `token` to two `uuid4().hex` values and moves `token_updated_at` to `now`.
//! The fixture makes `token_updated_at` non-NULL for every user
//! (`tools/fixtures/build.py`), so **every** avatar/cover write rotates the
//! token; the harness's `users.token` DB rule placeholders the value, which is
//! why a port that skipped the rotation would still show up as a *missing*
//! column. `updated_at` is the second `now` (`auto_now` in `pre_save`).

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks::user as user_hook;
use plane_db::models::asset::FileAsset;
use plane_db::models::user::User;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use plane_storage::{PresignPost, RequestInfo};
use uuid::Uuid;

use crate::common::client;
use crate::write;

use super::entities;
use super::support;
use super::workspace_write::{
    body_get, ensure_mapping, insert_asset, new_asset, publish_metadata_task, sanitized_name,
    size_arg, storage_metadata_truthy, type_error,
};
use super::write_a_queries;
use super::write_c_queries;

/// Register the two classes' write verbs.
///
/// Both shapes of `UserAssetsV2Endpoint` share one handler per method (the
/// Python's `TypeError` 500s are branches inside them), exactly like
/// `v2.rs`/`workspace_write.rs` do for the workspace class.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.asset.v2.UserAssetsV2Endpoint")?
        .method(http::Method::POST, post)?
        .method(http::Method::PATCH, patch)?
        .method(http::Method::DELETE, delete)?;
    registry
        .api_view("plane.app.views.asset.v2.AssetRestoreEndpoint")?
        .method(http::Method::POST, restore)?;
    Ok(())
}

/// `UserAssetsV2Endpoint.post` (`v2.py:151-210`).
pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("asset_id").is_some() {
            // `/api/assets/v2/user-assets/{asset_id}/` supplies an `asset_id`
            // this method does not take: `post() got an unexpected keyword
            // argument 'asset_id'` → 500.
            return Err(type_error(
                "UserAssetsV2Endpoint.post() got an unexpected keyword argument 'asset_id'",
            ));
        }
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;
        ensure_mapping(data)?;

        let name = sanitized_name(body_get(data, "name").as_ref());
        let mime = body_get(data, "type");
        let size = size_arg(data, ctx.settings.limits.file_size_limit)?;
        // `request.data.get("entity_type", False)`: `False` is falsy, so a
        // missing key is the same 400 as an unknown one (`v2.py:156,162`).
        let entity_type = body_get(data, "entity_type").unwrap_or(Json::Bool(false));

        let Some(entity_type) = entity_type.as_str() else {
            return entities::invalid_entity_type();
        };
        if !USER_ENTITY_TYPES.contains(&entity_type) {
            return entities::invalid_entity_type();
        }

        let file_type = match mime.as_ref() {
            // `request.data.get("type", "image/jpeg")`.
            None => "image/jpeg",
            Some(Json::Str(value)) => value.as_str().unwrap_or_default(),
            // Any other JSON value is not in the allow-list either.
            Some(_) => return entities::invalid_file_type(),
        };
        if !super::workspace_write::IMAGE_TYPES.contains(&file_type) {
            return entities::invalid_file_type();
        }

        let size_limit = size.min(ctx.settings.limits.file_size_limit);
        // `f"{uuid.uuid4().hex}-{name}"` (`v2.py:186`): no workspace prefix.
        let asset_key = format!("{}-{}", Uuid::new_v4().simple(), name);

        let mut asset = new_asset(&asset_key, &name, file_type, size_limit, entity_type, user)?;
        // `FileAsset.objects.create(..., user=request.user, created_by=request.user)`
        // (`v2.py:189-196`): the asset belongs to the caller, not to a
        // workspace.
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
        // `asset.asset_url` for USER_AVATAR/USER_COVER is the static route
        // (`plane/db/models/asset.py:85-91`), so no workspace slug is needed.
        let body = support::presign_upload_body(&asset, None, &post)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `UserAssetsV2Endpoint.patch` (`v2.py:212-231`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("asset_id").is_none() {
            return Err(type_error(
                "UserAssetsV2Endpoint.patch() missing 1 required positional argument: 'asset_id'",
            ));
        }
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(asset) = load_for_user(client, asset_id, user).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        // `asset.is_uploaded = True` is in memory only; the UPDATE below
        // writes it beside `attributes`.
        if !storage_metadata_truthy(&asset) {
            publish_metadata_task(ctx, asset_id).await?;
        }
        entity_asset_save(ctx, client, &asset).await?;

        // The first `request.data` access of this method (`v2.py:228`): a parse
        // error or a non-mapping body fails **after** the entity save.
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
            .map_err(super::workspace_write::integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `UserAssetsV2Endpoint.delete` (`v2.py:233-240`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("asset_id").is_none() {
            return Err(type_error(
                "UserAssetsV2Endpoint.delete() missing 1 required positional argument: 'asset_id'",
            ));
        }
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(asset) = load_for_user(client, asset_id, user).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        // `asset.deleted_at = timezone.now()` is assigned before the entity
        // delete (`v2.py:236-238`), so the value is fixed even though nothing
        // rolls back.
        let deleted_at = write::now();
        entity_asset_delete(ctx, client, &asset).await?;
        let statement = client
            .prepare_cached(write_a_queries::FILE_ASSET_SET_DELETED.sql)
            .await?;
        client
            .execute(&statement, &[&asset_id, &deleted_at])
            .await
            .map_err(super::workspace_write::integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `AssetRestoreEndpoint.post` (`v2.py:571-581`).
pub fn restore<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let asset_id = support::uuid_kwarg(ctx, "asset_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(asset) = load_any_state(client, asset_id, slug).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if !support::can_modify_asset(client, user, &asset).await? {
            return entities::no_permission();
        }
        let statement = client
            .prepare_cached(write_c_queries::FILE_ASSET_RESTORE.sql)
            .await?;
        client
            .execute(&statement, &[&asset_id])
            .await
            .map_err(super::workspace_write::integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `FileAsset.EntityTypeContext` values 20/21 accept (`v2.py:162`).
const USER_ENTITY_TYPES: [&str; 2] = ["USER_AVATAR", "USER_COVER"];

// ---------------------------------------------------------------------------
// Entity side effects (`v2.py:73-149`)
// ---------------------------------------------------------------------------

/// `UserAssetsV2Endpoint.asset_delete` (`v2.py:73-81`): the previous avatar or
/// cover, soft-deleted only when it belongs to the same user.
async fn asset_delete(
    client: &Client,
    asset_id: Uuid,
    user_id: Uuid,
    deleted_at: DateTime<Utc>,
) -> Result<(), ViewError> {
    entities::asset_delete(client, asset_id, Some(user_id), deleted_at).await
}

/// `UserAssetsV2Endpoint.entity_asset_save` (`v2.py:83-120`).
async fn entity_asset_save(
    ctx: &ViewCtx<'_>,
    client: &Client,
    asset: &FileAsset,
) -> Result<(), ViewError> {
    match asset.entity_type.as_deref() {
        Some("USER_AVATAR") => {
            let (user, user_id) = load_user_of(client, asset).await?;
            let mut user = user;
            // `user.avatar = ""` (`v2.py:87`).
            user.avatar = String::new();
            if let Some(previous) = user.avatar_asset_id {
                asset_delete(client, previous, user_id, write::now()).await?;
            }
            user.avatar_asset_id = Some(asset.id);
            save_user(client, &user).await?;
            entities::invalidate_entity(ctx, entities::USER_ENTITY_INVALIDATIONS).await
        }
        Some("USER_COVER") => {
            let (user, user_id) = load_user_of(client, asset).await?;
            let mut user = user;
            // `user.cover_image = None` (`v2.py:105`).
            user.cover_image = None;
            if let Some(previous) = user.cover_image_asset_id {
                asset_delete(client, previous, user_id, write::now()).await?;
            }
            user.cover_image_asset_id = Some(asset.id);
            save_user(client, &user).await?;
            entities::invalidate_entity(ctx, entities::USER_ENTITY_INVALIDATIONS).await
        }
        _ => Ok(()),
    }
}

/// `UserAssetsV2Endpoint.entity_asset_delete` (`v2.py:122-149`).
async fn entity_asset_delete(
    ctx: &ViewCtx<'_>,
    client: &Client,
    asset: &FileAsset,
) -> Result<(), ViewError> {
    match asset.entity_type.as_deref() {
        Some("USER_AVATAR") => {
            let (mut user, _) = load_user_of(client, asset).await?;
            user.avatar_asset_id = None;
            save_user(client, &user).await?;
            entities::invalidate_entity(ctx, entities::USER_ENTITY_INVALIDATIONS).await
        }
        Some("USER_COVER") => {
            let (mut user, _) = load_user_of(client, asset).await?;
            user.cover_image_asset_id = None;
            save_user(client, &user).await?;
            entities::invalidate_entity(ctx, entities::USER_ENTITY_INVALIDATIONS).await
        }
        _ => Ok(()),
    }
}

/// `User.objects.get(id=asset.user_id)` (`v2.py:86,104,125,138`).
///
/// A NULL `user_id` is `WHERE id IS NULL`, which matches nothing, so it is the
/// same `DoesNotExist` → 404 the Python raises.
async fn load_user_of(client: &Client, asset: &FileAsset) -> Result<(User, Uuid), ViewError> {
    let Some(user_id) = asset.user_id else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let statement = client
        .prepare_cached(crate::users_me::queries::USER_BY_ID.sql)
        .await?;
    let row = client.query_opt(&statement, &[&user_id]).await?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Ok((User::from_row(&row, 0)?, user_id))
}

/// `user.save()` (`v2.py:93,111,127,140`) — H30 plus the `auto_now`
/// `updated_at`, written as the six columns a save can change (see
/// [`write_c_queries::USER_SAVE_FOR_ASSET`]).
pub async fn save_user(client: &Client, user: &User) -> Result<(), ViewError> {
    // H30's `timezone.now()`, then `Model.save`'s own clock for `updated_at`
    // (`DateTimeField(auto_now=True).pre_save`). Two calls, as in Python.
    let hook_now = write::now();
    let prepared = user_hook::before_save(
        user.email.as_deref(),
        &user.display_name,
        user.is_superuser,
        user.is_staff,
        &user.token,
        user.token_updated_at,
        hook_now,
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let updated_at = write::now();
    let statement = client
        .prepare_cached(write_c_queries::USER_SAVE_FOR_ASSET.sql)
        .await?;
    client
        .execute(
            &statement,
            &[
                &user.id,
                &prepared.email,
                &prepared.display_name,
                &prepared.is_staff,
                &prepared.token,
                &prepared.token_updated_at,
                &updated_at,
                &user.avatar,
                &user.avatar_asset_id,
                &user.cover_image,
                &user.cover_image_asset_id,
            ],
        )
        .await
        .map_err(super::workspace_write::integrity)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

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

/// `FileAsset.all_objects.get(id=asset_id, workspace__slug=slug)`.
async fn load_any_state(
    client: &Client,
    asset_id: Uuid,
    slug: &str,
) -> Result<Option<FileAsset>, ViewError> {
    let statement = client
        .prepare_cached(write_c_queries::FILE_ASSET_ANY_STATE_IN_WORKSPACE.sql)
        .await?;
    let row = client.query_opt(&statement, &[&asset_id, &slug]).await?;
    row.as_ref()
        .map(|row| FileAsset::from_row(row, 0))
        .transpose()
        .map_err(ViewError::from)
}

/// `S3Storage(request=request)`'s host/scheme inputs
/// (`plane/settings/storage.py:42-54`).
fn request_info<'a>(ctx: &'a ViewCtx<'a>) -> RequestInfo<'a> {
    RequestInfo {
        scheme: if ctx.ctx.is_secure { "https" } else { "http" },
        host: ctx.ctx.header("host").unwrap_or_default(),
    }
}

/// `sanitize_filename(request.data.get("name")) or "unnamed"` is shared with
/// 20's POST through `workspace_write::sanitized_name`; nothing else lives here.
#[cfg(test)]
mod tests {
    use super::*;

    /// `UserAssetsV2Endpoint` accepts exactly two entity types (`v2.py:162`).
    #[test]
    fn the_entity_type_allow_list_is_avatar_and_cover() {
        assert_eq!(USER_ENTITY_TYPES, ["USER_AVATAR", "USER_COVER"]);
        assert!(!USER_ENTITY_TYPES.contains(&"WORKSPACE_LOGO"));
        assert!(!USER_ENTITY_TYPES.contains(&"PROJECT_COVER"));
    }

    /// 21's list is the intersection with the shared upload list; 20's
    /// allow-list is narrower but the same MIME check (`v2.py:169-183`).
    #[test]
    fn the_image_allow_list_matches_the_shared_one() {
        assert!(super::super::workspace_write::IMAGE_TYPES.contains(&"image/webp"));
        assert!(!super::super::workspace_write::IMAGE_TYPES.contains(&"text/plain"));
    }
}
