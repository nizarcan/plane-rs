// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `instances` (`deploy/route-groups.yaml`): the instance-admin
//! API under `/api/instances/`.
//!
//! Python: `apps/api/plane/license/` — 14 routes mounted by
//! `plane/license/urls.py`. Every one of them reads the **admin** session
//! cookie (`admin-session-id`), because the session middleware tests
//! `"instances" in request.path` (`plane/authentication/middleware/session.py:22-27`);
//! a plain `session-id` cookie is anonymous here.
//!
//! This module holds the pieces the group's handlers share; the handlers land
//! with set C of `PUBLIC_INSTANCES.md`:
//!
//! * [`serializer`] — the manual `InstanceConfigurationSerializer`
//!   (`PUBLIC_INSTANCES.md` §2e) and `PATCH`'s value normalisation (§2d);
//! * [`queries`] — the configuration statements;
//! * [`mail`] — the SMTP transport §2c routes for
//!   `email-credentials-check/` (Django's `EmailMultiAlternatives` bytes,
//!   envelope and connection behaviour; set C calls it).
//!
//! The handlers are split by the Python module they port:
//!
//! | Rust module | Python | Routes |
//! | --- | --- | --- |
//! | [`instance`] | `plane/license/api/views/instance.py` | 259, 269 |
//! | [`admin`] | `plane/license/api/views/admin.py` (list/detail/me/session) | 260-262, 264 |
//! | [`configuration`] | `plane/license/api/views/configuration.py` | 265, 266 |
//! | [`auth`] | `plane/license/api/views/admin.py` (sign-in/up/out) | 263, 267, 268 |
//! | [`email`] | `plane/license/api/views/configuration.py` (the check) | 270 |
//! | [`workspace`] | `plane/license/api/views/workspace.py` | 271, 272 |
//!
//! The admin cookie trap is the middleware's: `"instances" in request.path`
//! selects `admin-session-id` (`plane/authentication/middleware/session.py:22-27`).

use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

pub mod admin;
pub mod auth;
pub mod configuration;
pub mod email;
pub mod instance;
pub mod mail;
pub mod queries;
pub mod serializer;
pub mod workspace;

/// Register every `/api/instances/` handler, in `routes.json` order.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    instance::register(registry)?;
    admin::register(registry)?;
    configuration::register(registry)?;
    auth::register(registry)?;
    email::register(registry)?;
    workspace::register(registry)?;
    Ok(())
}

/// `request.data.get(key)` as a Python value's `str()`, for the plain views
/// and APIViews that pass a body value straight to an ORM call.
///
/// `None` for a missing key and a JSON `null` (the call sites test
/// truthiness and pass their own `False`/`None` default).
///
/// # Errors
///
/// [`ViewError::Unknown`] when the body is a JSON list or scalar: Python's
/// `request.data.get` raises `AttributeError` there.
pub(crate) fn body_value(
    ctx: &plane_drf::view::ViewCtx<'_>,
    key: &str,
) -> Result<Option<Json>, ViewError> {
    match ctx.data()? {
        plane_drf::parsers::Data::Form(query) => Ok(query
            .get(key)
            .map(|value| Json::Str(plane_serde::pyjson::PyStr::from(value)))),
        plane_drf::parsers::Data::Json(Json::Dict(dict)) => Ok(dict.get(key).cloned()),
        plane_drf::parsers::Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: 'str' object has no attribute 'get'".to_string(),
        )),
    }
}

/// `body_value(...).map(str)` for a string field.
pub(crate) fn body_string(
    ctx: &plane_drf::view::ViewCtx<'_>,
    key: &str,
) -> Result<Option<String>, ViewError> {
    Ok(body_value(ctx, key)?.and_then(|value| match &value {
        Json::Null => None,
        Json::Str(text) => Some(text.to_string_lossy().into_owned()),
        other => Some(crate::write::fields::python_str(
            crate::write::fields::Raw::Json(other),
        )),
    }))
}

/// `Some(int(value))` with CPython's `int()` for the values a body can hold.
///
/// # Errors
///
/// [`ViewError::Unknown`] for a value `int()` refuses (a `ValueError`/
/// `TypeError` in the view, i.e. a 500).
pub(crate) fn body_int(
    ctx: &plane_drf::view::ViewCtx<'_>,
    key: &str,
) -> Result<Option<i64>, ViewError> {
    let Some(value) = body_value(ctx, key)? else {
        return Ok(None);
    };
    match &value {
        Json::Null => Ok(None),
        Json::Bool(value) => Ok(Some(i64::from(*value))),
        Json::Int(value) => value.as_i64().map(Some).ok_or_else(|| {
            ViewError::Unknown("OverflowError: int too large to convert to i64".to_string())
        }),
        Json::Float(value) => Ok(Some(*value as i64)),
        Json::Str(text) => {
            let text = text.to_string_lossy();
            let trimmed = text.trim();
            trimmed.parse::<i64>().map(Some).map_err(|_| {
                ViewError::Unknown(format!(
                    "ValueError: invalid literal for int() with base 10: {trimmed:?}"
                ))
            })
        }
        other => Err(ViewError::Unknown(format!(
            "TypeError: int() argument must be a string, a bytes-like object or a real number, not '{}'",
            json_type_name(other)
        ))),
    }
}

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

/// `InstanceConfiguration.objects.bulk_update(values, ["value"], batch_size=100)`
/// (`plane/license/api/views/configuration.py:56`) — `PUBLIC_INSTANCES.md` §2d.
///
/// Only `value` is written: `bulk_update` bypasses `save()`, so there is no
/// `updated_at`, no `updated_by_id` and no model hook, and a pair whose id does
/// not exist updates nothing instead of inserting. Django batches the list at
/// 100 rows per statement; one statement for the whole set is observably the
/// same and fewer round trips.
///
/// `updates` is `(id, value)`; an empty slice performs no statement, exactly
/// like `bulk_update([])`, which returns 0 before touching the database.
///
/// The caller renders the response from the rows it already holds, with the new
/// `value` assigned, exactly as the view does (`configuration.py:51-58`: the
/// serializer at `:58` reads the queryset's populated result cache, it does not
/// re-query), so this returns only the affected-row count.
///
/// # Errors
///
/// A database failure as the family's 500.
pub async fn bulk_update_values(
    client: &Client,
    updates: &[(Uuid, String)],
) -> Result<u64, ViewError> {
    if updates.is_empty() {
        return Ok(0);
    }
    let ids: Vec<Uuid> = updates.iter().map(|(id, _)| *id).collect();
    let values: Vec<&str> = updates.iter().map(|(_, value)| value.as_str()).collect();
    let statement = queries::UPDATE_CONFIGURATION_VALUES.prepare(client).await?;
    let updated = client.execute(&statement, &[&ids, &values]).await?;
    Ok(updated)
}
