// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.workspace.estimate.WorkspaceEstimatesEndpoint`
//! (`GET /api/workspaces/{slug}/estimates/`).
//!
//! Python: `apps/api/plane/app/views/workspace/estimate.py:18-31`.
//! `WorkspaceEntityPermission`, `use_read_replica = true`, and
//! `@cache_response(60 * 60 * 2)` with the decorator's defaults (key
//! `request.get_full_path():<user id>`, value
//! `{"data": serializer.data, "status": 200}`, two hours).
//!
//! The stored value is DRF's `ReturnList` of **plain dicts** — the child
//! serializer's `to_representation` returns a `dict`; only the list is a
//! `ReturnList` (`rest_framework/serializers.py:530-551,727-737`) — with
//! `uuid.UUID` objects for every pk, so it is built as a `PyValue` and pickled
//! byte-identically to Django's.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use plane_cache::{PyInterner, PyValue};
use plane_core::error::ApiError;
use plane_db::models::estimate::Estimate;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;

use crate::common::cache;
use crate::common::client;
use crate::estimates::queries;
use crate::estimates::ser;

/// `cache_response(60 * 60 * 2)` (`plane/app/views/workspace/estimate.py:21`).
const CACHE_SECONDS: i64 = 2 * 60 * 60;

/// `WorkspaceEstimatesEndpoint.get` (`plane/app/views/workspace/estimate.py:22-30`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        cache::cache_response(ctx, CACHE_SECONDS, None, true, || async move {
            let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
            ctx.user
                .id()
                .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

            let statement = queries::WORKSPACE_ESTIMATES.prepare(client(ctx)?).await?;
            let rows = client(ctx)?.query(&statement, &[&slug]).await?;
            let mut estimates = Vec::with_capacity(rows.len());
            for row in &rows {
                estimates.push(Estimate::from_row(row, 0)?);
            }

            // `prefetch_related("points")`: one query for the whole set.
            let mut grouped = std::collections::HashMap::new();
            if !estimates.is_empty() {
                let ids: Vec<uuid::Uuid> = estimates.iter().map(|estimate| estimate.id).collect();
                let statement = queries::ESTIMATE_POINTS_BY_ESTIMATES
                    .prepare(client(ctx)?)
                    .await?;
                let point_rows = client(ctx)?.query(&statement, &[&ids]).await?;
                let mut points = Vec::with_capacity(point_rows.len());
                for row in &point_rows {
                    points.push(plane_db::models::estimate::EstimatePoint::from_row(row, 0)?);
                }
                grouped = ser::group_points(points);
            }
            let point_rows = ser::points_per_estimate(&estimates, grouped);

            // `serializer.data` holds DRF's *rendered* datetime strings, in the
            // active timezone (this route runs `TimezoneMixin`).
            let prepared = crate::common::render::RequestRender::from_ctx(ctx);
            let timezone = prepared.timezone();
            let mut interner = PyInterner::new();
            let data = PyValue::return_list(
                estimates
                    .iter()
                    .zip(point_rows.iter())
                    .map(|(estimate, points)| {
                        estimate_value(estimate, points, timezone, &mut interner)
                    })
                    .collect::<Result<Vec<_>, ViewError>>()?,
            );
            Ok((200, data))
        })
        .await
    })
}

/// A DRF `DateTimeField` value: `isoformat()` in the active timezone with
/// `+00:00` replaced by `Z` (`rest_framework/fields.py:1216-1232`).
///
/// `plane-serde` exposes the DRF datetime only as a `Serialize` newtype, and a
/// pickle needs the *string* the serializer produced, so the value is rendered
/// through the DRF dialect and its JSON quotes are dropped. A datetime string
/// never contains an escape, so stripping the delimiters is exact; the asserts
/// make that assumption fail loudly rather than silently.
fn drf_datetime_string(value: DateTime<Utc>, tz: Tz) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(Dialect::Drf, &plane_serializers::drf_datetime(value, &tz))?;
    let quoted = String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))?;
    let stripped = quoted
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .ok_or_else(|| {
            ViewError::Unknown(format!(
                "DRF datetime did not render as a JSON string: {quoted}"
            ))
        })?;
    Ok(stripped.to_string())
}

/// `WorkspaceEstimateSerializer(estimate).data`, as `serializer.data` holds it.
fn estimate_value(
    estimate: &Estimate,
    points: &[plane_db::models::estimate::EstimatePoint],
    tz: Tz,
    strings: &mut PyInterner,
) -> Result<PyValue, ViewError> {
    let mut point_values = Vec::with_capacity(points.len());
    for point in points {
        let created_at = drf_datetime_string(point.created_at, tz)?;
        let updated_at = drf_datetime_string(point.updated_at, tz)?;
        let deleted_at = match point.deleted_at {
            Some(value) => Some(drf_datetime_string(value, tz)?),
            None => None,
        };
        let entry = PyValue::mapping(vec![
            (strings.key("id"), PyValue::uuid(point.id)),
            (strings.key("created_at"), strings.value(&created_at)),
            (strings.key("updated_at"), strings.value(&updated_at)),
            (
                strings.key("deleted_at"),
                match deleted_at {
                    Some(value) => strings.value(&value),
                    None => PyValue::None,
                },
            ),
            (strings.key("key"), PyValue::int(i64::from(point.key))),
            (
                strings.key("description"),
                strings.value(&point.description),
            ),
            (strings.key("value"), strings.value(&point.value)),
            (
                strings.key("created_by"),
                point.created_by_id.map_or(PyValue::None, PyValue::uuid),
            ),
            (
                strings.key("updated_by"),
                point.updated_by_id.map_or(PyValue::None, PyValue::uuid),
            ),
            (strings.key("project"), PyValue::uuid(point.project_id)),
            (strings.key("workspace"), PyValue::uuid(point.workspace_id)),
            (strings.key("estimate"), PyValue::uuid(point.estimate_id)),
        ]);
        point_values.push(entry);
    }
    let created_at = drf_datetime_string(estimate.created_at, tz)?;
    let updated_at = drf_datetime_string(estimate.updated_at, tz)?;
    let deleted_at = match estimate.deleted_at {
        Some(value) => Some(drf_datetime_string(value, tz)?),
        None => None,
    };
    Ok(PyValue::mapping(vec![
        (strings.key("id"), PyValue::uuid(estimate.id)),
        (strings.key("points"), PyValue::list(point_values)),
        (strings.key("created_at"), strings.value(&created_at)),
        (strings.key("updated_at"), strings.value(&updated_at)),
        (
            strings.key("deleted_at"),
            match deleted_at {
                Some(value) => strings.value(&value),
                None => PyValue::None,
            },
        ),
        (strings.key("name"), strings.value(&estimate.name)),
        (
            strings.key("description"),
            strings.value(&estimate.description),
        ),
        // `Estimate.type` is a `ChoiceField`: DRF returns the model's choice
        // constant, the same compiler-interned object as a field name.
        (strings.key("type"), strings.choice(&estimate.r#type)),
        (strings.key("last_used"), PyValue::bool(estimate.last_used)),
        (
            strings.key("created_by"),
            estimate.created_by_id.map_or(PyValue::None, PyValue::uuid),
        ),
        (
            strings.key("updated_by"),
            estimate.updated_by_id.map_or(PyValue::None, PyValue::uuid),
        ),
        (strings.key("project"), PyValue::uuid(estimate.project_id)),
        (
            strings.key("workspace"),
            PyValue::uuid(estimate.workspace_id),
        ),
    ]))
}
