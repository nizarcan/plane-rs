// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two response bodies the estimates routes share.
//!
//! * [`estimate_read`] — `Response(EstimateReadSerializer(estimate).data)`: the
//!   13-key estimate object with its `points` (`plane/app/views/estimate/
//!   base.py:100-101`, `:105`, `:143-144`). `estimate.points` is a reverse
//!   manager, so Django issues one query per instance, ordered by the model's
//!   `Meta.ordering = ("value",)`; nothing prefetches it here.
//! * [`point_list`] — `Response(EstimatePointSerializer(points, many=True)
//!   .data)`: the 12-key point array, in the order the caller's queryset
//!   returned (`base.py:44-45`, `:265-268`).
//!
//! Both render under the request's `fields=`/`expand=`/timezone like every
//! other read (`crate::common::render`), which is what the Python serializers
//! do through `SerializerFactoryMixin`.

use plane_db::models::estimate::{Estimate, EstimatePoint};
use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;
use plane_serializers::generated::app_estimate::EstimatePointSerializer;

use crate::common::render::{self, RequestRender};
use crate::common::{client, render_list};
use crate::estimates::queries;
use crate::estimates::ser;

/// `EstimateReadSerializer(estimate).data`, with `points` re-read from the
/// database (`base.py:105`).
///
/// The reverse manager is why the create route's `bulk_create` result is
/// ignored and why a PATCH's response shows the points as they are stored.
///
/// # Errors
///
/// A failing points query, or a field that cannot be rendered.
pub async fn estimate_read(ctx: &ViewCtx<'_>, estimate: &Estimate) -> Result<Vec<u8>, ViewError> {
    let statement = queries::ESTIMATE_POINTS_OF_ESTIMATE
        .prepare(client(ctx)?)
        .await?;
    let rows = client(ctx)?.query(&statement, &[&estimate.id]).await?;
    let mut points = Vec::with_capacity(rows.len());
    for row in &rows {
        points.push(EstimatePoint::from_row(row, 0)?);
    }
    estimate_with_points(ctx, estimate, &points)
}

/// [`estimate_read`] for a caller that already holds the point rows.
///
/// # Errors
///
/// A field that cannot be rendered.
fn estimate_with_points(
    ctx: &ViewCtx<'_>,
    estimate: &Estimate,
    points: &[EstimatePoint],
) -> Result<Vec<u8>, ViewError> {
    let point_serializers: Vec<EstimatePointSerializer<'_>> =
        points.iter().map(ser::point_serializer).collect();
    let serializer = ser::estimate_read_serializer(estimate, &point_serializers);
    render::render(ctx, &serializer)
}

/// `EstimatePointSerializer(points, many=True).data`, in the caller's order.
///
/// # Errors
///
/// A field that cannot be rendered.
pub fn point_list(ctx: &ViewCtx<'_>, points: &[EstimatePoint]) -> Result<Vec<u8>, ViewError> {
    let serializers: Vec<EstimatePointSerializer<'_>> =
        points.iter().map(ser::point_serializer).collect();
    let prepared = RequestRender::from_ctx(ctx);
    render_list(&serializers, &prepared.render_ctx())
}
