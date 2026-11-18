// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/api/views/configuration.py`: routes 265 and 266.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/instances/configurations/` | `InstanceConfigurationEndpoint.get` | `configuration.py:26-34` |
//! | `PATCH /api/instances/configurations/` | `InstanceConfigurationEndpoint.patch` | `configuration.py:36-59` |
//! | `DELETE /api/instances/configurations/disable-email-feature/` | `DisableEmailFeatureEndpoint.delete` | `configuration.py:62-79` |
//!
//! The `PATCH` write is `bulk_update(values, ["value"], batch_size=100)`:
//! only `value` moves — no `updated_at`, no audit column, no hook and no
//! insert — and the response is rendered from the **same in-memory rows**
//! (`configuration.py:51-58`, no re-query), so an encrypted value round-trips
//! through `encrypt_data`/`decrypt_data` to the plaintext the client sent while
//! the stored ciphertext differs on every call (`PUBLIC_INSTANCES.md` §2d/§2f).
//!
//! `disable-email-feature` writes five rows to `""` (including the encrypted
//! `EMAIL_HOST_PASSWORD`, whose ciphertext becomes the empty string) and
//! `ENABLE_SMTP` to `"0"`; the ciphertext change is deterministic.

use plane_cache::{PyInterner, PyValue};
use plane_core::error::APPLICATION_JSON;
use plane_db::models::license::instance::InstanceConfiguration;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use super::queries;
use super::serializer::{InstanceConfigurationSerializer, body_value, column_value, stored_value};
use crate::common::cache;
use crate::common::client;

/// Register the routes this module owns.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.license.api.views.configuration.InstanceConfigurationEndpoint")?
        .method(http::Method::GET, get)?
        .method(http::Method::PATCH, patch)?;
    registry
        .api_view("plane.license.api.views.configuration.DisableEmailFeatureEndpoint")?
        .method(http::Method::DELETE, disable_email_feature)?;
    Ok(())
}

/// `GET /api/instances/configurations/` (`configuration.py:28-34`), with
/// `@cache_response(60 * 60 * 2, user=False)`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        cache::cache_response(ctx, 60 * 60 * 2, None, false, || async move {
            let client = client(ctx)?;
            let rows = load_all(client).await?;
            let secret = ctx.settings.secret_key.value();
            let mut strings = PyInterner::new();
            let entries = rows
                .iter()
                .map(|row| configuration_value(row, secret, &mut strings))
                .collect();
            Ok((200, PyValue::return_list(entries)))
        })
        .await
    })
}

async fn load_all(
    client: &deadpool_postgres::Client,
) -> Result<Vec<InstanceConfiguration>, ViewError> {
    let statement = client
        .prepare_cached(queries::ALL_CONFIGURATIONS.sql)
        .await?;
    let rows = client.query(&statement, &[]).await?;
    Ok(rows
        .iter()
        .map(|row| InstanceConfiguration::from_row(row, 0))
        .collect::<Result<Vec<_>, _>>()?)
}

/// One `InstanceConfigurationSerializer(row).data` as a pickle tree.
///
/// `to_representation` decrypts the `value` column when the row is encrypted
/// and non-`NULL` (`license/api/serializers/configuration.py:15-19`); a
/// decryption failure is `""`.
fn configuration_value(
    row: &InstanceConfiguration,
    secret_key: &str,
    strings: &mut PyInterner,
) -> PyValue {
    let serializer = InstanceConfigurationSerializer { row, secret_key };
    // `many=True`'s `to_representation` returns a **plain** dict per row (only
    // the outer `ReturnList` is reduced), but every row's keys are the same
    // `field_name` objects, so the pickler memoises them once
    // (`rest_framework/utils/serializer_helpers.py:6-28,71-88`). The interner
    // reproduces that identity; without it the cached frame is larger than
    // Django's and the harness — which compares a frame it cannot decode by
    // length — reports a diff.
    let created_at = super::admin::drf_string(row.created_at);
    let updated_at = super::admin::drf_string(row.updated_at);
    let deleted_at = row.deleted_at.map(super::admin::drf_string);
    let value = serializer.value();
    PyValue::mapping(vec![
        (strings.key("id"), PyValue::uuid(row.id)),
        (strings.key("created_at"), strings.value(&created_at)),
        (strings.key("updated_at"), strings.value(&updated_at)),
        (
            strings.key("deleted_at"),
            deleted_at
                .as_deref()
                .map_or(PyValue::None, |text| strings.value(text)),
        ),
        (strings.key("key"), strings.value(&row.key)),
        (
            strings.key("value"),
            value
                .as_deref()
                .map_or(PyValue::None, |text| strings.value(text)),
        ),
        (strings.key("category"), strings.value(&row.category)),
        (strings.key("is_encrypted"), PyValue::bool(row.is_encrypted)),
        (
            strings.key("created_by"),
            row.created_by_id.map_or(PyValue::None, PyValue::uuid),
        ),
        (
            strings.key("updated_by"),
            row.updated_by_id.map_or(PyValue::None, PyValue::uuid),
        ),
    ])
}

/// `PATCH /api/instances/configurations/` (`configuration.py:36-59`).
///
/// Both `@invalidate_cache` decorators run before the body, outermost first:
/// `/api/instances/configurations/` then `/api/instances/` (`:41-42`).
fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        cache::invalidate_cache(
            ctx,
            Some("/api/instances/configurations/"),
            false,
            false,
            false,
        )
        .await?;
        cache::invalidate_cache(ctx, Some("/api/instances/"), false, false, false).await?;
        let client = client(ctx)?;
        let body = ctx.data()?;
        let Some(keys) = body_keys(body) else {
            return Err(ViewError::Unknown(
                "AttributeError: request.data has no attribute 'keys'".to_string(),
            ));
        };
        if keys.is_empty() {
            // `filter(key__in=[])` is `EmptyResultSet`; `bulk_update([])`
            // returns 0 and `serializer.data` is `[]`.
            return Ok(empty_list_response());
        }
        let statement = client
            .prepare_cached(queries::CONFIGURATIONS_BY_KEY.sql)
            .await?;
        let rows = client.query(&statement, &[&keys]).await?;
        let mut configurations = rows
            .iter()
            .map(|row| InstanceConfiguration::from_row(row, 0))
            .collect::<Result<Vec<_>, _>>()?;

        let secret = ctx.settings.secret_key.value();
        let mut updates: Vec<(Uuid, String)> = Vec::with_capacity(configurations.len());
        for configuration in &mut configurations {
            let raw_value = body_value_for(body, &configuration.key)
                .unwrap_or_else(|| stored_value(configuration.value.as_deref()));
            configuration.value =
                Some(column_value(configuration.is_encrypted, &raw_value, secret));
            updates.push((
                configuration.id,
                configuration.value.clone().unwrap_or_default(),
            ));
        }
        crate::instances::bulk_update_values(client, &updates).await?;

        // `serializer.data` reads the same list, whose cache is already
        // populated, so the decrypted values come from memory.
        let mut strings = PyInterner::new();
        let entries = configurations
            .iter()
            .map(|row| configuration_value(row, secret, &mut strings))
            .collect();
        let data = PyValue::return_list(entries);
        let bytes = plane_serde::to_vec(Dialect::Drf, &data)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

fn empty_list_response() -> ViewResponse {
    ViewResponse::raw(200, APPLICATION_JSON, b"[]".to_vec())
}

/// `request.data.keys()` — `None` for a body that is not a mapping, which
/// Python answers with an `AttributeError` 500.
fn body_keys(data: &plane_drf::parsers::Data) -> Option<Vec<String>> {
    match data {
        plane_drf::parsers::Data::Form(query) => Some(query.keys().map(str::to_string).collect()),
        plane_drf::parsers::Data::Json(Json::Dict(dict)) => Some(
            dict.keys()
                .map(|key| key.to_string_lossy().into_owned())
                .collect(),
        ),
        _ => None,
    }
}

/// `request.data.get(key, configuration.value)` for one row.
fn body_value_for(data: &plane_drf::parsers::Data, key: &str) -> Option<String> {
    match data {
        plane_drf::parsers::Data::Form(query) => query
            .get(key)
            .map(|value| body_value(&Json::Str(plane_serde::pyjson::PyStr::from(value)))),
        plane_drf::parsers::Data::Json(Json::Dict(dict)) => dict.get(key).map(body_value),
        _ => None,
    }
}

/// `DELETE /api/instances/configurations/disable-email-feature/`
/// (`configuration.py:62-79`).
///
/// `@invalidate_cache(path="/api/instances/", user=False)` runs before the
/// body. The `try/except Exception` answers a 400 for any failure; only the
/// database error is reachable here and it is reported as the family's 500
/// instead (the exception is swallowed by Python's 400).
fn disable_email_feature<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        cache::invalidate_cache(ctx, Some("/api/instances/"), false, false, false).await?;
        let client = client(ctx)?;
        let keys = [
            "EMAIL_HOST",
            "EMAIL_HOST_USER",
            "EMAIL_HOST_PASSWORD",
            "ENABLE_SMTP",
            "EMAIL_PORT",
            "EMAIL_FROM",
        ];
        let statement = client
            .prepare_cached(queries::DISABLE_EMAIL_CONFIGURATION.sql)
            .await?;
        client
            .execute(&statement, &[&keys.map(str::to_string).to_vec()])
            .await?;
        Ok(ViewResponse::empty(200))
    })
}
