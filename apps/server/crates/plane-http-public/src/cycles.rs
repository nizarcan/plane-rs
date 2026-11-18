// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Cycles on `/api/v1/` (routes 279–287; `plane/api/views/cycle.py`).
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | 279 `pr/cycles/` | GET, POST | `CycleListCreateAPIEndpoint` (`cycle.py:82-357`) |
//! | 280 `pr/cycles-lite/` | GET | `CycleListLiteAPIEndpoint` (`cycle.py:360-405`) |
//! | 281 `pr/cycles/{pk}/` | GET, PATCH, DELETE | `CycleDetailAPIEndpoint` (`cycle.py:408-651`) |
//! | 282 `pr/cycles/{cycle_id}/cycle-issues/` | GET, POST | `CycleIssueListCreateAPIEndpoint` (`cycle.py:844-1070`) |
//! | 283 `pr/cycles/{cycle_id}/cycle-issues/{issue_id}/` | GET, DELETE | `CycleIssueDetailAPIEndpoint` (`cycle.py:1073-1178`) |
//! | 284 `pr/cycles/{cycle_id}/transfer-issues/` | POST | `TransferCycleIssueAPIEndpoint` (`cycle.py:1181-1274`) |
//! | 285 `pr/cycles/{cycle_id}/archive/` | POST | `CycleArchiveUnarchiveAPIEndpoint.post` (`cycle.py:801-821`) |
//! | 286 `pr/archived-cycles/` | GET | the same view's `get` (`cycle.py:779-789`) |
//! | 287 `pr/archived-cycles/{cycle_id}/unarchive/` | DELETE | the same view's `delete` (`cycle.py:832-841`) |
//!
//! ## Bugs ported as they are
//!
//! * **`CycleDetail.patch` ignores the narrowed body** (`cycle.py:559-573`):
//!   for a completed cycle the view builds `request_data = {"sort_order": …}`
//!   (or 400s without it) but then hands `request.data` to the serializer, so
//!   every field of the body is applied, not just `sort_order`.
//! * **`CycleCreateSerializer` never sets the serializer's date timezone**
//!   (`serializers/cycle.py:30-36` reads `context["project"]`, which the view
//!   does not pass, `cycle.py:310-312`), so a date is interpreted in the
//!   request's active timezone and `convert_to_utc` then re-localizes its
//!   *date* in the project's timezone.
//! * **`cycle_view=current` answers a plain array** (`cycle.py:203-212`) while
//!   every other value paginates.
//! * `cycle_view` compares the raw string (`:200`): `Current` is `all`.
//! * The list/archived-list/detail querysets never annotate the three estimate
//!   sums, so `CycleSerializer` skips those keys there; the archived list does
//!   annotate them (`:737-759`) and renders `null` when the sum is `NULL`.
//! * `?order_by=` is ignored by 279/281/286 (they read `self.kwargs`), and
//!   honoured by 280 through `sanitize_order_by` (`:392-398`).
//! * The archived list and the detail filter `archived_at IS NULL`; the PATCH,
//!   DELETE and archive lookups do not.
//!
//! ## Not ported yet (the routes stay on Django)
//!
//! `POST pr/cycles/` (create), `PATCH pr/cycles/{pk}/`, `POST …/cycle-issues/`
//! (bulk add), `DELETE …/cycle-issues/{issue_id}/` and
//! `POST …/transfer-issues/` have no registered handler: their placeholders
//! below answer the pipeline's explicit not-implemented error and
//! [`register`] does not bind them, so the coexistence proxy keeps serving
//! them from Django. The SQL for the create/PATCH half is already catalogued
//! ([`cycles_queries`]) — the next step is the two serializers' `validate`/
//! `create`/`update` plus `convert_to_utc`, then a write scenario.
//!
//! ## Reuse
//!
//! The list filters (membership, six status counts) are this surface's own SQL
//! ([`cycles_queries`]). The transfer endpoint reuses the app surface's
//! `transfer_cycle_issues` kernel ([`plane_http_app::cycles::write_transfer`])
//! after the two v1-only pre-checks, and the cycle-issue lists reuse
//! [`crate::issues`]' serializer pipeline.

use chrono::{DateTime, Utc};
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_query::paginator::OffsetWindow;
use plane_query::sql::{Frag, Value};
use plane_serializers::generated::api_cycle::{
    CycleIssueSerializer, CycleLiteSerializer, CycleSerializer,
};
use plane_serializers::{
    DrfError, DrfSerialize, DrfValue, DrfWriter, Expansion, Expansions, FieldValue, PgJson,
    RenderCtx,
};
use uuid::Uuid;

use crate::common::page;
use crate::common::render::RequestRender;
use crate::cycles_queries as queries;
use crate::issues;

/// The URL kwargs and user id every route of this file needs.
pub(crate) struct Params {
    pub(crate) slug: String,
    pub(crate) project_id: Uuid,
    pub(crate) user_id: Uuid,
}

pub(crate) fn params(ctx: &ViewCtx<'_>) -> Result<Params, ViewError> {
    Ok(Params {
        slug: ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned(),
        project_id: uuid_kwarg(ctx, "project_id")?,
        user_id: issues::user_id(ctx)?,
    })
}

pub(crate) fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    let value = ctx.kwarg(name).ok_or(ViewError::Key)?;
    Uuid::parse_str(value).map_err(|_| ViewError::Validation)
}

pub(crate) fn client<'a>(ctx: &'a ViewCtx<'a>) -> Result<&'a deadpool_postgres::Client, ViewError> {
    plane_http_app::common::client(ctx)
}

/// `Response({"error": …}, status)` — [`plane_http_app::write::error_body`].
pub(crate) fn error_body(status: u16, message: &str) -> ViewResponse {
    plane_http_app::write::error_body(status, message)
}

pub(crate) fn base_host(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    issues::base_host(ctx)
}

/// A Postgres failure with its message and SQLSTATE, which `Display` for
/// `tokio_postgres::Error` drops ("db error").
pub(crate) fn db_error(error: tokio_postgres::Error) -> ViewError {
    ViewError::Unknown(match error.as_db_error() {
        Some(db) => format!("{} (SQLSTATE {})", db.message(), db.code().code()),
        None => error.to_string(),
    })
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.cycle.CycleListCreateAPIEndpoint")?
        .method(http::Method::GET, list)?;
    registry
        .api_view("plane.api.views.cycle.CycleListLiteAPIEndpoint")?
        .method(http::Method::GET, lite_list)?;
    registry
        .api_view("plane.api.views.cycle.CycleDetailAPIEndpoint")?
        .method(http::Method::GET, detail)?
        .method(http::Method::DELETE, delete)?;
    registry
        .api_view("plane.api.views.cycle.CycleArchiveUnarchiveAPIEndpoint")?
        .method(http::Method::GET, archived_list)?
        .method(http::Method::POST, archive_post)?
        .method(http::Method::DELETE, unarchive)?;
    registry
        .api_view("plane.api.views.cycle.CycleIssueListCreateAPIEndpoint")?
        .method(http::Method::GET, issue_list)?;
    registry
        .api_view("plane.api.views.cycle.CycleIssueDetailAPIEndpoint")?
        .method(http::Method::GET, issue_detail)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One `cycles` row plus the annotations a statement selected.
#[derive(Debug)]
pub(crate) struct CycleRow {
    pub(crate) id: Uuid,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) deleted_at: Option<DateTime<Utc>>,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) start_date: Option<DateTime<Utc>>,
    pub(crate) end_date: Option<DateTime<Utc>>,
    pub(crate) view_props: Option<JsonText>,
    pub(crate) sort_order: f64,
    pub(crate) external_source: Option<String>,
    pub(crate) external_id: Option<String>,
    pub(crate) progress_snapshot: Option<JsonText>,
    pub(crate) archived_at: Option<DateTime<Utc>>,
    pub(crate) logo_props: Option<JsonText>,
    pub(crate) timezone: String,
    pub(crate) version: i64,
    pub(crate) created_by: Option<Uuid>,
    pub(crate) updated_by: Option<Uuid>,
    pub(crate) project: Uuid,
    pub(crate) workspace: Uuid,
    pub(crate) owned_by: Uuid,
    /// `total, cancelled, completed, started, unstarted, backlog` — `Absent`
    /// when the statement did not aggregate.
    pub(crate) counts: [FieldValue<i64>; 6],
    /// `total, completed, started` estimates — `Absent` unless the archived
    /// list selected them.
    pub(crate) estimates: [FieldValue<f64>; 3],
}

fn opt_text(row: &tokio_postgres::Row, index: usize) -> Result<Option<String>, ViewError> {
    row.try_get(index).map_err(db_error)
}

fn opt_json(row: &tokio_postgres::Row, index: usize) -> Result<Option<JsonText>, ViewError> {
    row.try_get(index).map_err(db_error)
}

impl CycleRow {
    fn from_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(db_error)?,
            created_at: row.try_get(1).map_err(db_error)?,
            updated_at: row.try_get(2).map_err(db_error)?,
            deleted_at: row.try_get(3).map_err(db_error)?,
            name: row.try_get(4).map_err(db_error)?,
            description: row.try_get(5).map_err(db_error)?,
            start_date: row.try_get(6).map_err(db_error)?,
            end_date: row.try_get(7).map_err(db_error)?,
            view_props: opt_json(row, 8)?,
            sort_order: row.try_get(9).map_err(db_error)?,
            external_source: opt_text(row, 10)?,
            external_id: opt_text(row, 11)?,
            progress_snapshot: opt_json(row, 12)?,
            archived_at: row.try_get(13).map_err(db_error)?,
            logo_props: opt_json(row, 14)?,
            timezone: row.try_get(15).map_err(db_error)?,
            version: i64::from(row.try_get::<_, i32>(16).map_err(db_error)?),
            created_by: row.try_get(17).map_err(db_error)?,
            updated_by: row.try_get(18).map_err(db_error)?,
            project: row.try_get(19).map_err(db_error)?,
            workspace: row.try_get(20).map_err(db_error)?,
            owned_by: row.try_get(21).map_err(db_error)?,
            counts: [
                FieldValue::Absent,
                FieldValue::Absent,
                FieldValue::Absent,
                FieldValue::Absent,
                FieldValue::Absent,
                FieldValue::Absent,
            ],
            estimates: [FieldValue::Absent, FieldValue::Absent, FieldValue::Absent],
        })
    }

    /// A read row: the 22 model columns then the six counts.
    fn from_read_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        let mut cycle = Self::from_row(row)?;
        cycle.counts = [
            FieldValue::Value(row.try_get(22).map_err(db_error)?),
            FieldValue::Value(row.try_get(23).map_err(db_error)?),
            FieldValue::Value(row.try_get(24).map_err(db_error)?),
            FieldValue::Value(row.try_get(25).map_err(db_error)?),
            FieldValue::Value(row.try_get(26).map_err(db_error)?),
            FieldValue::Value(row.try_get(27).map_err(db_error)?),
        ];
        Ok(cycle)
    }

    /// The archived list adds the three estimate sums; a `NULL` sum is a
    /// present attribute with value `None`, which DRF renders `null`.
    fn from_archived_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        let mut cycle = Self::from_read_row(row)?;
        // `Sum(IntegerField)` is a `bigint`; `CycleSerializer` declares the
        // field as a `FloatField`, so it renders `5.0` for the integer 5.
        let estimate = |index: usize| -> Result<FieldValue<f64>, ViewError> {
            Ok(
                match row.try_get::<_, Option<i64>>(index).map_err(db_error)? {
                    Some(value) => FieldValue::Value(value as f64),
                    None => FieldValue::Null,
                },
            )
        };
        cycle.estimates = [estimate(28)?, estimate(29)?, estimate(30)?];
        Ok(cycle)
    }

    fn serializer(&self) -> CycleSerializer<'_> {
        CycleSerializer {
            id: self.id,
            total_issues: self.counts[0],
            cancelled_issues: self.counts[1],
            completed_issues: self.counts[2],
            started_issues: self.counts[3],
            unstarted_issues: self.counts[4],
            backlog_issues: self.counts[5],
            total_estimates: self.estimates[0],
            completed_estimates: self.estimates[1],
            started_estimates: self.estimates[2],
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            name: &self.name,
            description: &self.description,
            start_date: self.start_date,
            end_date: self.end_date,
            view_props: self.view_props.as_ref().map(PgJson::from),
            sort_order: self.sort_order,
            external_source: self.external_source.as_deref(),
            external_id: self.external_id.as_deref(),
            progress_snapshot: self.progress_snapshot.as_ref().map(PgJson::from),
            archived_at: self.archived_at,
            logo_props: self.logo_props.as_ref().map(PgJson::from),
            timezone: &self.timezone,
            version: self.version,
            created_by: self.created_by,
            updated_by: self.updated_by,
            project: self.project,
            workspace: self.workspace,
            owned_by: self.owned_by,
        }
    }
}

/// One `cycles` row without annotations (`CycleLiteSerializer`).
#[derive(Debug)]
struct CycleLiteRow(CycleRow);

impl DrfSerialize for CycleLiteRow {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = &self.0;
        let serializer = CycleLiteSerializer {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
            name: &row.name,
            description: &row.description,
            start_date: row.start_date,
            end_date: row.end_date,
            view_props: row.view_props.as_ref().map(PgJson::from),
            sort_order: row.sort_order,
            external_source: row.external_source.as_deref(),
            external_id: row.external_id.as_deref(),
            progress_snapshot: row.progress_snapshot.as_ref().map(PgJson::from),
            archived_at: row.archived_at,
            logo_props: row.logo_props.as_ref().map(PgJson::from),
            timezone: &row.timezone,
            version: row.version,
            created_by: row.created_by,
            updated_by: row.updated_by,
            project: row.project,
            workspace: row.workspace,
            owned_by: row.owned_by,
        };
        serializer.drf_write(w, ctx)
    }
}

/// The expansion relations of one page of cycles, loaded once.
///
/// `expand=` resolves against the instance, so the values must outlive the
/// render call: [`render_cycles`] loads them for the whole page first.
#[derive(Default)]
pub(crate) struct CycleRelations {
    pub(crate) users: std::collections::HashMap<Uuid, issues::UserLiteOwned>,
    pub(crate) projects: std::collections::HashMap<Uuid, issues::ProjectOwned>,
    pub(crate) workspaces: std::collections::HashMap<Uuid, issues::WorkspaceOwned>,
}

/// `expand=` values for a cycle serializer.
///
/// `owned_by`, `created_by` and `updated_by` are users, `project` and
/// `workspace` their lite serializers (the 14-name mapper); every other name is
/// `getattr(instance, "<name>_id", None)`, which only the two relations have.
fn expansion_values<'a>(
    row: &'a CycleRow,
    relations: &'a CycleRelations,
    requested: &[&'a str],
) -> Vec<Expansion<'a>> {
    const FIELDS: &[&str] = CycleSerializer::FIELDS;
    let user_value = |id: Uuid| -> DrfValue<'a> {
        relations
            .users
            .get(&id)
            .map_or(DrfValue::Null, issues::user_value)
    };
    let mut values = Vec::with_capacity(requested.len());
    for name in requested {
        if !FIELDS.contains(name) {
            continue;
        }
        let value = match *name {
            "owned_by" => user_value(row.owned_by),
            "created_by" => row.created_by.map_or(DrfValue::Null, user_value),
            "updated_by" => row.updated_by.map_or(DrfValue::Null, user_value),
            "project" => relations
                .projects
                .get(&row.project)
                .map_or(DrfValue::Null, project_value),
            "workspace" => relations
                .workspaces
                .get(&row.workspace)
                .map_or(DrfValue::Null, workspace_value),
            _ => DrfValue::Null,
        };
        values.push(Expansion { name, value });
    }
    values
}

/// Load the relations a page's `expand=` names.
async fn load_relations(
    ctx: &ViewCtx<'_>,
    rows: &[CycleRow],
    requested: &[&str],
) -> Result<CycleRelations, ViewError> {
    if requested.is_empty() {
        return Ok(CycleRelations::default());
    }
    let client = client(ctx)?;
    let mut user_ids = Vec::new();
    let mut project_ids = Vec::new();
    let mut workspace_ids = Vec::new();
    for name in requested {
        match *name {
            "owned_by" => user_ids.extend(rows.iter().map(|row| row.owned_by)),
            "created_by" => user_ids.extend(rows.iter().filter_map(|row| row.created_by)),
            "updated_by" => user_ids.extend(rows.iter().filter_map(|row| row.updated_by)),
            "project" => project_ids.extend(rows.iter().map(|row| row.project)),
            "workspace" => workspace_ids.extend(rows.iter().map(|row| row.workspace)),
            _ => {}
        }
    }
    user_ids.sort_unstable();
    user_ids.dedup();
    project_ids.sort_unstable();
    project_ids.dedup();
    workspace_ids.sort_unstable();
    workspace_ids.dedup();
    Ok(CycleRelations {
        users: issues::load_users(client, &user_ids).await?,
        projects: issues::load_projects(client, &project_ids).await?,
        workspaces: issues::load_workspaces(client, &workspace_ids).await?,
    })
}

/// `ProjectLiteSerializer` (`api/serializers/project.py`), in declaration order.
pub(crate) fn project_value(project: &issues::ProjectOwned) -> DrfValue<'_> {
    DrfValue::Object(vec![
        ("id", DrfValue::Uuid(project.id)),
        ("identifier", DrfValue::Str(&project.identifier)),
        ("name", DrfValue::Str(&project.name)),
        (
            "cover_image",
            project
                .cover_image
                .as_deref()
                .map_or(DrfValue::Null, DrfValue::Str),
        ),
        (
            "icon_prop",
            project
                .icon_prop
                .as_ref()
                .map_or(DrfValue::Null, |json| DrfValue::JsonText(json.as_str())),
        ),
        (
            "emoji",
            project
                .emoji
                .as_deref()
                .map_or(DrfValue::Null, DrfValue::Str),
        ),
        ("description", DrfValue::Str(&project.description)),
        (
            "cover_image_url",
            project
                .cover_image_url
                .as_deref()
                .map_or(DrfValue::Null, DrfValue::Str),
        ),
        (
            "archived_at",
            project
                .archived_at
                .map_or(DrfValue::Null, DrfValue::DateTime),
        ),
    ])
}

/// `WorkspaceLiteSerializer` (`api/serializers/workspace.py:8-22`).
pub(crate) fn workspace_value(workspace: &issues::WorkspaceOwned) -> DrfValue<'_> {
    DrfValue::Object(vec![
        ("name", DrfValue::Str(&workspace.name)),
        ("slug", DrfValue::Str(&workspace.slug)),
        ("id", DrfValue::Uuid(workspace.id)),
    ])
}

fn requested_expansions<'a>(ctx: &'a ViewCtx<'a>) -> Vec<&'a str> {
    ctx.expand().unwrap_or_default()
}

fn render_cycle(
    ctx: &ViewCtx<'_>,
    relations: &CycleRelations,
    row: &CycleRow,
) -> Result<Vec<u8>, ViewError> {
    let prepared = RequestRender::from_ctx(ctx);
    let requested = requested_expansions(ctx);
    let values = expansion_values(row, relations, &requested);
    let expansions = Expansions::new(&requested, &values);
    let render_ctx = prepared.render_ctx().with_expansions(expansions);
    plane_serializers::render(&row.serializer(), &render_ctx).map_err(ViewError::from)
}

/// Render a page of cycles, loading the page's `expand=` relations once.
async fn render_cycles(ctx: &ViewCtx<'_>, rows: &[CycleRow]) -> Result<Vec<u8>, ViewError> {
    let requested = requested_expansions(ctx);
    let relations = load_relations(ctx, rows, &requested).await?;
    let mut body = Vec::with_capacity(rows.len() * 512 + 2);
    body.push(b'[');
    for (index, row) in rows.iter().enumerate() {
        if index != 0 {
            body.push(b',');
        }
        body.extend(render_cycle(ctx, &relations, row)?);
    }
    body.push(b']');
    Ok(body)
}

// ---------------------------------------------------------------------------
// 279 GET — list
// ---------------------------------------------------------------------------

/// Run one list statement, then the DvR page envelope.
async fn list_page(
    ctx: &ViewCtx<'_>,
    statement: &plane_db::Query,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<ViewResponse, ViewError> {
    let client = client(ctx)?;
    let page = page::default_query(ctx)?;
    let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
    let limit = window.row_limit() + 1;
    let offset = window.offset;
    let mut binds: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params.to_vec();
    binds.push(&limit);
    binds.push(&offset);
    let sql = format!(
        "{} LIMIT ${} OFFSET ${}",
        statement.sql,
        binds.len() - 1,
        binds.len()
    );
    let rows = client.query(&sql, &binds).await.map_err(db_error)?;
    let fetched = rows.len();
    let mut bytes = Vec::new();
    let mut decoded = Vec::with_capacity(fetched.min(window.row_limit() as usize));
    for row in rows.iter().take(window.row_limit() as usize) {
        decoded.push(CycleRow::from_read_row(row)?);
    }
    bytes.extend(render_cycles(ctx, &decoded).await?);
    let (next, prev) = window.cursors(fetched);
    let total = count_rows(ctx, statement.name, params).await?;
    Ok(ViewResponse::raw(
        200,
        APPLICATION_JSON,
        page::page_with_bytes(total, &window, &next, &prev, decoded.len(), &bytes),
    ))
}

/// The paginator's `queryset.count()`: one row per matching cycle, because the
/// statement groups by `cycles.id`.
async fn count_rows(
    ctx: &ViewCtx<'_>,
    name: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<i64, ViewError> {
    let client = client(ctx)?;
    let sql = match name {
        "v1_cycles_list" => COUNT_ALL,
        "v1_cycles_list_upcoming" => COUNT_UPCOMING,
        "v1_cycles_list_completed" => COUNT_COMPLETED,
        "v1_cycles_list_draft" => COUNT_DRAFT,
        "v1_cycles_list_incomplete" => COUNT_INCOMPLETE,
        "v1_cycles_archived_list" => COUNT_ARCHIVED,
        other => {
            return Err(ViewError::Unknown(format!(
                "no count statement for {other}"
            )));
        }
    };
    let row = client.query_one(sql, params).await.map_err(db_error)?;
    row.try_get(0).map_err(db_error)
}

const COUNT_ALL: &str = "SELECT COUNT(*) FROM (SELECT DISTINCT cycles.id FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN project_members ON cycles.project_id = project_members.project_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
       AND cycles.project_id = $2 AND project_members.is_active \
       AND project_members.member_id = $3 AND cycles.archived_at IS NULL) subquery";

const COUNT_UPCOMING: &str = "SELECT COUNT(*) FROM (SELECT DISTINCT cycles.id FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN project_members ON cycles.project_id = project_members.project_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
       AND cycles.project_id = $2 AND project_members.is_active \
       AND project_members.member_id = $3 AND cycles.archived_at IS NULL \
       AND cycles.start_date > $4) subquery";

const COUNT_COMPLETED: &str = "SELECT COUNT(*) FROM (SELECT DISTINCT cycles.id FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN project_members ON cycles.project_id = project_members.project_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
       AND cycles.project_id = $2 AND project_members.is_active \
       AND project_members.member_id = $3 AND cycles.archived_at IS NULL \
       AND cycles.end_date < $4) subquery";

const COUNT_DRAFT: &str = "SELECT COUNT(*) FROM (SELECT DISTINCT cycles.id FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN project_members ON cycles.project_id = project_members.project_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
       AND cycles.project_id = $2 AND project_members.is_active \
       AND project_members.member_id = $3 AND cycles.archived_at IS NULL \
       AND cycles.end_date IS NULL AND cycles.start_date IS NULL) subquery";

const COUNT_INCOMPLETE: &str = "SELECT COUNT(*) FROM (SELECT DISTINCT cycles.id FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN project_members ON cycles.project_id = project_members.project_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
       AND cycles.project_id = $2 AND project_members.is_active \
       AND project_members.member_id = $3 AND cycles.archived_at IS NULL \
       AND (cycles.end_date >= $4 OR cycles.end_date IS NULL)) subquery";

const COUNT_ARCHIVED: &str = "SELECT COUNT(*) FROM (SELECT DISTINCT cycles.id FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN project_members ON cycles.project_id = project_members.project_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
       AND cycles.project_id = $2 AND project_members.is_active \
       AND project_members.member_id = $3 \
       AND cycles.archived_at IS NOT NULL) subquery";

/// `CycleListCreateAPIEndpoint.get` (`cycle.py:192-283`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let client = client(ctx)?;
        // `Project.objects.get(workspace__slug=slug, pk=project_id)`
        // (`cycle.py:198`) — the 404 arrives before the queryset runs.
        let row = client
            .query_opt(
                "SELECT 1 FROM projects \
                 INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
                 WHERE projects.deleted_at IS NULL AND workspaces.slug = $1 \
                   AND projects.id = $2",
                &[&params.slug, &params.project_id],
            )
            .await
            .map_err(db_error)?;
        if row.is_none() {
            return Err(ViewError::ObjectDoesNotExist);
        }

        let now = Utc::now();
        let cycle_view = ctx.query().get_or("cycle_view", "all");
        match cycle_view {
            "current" => {
                let rows = run_rows(
                    ctx,
                    &queries::CYCLE_LIST_CURRENT,
                    &[&params.slug, &params.project_id, &params.user_id, &now],
                )
                .await?;
                let decoded = rows
                    .iter()
                    .map(CycleRow::from_read_row)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(ViewResponse::raw(
                    200,
                    APPLICATION_JSON,
                    render_cycles(ctx, &decoded).await?,
                ))
            }
            "upcoming" => {
                list_page(
                    ctx,
                    &queries::CYCLE_LIST_UPCOMING,
                    &[&params.slug, &params.project_id, &params.user_id, &now],
                )
                .await
            }
            "completed" => {
                list_page(
                    ctx,
                    &queries::CYCLE_LIST_COMPLETED,
                    &[&params.slug, &params.project_id, &params.user_id, &now],
                )
                .await
            }
            "draft" => {
                list_page(
                    ctx,
                    &queries::CYCLE_LIST_DRAFT,
                    &[&params.slug, &params.project_id, &params.user_id],
                )
                .await
            }
            "incomplete" => {
                list_page(
                    ctx,
                    &queries::CYCLE_LIST_INCOMPLETE,
                    &[&params.slug, &params.project_id, &params.user_id, &now],
                )
                .await
            }
            _ => {
                list_page(
                    ctx,
                    &queries::CYCLE_LIST,
                    &[&params.slug, &params.project_id, &params.user_id],
                )
                .await
            }
        }
    })
}

async fn run_rows(
    ctx: &ViewCtx<'_>,
    statement: &plane_db::Query,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let client = client(ctx)?;
    let prepared = statement.prepare(client).await?;
    client.query(&prepared, params).await.map_err(db_error)
}

// ---------------------------------------------------------------------------
// 280 GET — lite list
// ---------------------------------------------------------------------------

/// `CYCLE_ORDER_BY_ALLOWLIST` (`order_queryset.py:114-121`).
const CYCLE_ORDER_BY_ALLOWLIST: &[&str] = &[
    "created_at",
    "updated_at",
    "name",
    "start_date",
    "end_date",
    "sort_order",
];

/// `sanitize_order_by(value, CYCLE_ORDER_BY_ALLOWLIST, "-created_at")`.
fn sanitize_order_by(value: &str) -> String {
    let default = "-created_at".to_owned();
    if value.is_empty() {
        return default;
    }
    let descending = value.starts_with('-');
    let bare = if descending { &value[1..] } else { value };
    if bare.starts_with('-') || !CYCLE_ORDER_BY_ALLOWLIST.contains(&bare) {
        return default;
    }
    if descending {
        format!("-{bare}")
    } else {
        bare.to_owned()
    }
}

/// `CycleListLiteAPIEndpoint.get` (`cycle.py:382-405`).
pub fn lite_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let order = sanitize_order_by(ctx.query().get_or("order_by", "-created_at"));
        let (descending, bare) = match order.strip_prefix('-') {
            Some(bare) => (true, bare),
            None => (false, order.as_str()),
        };
        let direction = if descending { "DESC" } else { "ASC" };
        let sql = format!(
            "SELECT cycles.id, cycles.created_at, cycles.updated_at, \
                 cycles.deleted_at, cycles.name, cycles.description, \
                 cycles.start_date, cycles.end_date, cycles.view_props, \
                 cycles.sort_order, cycles.external_source, cycles.external_id, \
                 cycles.progress_snapshot, cycles.archived_at, cycles.logo_props, \
                 cycles.timezone, cycles.version, cycles.created_by_id, \
                 cycles.updated_by_id, cycles.project_id, cycles.workspace_id, \
                 cycles.owned_by_id \
             FROM cycles \
             INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
             WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
               AND cycles.project_id = $2 AND cycles.archived_at IS NULL \
             ORDER BY cycles.{bare} {direction} LIMIT $3 OFFSET $4"
        );
        let client = client(ctx)?;
        let page = page::default_query(ctx)?;
        let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
        let limit = window.row_limit() + 1;
        let offset = window.offset;
        let rows = client
            .query(&sql, &[&params.slug, &params.project_id, &limit, &offset])
            .await
            .map_err(db_error)?;
        let fetched = rows.len();
        let mut lite = Vec::with_capacity(fetched.min(window.row_limit() as usize));
        for row in rows.iter().take(window.row_limit() as usize) {
            lite.push(CycleLiteRow(CycleRow::from_row(row)?));
        }
        let prepared = RequestRender::from_ctx(ctx);
        let bytes = plane_http_app::common::render_list(&lite, &prepared.render_ctx())?;
        let total = {
            let row = client
                .query_one(
                    "SELECT COUNT(*) FROM cycles \
                     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
                     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
                       AND cycles.project_id = $2 AND cycles.archived_at IS NULL",
                    &[&params.slug, &params.project_id],
                )
                .await
                .map_err(db_error)?;
            row.try_get::<_, i64>(0).map_err(db_error)?
        };
        let (next, prev) = window.cursors(fetched);
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            page::page_with_bytes(total, &window, &next, &prev, lite.len(), &bytes),
        ))
    })
}

// ---------------------------------------------------------------------------
// 281 GET — detail
// ---------------------------------------------------------------------------

/// `CycleDetailAPIEndpoint.get` (`cycle.py:511-525`).
pub fn detail<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let client = client(ctx)?;
        let pk = uuid_kwarg(ctx, "pk")?;
        // `Project.objects.get(...)` for the serializer context (`cycle.py:517`).
        let row = client
            .query_opt(
                "SELECT 1 FROM projects \
                 INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
                 WHERE projects.deleted_at IS NULL AND workspaces.slug = $1 \
                   AND projects.id = $2",
                &[&params.slug, &params.project_id],
            )
            .await
            .map_err(db_error)?;
        if row.is_none() {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let rows = run_rows(
            ctx,
            &queries::CYCLE_DETAIL,
            &[&params.slug, &params.project_id, &params.user_id, &pk],
        )
        .await?;
        let Some(row) = rows.first() else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let decoded = CycleRow::from_read_row(row)?;
        let requested = requested_expansions(ctx);
        let relations = load_relations(ctx, std::slice::from_ref(&decoded), &requested).await?;
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_cycle(ctx, &relations, &decoded)?,
        ))
    })
}

// ---------------------------------------------------------------------------
// 286 GET — archived list
// ---------------------------------------------------------------------------

/// `CycleArchiveUnarchiveAPIEndpoint.get` (`cycle.py:779-789`).
pub fn archived_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        list_page_archived(ctx, &params).await
    })
}

async fn list_page_archived(ctx: &ViewCtx<'_>, params: &Params) -> Result<ViewResponse, ViewError> {
    let client = client(ctx)?;
    let page = page::default_query(ctx)?;
    let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
    let limit = window.row_limit() + 1;
    let offset = window.offset;
    let sql = format!("{} LIMIT $4 OFFSET $5", queries::CYCLE_ARCHIVED_LIST.sql);
    let rows = client
        .query(
            &sql,
            &[
                &params.slug,
                &params.project_id,
                &params.user_id,
                &limit,
                &offset,
            ],
        )
        .await
        .map_err(db_error)?;
    let fetched = rows.len();
    let mut decoded = Vec::with_capacity(fetched.min(window.row_limit() as usize));
    for row in rows.iter().take(window.row_limit() as usize) {
        decoded.push(CycleRow::from_archived_row(row)?);
    }
    let bytes = render_cycles(ctx, &decoded).await?;
    let total = count_rows(
        ctx,
        "v1_cycles_archived_list",
        &[&params.slug, &params.project_id, &params.user_id],
    )
    .await?;
    let (next, prev) = window.cursors(fetched);
    Ok(ViewResponse::raw(
        200,
        APPLICATION_JSON,
        page::page_with_bytes(total, &window, &next, &prev, decoded.len(), &bytes),
    ))
}

// ---------------------------------------------------------------------------
// 282 GET — cycle work items
// ---------------------------------------------------------------------------

/// The two through-table issue lists are the same statement with a different
/// bridge table.
#[derive(Clone, Copy)]
pub(crate) enum IssueScope {
    Cycle(Uuid),
    Module(Uuid),
}

impl IssueScope {
    fn table(self) -> (&'static str, &'static str) {
        match self {
            Self::Cycle(_) => ("cycle_issues", "cycle_id"),
            Self::Module(_) => ("module_issues", "module_id"),
        }
    }

    fn id(self) -> Uuid {
        match self {
            Self::Cycle(id) | Self::Module(id) => id,
        }
    }
}

/// `order_by` for the two through-table lists: a plain column (or a joined
/// one), never the app surface's `CASE` ranking.
fn scope_order_sql(order: &str) -> (&'static str, String) {
    let (descending, bare) = match order.strip_prefix('-') {
        Some(bare) => (true, bare),
        None => (false, order),
    };
    let direction = if descending { "DESC" } else { "ASC" };
    match bare {
        "state__name" => (
            "INNER JOIN states ON states.id = issues.state_id",
            format!("states.name {direction}"),
        ),
        "state__group" => (
            "INNER JOIN states ON states.id = issues.state_id",
            format!("states.\"group\" {direction}"),
        ),
        "assignees__first_name" => (
            "INNER JOIN issue_assignees ON issue_assignees.issue_id = issues.id \
             INNER JOIN users ON users.id = issue_assignees.assignee_id",
            format!("users.first_name {direction}"),
        ),
        "labels__name" => (
            "INNER JOIN issue_labels ON issue_labels.issue_id = issues.id \
             INNER JOIN labels ON labels.id = issue_labels.label_id",
            format!("labels.name {direction}"),
        ),
        "issue_module__module__name" => (
            "INNER JOIN issue_modules ON issue_modules.issue_id = issues.id \
             INNER JOIN modules ON modules.id = issue_modules.module_id",
            format!("modules.name {direction}"),
        ),
        other => ("", format!("issues.{other} {direction}")),
    }
}

/// The `Issue.issue_objects.filter(<through scope>)` page both v1 list views
/// build (`cycle.py:898-944`, `module.py:651-696`).
///
/// `order_by` defaults to `created_at` (ascending) and is sanitized through
/// `ISSUE_ORDER_BY_ALLOWLIST`; a restricted guest is narrowed to their own
/// work items.
pub(crate) async fn issue_scope_page(
    ctx: &ViewCtx<'_>,
    slug: &str,
    project_id: Uuid,
    scope: IssueScope,
    require_project_not_archived: bool,
) -> Result<ViewResponse, ViewError> {
    let client = client(ctx)?;
    let user = issues::user_id(ctx)?;
    let order = issues::sanitize_order_by(ctx.query().get_or("order_by", "created_at"));
    let page = page::default_query(ctx)?;
    let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
    let (table, column) = scope.table();

    let mut filters = Frag::raw("issues.project_id = ");
    filters.push(Frag::bind(Value::Uuid(project_id)));
    filters.push_raw(&format!(" AND {table}.{column} = "));
    filters.push(Frag::bind(Value::Uuid(scope.id())));
    filters.push_raw(&format!(" AND {table}.deleted_at IS NULL"));

    // `is_restricted_guest` (`issue.py:201-212`).
    let restricted = client
        .query_opt(
            "SELECT 1 FROM project_members \
             INNER JOIN projects ON projects.id = project_members.project_id \
             INNER JOIN workspaces ON workspaces.id = projects.workspace_id \
             WHERE workspaces.slug = $1 AND project_members.project_id = $2 \
               AND project_members.member_id = $3 AND project_members.is_active \
               AND project_members.deleted_at IS NULL \
               AND project_members.role = 5 AND projects.guest_view_all_features = false \
             LIMIT 1",
            &[&slug, &project_id, &user],
        )
        .await
        .map_err(db_error)?
        .is_some();
    if restricted {
        filters.push_raw(" AND issues.created_by_id = ");
        filters.push(Frag::bind(Value::Uuid(user)));
    }

    // `order_by(order_by)` on the *Issue* queryset (not
    // `order_issue_queryset`): every allowlisted name is a plain column or a
    // joined column, with no `CASE` ranking and no `DISTINCT`, so a
    // to-many ordering (`labels__name`, `assignees__first_name`) multiplies
    // rows exactly like Django's INNER JOIN does.
    let (order_joins, order_term) = scope_order_sql(&order);
    let mut select = Frag::raw(format!(
        "SELECT {} FROM issues \
         INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
         INNER JOIN {table} ON {table}.issue_id = issues.id {order_joins} ",
        issues::issue_columns(),
    ));
    select.push_raw("WHERE ");
    select.push_raw(&issues::issue_objects());
    select.push_raw(" AND workspaces.slug = ");
    select.push(Frag::bind(Value::Text(slug.to_owned())));
    if require_project_not_archived {
        select.push_raw(
            " AND NOT EXISTS (SELECT 1 FROM projects WHERE projects.id = issues.project_id \
              AND projects.archived_at IS NOT NULL)",
        );
    }
    select.push_raw(" AND ");
    select.push(filters.clone());
    select.push_raw(" ORDER BY ");
    select.push_raw(&order_term);
    select.push_raw(" LIMIT ");
    select.push(Frag::bind(Value::Int8(window.row_limit() + 1)));
    select.push_raw(" OFFSET ");
    select.push(Frag::bind(Value::Int8(window.offset)));

    let mut bundles = issues::load_bundles(ctx, client, select).await?;
    let fetched = bundles.len();
    bundles.truncate(window.row_limit() as usize);
    let (next, prev) = window.cursors(fetched);

    let mut total_sql = Frag::raw(format!(
        "SELECT COUNT(*) FROM issues \
         INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
         INNER JOIN {table} ON {table}.issue_id = issues.id {order_joins} WHERE "
    ));
    total_sql.push_raw(&issues::issue_objects());
    total_sql.push_raw(" AND workspaces.slug = ");
    total_sql.push(Frag::bind(Value::Text(slug.to_owned())));
    if require_project_not_archived {
        total_sql.push_raw(
            " AND NOT EXISTS (SELECT 1 FROM projects WHERE projects.id = issues.project_id \
              AND projects.archived_at IS NOT NULL)",
        );
    }
    total_sql.push_raw(" AND ");
    total_sql.push(filters);
    let total_row = total_sql
        .into_sql()
        .query(client)
        .await
        .map_err(db_error)?
        .into_iter()
        .next()
        .ok_or_else(|| ViewError::Unknown("count returned no row".to_owned()))?;
    let total: i64 = total_row.try_get(0).map_err(db_error)?;

    let results = issues::render_issues(ctx, &bundles)?;
    Ok(ViewResponse::raw(
        200,
        APPLICATION_JSON,
        page::page_with_bytes(total, &window, &next, &prev, bundles.len(), &results),
    ))
}

/// `CycleIssueListCreateAPIEndpoint.get` (`cycle.py:892-944`).
pub fn issue_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        issue_scope_page(ctx, &slug, project_id, IssueScope::Cycle(cycle_id), false).await
    })
}

// ---------------------------------------------------------------------------
// 283 GET — cycle work item detail
// ---------------------------------------------------------------------------

/// `CycleIssueDetailAPIEndpoint.get` (`cycle.py:1123-1140`).
pub fn issue_detail<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let client = client(ctx)?;

        // `Issue.objects.get(workspace__slug, project_id, pk)` (soft delete).
        let issue = {
            let sql = format!(
                "SELECT {} FROM issues \
                 INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
                 WHERE issues.deleted_at IS NULL AND workspaces.slug = $1 \
                   AND issues.project_id = $2 AND issues.id = $3",
                issues::issue_columns()
            );
            client
                .query_opt(&sql, &[&slug, &project_id, &issue_id])
                .await
                .map_err(db_error)?
        };
        let Some(issue) = issue else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let bundle = crate::issues::IssueBundle {
            issue: plane_db::models::issue::Issue::from_row(&issue, 0).map_err(ViewError::from)?,
            assignee_ids: Vec::new(),
            label_ids: Vec::new(),
            assignees: Vec::new(),
            labels: Vec::new(),
            workspace: None,
            project: None,
            state: None,
            parent: None,
            created_by: None,
            updated_by: None,
            estimate_point: None,
        };
        if issues::guest_cannot_view(ctx, client, &slug, &bundle.issue).await? {
            return Err(ViewError::ObjectDoesNotExist);
        }

        let prepared = client
            .prepare_cached(
                "SELECT cycle_issues.id, cycle_issues.created_at, \
                     cycle_issues.updated_at, cycle_issues.deleted_at, \
                     cycle_issues.created_by_id, cycle_issues.updated_by_id, \
                     cycle_issues.project_id, cycle_issues.workspace_id, \
                     cycle_issues.issue_id, cycle_issues.cycle_id \
                 FROM cycle_issues \
                 INNER JOIN workspaces ON cycle_issues.workspace_id = workspaces.id \
                 WHERE cycle_issues.deleted_at IS NULL AND workspaces.slug = $1 \
                   AND cycle_issues.project_id = $2 AND cycle_issues.cycle_id = $3 \
                   AND cycle_issues.issue_id = $4",
            )
            .await
            .map_err(db_error)?;
        let Some(row) = client
            .query_opt(&prepared, &[&slug, &project_id, &cycle_id, &issue_id])
            .await
            .map_err(db_error)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        // `CycleIssue.objects.get` does not annotate, so `sub_issues_count`
        // is skipped rather than rendered.
        let serializer = CycleIssueSerializer {
            id: row.try_get(0).map_err(db_error)?,
            sub_issues_count: FieldValue::Absent,
            created_at: row.try_get(1).map_err(db_error)?,
            updated_at: row.try_get(2).map_err(db_error)?,
            deleted_at: row.try_get(3).map_err(db_error)?,
            created_by: row.try_get(4).map_err(db_error)?,
            updated_by: row.try_get(5).map_err(db_error)?,
            project: row.try_get(6).map_err(db_error)?,
            workspace: row.try_get(7).map_err(db_error)?,
            issue: row.try_get(8).map_err(db_error)?,
            cycle: row.try_get(9).map_err(db_error)?,
        };
        let prepared = RequestRender::from_ctx(ctx);
        let requested = requested_expansions(ctx);
        let values: Vec<Expansion<'_>> =
            crate::common::render::expansions(&requested, &[], |name| match name {
                "issue" => Some(serializer.issue),
                "cycle" => Some(serializer.cycle),
                "project" => Some(serializer.project),
                "workspace" => Some(serializer.workspace),
                "created_by" => serializer.created_by,
                "updated_by" => serializer.updated_by,
                _ => None,
            });
        let expansions = Expansions::new(&requested, &values);
        let render_ctx = prepared.render_ctx().with_expansions(expansions);
        let body = plane_serializers::render(&serializer, &render_ctx).map_err(ViewError::from)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// Placeholder for the writes added below
// ---------------------------------------------------------------------------

pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { Err(ViewError::Unknown(format!("not implemented: {}", ctx.view))) })
}

pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { Err(ViewError::Unknown(format!("not implemented: {}", ctx.view))) })
}

/// `CycleDetailAPIEndpoint.delete` (`cycle.py:617-651`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let client = client(ctx)?;
        let row = run_rows(
            ctx,
            &queries::CYCLE_ROW,
            &[&params.slug, &params.project_id, &pk],
        )
        .await?;
        let Some(row) = row.first() else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let cycle = CycleRow::from_row(row)?;
        if cycle.created_by != Some(params.user_id) && !is_project_admin(ctx, &params).await? {
            return Ok(error_body(
                403,
                "Only admin or creator can delete the cycle",
            ));
        }
        let issues = {
            let prepared = queries::CYCLE_ISSUE_IDS
                .prepare(client)
                .await
                .map_err(ViewError::from)?;
            client
                .query(&prepared, &[&pk])
                .await
                .map_err(db_error)?
                .iter()
                .map(|row| row.try_get::<_, Uuid>(0).map_err(ViewError::from))
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut requested = plane_serde::pyjson::PyDict::with_capacity(3);
        requested.insert(
            plane_serde::pyjson::PyStr::new("cycle_id"),
            plane_serde::pyjson::Value::from(pk.to_string()),
        );
        requested.insert(
            plane_serde::pyjson::PyStr::new("cycle_name"),
            plane_serde::pyjson::Value::from(cycle.name.clone()),
        );
        requested.insert(
            plane_serde::pyjson::PyStr::new("issues"),
            plane_serde::pyjson::Value::List(
                issues
                    .iter()
                    .map(|issue| plane_serde::pyjson::Value::from(issue.to_string()))
                    .collect(),
            ),
        );
        let payload = plane_serde::pyjson::Value::Dict(requested);
        let base = plane_jobs::celery::tasks::IssueActivityBase::new(
            "cycle.activity.deleted",
            TaskArg::json_text(issues::dumps(&payload)?),
            params.user_id.to_string(),
            TaskArg::Null,
            TaskArg::str(params.project_id.to_string()),
            TaskArg::Null,
            Utc::now().timestamp(),
        );
        let call = plane_jobs::celery::tasks::IssueActivityCall::bare(base);
        let message = plane_jobs::celery::tasks::issue_activity(call);
        let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
        plane_http_app::common::jobs::dispatch_task(
            ctx,
            task,
            &message,
            plane_jobs::queue::Job::new(task),
        )
        .await?;
        // `cycle.delete()` — H03 then the cascade task.
        let statement = queries::CYCLE_SOFT_DELETE
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &plane_http_app::write::now(),
                    &plane_http_app::write::now(),
                    &Some(params.user_id),
                    &pk,
                ],
            )
            .await
            .map_err(db_error)?;
        plane_http_app::write::publish_soft_delete_cascade(ctx, "db", "cycle", pk).await?;
        let statement = queries::CYCLE_FAVORITE_SOFT_DELETE_ALL
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[&plane_http_app::write::now(), &pk, &params.project_id],
            )
            .await
            .map_err(db_error)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `ProjectAdminPermission.has_permission` (`plane/utils/permissions/project.py:126-149`).
async fn is_project_admin(ctx: &ViewCtx<'_>, params: &Params) -> Result<bool, ViewError> {
    let client = client(ctx)?;
    let prepared = queries::CYCLE_PROJECT_ADMIN
        .prepare(client)
        .await
        .map_err(ViewError::from)?;
    if client
        .query_opt(
            &prepared,
            &[&params.slug, &params.user_id, &params.project_id],
        )
        .await
        .map_err(db_error)?
        .is_some()
    {
        return Ok(true);
    }
    let prepared = queries::CYCLE_WORKSPACE_ADMIN
        .prepare(client)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&prepared, &[&params.slug, &params.user_id])
        .await
        .map_err(db_error)?
        .is_some())
}

/// `CycleArchiveUnarchiveAPIEndpoint.post` (`cycle.py:801-821`).
pub fn archive_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let client = client(ctx)?;
        let prepared = queries::CYCLE_ARCHIVE_LOOKUP
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&prepared, &[&cycle_id, &params.project_id, &params.slug])
            .await
            .map_err(db_error)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let end_date: Option<DateTime<Utc>> = row.try_get(0).map_err(ViewError::from)?;
        let now = Utc::now();
        if end_date.is_none_or(|end| end >= now) {
            return Ok(error_body(400, "Only completed cycles can be archived"));
        }
        let statement = queries::CYCLE_SET_ARCHIVED
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &plane_http_app::write::now(),
                    &plane_http_app::write::now(),
                    &Some(params.user_id),
                    &cycle_id,
                ],
            )
            .await
            .map_err(db_error)?;
        let statement = queries::CYCLE_FAVORITE_SOFT_DELETE_WS
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &plane_http_app::write::now(),
                    &cycle_id,
                    &params.project_id,
                    &params.slug,
                ],
            )
            .await
            .map_err(db_error)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `CycleArchiveUnarchiveAPIEndpoint.delete` (`cycle.py:832-841`).
pub fn unarchive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let client = client(ctx)?;
        let prepared = queries::CYCLE_ARCHIVE_LOOKUP
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        if client
            .query_opt(&prepared, &[&cycle_id, &params.project_id, &params.slug])
            .await
            .map_err(db_error)?
            .is_none()
        {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let statement = queries::CYCLE_CLEAR_ARCHIVED
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &plane_http_app::write::now(),
                    &Some(params.user_id),
                    &cycle_id,
                ],
            )
            .await
            .map_err(db_error)?;
        Ok(ViewResponse::empty(204))
    })
}

pub fn issue_add<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { Err(ViewError::Unknown(format!("not implemented: {}", ctx.view))) })
}

pub fn issue_remove<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { Err(ViewError::Unknown(format!("not implemented: {}", ctx.view))) })
}
