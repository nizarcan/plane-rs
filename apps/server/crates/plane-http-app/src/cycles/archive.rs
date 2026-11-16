// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/cycle/archive.py` — `CycleArchiveUnarchiveEndpoint.get`.
//!
//! Three routes, one method:
//!
//! * `GET …/archived-cycles/` — the archived list (`pk` absent);
//! * `GET …/archived-cycles/{pk}/` — one archived cycle with both
//!   distributions;
//! * `GET …/cycles/{cycle_id}/archive/` — the same method called with a
//!   `cycle_id` kwarg the signature does not accept, which is a `TypeError`
//!   (500) in Django (`archive.py:272`); reproduced by
//!   [`get`] returning [`ViewError::Unknown`] for that route.
//!
//! The single-cycle branch has two bugs ported as-is:
//!
//! * a `pk` that is not an archived cycle of the project makes the Python do
//!   `data["estimate_distribution"] = {}` on `None` → 500;
//! * the archived list does **not** call `user_timezone_converter`, so its
//!   datetimes render as UTC (`Z`) while the non-archived list's render in the
//!   project's timezone.

use chrono::Utc;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::ObjectWriter;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, PgJson, RenderCtx};
use uuid::Uuid;

use crate::cycles::base::{
    self, Chart, CycleArchivedListValue, CycleRow, DistributionKind, decode_archived_list_row,
    decode_archived_retrieve_row, estimate_type_exists, fetch_rows, issues_chart, params,
    points_chart, project_timezone, render_list, uuid_kwarg,
};
use crate::cycles::queries;

/// `CycleArchiveUnarchiveEndpoint.get` (`archive.py:271-584`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // The route with `{cycle_id}` calls `get(self, request, slug,
        // project_id, pk=None)` with an unexpected keyword argument, which
        // Django's `handle_exception` answers with its 500 body.
        if ctx.kwarg("pk").is_none() {
            if ctx.kwarg("cycle_id").is_some() {
                return Err(ViewError::Unknown(
                    "TypeError: CycleArchiveUnarchiveEndpoint.get() got an unexpected keyword \
                     argument 'cycle_id'"
                        .to_string(),
                ));
            }
            return list(ctx).await;
        }
        detail(ctx).await
    })
}

/// The list branch (`archive.py:273-304`).
async fn list(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let client = crate::common::client(ctx)?;
    let params = params(ctx)?;
    // `get_queryset()` reads the project for its timezone even though the list
    // branch never converts datetimes; a soft-deleted project is a 404 there.
    let timezone = project_timezone(client, params.project_id).await?;
    let now = Utc::now();
    let rows = fetch_rows(
        client,
        &queries::CYCLE_ARCHIVED_LIST,
        &[&params.slug, &params.project_id, &params.user_id, &now],
    )
    .await?;
    let decoded = rows
        .iter()
        .map(|row| decode_archived_list_row(row, timezone))
        .collect::<Result<Vec<CycleRow>, _>>()?;
    let values: Vec<CycleArchivedListValue<'_>> =
        decoded.iter().map(CycleArchivedListValue).collect();
    render_list(&values, timezone)
}

/// The single-cycle branch (`archive.py:305-584`).
async fn detail(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let client = crate::common::client(ctx)?;
    let params = params(ctx)?;
    let pk: Uuid = uuid_kwarg(ctx, "pk")?;
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let now = Utc::now();
    let rows = fetch_rows(
        client,
        &queries::CYCLE_ARCHIVED_RETRIEVE,
        &[&params.slug, &params.project_id, &params.user_id, &now, &pk],
    )
    .await?;
    let Some(row) = rows.first() else {
        // `data["estimate_distribution"] = {}` on `None`.
        return Err(ViewError::Unknown(
            "TypeError: 'NoneType' object does not support item assignment".to_string(),
        ));
    };
    let cycle = decode_archived_retrieve_row(row, timezone)?;
    let estimate_type = estimate_type_exists(client, &params).await?;

    let mut estimate_assignees = Vec::new();
    let mut estimate_labels = Vec::new();
    let mut estimate_chart = Chart(Vec::new());
    if estimate_type {
        estimate_assignees = base::distribute(
            client,
            &queries::CYCLE_ARCHIVE_ASSIGNEES_POINTS,
            &[&pk, &params.project_id, &params.slug],
            DistributionKind::ArchiveAssigneesPoints,
        )
        .await?;
        estimate_labels = base::distribute(
            client,
            &queries::CYCLE_ARCHIVE_LABELS_POINTS,
            &[&pk, &params.project_id, &params.slug],
            DistributionKind::ArchiveLabelsPoints,
        )
        .await?;
        if let (Some(start), Some(end)) = (cycle.start_date, cycle.end_date) {
            estimate_chart = points_chart(client, &params, pk, start, end, timezone).await?;
        }
    }

    let assignees = base::distribute(
        client,
        &queries::CYCLE_ARCHIVE_ASSIGNEES_ISSUES,
        &[&pk, &params.project_id, &params.slug],
        DistributionKind::ArchiveAssigneesIssues,
    )
    .await?;
    let labels = base::distribute(
        client,
        &queries::CYCLE_ARCHIVE_LABELS_ISSUES,
        &[&pk, &params.project_id, &params.slug],
        DistributionKind::ArchiveLabelsIssues,
    )
    .await?;
    let mut chart = Chart(Vec::new());
    if let (Some(start), Some(end)) = (cycle.start_date, cycle.end_date) {
        chart = issues_chart(
            client,
            &params,
            pk,
            start,
            end,
            cycle.total_issues,
            timezone,
        )
        .await?;
    }

    let body = ArchivedCycleBody {
        cycle,
        estimate_type,
        estimate_assignees,
        estimate_labels,
        estimate_chart,
        assignees,
        labels,
        chart,
    };
    let bytes = plane_serializers::render(&body, &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
}

/// `archive.py:321-584`'s response: the row's fields, then
/// `estimate_distribution`, then `distribution`.
#[derive(Debug)]
struct ArchivedCycleBody {
    cycle: CycleRow,
    estimate_type: bool,
    estimate_assignees: Vec<base::Distribution>,
    estimate_labels: Vec<base::Distribution>,
    estimate_chart: Chart,
    assignees: Vec<base::Distribution>,
    labels: Vec<base::Distribution>,
    chart: Chart,
}

impl DrfSerialize for ArchivedCycleBody {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        // Re-run the row's fields inside this object: `ObjectWriter` cannot be
        // split, so the archived-detail row is written field by field here.
        write_row(&mut obj, &self.cycle, ctx)?;

        // `data["estimate_distribution"]`: `{}` unless the project's estimate
        // is of type `points` (`archive.py:365-459`).
        if self.estimate_type {
            obj.field_nested_dyn(
                "estimate_distribution",
                ctx,
                Some(&base::DistributionGroup {
                    assignees: &self.estimate_assignees,
                    labels: &self.estimate_labels,
                    chart: Some(&self.estimate_chart),
                }),
            )?;
        } else {
            obj.field(
                "estimate_distribution",
                &std::collections::BTreeMap::<String, String>::new(),
            )?;
        }

        obj.field_nested_dyn(
            "distribution",
            ctx,
            Some(&base::DistributionGroup {
                assignees: &self.assignees,
                labels: &self.labels,
                chart: Some(&self.chart),
            }),
        )?;
        obj.end();
        Ok(())
    }
}

/// `CycleArchivedRetrieveValue`'s fields into an object already started.
fn write_row(
    obj: &mut ObjectWriter<'_>,
    row: &CycleRow,
    ctx: &RenderCtx<'_>,
) -> Result<(), DrfError> {
    // The archived detail does not convert datetimes: the values are UTC.
    use plane_serializers::DrfUuid;
    obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
    obj.field_dyn("workspace_id", ctx, &DrfUuid(row.workspace_id))?;
    obj.field_dyn("project_id", ctx, &DrfUuid(row.project_id))?;
    obj.field("name", &row.name)?;
    obj.field("description", &row.description)?;
    obj.field("start_date", &row.start_date)?;
    obj.field("end_date", &row.end_date)?;
    obj.field_dyn("owned_by_id", ctx, &DrfUuid(row.owned_by_id))?;
    obj.field_json("view_props", row.view_props.as_ref().map(PgJson::from))?;
    obj.field("sort_order", &row.sort_order)?;
    obj.field("external_source", &row.external_source)?;
    obj.field("external_id", &row.external_id)?;
    obj.field_json(
        "progress_snapshot",
        row.progress_snapshot.as_ref().map(PgJson::from),
    )?;
    obj.field("sub_issues", &row.sub_issues.unwrap_or_default())?;
    obj.field_json("logo_props", row.logo_props.as_ref().map(PgJson::from))?;
    obj.field(
        "completed_estimate_points",
        &row.completed_estimate_points.unwrap_or_default(),
    )?;
    obj.field(
        "total_estimate_points",
        &row.total_estimate_points.unwrap_or_default(),
    )?;
    obj.field("is_favorite", &row.is_favorite)?;
    obj.field("total_issues", &row.total_issues)?;
    obj.field(
        "cancelled_issues",
        &row.cancelled_issues.unwrap_or_default(),
    )?;
    obj.field("completed_issues", &row.completed_issues)?;
    obj.field("started_issues", &row.started_issues.unwrap_or_default())?;
    obj.field(
        "unstarted_issues",
        &row.unstarted_issues.unwrap_or_default(),
    )?;
    obj.field("backlog_issues", &row.backlog_issues.unwrap_or_default())?;
    obj.field(
        "assignee_ids",
        &row.assignee_ids
            .iter()
            .copied()
            .map(DrfUuid)
            .collect::<Vec<_>>(),
    )?;
    obj.field("status", &row.status)?;
    obj.field("created_by", &row.created_by)?;
    obj.field("archived_at", &row.archived_at)?;
    Ok(())
}
