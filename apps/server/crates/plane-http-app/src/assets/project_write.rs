// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ProjectAssetEndpoint` and `ProjectBulkAssetEndpoint`
//! (`plane/app/views/asset/v2.py:584-826`), routes 24, 25 and 26.
//!
//! | Route | Python | Handler |
//! | --- | --- | --- |
//! | `POST /api/assets/v2/workspaces/{slug}/projects/{project_id}/` | `:616-681` | [`post`] |
//! | `PATCH …/projects/{project_id}/{pk}/` | `:683-702` | [`patch`] |
//! | `DELETE …/projects/{project_id}/{pk}/` | `:704-718` | [`delete`] |
//! | `POST …/projects/{project_id}/{entity_id}/bulk/` | `:756-826` | [`bulk`] |
//!
//! ## The `PROJECT_COVER` `TypeError` (`v2.py:658-667`)
//!
//! 24 POST passes `project_id=project_id` **and**
//! `**self.get_entity_id_field(…)`, which returns `{"project_id": …}` for
//! `PROJECT_COVER`. CPython raises `TypeError: got multiple values for keyword
//! argument 'project_id'` while evaluating the call, so a project-cover upload
//! is always a family 500 — before the INSERT, and regardless of whether the
//! identifier is a valid UUID. [`post`] reproduces that.
//!
//! ## The bulk rebind (`v2.py:756-826`)
//!
//! `asset_ids` is filtered to the caller's own uploads in this workspace that
//! are either unassociated or already in this project (`:772-776`), so a
//! non-uploader gets the 404 body rather than a 403. `assets.first()` decides
//! which branch runs; the branch then runs one `UPDATE` over the whole set and
//! — for `PROJECT_COVER` — one `Project.objects.get` + `save()` **per asset**
//! (`:797`), so with several ids the newest-first iteration leaves the oldest
//! asset as the project's cover.
//!
//! Three branches wrap their `UPDATE` in `try/except IntegrityError`
//! (`:802-805`, `:809-813`, `:820-824`) because the issue/comment/draft may have
//! been deleted after the upload; a foreign-key failure is swallowed, every
//! other database error still propagates. The page branch (`:815-816`) has no
//! guard.
//!
//! ## No entity side effects
//!
//! Unlike 19, 25 PATCH/DELETE never rebind an entity: the project cover is only
//! set by 24 (`get_entity_id_field`), the entity save and the bulk rebind.

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use plane_storage::PresignPost;
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::write;

use super::workspace_write::{
    ENTITY_TYPES, IMAGE_TYPES, assign_entity_fk, body_get, ensure_mapping, entity_id_column,
    fk_uuid, insert_asset, integrity, is_integrity, new_asset, publish_metadata_task, request_info,
    sanitized_name, size_arg, storage_metadata_truthy, truthy, type_error,
};
use super::{entities, queries, support, write_a_queries};

/// `ProjectAssetEndpoint.post` (`v2.py:616-681`).
pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("pk").is_some() {
            // `/…/projects/{project_id}/{pk}/` supplies a `pk` this method does
            // not take: `post() got an unexpected keyword argument 'pk'` → 500.
            return Err(type_error(
                "ProjectAssetEndpoint.post() got an unexpected keyword argument 'pk'",
            ));
        }
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;
        ensure_mapping(data)?;

        let name = sanitized_name(body_get(data, "name").as_ref());
        let mime = body_get(data, "type");
        let size = size_arg(data, ctx.settings.limits.file_size_limit)?;
        // `request.data.get("entity_type", "")` (`v2.py:621`): the empty default
        // fails the allow-list exactly like an absent key.
        let entity_type = body_get(data, "entity_type");
        let entity_identifier = body_get(data, "entity_identifier").unwrap_or(Json::Null);

        let Some(entity_type) = entity_type.as_ref().and_then(Json::as_str) else {
            return entities::invalid_entity_type();
        };
        if !ENTITY_TYPES.contains(&entity_type) {
            return entities::invalid_entity_type();
        }

        let file_type = match mime.as_ref() {
            None => "image/jpeg",
            Some(Json::Str(value)) => value.as_str().unwrap_or_default(),
            Some(_) => return entities::invalid_file_type(),
        };
        if !IMAGE_TYPES.contains(&file_type) {
            return entities::invalid_file_type();
        }

        let size_limit = size.min(ctx.settings.limits.file_size_limit);
        let workspace = lookup::workspace_by_slug(client, slug).await?;

        // `FileAsset.objects.create(…, project_id=project_id,
        // **self.get_entity_id_field(entity_type, entity_identifier))`: the
        // cover branch binds `project_id` twice, which is a `TypeError` raised
        // before `create()` runs (`v2.py:658-667`).
        let column = entity_id_column(entity_type, true);
        if column == Some("project_id") {
            return Err(type_error(
                "TypeError: got multiple values for keyword argument 'project_id'",
            ));
        }

        let asset_key = format!("{}/{}-{}", workspace.id, Uuid::new_v4().simple(), name);
        let mut asset = new_asset(&asset_key, &name, file_type, size_limit, entity_type, user)?;
        asset.workspace_id = Some(workspace.id);
        asset.project_id = Some(project_id);
        if let Some(column) = column {
            let value = fk_uuid(&entity_identifier)?;
            assign_entity_fk(&mut asset, column, value);
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

/// `ProjectAssetEndpoint.patch` (`v2.py:683-702`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("pk").is_none() {
            return Err(type_error(
                "ProjectAssetEndpoint.patch() missing 1 required positional argument: 'pk'",
            ));
        }
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let pk = support::uuid_kwarg(ctx, "pk")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(asset) = load_project_asset(client, pk, slug, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if !support::can_modify_asset(client, user, &asset).await? {
            return entities::no_permission();
        }

        if !storage_metadata_truthy(&asset) {
            publish_metadata_task(ctx, pk).await?;
        }
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
            .execute(&statement, &[&pk, &attributes])
            .await
            .map_err(integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `ProjectAssetEndpoint.delete` (`v2.py:704-718`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("pk").is_none() {
            return Err(type_error(
                "ProjectAssetEndpoint.delete() missing 1 required positional argument: 'pk'",
            ));
        }
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let pk = support::uuid_kwarg(ctx, "pk")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(asset) = load_project_asset(client, pk, slug, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if !support::can_modify_asset(client, user, &asset).await? {
            return entities::no_permission();
        }

        let deleted_at = write::now();
        let statement = client
            .prepare_cached(write_a_queries::FILE_ASSET_SET_DELETED.sql)
            .await?;
        client
            .execute(&statement, &[&pk, &deleted_at])
            .await
            .map_err(integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `ProjectBulkAssetEndpoint.post` (`v2.py:756-826`).
pub fn bulk<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = support::kwarg(ctx, "slug")?;
        let project_id = support::uuid_kwarg(ctx, "project_id")?;
        let entity_id = support::uuid_kwarg(ctx, "entity_id")?;
        let user = support::user_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;
        ensure_mapping(data)?;

        // `asset_ids = request.data.get("asset_ids", [])` + `if not asset_ids:`
        // (`v2.py:758-762`).
        let asset_ids = match body_get(data, "asset_ids") {
            Some(value) if truthy(&value) => asset_id_list(&value)?,
            _ => return entities::no_asset_ids(),
        };

        let statement = client
            .prepare_cached(write_a_queries::FILE_ASSETS_FOR_BULK.sql)
            .await?;
        let rows = client
            .query(&statement, &[&asset_ids, &slug, &user, &project_id])
            .await?;
        let assets: Vec<FileAsset> = rows
            .iter()
            .map(|row| FileAsset::from_row(row, 0))
            .collect::<Result<_, _>>()?;

        // `asset = assets.first()` (`v2.py:779-785`).
        let Some(asset) = assets.first() else {
            return entities::asset_not_found();
        };

        // `if asset.entity_type == PROJECT_COVER:` (`:788-797`). The in-memory
        // `asset.project_id = project_id` is what `can_modify_asset` sees.
        if asset.entity_type.as_deref() == Some("PROJECT_COVER") {
            let mut probe = asset.clone();
            probe.project_id = Some(project_id);
            if !support::can_modify_asset(client, user, &probe).await? {
                return entities::no_permission();
            }
            execute_bulk(
                client,
                &write_a_queries::FILE_ASSETS_SET_PROJECT,
                &[&project_id, &asset_ids, &slug, &user, &project_id],
            )
            .await?;
            // `[self.save_project_cover(asset, project_id) for asset in assets]`
            // (`:797`): one `Project.objects.get` + full save per row, in
            // `-created_at` order.
            for asset in &assets {
                save_project_cover(client, asset.id, project_id, user).await?;
            }
        }

        if asset.entity_type.as_deref() == Some("ISSUE_DESCRIPTION") {
            // `try: assets.update(issue_id=entity_id, project_id=project_id)
            // except IntegrityError: pass` (`:802-805`).
            let result = execute_bulk(
                client,
                &write_a_queries::FILE_ASSETS_SET_ISSUE_PROJECT,
                &[
                    &entity_id,
                    &project_id,
                    &asset_ids,
                    &slug,
                    &user,
                    &project_id,
                ],
            )
            .await;
            swallow_integrity(result)?;
        }

        if asset.entity_type.as_deref() == Some("COMMENT_DESCRIPTION") {
            // `:807-813` — note that this branch does not set `project_id`.
            let result = execute_bulk(
                client,
                &write_a_queries::FILE_ASSETS_SET_COMMENT,
                &[&entity_id, &asset_ids, &slug, &user, &project_id],
            )
            .await;
            swallow_integrity(result)?;
        }

        if asset.entity_type.as_deref() == Some("PAGE_DESCRIPTION") {
            // `:815-816`, unguarded.
            execute_bulk(
                client,
                &write_a_queries::FILE_ASSETS_SET_PAGE,
                &[&entity_id, &asset_ids, &slug, &user, &project_id],
            )
            .await?;
        }

        if asset.entity_type.as_deref() == Some("DRAFT_ISSUE_DESCRIPTION") {
            // `:818-824`.
            let result = execute_bulk(
                client,
                &write_a_queries::FILE_ASSETS_SET_DRAFT_ISSUE,
                &[&entity_id, &asset_ids, &slug, &user, &project_id],
            )
            .await;
            swallow_integrity(result)?;
        }

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `FileAsset.objects.get(id=pk, workspace__slug=slug, project_id=project_id)`
/// (`v2.py:686`, `:707`).
async fn load_project_asset(
    client: &deadpool_postgres::Client,
    pk: Uuid,
    slug: &str,
    project_id: Uuid,
) -> Result<Option<FileAsset>, ViewError> {
    let row = client
        .query_opt(
            queries::FILE_ASSET_IN_PROJECT.sql,
            &[&slug, &project_id, &pk],
        )
        .await?;
    row.as_ref()
        .map(|row| FileAsset::from_row(row, 0))
        .transpose()
        .map_err(ViewError::from)
}

/// `ProjectBulkAssetEndpoint.save_project_cover` (`v2.py:751-754`).
async fn save_project_cover(
    client: &deadpool_postgres::Client,
    cover_asset_id: Uuid,
    project_id: Uuid,
    user: Uuid,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(write_a_queries::PROJECT_BY_ID_FOR_COVER.sql)
        .await?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    let identifier: String = row.try_get(1)?;
    let identifier = plane_db::hooks::project::before_update(&identifier);
    let now = write::now();
    let statement = client
        .prepare_cached(write_a_queries::PROJECT_SET_COVER_FROM_BULK.sql)
        .await?;
    client
        .execute(
            &statement,
            &[&project_id, &cover_asset_id, &now, &user, &identifier],
        )
        .await
        .map_err(integrity)?;
    Ok(())
}

/// One of the bulk rebind `UPDATE`s.
async fn execute_bulk(
    client: &deadpool_postgres::Client,
    query: &plane_db::Query,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<u64, tokio_postgres::Error> {
    let statement = client.prepare_cached(query.sql).await?;
    client.execute(&statement, params).await
}

/// `except IntegrityError: pass` — every other database error propagates.
fn swallow_integrity(result: Result<u64, tokio_postgres::Error>) -> Result<(), ViewError> {
    match result {
        Ok(_) => Ok(()),
        Err(error) if is_integrity(&error) => Ok(()),
        Err(error) => Err(ViewError::from(error)),
    }
}

/// `id__in=<asset_ids>` as `UUIDField.to_python` applies it per element
/// (`v2.py:772-776`).
///
/// A JSON list is the normal shape. A bare string is iterated character by
/// character by Django, so every element is an invalid UUID (`ValidationError`
/// → 400 `{"error": "Please provide valid detail"}`); an int or a dict is not
/// iterable (`TypeError` → 500). `null`, `""` and the empty containers never
/// reach here: [`truthy`] has already answered the 400 body.
fn asset_id_list(value: &Json) -> Result<Vec<Uuid>, ViewError> {
    match value {
        Json::List(items) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                // `None`/`""` in the list compile to `IN (NULL)`, which matches
                // nothing: dropping them is the same predicate.
                if let Some(id) = fk_uuid(item)? {
                    ids.push(id);
                }
            }
            Ok(ids)
        }
        Json::Str(_) => Err(ViewError::Validation),
        _ => Err(type_error(&format!(
            "TypeError: '{}' object is not iterable",
            match value {
                Json::Bool(_) => "bool",
                Json::Int(_) => "int",
                Json::Float(_) => "float",
                Json::Dict(_) => "dict",
                _ => "object",
            }
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::pyjson::PyStr;

    #[test]
    fn only_a_json_list_of_ids_reaches_the_query() {
        let list = Json::List(vec![
            Json::Str(PyStr::from("a91c3d80-5ed0-40f3-92a8-a28c015f4388")),
            // `null` and `""` are `IN (NULL)`: dropped, not an error.
            Json::Null,
            Json::Str(PyStr::from("")),
        ]);
        assert_eq!(
            asset_id_list(&list).expect("valid list"),
            vec![Uuid::parse_str("a91c3d80-5ed0-40f3-92a8-a28c015f4388").expect("uuid")]
        );
        // A bare string is iterated per character by Django: always invalid.
        assert_eq!(
            asset_id_list(&Json::Str(PyStr::from(
                "a91c3d80-5ed0-40f3-92a8-a28c015f4388"
            ))),
            Err(ViewError::Validation)
        );
        // Not iterable: the family 500.
        assert!(matches!(
            asset_id_list(&Json::from(7_i64)),
            Err(ViewError::Unknown(_))
        ));
    }

    #[test]
    fn an_invalid_uuid_element_is_a_validation_error() {
        let list = Json::List(vec![Json::Str(PyStr::from("abc"))]);
        assert_eq!(asset_id_list(&list), Err(ViewError::Validation));
    }

    #[test]
    fn a_falsy_asset_ids_value_is_the_empty_body() {
        // The handler's guard, mirrored: these never reach `asset_id_list`.
        assert!(!truthy(&Json::List(Vec::new())));
        assert!(!truthy(&Json::Null));
        assert!(!truthy(&Json::Str(PyStr::from(""))));
        assert!(truthy(&Json::List(vec![Json::Null])));
    }

    /// The type-error message path only needs the type name.
    #[test]
    fn non_iterable_values_name_their_type() {
        let error = asset_id_list(&Json::from(3_i64)).expect_err("not iterable");
        let ViewError::Unknown(message) = error else {
            panic!("a TypeError is the family 500");
        };
        assert!(
            message.contains("'int' object is not iterable"),
            "{message}"
        );
    }
}
