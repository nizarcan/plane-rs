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
use plane_cache::PyValue;
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

/// One `PyValue` per distinct string, shared by every occurrence.
///
/// CPython's pickler memoises by object identity, and a DRF `serializer.data`
/// graph shares its dict keys (they are the serializer's class-level field
/// names) and the empty string (CPython's singleton), so Django's frame
/// references them with `BINGET` after their first occurrence. Building each
/// `PyValue::string` independently would emit the literal again and grow the
/// entry: the same value measured 4223 bytes against Django's 3041, which is
/// past the differential harness's 4096-byte comparison cap.
///
/// Interning also collapses equal *values* that Python created separately. That
/// is still a valid pickle with the identical decoded value, and it never makes
/// the frame larger than Django's.
#[derive(Default)]
struct Interner {
    strings: std::collections::HashMap<String, PyValue>,
}

impl Interner {
    /// A dict key: DRF's field names are class-level strings, so every object
    /// in a page shares one.
    fn key(&mut self, name: &str) -> PyValue {
        if let Some(existing) = self.strings.get(name) {
            return existing.clone();
        }
        let py = PyValue::string(name.to_string());
        self.strings.insert(name.to_string(), py.clone());
        py
    }

    /// A value read from the database. CPython shares the empty string (its
    /// singleton) and every identifier-like literal (the compiler interns them),
    /// which matters because a `ChoiceField` renders a model `choices` entry by
    /// returning the class-level object — so an estimate whose `type` is
    /// `"points"` is the same string object as the `points` dict key and Django's
    /// frame references it with `BINGET`. Two equal non-identifier values from
    /// different rows are distinct objects in Python; sharing them here only
    /// makes the frame smaller, never larger.
    fn value(&mut self, text: &str) -> PyValue {
        if text.is_empty() || is_python_identifier(text) {
            return self.key(text);
        }
        PyValue::string(text.to_string())
    }
}

/// Whether CPython would intern `text` as an identifier: ASCII letters,
/// digits and underscores, not starting with a digit.
fn is_python_identifier(text: &str) -> bool {
    let mut bytes = text.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_alphabetic() || first == b'_' => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

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
            let mut interner = Interner::default();
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
    strings: &mut Interner,
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
        (strings.key("type"), strings.value(&estimate.r#type)),
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
