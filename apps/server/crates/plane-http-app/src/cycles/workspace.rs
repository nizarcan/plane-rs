// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/cycle.py` — `WorkspaceCyclesEndpoint.get`
//! (`GET /api/workspaces/{slug}/cycles/`).
//!
//! The response is `CycleSerializer(cycles, many=True)`: unlike the
//! project-scoped list it does not run `user_timezone_converter`, so the
//! serializer's `DateTimeField`s render in the request's active (user profile)
//! timezone, and the fields the queryset does not annotate (`is_favorite`,
//! `status`) are skipped by DRF's `SkipField` rule — the body has neither key.

use chrono::{DateTime, Utc};
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_cycle::CycleSerializer;
use plane_serializers::{PgJson, RenderCtx};
use uuid::Uuid;

use crate::common::{client, jobs};
use crate::cycles::queries;

/// One row of `WORKSPACE_CYCLES`.
struct Row {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    name: String,
    description: String,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
    owned_by_id: Uuid,
    view_props: Option<JsonText>,
    sort_order: f64,
    external_source: Option<String>,
    external_id: Option<String>,
    progress_snapshot: Option<JsonText>,
    logo_props: Option<JsonText>,
    total_issues: i64,
    cancelled_issues: i64,
    completed_issues: i64,
    started_issues: i64,
    unstarted_issues: i64,
    backlog_issues: i64,
}

/// `WorkspaceCyclesEndpoint.get` (`workspace/cycle.py:22-109`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user_id = jobs::user_id(ctx)?;
        let statement = client
            .prepare_cached(queries::WORKSPACE_CYCLES.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&slug, &user_id])
            .await
            .map_err(ViewError::from)?;
        let mut decoded = Vec::with_capacity(rows.len());
        for row in &rows {
            decoded.push(Row {
                id: row.try_get(0).map_err(ViewError::from)?,
                workspace_id: row.try_get(1).map_err(ViewError::from)?,
                project_id: row.try_get(2).map_err(ViewError::from)?,
                name: row.try_get(3).map_err(ViewError::from)?,
                description: row.try_get(4).map_err(ViewError::from)?,
                start_date: row.try_get(5).map_err(ViewError::from)?,
                end_date: row.try_get(6).map_err(ViewError::from)?,
                owned_by_id: row.try_get(7).map_err(ViewError::from)?,
                view_props: row.try_get(8).map_err(ViewError::from)?,
                sort_order: row.try_get(9).map_err(ViewError::from)?,
                external_source: row.try_get(10).map_err(ViewError::from)?,
                external_id: row.try_get(11).map_err(ViewError::from)?,
                progress_snapshot: row.try_get(12).map_err(ViewError::from)?,
                logo_props: row.try_get(13).map_err(ViewError::from)?,
                total_issues: row.try_get(14).map_err(ViewError::from)?,
                cancelled_issues: row.try_get(15).map_err(ViewError::from)?,
                completed_issues: row.try_get(16).map_err(ViewError::from)?,
                started_issues: row.try_get(17).map_err(ViewError::from)?,
                unstarted_issues: row.try_get(18).map_err(ViewError::from)?,
                backlog_issues: row.try_get(19).map_err(ViewError::from)?,
            });
        }
        let cycles: Vec<CycleSerializer<'_>> = decoded
            .iter()
            .map(|row| CycleSerializer {
                id: row.id,
                workspace_id: row.workspace_id,
                project_id: row.project_id,
                name: &row.name,
                description: &row.description,
                start_date: row.start_date,
                end_date: row.end_date,
                owned_by_id: row.owned_by_id,
                view_props: row.view_props.as_ref().map(PgJson::from),
                sort_order: row.sort_order,
                external_source: row.external_source.as_deref(),
                external_id: row.external_id.as_deref(),
                progress_snapshot: row.progress_snapshot.as_ref().map(PgJson::from),
                logo_props: row.logo_props.as_ref().map(PgJson::from),
                // `get_queryset` does not annotate either; `CycleSerializer`
                // skips a `required=False` field whose attribute is missing.
                is_favorite: None,
                total_issues: Some(row.total_issues),
                cancelled_issues: Some(row.cancelled_issues),
                completed_issues: Some(row.completed_issues),
                started_issues: Some(row.started_issues),
                unstarted_issues: Some(row.unstarted_issues),
                backlog_issues: Some(row.backlog_issues),
                status: None,
            })
            .collect();
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let bytes = plane_serializers::render_list(&cycles, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(
            200,
            plane_core::error::APPLICATION_JSON,
            bytes,
        ))
    })
}
