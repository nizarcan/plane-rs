// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `BulkEstimatePointEndpoint`'s write actions (routes 46 and 47).
//!
//! Python: `apps/api/plane/app/views/estimate/base.py:63-150`.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/projects/{project_id}/estimates/` | `create` | `:63-101` |
//! | `PATCH …/estimates/{estimate_id}/` | `partial_update` | `:108-144` |
//! | `DELETE …/estimates/{estimate_id}/` | `destroy` | `:146-150` |
//!
//! ## create (`:63-101`)
//!
//! The decorator invalidates `/api/workspaces/:slug/estimates/` before the
//! view. `request.data.get("estimate")` is dereferenced with `.get`, so a
//! non-mapping body, a missing key, `null`, a string or a list is an
//! `AttributeError` 500 (`:65-66`). The row is built by hand — no
//! `EstimateSerializer` — so `name`/`type`/`last_used` are only coerced by the
//! model field at INSERT time: a 256-character name is a `DataError` 500, a
//! duplicate `(name, project)` is the family `IntegrityError` 400, and a
//! `last_used` Django's `BooleanField` rejects is the family `ValidationError`
//! 400. `generate_random_name()` is evaluated eagerly even when the body
//! carries a name (`:66`).
//!
//! **The estimate is written before the points are validated** (`:69-80`): a
//! POST whose `estimate_points` fail (or are absent → `{"non_field_errors":
//! ["No data provided"]}`) answers 400 with the estimate already inserted. The
//! points themselves are inserted from the **raw** body list (`:82-98`), not
//! from `validated_data`, with `bulk_create(batch_size=10,
//! ignore_conflicts=True)`: no `save()`, but `auto_now`/`auto_now_add` still
//! run in the insert compiler and `created_by`/`updated_by` are the caller.
//!
//! ## partial_update (`:108-144`)
//!
//! The body is read **before** the lookup: an empty/absent `estimate_points`
//! is 400 `{"error": "Estimate points are required"}` even when the estimate
//! does not exist (`:110-116`). `estimate_points: null`/a number is `len()`
//! raising (`TypeError` 500); a non-empty string/dict passes the length check
//! and fails later in the comprehension (`AttributeError` 500).
//!
//! `estimate` is optional; a truthy value must be a mapping. `name`/`type` are
//! assigned raw and saved through a full-row save, so the same model-field
//! rules as create apply, and `updated_by`/`updated_at` change. The points
//! match body ids by `str(uuid)` against the queryset row and
//! `bulk_update(…, ["key", "value"])` writes only those two columns — no
//! timestamps, no audit — inside Django's `transaction.atomic`.
//!
//! ## destroy (`:146-150`)
//!
//! `Estimate.objects.get(...)` then H03: `deleted_at`, a full save and the
//! `soft_delete_related_objects` task. `projects.estimate_id` is **not**
//! cleared in the request; the cascade task does that in the worker.

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::estimate::Estimate;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use crate::common::{cache, client, lookup};
use crate::estimates::{queries, render, write_estimate_queries, write_queries};
use crate::views::write_support::{self, Body, body_get};
use crate::write::fields::{self, Raw};
use crate::write::generated::estimate_point_serializer;
use crate::write::{self, ChildError, ListError, Serializer, Value};

/// `@invalidate_cache(path="/api/workspaces/:slug/estimates/",
/// url_params=True, user=False)` (`base.py:63`, `:108`, `:146`).
///
/// The key has no user part here and the only consumer caches under
/// `get_full_path() + ":" + user_id`, so the delete is a permanent no-op —
/// ported as such (`plane/utils/cache.py:54-69`).
const CACHE_PATH: &str = "/api/workspaces/:slug/estimates/";

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

/// `BulkEstimatePointEndpoint.create` (`plane/app/views/estimate/base.py:63-101`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // The decorator runs before the view body (and therefore before the
        // parse and the lookup).
        cache::invalidate_cache(ctx, Some(CACHE_PATH), true, false, false).await?;

        let data = ctx.data()?;
        // `estimate = request.data.get("estimate")` (`:65`).
        let estimate_body = match body_get(data, "estimate")? {
            Body::Missing => return Err(no_get("NoneType")),
            Body::Form(_) => return Err(no_get("str")),
            Body::Json(Json::Dict(map)) => map,
            Body::Json(value) => return Err(no_get(json_type(value))),
        };
        // `estimate.get("name", generate_random_name())` (`:66`): the default
        // is evaluated eagerly, whether or not `name` is present.
        let generated = generate_random_name();
        let name_raw = estimate_body
            .get("name")
            .cloned()
            .unwrap_or(Json::from(generated.as_str()));
        let type_raw = estimate_body
            .get("type")
            .cloned()
            .unwrap_or_else(|| Json::from("categories"));
        let last_used_raw = estimate_body
            .get("last_used")
            .cloned()
            .unwrap_or(Json::Bool(false));

        // `Estimate.objects.create(...)` → `ProjectBaseModel.save` first reads
        // `self.project.workspace` (`plane/db/models/project.py:237-239`). The
        // forward FK uses `_base_manager`, so a soft-deleted project still
        // resolves; a missing one is `DoesNotExist` → 404 before the INSERT.
        let Some(row) = client
            .query_opt(write_queries::ESTIMATE_WRITE_PROJECT.sql, &[&project_id])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = row.get(0);

        // Field preparation happens in `local_concrete_fields` order, so
        // `last_used`'s `ValidationError` is raised after `name`/`type` were
        // merely stringified and before any SQL.
        let name = char_prep(&name_raw).ok_or(ViewError::Integrity)?;
        let estimate_type = char_prep(&type_raw).ok_or(ViewError::Integrity)?;
        let last_used = bool_prep(&last_used_raw)?;

        // `auto_now_add` / `auto_now` are two independent `timezone.now()`
        // calls (`django/db/models/fields/__init__.py:864-877`).
        let created_at = write::now();
        let updated_at = write::now();
        let estimate = Estimate {
            created_at,
            updated_at,
            created_by_id: Some(user_id),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            project_id,
            workspace_id,
            name,
            description: String::new(),
            r#type: estimate_type,
            last_used,
        };
        insert_estimate(client, &estimate).await?;

        // `estimate_points = request.data.get("estimate_points", [])` (`:76`)
        // followed by the `many=True` serializer (`:78-80`). The serializer's
        // validated values are only the gate; the rows are built from the raw
        // tree below, as Python does.
        let points_value = data.json().and_then(|body| body.get("estimate_points"));
        match write::list_of(points_value, |_index, item| async move {
            validate_point(ctx, item, client).await
        })
        .await
        {
            Ok(_) => {}
            Err(ListError::Validation(response)) => return Ok(response),
            Err(ListError::Fatal(error)) => return Err(error),
        }

        let points = match body_get(data, "estimate_points")? {
            Body::Missing | Body::Json(Json::Null) => &[][..],
            Body::Json(Json::List(items)) => items.as_slice(),
            Body::Json(_) | Body::Form(_) => {
                // `list_of` accepted the same value, so this is unreachable.
                return Err(ViewError::Unknown(
                    "estimate_points changed shape between validation and insert".to_owned(),
                ));
            }
        };
        insert_points(client, points, &estimate, user_id).await?;

        // `EstimateReadSerializer(estimate).data` (`:100-101`), whose `points`
        // is a reverse manager: the `bulk_create` result is ignored and the
        // rows are re-read in `("value",)` order.
        let body = render::estimate_read(ctx, &estimate).await?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// One child of `EstimatePointSerializer(data=…, many=True)`.
async fn validate_point<'a>(
    ctx: &'a ViewCtx<'a>,
    item: &'a Json,
    client: &deadpool_postgres::Client,
) -> Result<write::Validated, ChildError> {
    let mut serializer = Serializer::from_value(ctx, item, false);
    if let Err(error) = estimate_point_serializer::validate(&mut serializer, client).await {
        // A database failure inside the field pass is not a ValidationError;
        // `ListSerializer.to_internal_value` does not catch it.
        return Err(ChildError::fatal(error));
    }
    match serializer.finish() {
        Ok(values) => {
            // `EstimatePointSerializer.validate` (`plane/app/serializers/
            // estimate.py:21-27`) runs only after every field passed.
            if values.get("value").is_none() {
                return Err(ChildError::non_field("Estimate points are required"));
            }
            if let Some(Value::Text(value)) = values.get("value")
                && value.chars().count() > 20
            {
                return Err(ChildError::non_field(
                    "Value can't be more than 20 characters",
                ));
            }
            Ok(values)
        }
        Err(errors) => Err(ChildError::from_errors(errors)),
    }
}

/// `EstimatePoint.objects.bulk_create([...], batch_size=10,
/// ignore_conflicts=True)` (`base.py:82-98`), from the raw body list.
async fn insert_points(
    client: &deadpool_postgres::Client,
    points: &[Json],
    estimate: &Estimate,
    user_id: Uuid,
) -> Result<(), ViewError> {
    if points.is_empty() {
        return Ok(());
    }

    let mut ids: Vec<Uuid> = Vec::with_capacity(points.len());
    let mut keys: Vec<i32> = Vec::with_capacity(points.len());
    let mut descriptions: Vec<String> = Vec::with_capacity(points.len());
    let mut values: Vec<String> = Vec::with_capacity(points.len());
    for item in points {
        let Some(map) = item.as_dict() else {
            return Err(ViewError::Unknown(
                "AttributeError: an estimate_point is not an object".to_owned(),
            ));
        };
        // `estimate_point.get("key", 0)` / `.get("value", "")` /
        // `.get("description", "")` (`:86-88`) with Django's field coercion in
        // `IntegerField.get_prep_value` / `CharField.to_python`.
        let key = match map.get("key") {
            None => 0,
            Some(value) => int_prep(value)?
                .ok_or_else(|| ViewError::Unknown("integer field got NULL".to_owned()))?,
        };
        let value = match map.get("value") {
            None => String::new(),
            Some(value) => char_prep(value).ok_or(ViewError::Integrity)?,
        };
        let description = match map.get("description") {
            None => String::new(),
            Some(value) => char_prep(value).ok_or(ViewError::Integrity)?,
        };
        ids.push(Uuid::new_v4());
        keys.push(key);
        descriptions.push(description);
        values.push(value);
    }

    // `auto_now_add`/`auto_now` run in the insert compiler for `bulk_create`
    // too; every row's two timestamps are an independent `timezone.now()`
    // (`DateTimeField.pre_save`, `django/db/models/fields/__init__.py:864-877`).
    let mut created: Vec<chrono::DateTime<chrono::Utc>> = Vec::with_capacity(ids.len());
    let mut updated: Vec<chrono::DateTime<chrono::Utc>> = Vec::with_capacity(ids.len());
    for _ in &ids {
        created.push(write::now());
        updated.push(write::now());
    }
    let created_by = vec![Some(user_id); ids.len()];
    let updated_by = created_by.clone();
    let project_ids = vec![Some(estimate.project_id); ids.len()];
    let workspace_ids = vec![Some(estimate.workspace_id); ids.len()];
    let estimate_ids = vec![Some(estimate.id); ids.len()];

    let statement = client
        .prepare_cached(write_estimate_queries::ESTIMATE_POINT_BULK_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    // `bulk_create` wraps its batches in `transaction.atomic(savepoint=False)`
    // (`django/db/models/query.py:756`): a failure rolls every batch back.
    client
        .batch_execute("BEGIN")
        .await
        .map_err(ViewError::from)?;
    let result = client
        .execute(
            &statement,
            &[
                &created,
                &updated,
                &created_by,
                &updated_by,
                &ids,
                &project_ids,
                &workspace_ids,
                &estimate_ids,
                &keys,
                &descriptions,
                &values,
            ],
        )
        .await;
    match result {
        Ok(_) => {
            client
                .batch_execute("COMMIT")
                .await
                .map_err(ViewError::from)?;
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(integrity(error))
        }
    }
}

// ---------------------------------------------------------------------------
// partial_update
// ---------------------------------------------------------------------------

/// `BulkEstimatePointEndpoint.partial_update`
/// (`plane/app/views/estimate/base.py:108-144`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let estimate_id = lookup::uuid_kwarg(
            "estimate_id",
            ctx.kwarg("estimate_id").ok_or(ViewError::Key)?,
        )?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        cache::invalidate_cache(ctx, Some(CACHE_PATH), true, false, false).await?;

        // `if not len(request.data.get("estimate_points", []))` (`:110`):
        // before the lookup, so a bad body on a missing estimate is 400.
        let data = ctx.data()?;
        let points_value = body_get(data, "estimate_points")?;
        let length = match points_value {
            Body::Missing => 0,
            Body::Form(text) => text.chars().count(),
            Body::Json(Json::Null) => {
                return Err(ViewError::Unknown(
                    "TypeError: object of type 'NoneType' has no len()".to_owned(),
                ));
            }
            Body::Json(value) => python_len(value)?,
        };
        if length == 0 {
            return Ok(write::error_body(400, "Estimate points are required"));
        }

        // `Estimate.objects.get(pk=…, workspace__slug=…, project_id=…)`
        // (`:116`).
        let Some(row) = client
            .query_opt(
                queries::ESTIMATE_BY_ID.sql,
                &[&estimate_id, &slug, &project_id],
            )
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let mut estimate = Estimate::from_row(&row, 0)
            .map_err(|error| ViewError::Unknown(format!("decoding the estimate row: {error}")))?;

        // `if request.data.get("estimate"):` (`:118-121`). A form value is a
        // `QueryDict` string: non-empty is truthy and `.get` raises, empty is
        // falsy and skipped.
        let estimate_value = body_get(data, "estimate")?;
        let estimate_body = match &estimate_value {
            Body::Missing => None,
            Body::Form("") => None,
            Body::Form(_) => return Err(no_get("str")),
            Body::Json(value) if truthy(value) => {
                Some(value.as_dict().ok_or_else(|| no_get(json_type(value)))?)
            }
            Body::Json(_) => None,
        };
        if let Some(map) = estimate_body {
            if let Some(name) = map.get("name") {
                estimate.name = char_prep(name).ok_or(ViewError::Integrity)?;
            }
            if let Some(estimate_type) = map.get("type") {
                estimate.r#type = char_prep(estimate_type).ok_or(ViewError::Integrity)?;
            }
            // `ProjectBaseModel.save` re-derives the workspace and H01 sets
            // `updated_by`; `auto_now` sets `updated_at`.
            let Some(row) = client
                .query_opt(write_queries::ESTIMATE_WRITE_PROJECT.sql, &[&project_id])
                .await
                .map_err(ViewError::from)?
            else {
                return Err(ViewError::ObjectDoesNotExist);
            };
            estimate.workspace_id = row.get(0);
            estimate.updated_at = write::now();
            estimate.updated_by_id = Some(user_id);
            update_estimate(client, &estimate).await?;
        }

        // `estimate_points_data = request.data.get("estimate_points", [])`
        // (`:123`).
        let points_data: &[Json] = match body_get(data, "estimate_points")? {
            Body::Missing | Body::Json(Json::Null) => &[],
            Body::Json(Json::List(items)) => items.as_slice(),
            Body::Json(other) => {
                // A non-empty dict/string passes the length check, then the
                // comprehension calls `.get` on its keys/characters.
                return Err(no_get(match other {
                    Json::Dict(_) | Json::Str(_) => "str",
                    value => json_type(value),
                }));
            }
            Body::Form(_) => return Err(no_get("str")),
        };

        // `pk__in=[estimate_point.get("id") for …]` (`:125-130`): a non-object
        // element is an AttributeError; each id goes through Django's
        // `UUIDField.to_python`, whose `ValidationError` is the family 400.
        let mut ids = Vec::with_capacity(points_data.len());
        for item in points_data {
            let Some(map) = item.as_dict() else {
                return Err(no_get(json_type(item)));
            };
            ids.push(match map.get("id") {
                None | Some(Json::Null) => None,
                Some(value) => Some(django_uuid(value)?),
            });
        }
        let wanted: std::collections::HashSet<Uuid> = ids.into_iter().flatten().collect();

        let id_list: Vec<Uuid> = wanted.iter().copied().collect();
        let rows = client
            .query(
                write_estimate_queries::ESTIMATE_POINTS_BY_IDS.sql,
                &[&estimate_id, &project_id, &slug, &id_list],
            )
            .await
            .map_err(ViewError::from)?;
        let mut points = Vec::with_capacity(rows.len());
        for row in &rows {
            points.push(
                plane_db::models::estimate::EstimatePoint::from_row(row, 0).map_err(|error| {
                    ViewError::Unknown(format!("decoding the estimate point row: {error}"))
                })?,
            );
        }

        // `for estimate_point in estimate_points: …` (`:132-139`): the first
        // data element whose raw `id` stringifies to the row's id wins.
        let mut updates: Vec<(Uuid, Option<i32>, Option<String>)> = Vec::new();
        for point in points {
            if !wanted.contains(&point.id) {
                continue;
            }
            let Some(map) = points_data
                .iter()
                .find(|item| raw_id_matches(item, point.id))
                .and_then(Json::as_dict)
            else {
                continue;
            };
            // `bulk_update`'s `Value(attr, output_field=…)` runs the field's
            // `get_prep_value` (so a bool is Python `str(True)` = "True" and a
            // float is `repr(1.0)` = "1.0") and PostgreSQL then casts the CASE
            // to the column type; `varchar(255)` truncates silently.
            let key = match map.get("key") {
                None => Some(point.key),
                Some(value) => int_prep(value)?,
            };
            let value = match map.get("value") {
                None => Some(point.value),
                Some(value) => patch_value_prep(value),
            };
            updates.push((point.id, key, value));
        }

        // `bulk_update(…, ["key", "value"], batch_size=10)` (`:141`): one
        // `CASE` statement per batch inside `transaction.atomic`.
        if !updates.is_empty() {
            let statement = client
                .prepare_cached(write_estimate_queries::ESTIMATE_POINT_UPDATE.sql)
                .await
                .map_err(ViewError::from)?;
            client
                .batch_execute("BEGIN")
                .await
                .map_err(ViewError::from)?;
            let mut result = Ok(());
            for (id, key, value) in &updates {
                if let Err(error) = client.execute(&statement, &[key, value, id]).await {
                    result = Err(error);
                    break;
                }
            }
            match result {
                Ok(()) => {
                    client
                        .batch_execute("COMMIT")
                        .await
                        .map_err(ViewError::from)?;
                }
                Err(error) => {
                    let _ = client.batch_execute("ROLLBACK").await;
                    return Err(integrity(error));
                }
            }
        }

        // `EstimateReadSerializer(estimate).data` (`:143-144`).
        let body = render::estimate_read(ctx, &estimate).await?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// destroy
// ---------------------------------------------------------------------------

/// `BulkEstimatePointEndpoint.destroy` (`plane/app/views/estimate/base.py:146-150`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let estimate_id = lookup::uuid_kwarg(
            "estimate_id",
            ctx.kwarg("estimate_id").ok_or(ViewError::Key)?,
        )?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        cache::invalidate_cache(ctx, Some(CACHE_PATH), true, false, false).await?;

        // `Estimate.objects.get(pk=…, workspace__slug=…, project_id=…)`
        // (`:148`).
        let exists = client
            .query_opt(
                queries::ESTIMATE_BY_ID.sql,
                &[&estimate_id, &slug, &project_id],
            )
            .await
            .map_err(ViewError::from)?;
        if exists.is_none() {
            return Err(ViewError::ObjectDoesNotExist);
        }

        // H03: `deleted_at`, a full save, then the cascade task. A full save
        // would re-read `self.project.workspace`, whose value is unchanged;
        // `write::soft_delete` writes the same columns.
        write::soft_delete(
            client,
            "estimates",
            "id",
            estimate_id,
            Some(user_id),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "estimate", estimate_id).await?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn insert_estimate(
    client: &deadpool_postgres::Client,
    estimate: &Estimate,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(write_estimate_queries::ESTIMATE_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &estimate.created_at,
                &estimate.updated_at,
                &estimate.created_by_id,
                &estimate.updated_by_id,
                &estimate.id,
                &estimate.project_id,
                &estimate.workspace_id,
                &estimate.name,
                &estimate.description,
                &estimate.r#type,
                &estimate.last_used,
            ],
        )
        .await
        .map(|_| ())
        .map_err(integrity)
}

async fn update_estimate(
    client: &deadpool_postgres::Client,
    estimate: &Estimate,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(write_estimate_queries::ESTIMATE_UPDATE.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &estimate.created_at,
                &estimate.updated_at,
                &estimate.created_by_id,
                &estimate.updated_by_id,
                &estimate.deleted_at,
                &estimate.project_id,
                &estimate.workspace_id,
                &estimate.name,
                &estimate.description,
                &estimate.r#type,
                &estimate.last_used,
                &estimate.id,
            ],
        )
        .await
        .map(|_| ())
        .map_err(integrity)
}

/// SQLSTATE class 23 is Django's `IntegrityError` (the family 400); anything
/// else is the family 500.
fn integrity(error: tokio_postgres::Error) -> ViewError {
    write_support::integrity(error)
}

/// `str(request.user.id)` comparison of a body `id` (`base.py:135`): only a
/// JSON string can equal Python's canonical UUID string.
fn raw_id_matches(item: &Json, id: Uuid) -> bool {
    let rendered = id.to_string();
    match item.as_dict().and_then(|map| map.get("id")) {
        Some(Json::Str(text)) => text.as_str() == Some(rendered.as_str()),
        _ => false,
    }
}

/// Django's `UUIDField.to_python` (`django/db/models/fields/__init__.py`) for
/// one raw body value, used by 47 PATCH's `pk__in` list.
///
/// A string goes through `uuid.UUID(hex=…)`; an int (a bool included) through
/// `uuid.UUID(int=…)`; anything else is the `AttributeError` Django rewrites
/// into a `ValidationError`. `None` is left for `IN (NULL)` (matches nothing).
fn django_uuid(value: &Json) -> Result<Uuid, ViewError> {
    match value {
        Json::Str(text) => {
            Uuid::parse_str(text.as_str().unwrap_or_default()).map_err(|_| ViewError::Validation)
        }
        Json::Int(number) => {
            let Some(number) = number.as_i64() else {
                return Err(ViewError::Validation);
            };
            let Ok(number) = u128::try_from(number) else {
                return Err(ViewError::Validation);
            };
            Ok(Uuid::from_u128(number))
        }
        Json::Bool(value) => Ok(Uuid::from_u128(u128::from(*value))),
        _ => Err(ViewError::Validation),
    }
}

/// `len(value)` for the 47 PATCH guard.
fn python_len(value: &Json) -> Result<usize, ViewError> {
    match value {
        Json::List(items) => Ok(items.len()),
        Json::Dict(map) => Ok(map.len()),
        Json::Str(text) => Ok(text.char_len()),
        other => Err(ViewError::Unknown(format!(
            "TypeError: object of type '{}' has no len()",
            json_type(other)
        ))),
    }
}

/// Python truthiness, for `if request.data.get("estimate"):` and — shared with
/// set B — the estimate-point create guard and 49 DELETE's `if
/// new_estimate_id:`.
pub(crate) fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Int(value) => value.as_i64() != Some(0),
        Json::Float(value) => *value != 0.0,
        Json::Str(value) => value.char_len() > 0,
        Json::List(items) => !items.is_empty(),
        Json::Dict(map) => !map.is_empty(),
    }
}

/// `AttributeError: '<type>' object has no attribute 'get'` — a family 500.
fn no_get(type_name: &str) -> ViewError {
    ViewError::Unknown(format!(
        "AttributeError: '{type_name}' object has no attribute 'get'"
    ))
}

/// Python's `type(value).__name__`.
fn json_type(value: &Json) -> &'static str {
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

/// Django's `CharField.to_python`: `None` stays `None`, a string stays as it
/// is, every other JSON value is Python `str(value)`.
///
/// Shared with set B (48's raw `value`, 49 DELETE's `str(new_estimate_id)`).
pub(crate) fn char_prep(value: &Json) -> Option<String> {
    match value {
        Json::Null => None,
        Json::Str(text) => Some(text.as_str().unwrap_or_default().to_owned()),
        other => Some(fields::python_str(Raw::Json(other))),
    }
}

/// Django's `IntegerField.get_prep_value` (Python `int()`), then the column's
/// `int4` range. `None` is SQL NULL (a NOT NULL violation at the INSERT).
///
/// Shared with set B: 48's raw `key` goes through the same preparation.
pub(crate) fn int_prep(value: &Json) -> Result<Option<i32>, ViewError> {
    match value {
        Json::Null => Ok(None),
        Json::Bool(value) => Ok(Some(i32::from(*value))),
        Json::Int(value) => match value.as_i64() {
            Some(number) => i32::try_from(number)
                .map(Some)
                .map_err(|_| int_out_of_range()),
            None => Err(int_out_of_range()),
        },
        Json::Float(value) => {
            let number = value.trunc();
            if !number.is_finite() || number < f64::from(i32::MIN) || number > f64::from(i32::MAX) {
                return Err(int_out_of_range());
            }
            Ok(Some(number as i32))
        }
        Json::Str(text) => python_int(text.as_str().unwrap_or_default())
            .and_then(|number| i32::try_from(number).map_err(|_| int_out_of_range()))
            .map(Some),
        Json::List(_) | Json::Dict(_) => Err(ViewError::Unknown(
            "TypeError: int() argument must be a string, a bytes-like object or a real number"
                .to_owned(),
        )),
    }
}

/// Python `int(str)`: ASCII whitespace around an optional sign, `_` allowed
/// between digits. Anything else is the `ValueError` Django re-raises.
///
/// Shared with set B for a *form* body's raw `key`, which `QueryDict` hands
/// over as a string.
pub(crate) fn python_int(text: &str) -> Result<i64, ViewError> {
    let text = text.trim();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() {
        return Err(int_error());
    }
    let mut value: i64 = 0;
    let mut previous_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if previous_underscore {
                return Err(int_error());
            }
            previous_underscore = true;
            continue;
        }
        let Some(digit) = ch.to_digit(10) else {
            return Err(int_error());
        };
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i64::from(digit)))
            .ok_or_else(int_out_of_range)?;
        previous_underscore = false;
    }
    if previous_underscore {
        return Err(int_error());
    }
    Ok(if negative { -value } else { value })
}

fn int_error() -> ViewError {
    ViewError::Unknown("ValueError: invalid literal for int()".to_owned())
}

pub(crate) fn int_out_of_range() -> ViewError {
    ViewError::Unknown("DataError: integer out of range".to_owned())
}

/// 47 PATCH's `value` for `bulk_update`: `CharField.get_prep_value` (Python
/// `str`), then the `CAST(… AS varchar(255))` Django wraps around the CASE on
/// PostgreSQL (`requires_casted_case_in_updates`), which **truncates** to 255
/// characters instead of raising — unlike create, where the INSERT's raw
/// varchar(255) column overflows.
fn patch_value_prep(value: &Json) -> Option<String> {
    char_prep(value).map(|text| text.chars().take(255).collect())
}

/// Django's `BooleanField.to_python` (`django/db/models/fields/__init__.py`):
/// `True`/`False` (and `1`/`0`, which compare equal), the four exact strings,
/// then `ValidationError` → the family's 400 `Please provide valid detail`.
fn bool_prep(value: &Json) -> Result<bool, ViewError> {
    match value {
        Json::Bool(value) => Ok(*value),
        Json::Int(value) => match value.as_i64() {
            Some(1) => Ok(true),
            Some(0) => Ok(false),
            _ => Err(ViewError::Validation),
        },
        Json::Float(value) if *value == 1.0 => Ok(true),
        Json::Float(value) if *value == 0.0 => Ok(false),
        Json::Str(text) => match text.as_str() {
            Some("t" | "True" | "1") => Ok(true),
            Some("f" | "False" | "0") => Ok(false),
            _ => Err(ViewError::Validation),
        },
        _ => Err(ViewError::Validation),
    }
}

/// `generate_random_name()` (`plane/app/views/estimate/base.py:29-31`): ten
/// `random.choice(string.ascii_lowercase)`.
fn generate_random_name() -> String {
    let bytes = Uuid::new_v4().into_bytes();
    bytes
        .iter()
        .take(10)
        .map(|byte| char::from(b'a' + byte % 26))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generated_name_is_ten_lowercase_letters() {
        let name = generate_random_name();
        assert_eq!(name.len(), 10);
        assert!(name.chars().all(|ch| ch.is_ascii_lowercase()));
    }

    #[test]
    fn python_int_matches_cpython_for_the_visible_cases() {
        assert_eq!(python_int(" 3 ").ok(), Some(3));
        assert_eq!(python_int("+3").ok(), Some(3));
        assert_eq!(python_int("-3").ok(), Some(-3));
        assert_eq!(python_int("1_0").ok(), Some(10));
        assert!(python_int("3.0").is_err());
        assert!(python_int("abc").is_err());
        assert!(python_int("").is_err());
        assert_eq!(int_prep(&Json::Float(1.9)).ok(), Some(Some(1)));
        assert_eq!(int_prep(&Json::Float(-1.9)).ok(), Some(Some(-1)));
        assert_eq!(int_prep(&Json::Bool(true)).ok(), Some(Some(1)));
    }

    #[test]
    fn boolean_preparation_follows_djangos_exact_strings() {
        assert_eq!(bool_prep(&Json::Bool(false)).ok(), Some(false));
        assert_eq!(bool_prep(&Json::from("True")).ok(), Some(true));
        assert_eq!(bool_prep(&Json::from("0")).ok(), Some(false));
        // Django does not lower-case: "true" is a ValidationError.
        assert_eq!(bool_prep(&Json::from("true")), Err(ViewError::Validation));
        assert_eq!(bool_prep(&Json::Null), Err(ViewError::Validation));
    }

    /// `bulk_update`'s value prep is Python `str`; the cast truncates.
    #[test]
    fn the_patch_value_prep_truncates_to_255_characters() {
        let long = "é".repeat(300);
        let stored = patch_value_prep(&Json::from(long.as_str())).expect("value");
        assert_eq!(stored.chars().count(), 255);
        // `str(True)` is "True"; `repr(1.0)` is "1.0".
        assert_eq!(patch_value_prep(&Json::Bool(true)).as_deref(), Some("True"));
        assert_eq!(patch_value_prep(&Json::Float(1.0)).as_deref(), Some("1.0"));
        assert_eq!(
            patch_value_prep(&Json::from("plain")).as_deref(),
            Some("plain")
        );
        assert_eq!(patch_value_prep(&Json::Null), None);
    }
}
