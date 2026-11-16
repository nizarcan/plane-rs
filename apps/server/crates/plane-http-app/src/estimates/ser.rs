// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Serialising an `Estimate` together with its points.
//!
//! Both `EstimateReadSerializer` and `WorkspaceEstimateSerializer` start with
//! `id`, then the nested `points` list, then the estimate's own columns
//! (`plane/app/serializers/estimate.py`), and `points` is a reverse manager on
//! the model, so the value is whatever query Django ran for it: the
//! `prefetch_related("points")` page query on the collection routes and a
//! single `EstimatePoint.objects.filter(estimate_id=…)` on the detail route.
//!
//! The helpers below are shared by the two handler modules so each does not
//! hold its own copy of the borrow order: the caller owns the point rows, then
//! builds the point serializers that borrow them, then the estimate
//! serializers that borrow slices of those vectors.

use std::collections::HashMap;

use plane_db::models::estimate::{Estimate, EstimatePoint};
use plane_serializers::generated::app_estimate::{EstimatePointSerializer, EstimateReadSerializer};

/// `EstimatePointSerializer(point)`.
pub fn point_serializer(point: &EstimatePoint) -> EstimatePointSerializer<'_> {
    EstimatePointSerializer {
        id: point.id,
        created_at: point.created_at,
        updated_at: point.updated_at,
        deleted_at: point.deleted_at,
        key: i64::from(point.key),
        description: &point.description,
        value: &point.value,
        created_by: point.created_by_id,
        updated_by: point.updated_by_id,
        project: point.project_id,
        workspace: point.workspace_id,
        estimate: point.estimate_id,
    }
}

/// Group a page of points by their estimate, keeping the query's order.
pub fn group_points(points: Vec<EstimatePoint>) -> HashMap<uuid::Uuid, Vec<EstimatePoint>> {
    let mut grouped: HashMap<uuid::Uuid, Vec<EstimatePoint>> = HashMap::new();
    for point in points {
        grouped.entry(point.estimate_id).or_default().push(point);
    }
    grouped
}

/// One row vector per estimate, in the estimates' order.
///
/// The caller keeps this alive for as long as the serializers built from it.
pub fn points_per_estimate(
    estimates: &[Estimate],
    mut grouped: HashMap<uuid::Uuid, Vec<EstimatePoint>>,
) -> Vec<Vec<EstimatePoint>> {
    estimates
        .iter()
        .map(|estimate| grouped.remove(&estimate.id).unwrap_or_default())
        .collect()
}

/// One `EstimateReadSerializer` per estimate, borrowing `rows` and `points`.
///
/// `points` must be the point serializers of `rows` (same length and order);
/// passing it in lets the caller own it for as long as the estimates are
/// rendered.
pub fn estimate_read_serializers<'a>(
    estimates: &'a [Estimate],
    points: &'a [Vec<EstimatePointSerializer<'a>>],
) -> Vec<EstimateReadSerializer<'a>> {
    estimates
        .iter()
        .zip(points.iter())
        .map(|(estimate, points)| EstimateReadSerializer {
            id: estimate.id,
            points,
            created_at: estimate.created_at,
            updated_at: estimate.updated_at,
            deleted_at: estimate.deleted_at,
            name: &estimate.name,
            description: &estimate.description,
            r#type: &estimate.r#type,
            last_used: estimate.last_used,
            created_by: estimate.created_by_id,
            updated_by: estimate.updated_by_id,
            project: estimate.project_id,
            workspace: estimate.workspace_id,
        })
        .collect()
}
