// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.estimate.base` — the three read actions.
//!
//! Python: `apps/api/plane/app/views/estimate/base.py:35-107`.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `GET …/project-estimates/` | `ProjectEstimatePointEndpoint.get` | `:35-47` |
//! | `GET …/estimates/` | `BulkEstimatePointEndpoint.list` | `:59-71` |
//! | `GET …/estimates/{estimate_id}/` | `BulkEstimatePointEndpoint.retrieve` | `:103-107` |
//!
//! `ProjectEstimatePointEndpoint` looks the project up itself
//! (`Project.objects.get(workspace__slug=slug, pk=project_id)`) and returns `[]`
//! — not a 404 — when the project has no estimate. The lookup's
//! `DoesNotExist` becomes the family handler's
//! `404 {"error": "The required object does not exist."}`.

use plane_core::error::APPLICATION_JSON;
use plane_db::models::estimate::{Estimate, EstimatePoint};
use plane_db::models::project::Project;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_estimate::{EstimatePointSerializer, EstimateReadSerializer};

use crate::common::render::RequestRender;
use crate::common::{client, lookup, render_list};
use crate::estimates::queries;
use crate::estimates::ser;

/// `ProjectEstimatePointEndpoint.get` (`plane/app/views/estimate/base.py:35-47`).
pub fn project_estimates<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;

        let statement = queries::PROJECT_BY_SLUG_AND_ID
            .prepare(client(ctx)?)
            .await?;
        let project_row = client(ctx)?
            .query_opt(&statement, &[&project_id, &slug])
            .await?;
        let Some(project_row) = project_row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let project = Project::from_row(&project_row, 0)?;

        let prepared = RequestRender::from_ctx(ctx);
        let Some(estimate_id) = project.estimate_id else {
            // `return Response([], status=200)` — an empty list, not a 404.
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, b"[]".to_vec()));
        };

        let rows = client(ctx)?
            .query(
                queries::ESTIMATE_POINTS_BY_PROJECT.sql,
                &[&estimate_id, &project_id, &slug],
            )
            .await?;
        let mut points = Vec::with_capacity(rows.len());
        for row in &rows {
            points.push(EstimatePoint::from_row(row, 0)?);
        }
        let serializers: Vec<EstimatePointSerializer<'_>> =
            points.iter().map(ser::point_serializer).collect();
        let body = render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `BulkEstimatePointEndpoint.list` (`plane/app/views/estimate/base.py:59-71`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;

        let statement = queries::ESTIMATES_BY_PROJECT.prepare(client(ctx)?).await?;
        let rows = client(ctx)?
            .query(&statement, &[&project_id, &slug])
            .await?;
        let mut estimates = Vec::with_capacity(rows.len());
        for row in &rows {
            estimates.push(Estimate::from_row(row, 0)?);
        }

        let body = render_estimates(ctx, &estimates).await?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `BulkEstimatePointEndpoint.retrieve` (`plane/app/views/estimate/base.py:103-107`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let estimate_id = lookup::uuid_kwarg(
            "estimate_id",
            ctx.kwarg("estimate_id").ok_or(ViewError::Key)?,
        )?;

        let row = client(ctx)?
            .query_opt(
                queries::ESTIMATE_BY_ID.sql,
                &[&estimate_id, &slug, &project_id],
            )
            .await?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let estimate = Estimate::from_row(&row, 0)?;

        // `EstimateReadSerializer(estimate)` walks `estimate.points`, which is a
        // reverse manager: one query for this instance.
        let statement = queries::ESTIMATE_POINTS_OF_ESTIMATE
            .prepare(client(ctx)?)
            .await?;
        let point_rows = client(ctx)?.query(&statement, &[&estimate.id]).await?;
        let mut points = Vec::with_capacity(point_rows.len());
        for row in &point_rows {
            points.push(EstimatePoint::from_row(row, 0)?);
        }
        let point_serializers: Vec<EstimatePointSerializer<'_>> =
            points.iter().map(ser::point_serializer).collect();
        let serializer = EstimateReadSerializer {
            id: estimate.id,
            points: &point_serializers,
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
        };
        let body = crate::common::render(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// The page of estimates with their prefetched points.
///
/// Django issues one query for the estimates and one for **all** their points
/// (`prefetch_related`), which is what this does; the per-estimate
/// `issue_sequences`-style N+1 does not exist here.
pub(crate) async fn render_estimates(
    ctx: &ViewCtx<'_>,
    estimates: &[Estimate],
) -> Result<Vec<u8>, ViewError> {
    if estimates.is_empty() {
        return Ok(b"[]".to_vec());
    }
    let ids: Vec<uuid::Uuid> = estimates.iter().map(|estimate| estimate.id).collect();
    // The prefetch is a second round trip after the estimates query, exactly as
    // Django's `prefetch_related` does it.
    let statement = queries::ESTIMATE_POINTS_BY_ESTIMATES
        .prepare(client(ctx)?)
        .await?;
    let point_rows = client(ctx)?.query(&statement, &[&ids]).await?;
    let mut points = Vec::with_capacity(point_rows.len());
    for row in &point_rows {
        points.push(EstimatePoint::from_row(row, 0)?);
    }

    let grouped = ser::group_points(points);
    let rows = ser::points_per_estimate(estimates, grouped);
    let point_serializers: Vec<Vec<EstimatePointSerializer<'_>>> = rows
        .iter()
        .map(|list| list.iter().map(ser::point_serializer).collect())
        .collect();
    let serializers = ser::estimate_read_serializers(estimates, &point_serializers);
    let prepared = RequestRender::from_ctx(ctx);
    render_list(&serializers, &prepared.render_ctx())
}
