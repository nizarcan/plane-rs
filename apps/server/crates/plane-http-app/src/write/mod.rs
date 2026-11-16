// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The write pipeline every `/api/` domain uses (Phase 5).
//!
//! A read handler answers from one statement and is done. A write handler is a
//! small pipeline, and each stage has a Python original that decides observable
//! behaviour:
//!
//! | Stage | Python | Here |
//! | --- | --- | --- |
//! | request body | DRF `Request._parse` (`rest_framework/request.py:330-384`) | already parsed by `plane-drf`; [`Serializer`] turns it into validated field values |
//! | validation | DRF `Serializer.to_internal_value` + field classes | [`fields`] (messages, codes and formats) and [`serializer`] (order, `partial=`, `non_field_errors`) |
//! | origin check | `BaseSessionAuthentication.enforce_csrf` (`plane/authentication/session.py:56-63`) | `plane-drf`'s dispatcher, before the handler |
//! | audit | `BaseModel.save` (`plane/db/models/base.py:23-45`) + `crum` | [`create_audit`]/[`update_audit`] |
//! | timestamps | `auto_now_add`/`auto_now` (`django/db/models/fields/__init__.py:864-877`) | [`now`] (microsecond precision, in Python) |
//! | hooks | `Model.save()` overrides (`MODEL_HOOKS.md`) | [`crate::model_hooks`] — every write path calls them explicitly |
//! | delete | `SoftDeleteModel.delete` (`plane/db/mixins.py:65-77`) | [`soft_delete`] |
//! | connection | Django's router: non-GET always primary (`plane/middleware/db_routing.py:38-45`) | `common::client(ctx)` — the request's own connection |
//!
//! ## Transactions
//!
//! Django runs with `ATOMIC_REQUESTS` unset (`plane/settings/common.py:250-268`
//! configures only `CONN_MAX_AGE` and `OPTIONS`), so a view is a sequence of
//! autocommitting statements unless it opens `transaction.atomic()` itself. A
//! write handler must therefore reproduce the Python view's own boundaries: no
//! transaction where the view has none, and an explicit `client.transaction()`
//! where it does (the `IssueView` family wraps its `save()`s; `Sticky` does
//! not).
//!
//! ## One connection, the request's
//!
//! A write handler uses `common::client(ctx)` — the connection the runtime
//! checked out for the request — and **never** checks out a second one. Django
//! opens one connection per request; the route's `use_read_replica` rule
//! already resolves to the primary for every non-`GET`/`HEAD`/`OPTIONS` method
//! (`plane/middleware/db_routing.py:38-45`), so `ctx.db` is the write
//! connection.
//!
//! ## Response bodies
//!
//! Django answers a create/update with `serializer.data` from the **in-memory
//! instance**, not a re-read: a `jsonb` column would come back re-ordered. A
//! handler builds the read serializer from the values it wrote
//! (`JsonText::from_json` for the JSON columns) so the bytes match.
//!
//! ## Example
//!
//! ```ignore
//! let mut serializer = Serializer::new(ctx, /* partial = */ false)?;
//! let name = serializer.char_field("name", CharOptions::nullable());
//! let description = serializer.json_field("description", false);
//! let values = serializer.finish()?;
//! // … run the model hook, INSERT, render the read serializer from `values`.
//! ```

pub mod description;
pub mod django_serialize;
pub mod fields;
pub mod generated;
pub mod html;
pub mod serializer;
pub mod url;

pub use fields::{CharOptions, Detail, IntegerOptions};
/// The H01 audit columns (`plane/db/models/base.py:23-45`).
pub use plane_db::hooks::audit::Audit;
pub use serializer::{PrimaryKeyListOptions, Serializer, Validated, ValidationErrors, Value};

use chrono::{DateTime, SubsecRound, Utc};
use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use plane_drf::view::{ViewCtx, ViewResponse};
use uuid::Uuid;

/// `timezone.now()` at the precision Django stores: microseconds.
///
/// `django.utils.timezone.now` is `datetime.now(timezone.utc)` (microsecond
/// clock); Postgres keeps microseconds. Truncating here keeps the response
/// body's `isoformat()` and the row identical to Python's.
#[must_use]
pub fn now() -> DateTime<Utc> {
    Utc::now().trunc_subsecs(6)
}

/// `SoftDeleteModel.delete(soft=True)` for one row — H03.
///
/// The SQL is `plane_db::hooks::soft_delete::delete_instance`; this wraps it
/// as the family's error so a handler does not map it twice.
///
/// # Errors
///
/// A database failure as the family's 500.
pub async fn soft_delete(
    client: &Client,
    table: &str,
    key_column: &str,
    pk: Uuid,
    updated_by: Option<Uuid>,
    deleted_at: DateTime<Utc>,
) -> Result<(), ViewError> {
    plane_db::hooks::soft_delete::delete_instance(
        client, table, key_column, pk, updated_by, deleted_at,
    )
    .await
    .map_err(ViewError::from)
}

/// Publish `soft_delete_related_objects` for a row this request soft-deleted
/// (`plane/db/mixins.py:71-75`; the task is
/// `plane.bgtasks.deletion_task.soft_delete_related_objects`).
///
/// `app_label`/`model_name` are the Django `_meta` pair
/// (`plane/db/models/sticky.py` → `("db", "sticky")`), published as plain
/// strings, and `using` is `None` at every call site.
///
/// # Errors
///
/// A broker failure as the family's 500 (`common::jobs::publish`).
pub async fn publish_soft_delete_cascade(
    ctx: &ViewCtx<'_>,
    app_label: &str,
    model_name: &str,
    pk: Uuid,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::SOFT_DELETE_RELATED_OBJECTS;
    let message = plane_jobs::celery::tasks::soft_delete_related_objects(
        app_label.to_owned(),
        model_name.to_owned(),
        pk,
        None,
    );
    // Phase 4's routing API: this task is Celery-owned during coexistence, so
    // this publishes the same byte-identical message `.delay()` does.
    crate::common::jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task))
        .await?;
    Ok(())
}

/// `Response({"error": …}, status)` — the body Plane's views build by hand
/// (`plane/app/views/project/base.py:334-337`, `:422-425`), in DRF's dialect.
#[must_use]
pub fn error_body(status: u16, message: &str) -> ViewResponse {
    ViewResponse::raw(
        status,
        plane_core::error::APPLICATION_JSON,
        plane_query::json::Json::object(vec![("error", plane_query::json::Json::string(message))])
            .to_drf(),
    )
}

/// One field's serializer error, as `validate_<field>` and `validate()` produce
/// it: `Response(serializer.errors)` renders `{"field": ["message"]}`.
///
/// `ValidationError(detail="MSG")` from a field hook is coerced to a
/// one-element list (`rest_framework/exceptions.py:148-161`), and a
/// `validate()` dict goes through `as_serializer_error`
/// (`rest_framework/serializers.py:314-345`), which wraps every dict value in a
/// list. Both land here.
#[must_use]
pub fn field_string_error(field: &str, message: &str) -> ViewResponse {
    ViewResponse::raw(
        400,
        plane_core::error::APPLICATION_JSON,
        plane_query::json::Json::object(vec![(
            field,
            plane_query::json::Json::Array(vec![plane_query::json::Json::string(message)]),
        )])
        .to_drf(),
    )
}

/// `requested_data=request.data` for the activity/webhook tasks
/// (`plane_jobs::celery::args::TaskArg::Json`).
///
/// For a JSON body this is the parsed tree. A form body is a Django
/// `QueryDict`, which `json.dumps` refuses, so Django's `.delay()` would raise
/// `TypeError` and the family handler would answer 500; that is reproduced as
/// an error rather than a silently different payload.
///
/// # Errors
///
/// `ViewError::Unknown` for a form body.
pub fn request_data_arg(ctx: &ViewCtx<'_>) -> Result<plane_jobs::celery::args::TaskArg, ViewError> {
    match ctx.data()? {
        plane_drf::parsers::Data::Json(value) => {
            Ok(plane_jobs::celery::args::TaskArg::Json(value.clone()))
        }
        plane_drf::parsers::Data::Form(_) => Err(ViewError::Unknown(
            "TypeError: Object of type QueryDict is not JSON serializable".to_owned(),
        )),
    }
}

/// `request.data.get(key, current)` for a JSON column.
///
/// # Errors
///
/// A JSON value Django's encoder refuses.
pub fn json_or(
    data: &plane_drf::parsers::Data,
    key: &str,
    current: &plane_db::JsonText,
) -> Result<plane_db::JsonText, ViewError> {
    match data.json().and_then(|value| value.get(key)) {
        Some(value) if !value.is_null() => {
            plane_db::JsonText::from_json(value).map_err(ViewError::from)
        }
        _ => Ok(current.clone()),
    }
}

/// `request.data.get(key, current)` for a float column; Python `float()`
/// failures are the 500 the ORM would raise.
///
/// # Errors
///
/// `ViewError::Unknown` for a non-numeric value.
pub fn float_or(
    data: &plane_drf::parsers::Data,
    key: &str,
    current: f64,
) -> Result<f64, ViewError> {
    let Some(value) = data.get(key) else {
        return Ok(current);
    };
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| ViewError::Unknown(format!("ValueError: could not convert {key} to float")))
}

/// `request.data.get(key)` as a string (`None` when absent).
#[must_use]
pub fn string_field(data: &plane_drf::parsers::Data, key: &str) -> Option<String> {
    data.get(key).map(str::to_owned)
}

/// The whole body as a JSON object, for a view that merges `request.data` into
/// a stored dict.
///
/// # Errors
///
/// `ViewError::Unknown` when the body is not an object (the `TypeError` a
/// `.update()` on a non-mapping would raise).
pub fn json_object(
    data: &plane_drf::parsers::Data,
) -> Result<plane_serde::pyjson::Value, ViewError> {
    match data.json() {
        Some(value) if matches!(value, plane_serde::pyjson::Value::Dict(_)) => Ok(value.clone()),
        _ => Err(ViewError::Unknown(
            "TypeError: 'NoneType' object is not a mapping".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn now_has_microsecond_precision() {
        assert_eq!(now().nanosecond() % 1_000, 0);
    }
}
