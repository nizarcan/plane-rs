// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `EstimatePointEndpoint`'s actions (routes 48 and 49).
//!
//! Python: `apps/api/plane/app/views/estimate/base.py:153-268`.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/estimates/{estimate_id}/estimate-points/` | `create` | `:155-179` |
//! | `PATCH …/estimate-points/{estimate_point_id}/` | `partial_update` | `:182-194` |
//! | `DELETE …/estimate-points/{estimate_point_id}/` | `destroy` | `:197-268` |
//!
//! `@allow_permission([ADMIN, MEMBER], PROJECT)` runs before every one of them
//! (`:154`, `:181`, `:196`) and is the runtime's, not this module's.
//!
//! ## create (`:155-179`)
//!
//! The view never builds an `EstimatePointSerializer` for input: it reads two
//! raw body values (`if not request.data.get("key") or not
//! request.data.get("value")`), looks the estimate up and then calls
//! `EstimatePoint.objects.create` with the **raw** values (`:173-177`). So a
//! falsy `key` (`0`, `false`, `""`, `null`) or `value` is the hand-built
//! `{"error": "Key and value are required"}` 400, `key: 1.5` becomes `1`
//! (Python `int()`), `key: "3.0"` is a 500, `value` is never length-checked
//! (21 characters passes; 256 is a `DataError` 500) and `description` is the
//! model default.
//!
//! ## partial_update (`:182-194`)
//!
//! `EstimatePoint.objects.get(pk=…)` runs **before** the body is validated, so
//! a bad body on a missing point is 404. The point id is a plain `<str:…>`
//! kwarg, not `<uuid:…>`, so an unparsable id is Django's `UUIDField`
//! `ValidationError` 400 and not a router 404. The serializer is
//! `EstimatePointSerializer(instance, data, partial=True)`, whose `validate()`
//! rejects an **empty** `validated_data` and a `value` longer than 20
//! characters (`plane/app/serializers/estimate.py:21-27`) — the same check the
//! create route skips. `save()` is a full `Model.save()`: `updated_by`, a fresh
//! `updated_at` and the re-derived `workspace`.
//!
//! ## destroy (`:197-268`)
//!
//! The fan-out runs **before** the point is looked up (`:203-239` vs
//! `:242-252`): a DELETE of a point that no longer exists still publishes one
//! `issue_activity` per issue that references it and then answers 404. With a
//! truthy `new_estimate_id` each iteration also runs
//! `issues.update(estimate_point_id=new_estimate_id)` — a raw body value
//! coerced by `UUIDField`, so a non-UUID is a 400 *after* the first message and
//! a point of another project is accepted (the FK is all that is checked; the
//! worker is what skips its activity row). The key shift and
//! `old_estimate_point.delete()` only run once the point was found.

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::estimate::EstimatePoint;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{IssueActivityBase, IssueActivityCall};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use uuid::Uuid;

use crate::common::{client, lookup, render};
use crate::estimates::{write_estimate, write_point_queries, write_queries};
use crate::issues::write_issue::publish_issue_activity;
use crate::views::write_support::{self, Body, body_get};
use crate::write::fields::{self, Raw};
use crate::write::generated::estimate_point_serializer;
use crate::write::{self, Serializer, Value};

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

/// `EstimatePointEndpoint.create` (`plane/app/views/estimate/base.py:155-179`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
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

        // `if not request.data.get("key") or not request.data.get("value")`
        // (`:157-161`). Both reads are plain mapping gets, so a body that is
        // not a mapping is the `AttributeError` 500 from `body_get` and a
        // missing key is Python `None`.
        let data = ctx.data()?;
        let key_body = body_get(data, "key")?;
        let value_body = body_get(data, "value")?;
        if !body_truthy(&key_body) || !body_truthy(&value_body) {
            return Ok(write::error_body(400, "Key and value are required"));
        }

        // `Estimate.objects.filter(pk=…, workspace__slug=…, project_id=…)
        // .first()` (`:163-172`): the default manager, so a soft-deleted
        // estimate is "Estimate not found" too.
        let found = client
            .query_opt(
                crate::estimates::queries::ESTIMATE_BY_ID.sql,
                &[&estimate_id, &slug, &project_id],
            )
            .await
            .map_err(ViewError::from)?;
        if found.is_none() {
            return Ok(write::error_body(404, "Estimate not found"));
        }

        // `key = request.data.get("key", 0)` / `value = …` (`:173-174`) through
        // Django's `IntegerField`/`CharField` preparation. The defaults are
        // unreachable (the guard above refused both).
        let key = prep_int(&key_body)?
            .ok_or_else(|| ViewError::Unknown("integer field got NULL".to_owned()))?;
        let value = prep_char(&value_body).ok_or(ViewError::Integrity)?;

        // `EstimatePoint.objects.create(...)` → `ProjectBaseModel.save` first
        // reads `self.project.workspace` (`plane/db/models/project.py:237-239`);
        // a forward FK uses `_base_manager`, so a soft-deleted project still
        // resolves and a missing one is `DoesNotExist` → 404.
        let Some(row) = client
            .query_opt(write_queries::ESTIMATE_WRITE_PROJECT.sql, &[&project_id])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = row.get(0);

        // `auto_now_add`/`auto_now` are two independent `timezone.now()`
        // calls, and H01 makes the caller both audit columns on create.
        let created_at = write::now();
        let updated_at = write::now();
        let point = EstimatePoint {
            created_at,
            updated_at,
            created_by_id: Some(user_id),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            project_id,
            workspace_id,
            estimate_id,
            key,
            description: String::new(),
            value,
        };
        insert_point(client, &point).await?;

        // `EstimatePointSerializer(estimate_point).data` (`:178-179`).
        let body = render::render(ctx, &crate::estimates::ser::point_serializer(&point))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `EstimatePoint.objects.create(...)` (`base.py:175-177`) → one-row INSERT.
async fn insert_point(
    client: &deadpool_postgres::Client,
    point: &EstimatePoint,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(write_point_queries::ESTIMATE_WRITE_POINT_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &point.created_at,
                &point.updated_at,
                &point.created_by_id,
                &point.updated_by_id,
                &point.id,
                &point.project_id,
                &point.workspace_id,
                &point.estimate_id,
                &point.key,
                &point.description,
                &point.value,
            ],
        )
        .await
        .map(|_| ())
        .map_err(write_support::integrity)
}

// ---------------------------------------------------------------------------
// partial_update
// ---------------------------------------------------------------------------

/// `EstimatePointEndpoint.partial_update`
/// (`plane/app/views/estimate/base.py:182-194`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let estimate_id = lookup::uuid_kwarg(
            "estimate_id",
            ctx.kwarg("estimate_id").ok_or(ViewError::Key)?,
        )?;
        // `<estimate_point_id>` has no `uuid:` converter, so the raw string goes
        // through Django's `UUIDField.to_python` inside the lookup: an invalid
        // value is the family `ValidationError` 400, not a router 404.
        let point_id = lookup::uuid_kwarg(
            "estimate_point_id",
            ctx.kwarg("estimate_point_id").ok_or(ViewError::Key)?,
        )?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `EstimatePoint.objects.get(pk=…, estimate_id=…, project_id=…,
        // workspace__slug=…)` (`:184-189`) — before `request.data` is touched.
        let Some(row) = client
            .query_opt(
                write_queries::ESTIMATE_WRITE_POINT.sql,
                &[&point_id, &estimate_id, &project_id, &slug],
            )
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let mut point = EstimatePoint::from_row(&row, 0).map_err(|error| {
            ViewError::Unknown(format!("decoding the estimate point row: {error}"))
        })?;

        // `EstimatePointSerializer(estimate_point, data=request.data,
        // partial=True)` (`:190-192`).
        let mut serializer = Serializer::new(ctx, true)?;
        estimate_point_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };
        // `EstimatePointSerializer.validate` (`plane/app/serializers/
        // estimate.py:21-27`) runs only when every field passed.
        let touched = ["deleted_at", "key", "description", "value"]
            .iter()
            .any(|name| values.contains(name));
        if !touched {
            return Ok(write::field_string_error(
                "non_field_errors",
                "Estimate points are required",
            ));
        }
        if let Some(Value::Text(value)) = values.get("value")
            && value.chars().count() > 20
        {
            return Ok(write::field_string_error(
                "non_field_errors",
                "Value can't be more than 20 characters",
            ));
        }

        // `ModelSerializer.update`: assign every validated field, then
        // `instance.save()` — a full-row UPDATE with `auto_now`'s
        // `updated_at`, H01's `updated_by` and `ProjectBaseModel.save`'s
        // re-derived `workspace`.
        if values.contains("deleted_at") {
            point.deleted_at = values.datetime("deleted_at");
        }
        if let Some(key) = values.integer("key") {
            point.key = i32::try_from(key)
                .map_err(|_| ViewError::Unknown("integer out of range".to_owned()))?;
        }
        if let Some(description) = values.text("description") {
            point.description = description.to_owned();
        }
        if let Some(value) = values.text("value") {
            point.value = value.to_owned();
        }

        let Some(row) = client
            .query_opt(write_queries::ESTIMATE_WRITE_PROJECT.sql, &[&project_id])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        point.workspace_id = row.get(0);
        point.updated_at = write::now();
        point.updated_by_id = Some(user_id);
        update_point(client, &point).await?;

        // `serializer.data` (`:194`) from the in-memory instance.
        let body = render::render(ctx, &crate::estimates::ser::point_serializer(&point))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `instance.save()` for the PATCH: every non-pk column in one statement.
async fn update_point(
    client: &deadpool_postgres::Client,
    point: &EstimatePoint,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(write_point_queries::ESTIMATE_WRITE_POINT_UPDATE.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &point.created_at,
                &point.updated_at,
                &point.created_by_id,
                &point.updated_by_id,
                &point.deleted_at,
                &point.project_id,
                &point.workspace_id,
                &point.estimate_id,
                &point.key,
                &point.description,
                &point.value,
                &point.id,
            ],
        )
        .await
        .map(|_| ())
        .map_err(write_support::integrity)
}

// ---------------------------------------------------------------------------
// destroy
// ---------------------------------------------------------------------------

/// `EstimatePointEndpoint.destroy`
/// (`plane/app/views/estimate/base.py:197-268`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let estimate_id = lookup::uuid_kwarg(
            "estimate_id",
            ctx.kwarg("estimate_id").ok_or(ViewError::Key)?,
        )?;
        let raw_point_id = ctx.kwarg("estimate_point_id").ok_or(ViewError::Key)?;
        let user_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `new_estimate_id = request.data.get("new_estimate_id", None)` (`:198`).
        let data = ctx.data()?;
        let new_estimate_body = body_get(data, "new_estimate_id")?;
        let replace = body_truthy(&new_estimate_body);

        // `Issue.objects.filter(..., estimate_point_id=estimate_point_id)`
        // (`:204-208`, `:223-227`) coerces the raw `<str:…>` kwarg through
        // `UUIDField.get_prep_value` when the queryset is **built**, so an
        // unparsable id is the family 400 before any SQL; `.filter()` on
        // `Issue.objects` keeps `deleted_at IS NULL` only.
        let point_id = lookup::uuid_kwarg("estimate_point_id", raw_point_id)?;
        let issues = client
            .query(
                write_queries::ESTIMATE_WRITE_POINT_ISSUES.sql,
                &[&point_id, &project_id, &slug],
            )
            .await
            .map_err(ViewError::from)?;

        // `json.dumps({"estimate_point": str(new_estimate_id) if
        // new_estimate_id else None})` (`:212`, `:231`): the raw body value,
        // stringified by Python `str`.
        let requested = if replace {
            Some(body_str(&new_estimate_body))
        } else {
            None
        };
        for row in &issues {
            let issue_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
            // `current_instance` is `str(issue.estimate_point_id)` **as loaded
            // before the UPDATE** (`:216-218`).
            let current: Option<Uuid> = row.try_get(1).map_err(ViewError::from)?;
            publish_point_activity(
                ctx,
                requested.as_deref(),
                issue_id,
                project_id,
                current,
                user_id,
            )
            .await?;
            if replace {
                // `issues.update(estimate_point_id=new_estimate_id)` (`:221`)
                // re-runs on every iteration; the raw value is coerced here,
                // after the message, exactly where Django raises.
                let new_point = body_uuid(&new_estimate_body)?;
                let statement = client
                    .prepare_cached(write_point_queries::ESTIMATE_WRITE_ISSUE_POINT_UPDATE.sql)
                    .await
                    .map_err(ViewError::from)?;
                client
                    .execute(&statement, &[&new_point, &point_id, &project_id, &slug])
                    .await
                    .map_err(write_support::integrity)?;
            }
        }

        // `EstimatePoint.objects.filter(pk=…, …).first()` (`:242-252`): after
        // the fan-out, so a missing point is a 404 that already wrote.
        let Some(row) = client
            .query_opt(
                write_queries::ESTIMATE_WRITE_POINT.sql,
                &[&point_id, &estimate_id, &project_id, &slug],
            )
            .await
            .map_err(ViewError::from)?
        else {
            return Ok(write::error_body(404, "Estimate point not found"));
        };
        let old_point = EstimatePoint::from_row(&row, 0).map_err(|error| {
            ViewError::Unknown(format!("decoding the estimate point row: {error}"))
        })?;

        // `for estimate_point in estimate_points` (`:256-259`): the queryset
        // of `:199-201` is evaluated here, in `Meta.ordering = ("value",)`.
        let rows = client
            .query(
                crate::estimates::queries::ESTIMATE_POINTS_BY_PROJECT.sql,
                &[&estimate_id, &project_id, &slug],
            )
            .await
            .map_err(ViewError::from)?;
        let mut updated: Vec<EstimatePoint> = Vec::new();
        for row in &rows {
            let mut point = EstimatePoint::from_row(row, 0).map_err(|error| {
                ViewError::Unknown(format!("decoding the estimate point row: {error}"))
            })?;
            if point.key > old_point.key {
                // Python's int is unbounded, but the column is `int4`: a
                // decrement past `i32::MIN` is psycopg's `DataError` (500).
                point.key = point.key.checked_sub(1).ok_or_else(|| {
                    ViewError::Unknown("DataError: integer out of range".to_owned())
                })?;
                updated.push(point);
            }
        }

        // `bulk_update(updated_estimate_points, ["key"], batch_size=10)`
        // (`:261`): `key` only, inside Django's `transaction.atomic`.
        shift_keys(client, &updated).await?;

        // `old_estimate_point.delete()` (`:263`) — H03 plus the cascade task
        // (`("db", "estimatepoint", pk)`).
        write::soft_delete(
            client,
            "estimate_points",
            "id",
            point_id,
            Some(user_id),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "estimatepoint", point_id).await?;

        // `EstimatePointSerializer(updated_estimate_points, many=True).data`
        // (`:265-268`) — the in-memory rows with their decremented keys.
        let body = crate::estimates::render::point_list(ctx, &updated)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// One `bulk_update(…, ["key"])` call: every row in one transaction.
async fn shift_keys(
    client: &deadpool_postgres::Client,
    points: &[EstimatePoint],
) -> Result<(), ViewError> {
    if points.is_empty() {
        return Ok(());
    }
    let statement = client
        .prepare_cached(write_point_queries::ESTIMATE_WRITE_POINT_KEY_UPDATE.sql)
        .await
        .map_err(ViewError::from)?;
    // `bulk_update` wraps its batches in `transaction.atomic(savepoint=False)`.
    client
        .batch_execute("BEGIN")
        .await
        .map_err(ViewError::from)?;
    let mut result = Ok(());
    for point in points {
        if let Err(error) = client.execute(&statement, &[&point.key, &point.id]).await {
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
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(write_support::integrity(error))
        }
    }
}

/// One `issue_activity.delay(...)` of 49 DELETE (`base.py:210-220`, `:229-239`):
/// the **Bare** shape, `issue_id` as a `UUID` object (`TASKS.md:1133`) and both
/// payloads as `json.dumps` strings.
async fn publish_point_activity(
    ctx: &ViewCtx<'_>,
    requested_point: Option<&str>,
    issue_id: Uuid,
    project_id: Uuid,
    current_point: Option<Uuid>,
    user_id: Uuid,
) -> Result<(), ViewError> {
    let requested = activity_payload(requested_point)?;
    let current = activity_payload(current_point.map(|id| id.to_string()).as_deref())?;
    let base = IssueActivityBase::new(
        "issue.activity.updated",
        requested,
        user_id.to_string(),
        // `issue_id=issue.id` — a `uuid.UUID`, so kombu's `uuid` envelope.
        TaskArg::uuid(issue_id),
        TaskArg::str(project_id.to_string()),
        current,
        // `epoch=int(timezone.now().timestamp())`, read per iteration.
        chrono::Utc::now().timestamp(),
    );
    publish_issue_activity(ctx, IssueActivityCall::bare(base)).await
}

/// `json.dumps({"estimate_point": value})` for one activity payload.
fn activity_payload(value: Option<&str>) -> Result<TaskArg, ViewError> {
    let mut map = PyDict::new();
    map.insert(
        PyStr::new("estimate_point"),
        value.map_or(Json::Null, Json::from),
    );
    TaskArg::json_text_from(&Json::Dict(map)).map_err(|error| ViewError::Unknown(error.to_string()))
}

// ---------------------------------------------------------------------------
// Body preparation
// ---------------------------------------------------------------------------

/// Python truthiness of `request.data.get(key)`.
fn body_truthy(body: &Body<'_>) -> bool {
    match body {
        Body::Missing => false,
        Body::Form(text) => !text.is_empty(),
        Body::Json(value) => write_estimate::truthy(value),
    }
}

/// Python `str(request.data.get(key))`, for the activity payload.
fn body_str(body: &Body<'_>) -> String {
    match body {
        Body::Missing => "None".to_owned(),
        Body::Form(text) => (*text).to_owned(),
        Body::Json(value) => fields::python_str(Raw::Json(value)),
    }
}

/// Django's `CharField.get_prep_value`: a form value is already a string, a
/// JSON value goes through Python `str()`.
fn prep_char(body: &Body<'_>) -> Option<String> {
    match body {
        Body::Missing => None,
        Body::Form(text) => Some((*text).to_owned()),
        Body::Json(value) => write_estimate::char_prep(value),
    }
}

/// Django's `IntegerField.get_prep_value` (Python `int()`), then the column's
/// `int4` range. A value outside the range is the family 500, as psycopg's
/// `DataError` is.
fn prep_int(body: &Body<'_>) -> Result<Option<i32>, ViewError> {
    match body {
        Body::Missing => Ok(None),
        Body::Form(text) => write_estimate::python_int(text)
            .and_then(|value| i32::try_from(value).map_err(|_| write_estimate::int_out_of_range()))
            .map(Some),
        Body::Json(value) => write_estimate::int_prep(value),
    }
}

/// Django's `UUIDField.get_prep_value` for 49 DELETE's raw `new_estimate_id`:
/// a string is `uuid.UUID(hex=…)`, an int (a bool included, as `bool`
/// subclasses `int`) `uuid.UUID(int=…)`, anything else the family
/// `ValidationError` 400.
fn body_uuid(body: &Body<'_>) -> Result<Uuid, ViewError> {
    match body {
        Body::Missing => Err(ViewError::Validation),
        Body::Form(text) => Uuid::parse_str(text).map_err(|_| ViewError::Validation),
        Body::Json(Json::Str(text)) => {
            Uuid::parse_str(text.as_str().unwrap_or_default()).map_err(|_| ViewError::Validation)
        }
        Body::Json(Json::Int(number)) => number
            .to_decimal_string()
            .parse::<u128>()
            .map(Uuid::from_u128)
            .map_err(|_| ViewError::Validation),
        Body::Json(Json::Bool(flag)) => Ok(Uuid::from_u128(u128::from(*flag))),
        Body::Json(_) => Err(ViewError::Validation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_body(text: &str) -> plane_drf::parsers::Data {
        plane_drf::parsers::Data::Json(
            plane_serde::pyjson::loads(text.as_bytes(), "utf-8").expect("json"),
        )
    }

    #[test]
    fn the_create_guard_uses_python_truthiness() {
        let data = json_body(r#"{"key": 0, "value": "x"}"#);
        assert!(!body_truthy(&body_get(&data, "key").expect("key")));
        let data = json_body(r#"{"key": false, "value": "x"}"#);
        assert!(!body_truthy(&body_get(&data, "key").expect("key")));
        let data = json_body(r#"{"key": 1, "value": ""}"#);
        assert!(!body_truthy(&body_get(&data, "value").expect("value")));
        let data = json_body(r#"{"key": 1, "value": {"a": 1}}"#);
        assert!(body_truthy(&body_get(&data, "value").expect("value")));
        let data = json_body(r#"{"key": 1}"#);
        assert!(!body_truthy(&body_get(&data, "value").expect("value")));
    }

    #[test]
    fn the_key_prep_is_python_int() {
        let data = json_body(r#"{"key": 1.5}"#);
        assert_eq!(
            prep_int(&body_get(&data, "key").expect("key")).expect("int"),
            Some(1)
        );
        let data = json_body(r#"{"key": true}"#);
        assert_eq!(
            prep_int(&body_get(&data, "key").expect("key")).expect("bool"),
            Some(1)
        );
        // `int("3.0")` is the `ValueError` the family 500 renders.
        let form = plane_drf::parsers::Data::Form(
            plane_drf::request::QueryDict::parse("key=3.0", None).expect("form"),
        );
        assert!(prep_int(&body_get(&form, "key").expect("key")).is_err());
    }

    #[test]
    fn the_value_prep_is_python_str() {
        let data = json_body(r#"{"value": 7}"#);
        assert_eq!(
            prep_char(&body_get(&data, "value").expect("value")).as_deref(),
            Some("7")
        );
        let data = json_body(r#"{"value": true}"#);
        assert_eq!(
            prep_char(&body_get(&data, "value").expect("value")).as_deref(),
            Some("True")
        );
        let data = json_body(r#"{"value": {"a": 1}}"#);
        assert_eq!(
            prep_char(&body_get(&data, "value").expect("value")).as_deref(),
            Some("{'a': 1}")
        );
    }

    #[test]
    fn new_estimate_id_follows_djangos_uuid_prep() {
        let data = json_body(r#"{"new_estimate_id": "ddea004d-0068-44bb-90c6-7197c8b915e6"}"#);
        assert_eq!(
            body_uuid(&body_get(&data, "new_estimate_id").expect("id")).expect("uuid"),
            Uuid::parse_str("ddea004d-0068-44bb-90c6-7197c8b915e6").expect("uuid")
        );
        let data = json_body(r#"{"new_estimate_id": 1}"#);
        assert_eq!(
            body_uuid(&body_get(&data, "new_estimate_id").expect("id")).expect("int"),
            Uuid::from_u128(1)
        );
        let data = json_body(r#"{"new_estimate_id": "nope"}"#);
        assert_eq!(
            body_uuid(&body_get(&data, "new_estimate_id").expect("id")),
            Err(ViewError::Validation)
        );
        let data = json_body(r#"{"new_estimate_id": 1.5}"#);
        assert_eq!(
            body_uuid(&body_get(&data, "new_estimate_id").expect("id")),
            Err(ViewError::Validation)
        );
    }
}
