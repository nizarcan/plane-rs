// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/cycle/base.py`.
//!
//! * `CycleViewSet.list` / `retrieve` — `GET …/cycles/` and `GET …/cycles/{pk}/`;
//! * `CycleUserPropertiesEndpoint.get` — `GET …/cycles/{cycle_id}/user-properties/`;
//! * `CycleProgressEndpoint.get` — `GET …/cycles/{cycle_id}/progress/`;
//! * `CycleAnalyticsEndpoint.get` — `GET …/cycles/{cycle_id}/analytics/`.
//!
//! Every one of them builds its body from `.values()` dictionaries or plain
//! dicts rather than a serializer, so the key order is the Python's insertion
//! order and the numbers are Python's: `0.0 or 0` is the int `0`
//! (`progress`), a `NULL` sum stays `null` (`analytics`), a `jsonb` column keeps
//! Postgres' key order (`PgJson` → `plane_serde::transcode_json`), and datetimes
//! are rendered in the **project's** timezone for the list/retrieve
//! (`user_timezone_converter`, `base.py:235,268,467`) and in the request's
//! active timezone everywhere else.
//!
//! The issue lists, the archive endpoints and the workspace-wide endpoint live
//! in `issue.rs`, `archive.rs` and `workspace.rs`.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::{
    DrfDateTime, DrfError, DrfSerialize, DrfUuid, DrfValue, DrfWriter, ObjectWriter, PgJson,
    RenderCtx,
};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::common::{client, jobs, lookup};
use crate::cycles::queries;

/// `Project.objects.get(id=project_id)`'s timezone, or the 404 a `DoesNotExist`
/// renders (`base.py:79-86`). A timezone pytz cannot resolve is a 500 there
/// (`UnknownTimeZoneError`), so it is [`ViewError::Unknown`] here.
pub(crate) async fn project_timezone(client: &Client, project_id: Uuid) -> Result<Tz, ViewError> {
    let statement = client
        .prepare_cached(queries::CYCLE_PROJECT_TIMEZONE.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await
        .map_err(ViewError::from)?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    let name: String = row.try_get(0).map_err(ViewError::from)?;
    Tz::from_str(&name).map_err(|_| ViewError::Unknown(format!("UnknownTimeZoneError: {name}")))
}

/// The URL kwargs and user id every route of this file needs.
pub(crate) struct Params {
    pub(crate) slug: String,
    pub(crate) project_id: Uuid,
    pub(crate) user_id: Uuid,
}

pub(crate) fn params(ctx: &ViewCtx<'_>) -> Result<Params, ViewError> {
    Ok(Params {
        slug: ctx
            .kwarg("slug")
            .map(str::to_string)
            .ok_or(ViewError::Key)?,
        project_id: lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?,
        user_id: jobs::user_id(ctx)?,
    })
}

pub(crate) fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    Ok(lookup::uuid_kwarg(
        name,
        ctx.kwarg(name).ok_or(ViewError::Key)?,
    )?)
}

/// Fetch `query`'s rows with prepared (cached) parameters.
pub(crate) async fn fetch_rows(
    client: &Client,
    query: &plane_db::Query,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Vec<Row>, ViewError> {
    let statement = client
        .prepare_cached(query.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .query(&statement, params)
        .await
        .map_err(ViewError::from)
}

/// Serialize `rows` as one JSON array, in the order Postgres returned them.
pub(crate) fn render_list<T: DrfSerialize>(
    rows: &[T],
    timezone: Tz,
) -> Result<ViewResponse, ViewError> {
    let ctx = RenderCtx::full(&timezone);
    let mut writer = DrfWriter::new();
    let mut array = writer.begin_array();
    for row in rows {
        array.nested(&ctx, row)?;
    }
    array.end();
    Ok(ViewResponse::raw(200, APPLICATION_JSON, writer.into_vec()))
}

/// Serialize one object.
pub(crate) fn render_one<T: DrfSerialize>(
    value: &T,
    timezone: Tz,
) -> Result<ViewResponse, ViewError> {
    let bytes = plane_serializers::render(value, &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
}

/// A one-key `{"error": …}` body (the family `handle_exception` shape).
pub(crate) fn error_body(status: u16, message: &str) -> ViewResponse {
    ViewResponse::json(status, serde_json::json!({ "error": message }))
}

fn opt_text(row: &Row, index: usize) -> Result<Option<String>, ViewError> {
    row.try_get(index).map_err(ViewError::from)
}

fn text(row: &Row, index: usize) -> Result<String, ViewError> {
    row.try_get(index).map_err(ViewError::from)
}

fn opt_json(row: &Row, index: usize) -> Result<Option<JsonText>, ViewError> {
    row.try_get(index).map_err(ViewError::from)
}

fn write_datetime(value: Option<DateTime<Utc>>, timezone: Tz) -> Option<DrfDateTime<Tz>> {
    value.map(|value| DrfDateTime(value.with_timezone(&timezone)))
}

fn uuid_list(values: &[Uuid]) -> Vec<DrfUuid> {
    values.iter().copied().map(DrfUuid).collect()
}

/// One row of `CycleViewSet.list`'s `.values()` (`base.py:208-233,240-266`).
#[derive(Debug)]
pub(crate) struct CycleRow {
    pub(crate) id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) start_date: Option<DateTime<Utc>>,
    pub(crate) end_date: Option<DateTime<Utc>>,
    pub(crate) owned_by_id: Uuid,
    pub(crate) view_props: Option<JsonText>,
    pub(crate) sort_order: f64,
    pub(crate) external_source: Option<String>,
    pub(crate) external_id: Option<String>,
    pub(crate) progress_snapshot: Option<JsonText>,
    pub(crate) sub_issues: Option<i64>,
    pub(crate) logo_props: Option<JsonText>,
    pub(crate) completed_estimate_points: Option<f64>,
    pub(crate) total_estimate_points: Option<f64>,
    pub(crate) is_favorite: bool,
    pub(crate) total_issues: i64,
    pub(crate) cancelled_issues: Option<i64>,
    pub(crate) completed_issues: i64,
    pub(crate) started_issues: Option<i64>,
    pub(crate) unstarted_issues: Option<i64>,
    pub(crate) backlog_issues: Option<i64>,
    pub(crate) assignee_ids: Vec<Uuid>,
    pub(crate) status: String,
    pub(crate) version: Option<i32>,
    pub(crate) created_by: Option<Uuid>,
    pub(crate) archived_at: Option<DateTime<Utc>>,
    /// The project's timezone, which `user_timezone_converter` applies.
    pub(crate) timezone: Tz,
}

impl CycleRow {
    /// `…, progress_snapshot, logo_props` — the prefix shared by all shapes.
    fn write_prefix(
        &self,
        obj: &mut ObjectWriter<'_>,
        ctx: &RenderCtx<'_>,
    ) -> Result<(), DrfError> {
        obj.field_dyn("id", ctx, &DrfUuid(self.id))?;
        obj.field_dyn("workspace_id", ctx, &DrfUuid(self.workspace_id))?;
        obj.field_dyn("project_id", ctx, &DrfUuid(self.project_id))?;
        obj.field("name", &self.name)?;
        obj.field("description", &self.description)?;
        obj.field(
            "start_date",
            &write_datetime(self.start_date, self.timezone),
        )?;
        obj.field("end_date", &write_datetime(self.end_date, self.timezone))?;
        obj.field_dyn("owned_by_id", ctx, &DrfUuid(self.owned_by_id))?;
        obj.field_json("view_props", self.view_props.as_ref().map(PgJson::from))?;
        obj.field("sort_order", &self.sort_order)?;
        obj.field("external_source", &self.external_source)?;
        obj.field("external_id", &self.external_id)?;
        obj.field_json(
            "progress_snapshot",
            self.progress_snapshot.as_ref().map(PgJson::from),
        )?;
        Ok(())
    }
}

/// `CycleViewSet.list`'s row (`base.py:240-266`).
#[derive(Debug)]
pub(crate) struct CycleListValue<'a>(pub(crate) &'a CycleRow);

/// The `cycle_view=current` branch's row (`base.py:208-233`): the same fields
/// with `completed_issues` before `cancelled_issues`, which is the order that
/// branch's `.values()` call uses.
#[derive(Debug)]
pub(crate) struct CycleListCurrentValue<'a>(pub(crate) &'a CycleRow);

impl DrfSerialize for CycleListCurrentValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = self.0;
        let mut obj = w.begin_object();
        row.write_prefix(&mut obj, ctx)?;
        obj.field_json("logo_props", row.logo_props.as_ref().map(PgJson::from))?;
        obj.field("is_favorite", &row.is_favorite)?;
        obj.field("total_issues", &row.total_issues)?;
        obj.field("completed_issues", &row.completed_issues)?;
        obj.field(
            "cancelled_issues",
            &row.cancelled_issues.unwrap_or_default(),
        )?;
        obj.field("assignee_ids", &uuid_list(&row.assignee_ids))?;
        obj.field("status", &row.status)?;
        obj.field("version", &row.version.unwrap_or(1))?;
        obj.field("created_by", &row.created_by)?;
        obj.end();
        Ok(())
    }
}

impl DrfSerialize for CycleListValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = self.0;
        let mut obj = w.begin_object();
        row.write_prefix(&mut obj, ctx)?;
        obj.field_json("logo_props", row.logo_props.as_ref().map(PgJson::from))?;
        obj.field("is_favorite", &row.is_favorite)?;
        obj.field("total_issues", &row.total_issues)?;
        obj.field(
            "cancelled_issues",
            &row.cancelled_issues.unwrap_or_default(),
        )?;
        obj.field("completed_issues", &row.completed_issues)?;
        obj.field("assignee_ids", &uuid_list(&row.assignee_ids))?;
        obj.field("status", &row.status)?;
        obj.field("version", &row.version.unwrap_or(1))?;
        obj.field("created_by", &row.created_by)?;
        obj.end();
        Ok(())
    }
}

/// `CycleViewSet.retrieve`'s row (`base.py:429-455`).
#[derive(Debug)]
pub(crate) struct CycleRetrieveValue<'a>(pub(crate) &'a CycleRow);

impl DrfSerialize for CycleRetrieveValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = self.0;
        let mut obj = w.begin_object();
        row.write_prefix(&mut obj, ctx)?;
        obj.field("sub_issues", &row.sub_issues.unwrap_or_default())?;
        obj.field_json("logo_props", row.logo_props.as_ref().map(PgJson::from))?;
        obj.field("version", &row.version.unwrap_or(1))?;
        obj.field("is_favorite", &row.is_favorite)?;
        obj.field("total_issues", &row.total_issues)?;
        obj.field("completed_issues", &row.completed_issues)?;
        obj.field("assignee_ids", &uuid_list(&row.assignee_ids))?;
        obj.field("status", &row.status)?;
        obj.field("created_by", &row.created_by)?;
        obj.end();
        Ok(())
    }
}

/// The archive list's row (`archive.py:275-302`).
#[derive(Debug)]
pub(crate) struct CycleArchivedListValue<'a>(pub(crate) &'a CycleRow);

impl DrfSerialize for CycleArchivedListValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = self.0;
        // The archived list has no `user_timezone_converter` call: the raw UTC
        // datetimes go through `JSONRenderer`, so `start_date` etc. render `Z`.
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
        obj.field_dyn("workspace_id", ctx, &DrfUuid(row.workspace_id))?;
        obj.field_dyn("project_id", ctx, &DrfUuid(row.project_id))?;
        obj.field("name", &row.name)?;
        obj.field("description", &row.description)?;
        obj.field("start_date", &row.start_date.map(DrfDateTime))?;
        obj.field("end_date", &row.end_date.map(DrfDateTime))?;
        obj.field_dyn("owned_by_id", ctx, &DrfUuid(row.owned_by_id))?;
        obj.field_json("view_props", row.view_props.as_ref().map(PgJson::from))?;
        obj.field("sort_order", &row.sort_order)?;
        obj.field("external_source", &row.external_source)?;
        obj.field("external_id", &row.external_id)?;
        obj.field_json(
            "progress_snapshot",
            row.progress_snapshot.as_ref().map(PgJson::from),
        )?;
        obj.field("total_issues", &row.total_issues)?;
        obj.field("is_favorite", &row.is_favorite)?;
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
        obj.field("assignee_ids", &uuid_list(&row.assignee_ids))?;
        obj.field("status", &row.status)?;
        obj.field("archived_at", &row.archived_at.map(DrfDateTime))?;
        obj.end();
        Ok(())
    }
}

/// One row of `CYCLE_LIST`/`CYCLE_LIST_CURRENT`.
pub(crate) fn decode_list_row(row: &Row, timezone: Tz) -> Result<CycleRow, ViewError> {
    Ok(CycleRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        project_id: row.try_get(2).map_err(ViewError::from)?,
        name: text(row, 3)?,
        description: text(row, 4)?,
        start_date: row.try_get(5).map_err(ViewError::from)?,
        end_date: row.try_get(6).map_err(ViewError::from)?,
        owned_by_id: row.try_get(7).map_err(ViewError::from)?,
        view_props: opt_json(row, 8)?,
        sort_order: row.try_get(9).map_err(ViewError::from)?,
        external_source: opt_text(row, 10)?,
        external_id: opt_text(row, 11)?,
        progress_snapshot: opt_json(row, 12)?,
        sub_issues: None,
        logo_props: opt_json(row, 13)?,
        completed_estimate_points: None,
        total_estimate_points: None,
        is_favorite: row.try_get(14).map_err(ViewError::from)?,
        total_issues: row.try_get(15).map_err(ViewError::from)?,
        cancelled_issues: row.try_get(17).map_err(ViewError::from)?,
        completed_issues: row.try_get(16).map_err(ViewError::from)?,
        started_issues: None,
        unstarted_issues: None,
        backlog_issues: None,
        assignee_ids: row
            .try_get::<_, Option<Vec<Uuid>>>(18)
            .map_err(ViewError::from)?
            .unwrap_or_default(),
        status: text(row, 19)?,
        version: Some(row.try_get(20).map_err(ViewError::from)?),
        created_by: row.try_get(21).map_err(ViewError::from)?,
        archived_at: None,
        timezone,
    })
}

/// One row of `CYCLE_RETRIEVE`.
fn decode_retrieve_row(row: &Row, timezone: Tz) -> Result<CycleRow, ViewError> {
    Ok(CycleRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        project_id: row.try_get(2).map_err(ViewError::from)?,
        name: text(row, 3)?,
        description: text(row, 4)?,
        start_date: row.try_get(5).map_err(ViewError::from)?,
        end_date: row.try_get(6).map_err(ViewError::from)?,
        owned_by_id: row.try_get(7).map_err(ViewError::from)?,
        view_props: opt_json(row, 8)?,
        sort_order: row.try_get(9).map_err(ViewError::from)?,
        external_source: opt_text(row, 10)?,
        external_id: opt_text(row, 11)?,
        progress_snapshot: opt_json(row, 12)?,
        sub_issues: row.try_get(13).map_err(ViewError::from)?,
        logo_props: opt_json(row, 14)?,
        completed_estimate_points: None,
        total_estimate_points: None,
        is_favorite: row.try_get(16).map_err(ViewError::from)?,
        total_issues: row.try_get(17).map_err(ViewError::from)?,
        cancelled_issues: None,
        completed_issues: row.try_get(18).map_err(ViewError::from)?,
        started_issues: None,
        unstarted_issues: None,
        backlog_issues: None,
        assignee_ids: row
            .try_get::<_, Option<Vec<Uuid>>>(19)
            .map_err(ViewError::from)?
            .unwrap_or_default(),
        status: text(row, 20)?,
        version: Some(row.try_get(15).map_err(ViewError::from)?),
        created_by: row.try_get(21).map_err(ViewError::from)?,
        archived_at: None,
        timezone,
    })
}

/// One row of `CYCLE_ARCHIVED_LIST` (`archive.py:275-302`).
pub(crate) fn decode_archived_list_row(row: &Row, timezone: Tz) -> Result<CycleRow, ViewError> {
    Ok(CycleRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        project_id: row.try_get(2).map_err(ViewError::from)?,
        name: text(row, 3)?,
        description: text(row, 4)?,
        start_date: row.try_get(5).map_err(ViewError::from)?,
        end_date: row.try_get(6).map_err(ViewError::from)?,
        owned_by_id: row.try_get(7).map_err(ViewError::from)?,
        view_props: opt_json(row, 8)?,
        sort_order: row.try_get(9).map_err(ViewError::from)?,
        external_source: opt_text(row, 10)?,
        external_id: opt_text(row, 11)?,
        progress_snapshot: opt_json(row, 12)?,
        sub_issues: None,
        logo_props: None,
        completed_estimate_points: None,
        total_estimate_points: None,
        is_favorite: row.try_get(14).map_err(ViewError::from)?,
        total_issues: row.try_get(13).map_err(ViewError::from)?,
        cancelled_issues: row.try_get(15).map_err(ViewError::from)?,
        completed_issues: row.try_get(16).map_err(ViewError::from)?,
        started_issues: row.try_get(17).map_err(ViewError::from)?,
        unstarted_issues: row.try_get(18).map_err(ViewError::from)?,
        backlog_issues: row.try_get(19).map_err(ViewError::from)?,
        assignee_ids: row
            .try_get::<_, Option<Vec<Uuid>>>(20)
            .map_err(ViewError::from)?
            .unwrap_or_default(),
        status: text(row, 21)?,
        version: None,
        created_by: None,
        archived_at: row.try_get(22).map_err(ViewError::from)?,
        timezone,
    })
}

/// One row of `CYCLE_ARCHIVED_RETRIEVE` (`archive.py:321-353`).
pub(crate) fn decode_archived_retrieve_row(row: &Row, timezone: Tz) -> Result<CycleRow, ViewError> {
    Ok(CycleRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        project_id: row.try_get(2).map_err(ViewError::from)?,
        name: text(row, 3)?,
        description: text(row, 4)?,
        start_date: row.try_get(5).map_err(ViewError::from)?,
        end_date: row.try_get(6).map_err(ViewError::from)?,
        owned_by_id: row.try_get(7).map_err(ViewError::from)?,
        view_props: opt_json(row, 8)?,
        sort_order: row.try_get(9).map_err(ViewError::from)?,
        external_source: opt_text(row, 10)?,
        external_id: opt_text(row, 11)?,
        progress_snapshot: opt_json(row, 12)?,
        sub_issues: row.try_get(13).map_err(ViewError::from)?,
        logo_props: opt_json(row, 14)?,
        completed_estimate_points: row.try_get(15).map_err(ViewError::from)?,
        total_estimate_points: row.try_get(16).map_err(ViewError::from)?,
        is_favorite: row.try_get(17).map_err(ViewError::from)?,
        total_issues: row.try_get(18).map_err(ViewError::from)?,
        cancelled_issues: row.try_get(19).map_err(ViewError::from)?,
        completed_issues: row.try_get(20).map_err(ViewError::from)?,
        started_issues: row.try_get(21).map_err(ViewError::from)?,
        unstarted_issues: row.try_get(22).map_err(ViewError::from)?,
        backlog_issues: row.try_get(23).map_err(ViewError::from)?,
        assignee_ids: row
            .try_get::<_, Option<Vec<Uuid>>>(24)
            .map_err(ViewError::from)?
            .unwrap_or_default(),
        status: text(row, 25)?,
        version: None,
        created_by: row.try_get(26).map_err(ViewError::from)?,
        archived_at: row.try_get(27).map_err(ViewError::from)?,
        timezone,
    })
}

/// A handler for an action the runtime's `BaseViewSet` guard always refuses
/// (`plane/app/views/base.py:103-198`): DRF's generic mixin serves the action
/// and no permission class authorizes it, so the policy answers
/// `405 {"detail": "Method \"GET\" not allowed."}` before a handler runs.
///
/// Registering the action is what makes the guard reachable at all — an
/// unregistered route answers the pipeline's 501 — and the handler below can
/// therefore never execute. It fails loudly if that ever changes instead of
/// pretending to be the mixin.
pub(crate) fn guard_only<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        Err(ViewError::Unknown(format!(
            "{} reached a handler its BaseViewSet guard always refuses",
            ctx.view
        )))
    })
}

/// `CycleViewSet.list` (`base.py:184-269`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let timezone = project_timezone(client, params.project_id).await?;
        let now = Utc::now();
        let current = ctx.query().get_or("cycle_view", "all") == "current";

        if current {
            let rows = fetch_rows(
                client,
                &queries::CYCLE_LIST_CURRENT,
                &[
                    &params.slug,
                    &params.project_id,
                    &params.user_id,
                    &now,
                    &now,
                ],
            )
            .await?;
            if !rows.is_empty() {
                let decoded = rows
                    .iter()
                    .map(|row| decode_list_row(row, timezone))
                    .collect::<Result<Vec<_>, _>>()?;
                let values: Vec<CycleListCurrentValue<'_>> =
                    decoded.iter().map(CycleListCurrentValue).collect();
                return render_list(&values, timezone);
            }
        }

        let rows = fetch_rows(
            client,
            &queries::CYCLE_LIST,
            &[&params.slug, &params.project_id, &params.user_id, &now],
        )
        .await?;
        let decoded = rows
            .iter()
            .map(|row| decode_list_row(row, timezone))
            .collect::<Result<Vec<_>, _>>()?;
        let values: Vec<CycleListValue<'_>> = decoded.iter().map(CycleListValue).collect();
        render_list(&values, timezone)
    })
}

/// `CycleViewSet.retrieve` (`base.py:411-476`).
///
/// A missing (or archived) cycle is `404 {"error": "Cycle not found"}`, then
/// `recent_visited_task.delay(...)` is published with the URL's `pk` as a UUID
/// object and the project/user ids (`TASKS.md` §8.3 line 793).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let timezone = project_timezone(client, params.project_id).await?;
        let now = Utc::now();
        let rows = fetch_rows(
            client,
            &queries::CYCLE_RETRIEVE,
            &[&params.slug, &params.project_id, &params.user_id, &now, &pk],
        )
        .await?;
        let Some(row) = rows.first() else {
            return Ok(error_body(404, "Cycle not found"));
        };
        let decoded = decode_retrieve_row(row, timezone)?;
        let _ = jobs::recent_visited_entity(
            ctx,
            "cycle",
            plane_jobs::celery::args::TaskArg::uuid(pk),
            Some(plane_jobs::celery::args::TaskArg::uuid(params.project_id)),
            plane_jobs::celery::args::TaskArg::uuid(params.user_id),
        )
        .await?;
        render_one(&CycleRetrieveValue(&decoded), timezone)
    })
}

/// `jsonb` text of one top-level member of a JSON object, borrowed from the
/// document. Used to answer a `progress_snapshot`'s `distribution` members
/// without re-encoding them (key order and number lexemes stay Postgres').
pub(crate) fn json_member<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let bytes = text.as_bytes();
    let mut index = skip_ws(bytes, 0);
    if bytes.get(index) != Some(&b'{') {
        return None;
    }
    index += 1;
    loop {
        index = skip_ws(bytes, index);
        if bytes.get(index) != Some(&b'"') {
            return None;
        }
        let (name, next) = json_string_span(bytes, index)?;
        let mut value_start = skip_ws(bytes, next);
        if bytes.get(value_start) != Some(&b':') {
            return None;
        }
        value_start = skip_ws(bytes, value_start + 1);
        let end = json_value_end(bytes, value_start)?;
        if &text[name.0..name.1] == key {
            return Some(&text[value_start..end]);
        }
        index = skip_ws(bytes, end);
        match bytes.get(index) {
            Some(b',') => index += 1,
            Some(b'}') => return None,
            _ => return None,
        }
    }
}

fn skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        index += 1;
    }
    index
}

/// `(content start, content end)` of a JSON string and the index past its
/// closing quote.
fn json_string_span(bytes: &[u8], start: usize) -> Option<((usize, usize), usize)> {
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some(((start + 1, index), index + 1)),
            _ => index += 1,
        }
    }
    None
}

/// The index one past the end of the JSON value starting at `start`.
fn json_value_end(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start)? {
        b'"' => Some(json_string_span(bytes, start)?.1),
        b'{' | b'[' => {
            let mut depth = 0_i32;
            let mut index = start;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => {
                        index = json_string_span(bytes, index)?.1;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(index + 1);
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
            None
        }
        _ => {
            let mut index = start;
            while index < bytes.len()
                && !matches!(
                    bytes[index],
                    b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r'
                )
            {
                index += 1;
            }
            Some(index)
        }
    }
}

/// `CycleUserPropertiesEndpoint.get` (`base.py:651-660`).
///
/// `get_or_create` writes the row on a miss (audit columns from the request
/// user, workspace from the project), which the harness sees as a database
/// diff. A cycle id that violates the `cycle_user_properties.cycle_id` foreign
/// key is Django's `IntegrityError` → `400 {"error": "The payload is not
/// valid"}`.
pub fn user_properties_get<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let statement = client
            .prepare_cached(queries::CYCLE_USER_PROPERTIES_BY_KEY.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_opt(
                &statement,
                &[&params.user_id, &params.project_id, &cycle_id, &params.slug],
            )
            .await
            .map_err(ViewError::from)?;
        let mut created = false;
        let row = match row {
            Some(row) => row,
            None => {
                created = true;
                let insert = client
                    .prepare_cached(queries::CYCLE_USER_PROPERTIES_INSERT.sql)
                    .await
                    .map_err(ViewError::from)?;
                let now = Utc::now();
                let id = Uuid::new_v4();
                match client
                    .query_one(
                        &insert,
                        &[&now, &params.user_id, &id, &params.project_id, &cycle_id],
                    )
                    .await
                {
                    Ok(row) => row,
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(ViewError::Integrity);
                    }
                    Err(error) => return Err(ViewError::from(error)),
                }
            }
        };
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        render_one(&user_properties_value(&row, created)?, timezone)
    })
}

fn is_foreign_key_violation(error: &tokio_postgres::Error) -> bool {
    error.code() == Some(&tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION)
}

/// `CycleUserPropertiesSerializer(cycle_properties).data` (`base.py:659`):
/// 14 fields in declaration order (`SERIALIZERS.md`).
pub(crate) fn user_properties_value(row: &Row, created: bool) -> Result<UserProperties, ViewError> {
    Ok(UserProperties {
        id: row.try_get(5).map_err(ViewError::from)?,
        created_at: row.try_get(0).map_err(ViewError::from)?,
        updated_at: row.try_get(1).map_err(ViewError::from)?,
        deleted_at: row.try_get(4).map_err(ViewError::from)?,
        filters: if created {
            Some(DEFAULT_FILTERS.to_string())
        } else {
            opt_json(row, 10)?.map(JsonText::into_string)
        },
        display_filters: if created {
            Some(DEFAULT_DISPLAY_FILTERS.to_string())
        } else {
            opt_json(row, 11)?.map(JsonText::into_string)
        },
        display_properties: if created {
            Some(DEFAULT_DISPLAY_PROPERTIES.to_string())
        } else {
            opt_json(row, 12)?.map(JsonText::into_string)
        },
        rich_filters: if created {
            Some("{}".to_string())
        } else {
            opt_json(row, 13)?.map(JsonText::into_string)
        },
        created_by: row.try_get(2).map_err(ViewError::from)?,
        updated_by: row.try_get(3).map_err(ViewError::from)?,
        project_id: row.try_get(6).map_err(ViewError::from)?,
        workspace_id: row.try_get(7).map_err(ViewError::from)?,
        cycle_id: row.try_get(8).map_err(ViewError::from)?,
        user_id: row.try_get(9).map_err(ViewError::from)?,
    })
}

/// `CycleUserPropertiesSerializer`'s fields.
#[derive(Debug)]
pub(crate) struct UserProperties {
    pub(crate) id: Uuid,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) deleted_at: Option<DateTime<Utc>>,
    pub(crate) filters: Option<String>,
    pub(crate) display_filters: Option<String>,
    pub(crate) display_properties: Option<String>,
    pub(crate) rich_filters: Option<String>,
    pub(crate) created_by: Option<Uuid>,
    pub(crate) updated_by: Option<Uuid>,
    pub(crate) project_id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) cycle_id: Uuid,
    pub(crate) user_id: Uuid,
}

impl DrfSerialize for UserProperties {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(self.id))?;
        obj.field(
            "created_at",
            &DrfDateTime(self.created_at.with_timezone(ctx.tz())),
        )?;
        obj.field(
            "updated_at",
            &DrfDateTime(self.updated_at.with_timezone(ctx.tz())),
        )?;
        obj.field(
            "deleted_at",
            &self
                .deleted_at
                .map(|value| DrfDateTime(value.with_timezone(ctx.tz()))),
        )?;
        obj.field_json("filters", self.filters.as_deref().map(PgJson::new))?;
        obj.field_json(
            "display_filters",
            self.display_filters.as_deref().map(PgJson::new),
        )?;
        obj.field_json(
            "display_properties",
            self.display_properties.as_deref().map(PgJson::new),
        )?;
        obj.field_json(
            "rich_filters",
            self.rich_filters.as_deref().map(PgJson::new),
        )?;
        obj.field("created_by", &self.created_by)?;
        obj.field("updated_by", &self.updated_by)?;
        obj.field_dyn("project", ctx, &DrfUuid(self.project_id))?;
        obj.field_dyn("workspace", ctx, &DrfUuid(self.workspace_id))?;
        obj.field_dyn("cycle", ctx, &DrfUuid(self.cycle_id))?;
        obj.field_dyn("user", ctx, &DrfUuid(self.user_id))?;
        obj.end();
        Ok(())
    }
}

/// `CycleProgressEndpoint.get` (`base.py:663-788`).
///
/// With a `progress_snapshot` the six issue counts come from it; otherwise they
/// are six `COUNT(*)`s. The estimate sums are always aggregated. Every `or 0`
/// in the Python turns a `NULL` **or a zero float** into the int `0`, which is
/// why the estimate values are chosen with that rule here.
pub fn progress<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let row = {
            let statement = client
                .prepare_cached(queries::CYCLE_PROGRESS_ROW.sql)
                .await
                .map_err(ViewError::from)?;
            client
                .query_opt(&statement, &[&params.slug, &cycle_id, &params.project_id])
                .await
                .map_err(ViewError::from)?
        };
        let Some(row) = row else {
            return Ok(error_body(404, "Cycle not found"));
        };
        let snapshot: Option<JsonText> = row.try_get(0).map_err(ViewError::from)?;
        let snapshot = snapshot
            .as_ref()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text.as_str()).ok())
            .filter(|value| python_truthy(value.clone()));

        let estimates = {
            let statement = client
                .prepare_cached(queries::CYCLE_PROGRESS_ESTIMATES.sql)
                .await
                .map_err(ViewError::from)?;
            client
                .query_one(&statement, &[&cycle_id, &params.slug, &params.project_id])
                .await
                .map_err(ViewError::from)?
        };
        let sums: [Option<f64>; 6] = [
            estimates.try_get(0).map_err(ViewError::from)?,
            estimates.try_get(1).map_err(ViewError::from)?,
            estimates.try_get(2).map_err(ViewError::from)?,
            estimates.try_get(3).map_err(ViewError::from)?,
            estimates.try_get(4).map_err(ViewError::from)?,
            estimates.try_get(5).map_err(ViewError::from)?,
        ];

        let counts = match &snapshot {
            Some(value) => {
                let get = |key: &str| -> i64 {
                    value
                        .get(key)
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0)
                };
                [
                    get("backlog_issues"),
                    get("unstarted_issues"),
                    get("started_issues"),
                    get("cancelled_issues"),
                    get("completed_issues"),
                    get("total_issues"),
                ]
            }
            None => {
                let statement = client
                    .prepare_cached(queries::CYCLE_PROGRESS_COUNTS.sql)
                    .await
                    .map_err(ViewError::from)?;
                let row = client
                    .query_one(&statement, &[&cycle_id, &params.slug, &params.project_id])
                    .await
                    .map_err(ViewError::from)?;
                [
                    row.try_get(0).map_err(ViewError::from)?,
                    row.try_get(1).map_err(ViewError::from)?,
                    row.try_get(2).map_err(ViewError::from)?,
                    row.try_get(3).map_err(ViewError::from)?,
                    row.try_get(4).map_err(ViewError::from)?,
                    row.try_get(5).map_err(ViewError::from)?,
                ]
            }
        };

        let estimate_value = |value: Option<f64>| match value {
            Some(value) if value != 0.0 => DrfValue::Float(value),
            _ => DrfValue::Int(0),
        };
        let body = ProgressBody {
            estimates: [
                estimate_value(sums[0]),
                estimate_value(sums[1]),
                estimate_value(sums[2]),
                estimate_value(sums[3]),
                estimate_value(sums[4]),
                DrfValue::Float(sums[5].unwrap_or(0.0)),
            ],
            counts,
        };
        render_one(&body, ctx.timezone.copied().unwrap_or(chrono_tz::UTC))
    })
}

/// `CycleProgressEndpoint`'s response keys, in the Python's order.
#[derive(Debug)]
struct ProgressBody {
    estimates: [DrfValue<'static>; 6],
    counts: [i64; 6],
}

impl DrfSerialize for ProgressBody {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        let estimate_keys = [
            "backlog_estimate_points",
            "unstarted_estimate_points",
            "started_estimate_points",
            "cancelled_estimate_points",
            "completed_estimate_points",
            "total_estimate_points",
        ];
        for (key, value) in estimate_keys.iter().zip(self.estimates.iter()) {
            obj.field_value(key, ctx, value)?;
        }
        let count_order: [(&str, usize); 6] = [
            ("backlog_issues", 0),
            ("total_issues", 5),
            ("completed_issues", 4),
            ("cancelled_issues", 3),
            ("started_issues", 2),
            ("unstarted_issues", 1),
        ];
        for (key, index) in count_order {
            obj.field(key, &self.counts[index])?;
        }
        obj.end();
        Ok(())
    }
}

/// The `cycle` row `CycleAnalyticsEndpoint` reads (`base.py:795-810`).
struct AnalyticsCycle {
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
    progress_snapshot: Option<JsonText>,
    total_issues: i64,
}

/// `CycleAnalyticsEndpoint.get` (`base.py:791-1054`).
///
/// Order of the Python's branches, all of which decide the body:
///
/// 1. a missing cycle is an `AttributeError` (500), not a 404;
/// 2. a cycle without both dates is `400 {"error": "Cycle has no start or end
///    date"}`;
/// 3. a non-empty `progress_snapshot` answers
///    `{labels, assignees, completion_chart}` from its `distribution` key —
///    note the key order differs from the computed branch, which is
///    `{assignees, labels, completion_chart}`;
/// 4. `type=points` only computes when the project's estimate is of type
///    `points`; any other `type` leaves the empty lists.
pub fn analytics<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let analytic_type = ctx.query().get_or("type", "issues");
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);

        let cycle = {
            let statement = client
                .prepare_cached(queries::CYCLE_ANALYTICS_ROW.sql)
                .await
                .map_err(ViewError::from)?;
            client
                .query_opt(&statement, &[&params.slug, &cycle_id, &params.project_id])
                .await
                .map_err(ViewError::from)?
        };
        let Some(row) = cycle else {
            // `cycle.start_date` on `None` is an `AttributeError`.
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'start_date'".to_string(),
            ));
        };
        let cycle = AnalyticsCycle {
            start_date: row.try_get(1).map_err(ViewError::from)?,
            end_date: row.try_get(2).map_err(ViewError::from)?,
            progress_snapshot: row.try_get(3).map_err(ViewError::from)?,
            total_issues: row.try_get(4).map_err(ViewError::from)?,
        };

        if cycle.start_date.is_none() || cycle.end_date.is_none() {
            return Ok(error_body(400, "Cycle has no start or end date"));
        }

        let snapshot = cycle.progress_snapshot.as_ref().map(JsonText::as_str);
        if snapshot.is_some_and(snapshot_is_truthy) {
            let snapshot = snapshot.unwrap_or_default();
            let distribution = json_member(snapshot, "distribution").unwrap_or("{}");
            let labels = json_member(distribution, "labels").unwrap_or("[]");
            let assignees = json_member(distribution, "assignees").unwrap_or("[]");
            let chart = json_member(distribution, "completion_chart").unwrap_or("{}");
            return render_one(
                &SnapshotAnalytics {
                    labels,
                    assignees,
                    chart,
                },
                timezone,
            );
        }

        let estimate_type = estimate_type_exists(client, &params).await?;
        let (Some(start), Some(end)) = (cycle.start_date, cycle.end_date) else {
            return Ok(error_body(400, "Cycle has no start or end date"));
        };

        let mut assignees: Vec<Distribution> = Vec::new();
        let mut labels: Vec<Distribution> = Vec::new();
        let mut completion_chart = Chart(Vec::new());

        if analytic_type == "points" && estimate_type {
            assignees = distribute(
                client,
                &queries::CYCLE_ANALYTICS_ASSIGNEES_POINTS,
                &[&cycle_id, &params.project_id, &params.slug],
                DistributionKind::AnalyticsAssigneesPoints,
            )
            .await?;
            labels = distribute(
                client,
                &queries::CYCLE_ANALYTICS_LABELS_POINTS,
                &[&cycle_id, &params.project_id, &params.slug],
                DistributionKind::AnalyticsLabelsPoints,
            )
            .await?;
            completion_chart =
                points_chart(client, &params, cycle_id, start, end, timezone).await?;
        }

        if analytic_type == "issues" {
            assignees = distribute(
                client,
                &queries::CYCLE_ANALYTICS_ASSIGNEES_ISSUES,
                &[&cycle_id, &params.project_id, &params.slug],
                DistributionKind::AnalyticsAssigneesIssues,
            )
            .await?;
            labels = distribute(
                client,
                &queries::CYCLE_ANALYTICS_LABELS_ISSUES,
                &[&cycle_id, &params.project_id, &params.slug],
                DistributionKind::AnalyticsLabelsIssues,
            )
            .await?;
            completion_chart = issues_chart(
                client,
                &params,
                cycle_id,
                start,
                end,
                cycle.total_issues,
                timezone,
            )
            .await?;
        }

        render_one(
            &ComputedAnalytics {
                assignees,
                labels,
                completion_chart,
            },
            timezone,
        )
    })
}

fn snapshot_is_truthy(text: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => python_truthy(value),
        Err(_) => true,
    }
}

pub(crate) fn python_truthy(value: serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(value) => value,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        serde_json::Value::String(value) => !value.is_empty(),
        serde_json::Value::Array(values) => !values.is_empty(),
        serde_json::Value::Object(entries) => !entries.is_empty(),
    }
}

/// The branch that answers a cycle's `progress_snapshot` `distribution`
/// verbatim (`base.py:826-835`), keys in the Python's order.
#[derive(Debug)]
struct SnapshotAnalytics<'a> {
    labels: &'a str,
    assignees: &'a str,
    chart: &'a str,
}

impl DrfSerialize for SnapshotAnalytics<'_> {
    fn drf_write(&self, w: &mut DrfWriter, _ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_json("labels", Some(PgJson::new(self.labels)))?;
        obj.field_json("assignees", Some(PgJson::new(self.assignees)))?;
        obj.field_json("completion_chart", Some(PgJson::new(self.chart)))?;
        obj.end();
        Ok(())
    }
}

/// The computed branch's body (`base.py:1047-1054`).
#[derive(Debug)]
struct ComputedAnalytics {
    assignees: Vec<Distribution>,
    labels: Vec<Distribution>,
    completion_chart: Chart,
}

impl DrfSerialize for ComputedAnalytics {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_nested_dyn("assignees", ctx, Some(&DistributionList(&self.assignees)))?;
        obj.field_nested_dyn("labels", ctx, Some(&DistributionList(&self.labels)))?;
        obj.field_nested_dyn("completion_chart", ctx, Some(&self.completion_chart))?;
        obj.end();
        Ok(())
    }
}

/// `many=True` for a distribution list.
#[derive(Debug)]
pub(crate) struct DistributionList<'a>(pub(crate) &'a [Distribution]);

/// `{assignees, labels, completion_chart}` — the shape both the archived cycle
/// and the module detail append.
#[derive(Debug)]
pub(crate) struct DistributionGroup<'a> {
    pub(crate) assignees: &'a [Distribution],
    pub(crate) labels: &'a [Distribution],
    /// `completion_chart` is omitted when the Python never assigns it: the
    /// module detail only adds it when the module has both dates
    /// (`module/base.py:527-537`).
    pub(crate) chart: Option<&'a Chart>,
}

impl DrfSerialize for DistributionGroup<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_nested_dyn("assignees", ctx, Some(&DistributionList(self.assignees)))?;
        obj.field_nested_dyn("labels", ctx, Some(&DistributionList(self.labels)))?;
        if let Some(chart) = self.chart {
            obj.field_nested_dyn("completion_chart", ctx, Some(chart))?;
        }
        obj.end();
        Ok(())
    }
}

impl DrfSerialize for DistributionList<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut array = w.begin_array();
        for row in self.0.iter() {
            array.nested(ctx, row)?;
        }
        array.end();
        Ok(())
    }
}

/// The literal JSON of `cycle.py:14-68`'s / `module.py:14-55`'s model
/// defaults, in Python dict order.
///
/// `get_or_create` returns the *in-memory* instance, so DRF renders the
/// default dicts in insertion order; a row read back from Postgres renders its
/// `jsonb` text order instead (which differs). The created-row response must
/// use these.
pub(crate) const DEFAULT_FILTERS: &str = "{\"priority\": null, \"state\": null, \"state_group\": null, \"assignees\": null, \"created_by\": null, \"labels\": null, \"start_date\": null, \"target_date\": null, \"subscriber\": null}";
pub(crate) const DEFAULT_DISPLAY_FILTERS: &str = "{\"group_by\": null, \"order_by\": \"-created_at\", \"type\": null, \"sub_issue\": true, \"show_empty_groups\": true, \"layout\": \"list\", \"calendar_date_range\": \"\"}";
pub(crate) const DEFAULT_DISPLAY_PROPERTIES: &str = "{\"assignee\": true, \"attachment_count\": true, \"created_on\": true, \"due_date\": true, \"estimate\": true, \"key\": true, \"labels\": true, \"link\": true, \"priority\": true, \"start_date\": true, \"state\": true, \"sub_issue_count\": true, \"updated_on\": true}";

/// The `estimate_type` guard of `burndown_plot`/analytics/archive
/// (`analytics_plot.py:127-132`).
pub(crate) async fn estimate_type_exists(
    client: &Client,
    params: &Params,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::CYCLE_ESTIMATE_TYPE_EXISTS.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_one(&statement, &[&params.project_id, &params.slug])
        .await
        .map_err(ViewError::from)?;
    row.try_get(0).map_err(ViewError::from)
}

/// Which response shape a distribution statement produces: the JSON keys and
/// their order differ between the analytics endpoints (`display_name` first)
/// and the archive/module ones (`first_name`, `last_name` first), and between
/// counts and estimate sums.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DistributionKind {
    AnalyticsAssigneesIssues,
    AnalyticsLabelsIssues,
    AnalyticsAssigneesPoints,
    AnalyticsLabelsPoints,
    ArchiveAssigneesIssues,
    ArchiveLabelsIssues,
    ArchiveAssigneesPoints,
    ArchiveLabelsPoints,
    /// The module detail's estimate distribution keeps the names
    /// (`module/base.py:438-468`), unlike the cycle archive's.
    ModuleAssigneesPoints,
}

impl DistributionKind {
    /// The JSON keys, in the order the Python's `.values()`/`.annotate()` calls
    /// produce them.
    pub(crate) fn keys(self) -> &'static [&'static str] {
        match self {
            Self::AnalyticsAssigneesIssues => &[
                "display_name",
                "assignee_id",
                "avatar_url",
                "total_issues",
                "completed_issues",
                "pending_issues",
            ],
            Self::AnalyticsLabelsIssues => &[
                "label_name",
                "color",
                "label_id",
                "total_issues",
                "completed_issues",
                "pending_issues",
            ],
            Self::AnalyticsLabelsPoints => &[
                "label_name",
                "color",
                "label_id",
                "total_estimates",
                "completed_estimates",
                "pending_estimates",
            ],
            Self::AnalyticsAssigneesPoints | Self::ArchiveAssigneesPoints => &[
                "display_name",
                "assignee_id",
                "avatar_url",
                "total_estimates",
                "completed_estimates",
                "pending_estimates",
            ],
            Self::ArchiveAssigneesIssues => &[
                "first_name",
                "last_name",
                "assignee_id",
                "avatar_url",
                "display_name",
                "total_issues",
                "completed_issues",
                "pending_issues",
            ],
            Self::ArchiveLabelsIssues => &[
                "label_name",
                "color",
                "label_id",
                "total_issues",
                "completed_issues",
                "pending_issues",
            ],
            Self::ModuleAssigneesPoints => &[
                "first_name",
                "last_name",
                "assignee_id",
                "avatar_url",
                "display_name",
                "total_estimates",
                "completed_estimates",
                "pending_estimates",
            ],
            Self::ArchiveLabelsPoints => &[
                "label_name",
                "color",
                "label_id",
                "total_estimates",
                "completed_estimates",
                "pending_estimates",
            ],
        }
    }

    /// The label variants of the two families share their keys, so the metric
    /// suffix decides the value type.
    fn is_points(self) -> bool {
        matches!(
            self,
            Self::AnalyticsAssigneesPoints
                | Self::AnalyticsLabelsPoints
                | Self::ArchiveAssigneesPoints
                | Self::ArchiveLabelsPoints
                | Self::ModuleAssigneesPoints
        )
    }

    fn has_names(self) -> bool {
        matches!(
            self,
            Self::ArchiveAssigneesIssues | Self::ModuleAssigneesPoints
        )
    }
}

/// One distribution row: the values, in the order [`DistributionKind::keys`]
/// names them.
#[derive(Debug)]
pub(crate) struct Distribution {
    kind: DistributionKind,
    values: Vec<DistValue>,
}

/// One distribution column value.
///
/// A plain enum rather than a [`DrfValue`]: `DrfValue::Nested` holds a
/// `&dyn DrfSerialize`, which is not `Send`, and these rows are built between
/// `await`s (the request future must stay `Send`). Converted in `drf_write`.
#[derive(Debug, Clone)]
pub(crate) enum DistValue {
    Null,
    Int(i64),
    Float(f64),
    Uuid(Uuid),
    String(Option<String>),
}

impl DistValue {
    fn to_drf(&self) -> DrfValue<'static> {
        match self {
            Self::Null => DrfValue::Null,
            Self::Int(value) => DrfValue::Int(*value),
            Self::Float(value) => DrfValue::Float(*value),
            Self::Uuid(value) => DrfValue::Uuid(*value),
            Self::String(value) => match value {
                Some(value) => DrfValue::String(value.clone()),
                None => DrfValue::Null,
            },
        }
    }
}

impl DrfSerialize for Distribution {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        for (key, value) in self.kind.keys().iter().zip(self.values.iter()) {
            obj.field_value(key, ctx, &value.to_drf())?;
        }
        obj.end();
        Ok(())
    }
}

pub(crate) async fn distribute(
    client: &Client,
    query: &plane_db::Query,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    kind: DistributionKind,
) -> Result<Vec<Distribution>, ViewError> {
    let statement = client
        .prepare_cached(query.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, params)
        .await
        .map_err(ViewError::from)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(distribution_row(row, kind)?);
    }
    Ok(out)
}

fn distribution_row(row: &Row, kind: DistributionKind) -> Result<Distribution, ViewError> {
    let text = |index: usize| -> Result<DistValue, ViewError> {
        Ok(DistValue::String(opt_text(row, index)?))
    };
    let uuid = |index: usize| -> Result<DistValue, ViewError> {
        let value: Option<Uuid> = row.try_get(index).map_err(ViewError::from)?;
        Ok(match value {
            Some(value) => DistValue::Uuid(value),
            None => DistValue::Null,
        })
    };
    let int = |index: usize| -> Result<DistValue, ViewError> {
        Ok(DistValue::Int(row.try_get(index).map_err(ViewError::from)?))
    };
    let float = |index: usize| -> Result<DistValue, ViewError> {
        let value: Option<f64> = row.try_get(index).map_err(ViewError::from)?;
        Ok(match value {
            Some(value) => DistValue::Float(value),
            None => DistValue::Null,
        })
    };
    let mut values = Vec::with_capacity(kind.keys().len());
    let index = if kind.has_names() {
        values.push(text(0)?);
        values.push(text(1)?);
        values.push(uuid(2)?);
        values.push(text(3)?);
        values.push(text(4)?);
        5
    } else if kind.keys()[0] == "label_name" {
        values.push(text(0)?);
        values.push(text(1)?);
        values.push(uuid(2)?);
        3
    } else {
        values.push(text(0)?);
        values.push(uuid(1)?);
        values.push(text(2)?);
        3
    };
    for slot in index..kind.keys().len() {
        values.push(if kind.is_points() {
            float(slot)?
        } else {
            int(slot)?
        });
    }
    Ok(Distribution { kind, values })
}

/// `burndown_plot(plot_type="points")` for a cycle (`analytics_plot.py:123-265`).
pub(crate) async fn points_chart(
    client: &Client,
    params: &Params,
    cycle_id: Uuid,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    timezone: Tz,
) -> Result<Chart, ViewError> {
    // `sum([float(value) for value in …])` is the int `0` for an empty list
    // and a float otherwise, which decides whether the chart's numbers render
    // `0` or `0.0`.
    let (total_estimates, total_is_float) = {
        let statement = client
            .prepare_cached(queries::CYCLE_BURNDOWN_TOTAL_ESTIMATES.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&cycle_id, &params.project_id, &params.slug])
            .await
            .map_err(ViewError::from)?;
        let mut sum = 0.0_f64;
        for row in &rows {
            let value: f64 = row.try_get(0).map_err(ViewError::from)?;
            sum += value;
        }
        (sum, !rows.is_empty())
    };
    let distribution = {
        let statement = client
            .prepare_cached(queries::CYCLE_BURNDOWN_COMPLETED_POINTS.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(
                &statement,
                &[
                    &cycle_id,
                    &params.project_id,
                    &params.slug,
                    &timezone.name(),
                ],
            )
            .await
            .map_err(ViewError::from)?;
        let mut out: Vec<(chrono::NaiveDate, f64)> = Vec::with_capacity(rows.len());
        for row in &rows {
            let date: Option<chrono::NaiveDate> = row.try_get(0).map_err(ViewError::from)?;
            let value: f64 = row.try_get(1).map_err(ViewError::from)?;
            if let Some(date) = date {
                out.push((date, value));
            }
        }
        out
    };
    Ok(date_chart(
        start.date_naive(),
        end.date_naive(),
        ChartKind::Points {
            total: total_estimates,
            is_float: total_is_float,
        },
        Some(&distribution),
        None,
    ))
}

/// `burndown_plot(plot_type="issues")` for a cycle.
pub(crate) async fn issues_chart(
    client: &Client,
    params: &Params,
    cycle_id: Uuid,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    total_issues: i64,
    timezone: Tz,
) -> Result<Chart, ViewError> {
    let distribution = {
        let statement = client
            .prepare_cached(queries::CYCLE_BURNDOWN_COMPLETED_ISSUES.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(
                &statement,
                &[
                    &cycle_id,
                    &params.project_id,
                    &params.slug,
                    &timezone.name(),
                ],
            )
            .await
            .map_err(ViewError::from)?;
        let mut out: Vec<(chrono::NaiveDate, i64)> = Vec::with_capacity(rows.len());
        for row in &rows {
            let date: Option<chrono::NaiveDate> = row.try_get(0).map_err(ViewError::from)?;
            let count: i64 = row.try_get(1).map_err(ViewError::from)?;
            if let Some(date) = date {
                out.push((date, count));
            }
        }
        out
    };
    Ok(date_chart(
        start.date_naive(),
        end.date_naive(),
        ChartKind::Issues(total_issues),
        None,
        Some(&distribution),
    ))
}

pub(crate) enum ChartKind {
    /// `total_estimate_points`: Python's `sum` of an empty list is the int `0`.
    Points {
        total: f64,
        is_float: bool,
    },
    Issues(i64),
}

/// The chart `burndown_plot` builds: one key per date of the range
/// (inclusive), the cumulative pending work up to that date, and `null` for a
/// date after `timezone.now().date()` — Django's `date()` on an aware
/// `datetime` is the **UTC** date, not the active timezone's.
#[derive(Debug)]
pub(crate) struct Chart(pub(crate) Vec<(String, ChartValue)>);

/// One chart value (`burndown_plot` writes floats, ints and `None`).
#[derive(Debug, Clone, Copy)]
pub(crate) enum ChartValue {
    Null,
    Int(i64),
    Float(f64),
}

impl DrfSerialize for Chart {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        for (key, value) in &self.0 {
            let value = match value {
                ChartValue::Null => DrfValue::Null,
                ChartValue::Int(value) => DrfValue::Int(*value),
                ChartValue::Float(value) => DrfValue::Float(*value),
            };
            obj.field_value(key, ctx, &value)?;
        }
        obj.end();
        Ok(())
    }
}

pub(crate) fn date_chart(
    first: chrono::NaiveDate,
    last: chrono::NaiveDate,
    kind: ChartKind,
    points: Option<&[(chrono::NaiveDate, f64)]>,
    issues: Option<&[(chrono::NaiveDate, i64)]>,
) -> Chart {
    let today = Utc::now().date_naive();
    let mut entries = Vec::new();
    if last >= first {
        let mut date = first;
        loop {
            let pending = match kind {
                ChartKind::Points { total, is_float } => {
                    let mut completed = 0.0_f64;
                    let mut completed_is_float = false;
                    for (day, value) in points.unwrap_or_default() {
                        if *day <= date {
                            completed += *value;
                            completed_is_float = true;
                        }
                    }
                    if is_float || completed_is_float {
                        ChartValue::Float(total - completed)
                    } else {
                        ChartValue::Int(total as i64)
                    }
                }
                ChartKind::Issues(total) => {
                    let completed: i64 = issues
                        .unwrap_or_default()
                        .iter()
                        .filter(|(day, _)| *day <= date)
                        .map(|(_, value)| *value)
                        .sum();
                    ChartValue::Int(total - completed)
                }
            };
            let value = if date > today {
                ChartValue::Null
            } else {
                pending
            };
            entries.push((date.to_string(), value));
            if date >= last {
                break;
            }
            match date.succ_opt() {
                Some(next) => date = next,
                None => break,
            }
        }
    }
    Chart(entries)
}
