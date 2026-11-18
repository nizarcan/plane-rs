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
//! ## The writes
//!
//! `POST pr/cycles/` and `PATCH pr/cycles/{pk}/` use the generated
//! `CycleCreateSerializer`/`CycleUpdateSerializer` field tables plus
//! `CycleCreateSerializer.validate` (`api/serializers/cycle.py:61-98`) — the
//! project lookup, the `cycle_view` gate, the date order, `convert_to_utc`
//! (the `+1s`/`23:59:00` rule and the "today in the project's zone → now"
//! branch, [`plane_http_app::cycles::write::convert_to_utc`]) and the
//! `owned_by` default. The create's duplicate `(external_source, external_id)`
//! check answers 409 with the existing row's id; the PATCH's reports the
//! **URL** cycle's id.
//!
//! `POST …/cycle-issues/` inserts one row per requested issue with
//! `ignore_conflicts=True` (so an issue already in this cycle is a no-op but
//! still appears in the activity payload), moves rows that sit in another
//! cycle with a FK-only `bulk_update`, and answers 200 with the whole
//! through-table list. `DELETE …/cycle-issues/{issue_id}/` is H03 plus the
//! cascade task. `POST …/transfer-issues/` runs the two v1-only pre-checks and
//! then the app surface's kernel ([`plane_http_app::cycles::write_transfer`]).
//!
//! ## Reuse
//!
//! The list filters (membership, six status counts) are this surface's own SQL
//! ([`cycles_queries`]). The transfer endpoint reuses the app surface's
//! `transfer_cycle_issues` kernel ([`plane_http_app::cycles::write_transfer`])
//! after the two v1-only pre-checks, and the cycle-issue lists reuse
//! [`crate::issues`]' serializer pipeline.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_query::paginator::OffsetWindow;
use plane_query::sql::{Frag, Value};
use plane_serializers::generated::api_cycle::{
    CycleIssueSerializer, CycleLiteSerializer, CycleSerializer,
};
use plane_serializers::generated::api_module::ModuleIssueSerializer;
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
        .method(http::Method::GET, list)?
        .method(http::Method::POST, create)?;
    registry
        .api_view("plane.api.views.cycle.CycleListLiteAPIEndpoint")?
        .method(http::Method::GET, lite_list)?;
    registry
        .api_view("plane.api.views.cycle.CycleDetailAPIEndpoint")?
        .method(http::Method::GET, detail)?
        .method(http::Method::PATCH, patch)?
        .method(http::Method::DELETE, delete)?;
    registry
        .api_view("plane.api.views.cycle.CycleArchiveUnarchiveAPIEndpoint")?
        .method(http::Method::GET, archived_list)?
        .method(http::Method::POST, archive_post)?
        .method(http::Method::DELETE, unarchive)?;
    registry
        .api_view("plane.api.views.cycle.CycleIssueListCreateAPIEndpoint")?
        .method(http::Method::GET, issue_list)?
        .method(http::Method::POST, issue_add)?;
    registry
        .api_view("plane.api.views.cycle.CycleIssueDetailAPIEndpoint")?
        .method(http::Method::GET, issue_detail)?
        .method(http::Method::DELETE, issue_remove)?;
    registry
        .api_view("plane.api.views.cycle.TransferCycleIssueAPIEndpoint")?
        .method(http::Method::POST, transfer)?;
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

// ---------------------------------------------------------------------------
// Body helpers (`request.data.get` semantics on a JSON or form body)
// ---------------------------------------------------------------------------

/// `request.data` as a mapping. `request.data.get` on a JSON list/scalar is an
/// `AttributeError` (family 500) before the view's own logic
/// (`cycle.py:307-309`).
fn ensure_mapping(data: &Data) -> Result<(), ViewError> {
    match data {
        Data::Json(plane_serde::pyjson::Value::Dict(_)) | Data::Form(_) => Ok(()),
        Data::Json(plane_serde::pyjson::Value::Null) => Err(ViewError::Unknown(
            "AttributeError: 'NoneType' object has no attribute 'get'".to_owned(),
        )),
        Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: 'list' object has no attribute 'get'".to_owned(),
        )),
    }
}

/// `request.data.get(key, None) is not None`.
fn has_value(data: &Data, key: &str) -> bool {
    match data {
        Data::Json(plane_serde::pyjson::Value::Dict(map)) => {
            map.get(key).is_some_and(|value| !value.is_null())
        }
        Data::Form(query) => query.get(key).is_some(),
        Data::Json(_) => false,
    }
}

/// `key in request.data`.
fn has_key(data: &Data, key: &str) -> bool {
    match data {
        Data::Json(plane_serde::pyjson::Value::Dict(map)) => map.contains_key(key),
        Data::Form(query) => query.contains_key(key),
        Data::Json(_) => false,
    }
}

/// `request.data.get(key)` as the JSON value Django would hand a task.
pub(crate) fn raw_body_field(data: &Data, key: &str) -> Option<plane_serde::pyjson::Value> {
    match data {
        Data::Json(plane_serde::pyjson::Value::Dict(map)) => map.get(key).cloned(),
        Data::Form(query) => query.get(key).map(plane_serde::pyjson::Value::from),
        Data::Json(_) => None,
    }
}

/// `serializers.ValidationError("MSG")` from a serializer's `validate()`:
/// DRF wraps the string in `non_field_errors` (`rest_framework/serializers.py:
/// 314-345`).
pub(crate) fn non_field_errors(message: &str) -> ViewResponse {
    plane_http_app::write::field_string_error("non_field_errors", message)
}

// ---------------------------------------------------------------------------
// Rendering a bare instance (`CycleSerializer(cycle).data`)
// ---------------------------------------------------------------------------

/// `CycleSerializer(cycle).data` for a row read straight from
/// `Cycle.objects.get` — the three estimate fields and the six counts are not
/// annotated, so DRF skips them.
fn render_plain_cycle(ctx: &ViewCtx<'_>, row: &CycleRow) -> Result<Vec<u8>, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    plane_serde::to_vec(
        plane_serde::Dialect::Drf,
        &PlainCycle(row).to_json(timezone),
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `json.dumps(CycleSerializer(cycle).data, cls=DjangoJSONEncoder)` — the
/// PATCH activity's `current_instance` (`cycle.py:551`).
fn plain_cycle_json(row: &CycleRow) -> Result<String, ViewError> {
    let value = PlainCycle(row).to_json(chrono_tz::UTC);
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, &value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// The body [`render_plain_cycle`] and [`plain_cycle_json`] share: the bare
/// model columns in `CycleSerializer`'s `_readable_fields` order.
struct PlainCycle<'a>(&'a CycleRow);

impl PlainCycle<'_> {
    fn to_json(&self, timezone: chrono_tz::Tz) -> plane_serde::pyjson::Value {
        use plane_serde::pyjson::{PyDict, PyStr, Value};
        let row = self.0;
        let mut obj = PyDict::with_capacity(22);
        let mut put = |key: &str, value: Value| {
            obj.insert(PyStr::new(key), value);
        };
        put("id", Value::from(row.id.to_string()));
        put("created_at", drf_datetime(row.created_at, timezone));
        put("updated_at", drf_datetime(row.updated_at, timezone));
        put(
            "deleted_at",
            row.deleted_at
                .map_or(Value::Null, |value| drf_datetime(value, timezone)),
        );
        put("name", Value::from(row.name.as_str()));
        put("description", Value::from(row.description.as_str()));
        put(
            "start_date",
            row.start_date
                .map_or(Value::Null, |value| drf_datetime(value, timezone)),
        );
        put(
            "end_date",
            row.end_date
                .map_or(Value::Null, |value| drf_datetime(value, timezone)),
        );
        put("view_props", json_column(row.view_props.as_ref()));
        put("sort_order", Value::from(row.sort_order));
        put(
            "external_source",
            row.external_source
                .as_deref()
                .map_or(Value::Null, Value::from),
        );
        put(
            "external_id",
            row.external_id.as_deref().map_or(Value::Null, Value::from),
        );
        put(
            "progress_snapshot",
            json_column(row.progress_snapshot.as_ref()),
        );
        put(
            "archived_at",
            row.archived_at
                .map_or(Value::Null, |value| drf_datetime(value, timezone)),
        );
        put("logo_props", json_column(row.logo_props.as_ref()));
        put("timezone", Value::from(row.timezone.as_str()));
        put("version", Value::from(row.version));
        put(
            "created_by",
            row.created_by
                .map_or(Value::Null, |id| Value::from(id.to_string())),
        );
        put(
            "updated_by",
            row.updated_by
                .map_or(Value::Null, |id| Value::from(id.to_string())),
        );
        put("project", Value::from(row.project.to_string()));
        put("workspace", Value::from(row.workspace.to_string()));
        put("owned_by", Value::from(row.owned_by.to_string()));
        Value::Dict(obj)
    }
}

/// DRF's `DateTimeField` output for an aware value:
/// `value.isoformat().replace("+00:00", "Z")` (`rest_framework/fields.py:1315-1330`).
fn drf_datetime(value: DateTime<Utc>, timezone: chrono_tz::Tz) -> plane_serde::pyjson::Value {
    use chrono::SecondsFormat;
    let local = value.with_timezone(&timezone);
    let mut text = local.to_rfc3339_opts(SecondsFormat::Micros, false);
    // `isoformat()` drops the microseconds when they are zero.
    if text.contains(".000000") {
        text = text.replace(".000000", "");
    }
    if text.ends_with("+00:00") {
        text.truncate(text.len() - 6);
        text.push('Z');
    }
    plane_serde::pyjson::Value::from(text)
}

fn json_column(value: Option<&JsonText>) -> plane_serde::pyjson::Value {
    match value {
        Some(text) => plane_serde::pyjson::loads(text.as_str().as_bytes(), "utf-8")
            .unwrap_or(plane_serde::pyjson::Value::Null),
        None => plane_serde::pyjson::Value::Null,
    }
}

// ---------------------------------------------------------------------------
// 279 POST — create
// ---------------------------------------------------------------------------

/// The `CycleCreateSerializer.validate` fields, after the generated field pass.
struct CycleWrite {
    name: Option<String>,
    description: String,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
    owned_by: Option<Uuid>,
    external_source: Option<String>,
    external_id: Option<String>,
    timezone: String,
    /// Whether the body carried each nullable field (explicit `null` included).
    start_date_present: bool,
    end_date_present: bool,
}

/// Read a finished `CycleCreateSerializer`/`CycleUpdateSerializer` pass.
fn cycle_write(values: &plane_http_app::write::Validated) -> Result<CycleWrite, ViewError> {
    Ok(CycleWrite {
        name: values.text("name").map(str::to_owned),
        description: values.text("description").unwrap_or_default().to_owned(),
        start_date: values.datetime("start_date"),
        end_date: values.datetime("end_date"),
        owned_by: values.uuid("owned_by"),
        external_source: values.text("external_source").map(str::to_owned),
        external_id: values.text("external_id").map(str::to_owned),
        timezone: values.text("timezone").unwrap_or("UTC").to_owned(),
        start_date_present: values.contains("start_date"),
        end_date_present: values.contains("end_date"),
    })
}

/// `CycleCreateSerializer.validate` (`api/serializers/cycle.py:61-98`): the
/// project lookup, the `cycle_view` gate, the date order, `convert_to_utc`
/// and the `owned_by` default.
async fn validate_cycle_write(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    partial: bool,
    write: &mut CycleWrite,
) -> Result<Option<ViewResponse>, ViewError> {
    // `Project.objects.filter(id=project_id).first()` (`:71-75`) — the default
    // manager, so a soft-deleted project is "Project not found".
    let project = client
        .query_opt(queries::CYCLE_PROJECT.sql, &[&project_id])
        .await
        .map_err(db_error)?;
    let Some(project) = project else {
        return Ok(Some(non_field_errors("Project not found")));
    };
    let cycle_view: bool = project.try_get(1).map_err(db_error)?;
    if !cycle_view {
        return Ok(Some(non_field_errors(
            "Cycles are not enabled for this project",
        )));
    }
    if let (Some(start), Some(end)) = (write.start_date, write.end_date) {
        if start > end {
            return Ok(Some(non_field_errors("Start date cannot exceed end date")));
        }
        let timezone: String = project.try_get(2).map_err(db_error)?;
        let project_timezone = chrono_tz::Tz::from_str(&timezone)
            .map_err(|_| ViewError::Unknown(format!("UnknownTimeZoneError: {timezone}")))?;
        // `convert_to_utc` re-reads the project itself (`timezone_converter.py:
        // 57`), so its timezone is the one just loaded.
        let now = plane_http_app::write::now();
        let active = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        write.start_date = Some(plane_http_app::cycles::write::convert_to_utc(
            start.with_timezone(&active).date_naive(),
            project_timezone,
            true,
            now,
        ));
        write.end_date = Some(plane_http_app::cycles::write::convert_to_utc(
            end.with_timezone(&active).date_naive(),
            project_timezone,
            false,
            now,
        ));
    }
    // `if not data.get("owned_by") and (self.instance is None or "owned_by" in
    // data)` (`:95-96`).
    if write.owned_by.is_none() && (!partial || has_key(ctx.data()?, "owned_by")) {
        write.owned_by = Some(issues::user_id(ctx)?);
    }
    Ok(None)
}

/// `CycleListCreateAPIEndpoint.post` (`api/views/cycle.py:301-357`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;
        ensure_mapping(data)?;
        // `(both None) or (both not None)` (`:307-309`).
        let start = has_value(data, "start_date");
        let end = has_value(data, "end_date");
        if start != end {
            return Ok(error_body(
                400,
                "Both start date and end date are either required or are to be null",
            ));
        }

        let mut serializer = plane_http_app::write::Serializer::new(ctx, false)?;
        plane_http_app::write::generated::cycle_create_serializer::validate(
            &mut serializer,
            client,
        )
        .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };
        let mut write = cycle_write(&values)?;
        if let Some(response) =
            validate_cycle_write(ctx, client, params.project_id, false, &mut write).await?
        {
            return Ok(response);
        }

        // The duplicate external id check (`:314-336`) runs before the INSERT.
        if let (Some(external_source), Some(external_id)) = (
            data_string(data, "external_source"),
            data_string(data, "external_id"),
        ) && data_truthy(data, "external_source")
            && data_truthy(data, "external_id")
        {
            let existing = client
                .query_opt(
                    queries::CYCLE_EXTERNAL_ID.sql,
                    &[
                        &params.project_id,
                        &params.slug,
                        &external_source,
                        &external_id,
                    ],
                )
                .await
                .map_err(db_error)?;
            if let Some(row) = existing {
                let id: Uuid = row.try_get(0).map_err(db_error)?;
                return Ok(conflict_body(
                    "Cycle with the same external id and external source already exists",
                    id,
                ));
            }
        }

        // `serializer.save(project_id=project_id)` → `Cycle.objects.create`
        // (`db/models/cycle.py:88-97`, H23 + H07 + H01).
        let project_row = client
            .query_opt(queries::CYCLE_PROJECT.sql, &[&params.project_id])
            .await
            .map_err(db_error)?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let workspace_id: Uuid = project_row.try_get(3).map_err(db_error)?;
        let sort_order = hooks::cycle::before_insert(client, params.project_id, 65535.0)
            .await
            .map_err(ViewError::from)?;
        let now = plane_http_app::write::now();
        let id = Uuid::new_v4();
        let insert = queries::CYCLE_INSERT
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &insert,
                &[
                    &id,
                    &now,
                    &now,
                    &Some(params.user_id),
                    &params.project_id,
                    &workspace_id,
                    &write.name,
                    &write.description,
                    &write.start_date,
                    &write.end_date,
                    &write.owned_by.unwrap_or(params.user_id),
                    &sort_order,
                    &write.external_source,
                    &write.external_id,
                    &write.timezone,
                    &1_i32,
                ],
            )
            .await
            .map_err(integrity_or_unknown)?;

        // `model_activity.delay(..., current_instance=None, ...)` (`:339-347`).
        publish_cycle_activity(ctx, &params.slug, id, None, params.user_id).await?;

        // `Cycle.objects.get(pk=serializer.instance.id)` (`:349`).
        let row = client
            .query_opt(queries::CYCLE_BY_PK.sql, &[&id])
            .await
            .map_err(db_error)?
            .ok_or_else(|| ViewError::Unknown("the created cycle vanished".to_owned()))?;
        let cycle = CycleRow::from_row(&row)?;
        Ok(ViewResponse::raw(
            201,
            APPLICATION_JSON,
            render_plain_cycle(ctx, &cycle)?,
        ))
    })
}

/// `Response({"error": …, "id": str(cycle.id)}, 409)` (`cycle.py:330-336`).
pub(crate) fn conflict_body(message: &str, id: Uuid) -> ViewResponse {
    let body = plane_query::json::Json::object(vec![
        ("error", plane_query::json::Json::string(message)),
        ("id", plane_query::json::Json::string(id.to_string())),
    ]);
    ViewResponse::raw(409, APPLICATION_JSON, body.to_drf())
}

/// `request.data.get(key)` when it is a string (`None` otherwise).
fn data_string<'a>(data: &'a Data, key: &str) -> Option<&'a str> {
    match data {
        Data::Json(plane_serde::pyjson::Value::Dict(map)) => {
            map.get(key).and_then(|value| match value {
                plane_serde::pyjson::Value::Str(text) => text.as_str(),
                _ => None,
            })
        }
        Data::Form(query) => query.get(key),
        Data::Json(_) => None,
    }
}

/// Python truthiness of `request.data.get(key)`.
fn data_truthy(data: &Data, key: &str) -> bool {
    let Some(value) = raw_body_field(data, key) else {
        return false;
    };
    use plane_serde::pyjson::Value as Py;
    match &value {
        Py::Null => false,
        Py::Bool(flag) => *flag,
        Py::Int(number) => number.as_i64() != Some(0),
        Py::Float(number) => *number != 0.0,
        Py::Str(text) => text.as_str().is_some_and(|text| !text.is_empty()),
        Py::List(items) => !items.is_empty(),
        Py::Dict(map) => map.iter().next().is_some(),
    }
}

/// A constraint violation on a write (`IntegrityError` → the family's 400),
/// anything else a 500.
pub(crate) fn integrity_or_unknown(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => db_error(error),
    }
}

/// `model_activity.delay(model_name="module", …)` (`module.py:232-240`,
/// `:490-498`).
pub(crate) async fn publish_model_activity(
    ctx: &ViewCtx<'_>,
    slug: &str,
    model_id: Uuid,
    current_instance: Option<String>,
    user: Uuid,
) -> Result<(), ViewError> {
    publish_model_activity_named(ctx, "module", slug, model_id, current_instance, user).await
}

/// `model_activity.delay(model_name="cycle", …)` (`cycle.py:339-347`,
/// `:595-603`).
async fn publish_cycle_activity(
    ctx: &ViewCtx<'_>,
    slug: &str,
    cycle_id: Uuid,
    current_instance: Option<String>,
    user: Uuid,
) -> Result<(), ViewError> {
    publish_model_activity_named(ctx, "cycle", slug, cycle_id, current_instance, user).await
}

async fn publish_model_activity_named(
    ctx: &ViewCtx<'_>,
    model_name: &str,
    slug: &str,
    cycle_id: Uuid,
    current_instance: Option<String>,
    user: Uuid,
) -> Result<(), ViewError> {
    let origin = plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let task = plane_jobs::celery::task_name::MODEL_ACTIVITY;
    let message =
        plane_jobs::celery::tasks::model_activity(plane_jobs::celery::tasks::ModelActivityCall {
            model_name: model_name.to_owned(),
            model_id: cycle_id.to_string(),
            requested_data: plane_http_app::write::request_data_arg(ctx)?,
            current_instance,
            actor_id: user,
            slug: slug.to_owned(),
            origin,
        });
    plane_http_app::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::new(task),
    )
    .await
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// 281 PATCH — update
// ---------------------------------------------------------------------------

/// `CycleDetailAPIEndpoint.patch` (`api/views/cycle.py:543-607`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let client = client(ctx)?;

        // `Cycle.objects.get(workspace__slug, project_id, pk)` (`:549`).
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
        // `current_instance = json.dumps(CycleSerializer(cycle).data, …)`
        // (`:551`) — before the archived guard.
        let current_instance = plain_cycle_json(&cycle)?;
        if cycle.archived_at.is_some() {
            return Ok(error_body(400, "Archived cycle cannot be edited"));
        }

        let data = ctx.data()?;
        if cycle
            .end_date
            .is_some_and(|end| end < plane_http_app::write::now())
            && !has_key(data, "sort_order")
        {
            return Ok(error_body(
                400,
                "The Cycle has already been completed so it cannot be edited",
            ));
        }

        let mut serializer = plane_http_app::write::Serializer::new(ctx, true)?;
        plane_http_app::write::generated::cycle_update_serializer::validate(
            &mut serializer,
            client,
        )
        .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };
        let mut write = cycle_write(&values)?;
        if let Some(response) =
            validate_cycle_write(ctx, client, params.project_id, true, &mut write).await?
        {
            return Ok(response);
        }

        // The duplicate check reports the **URL** object's id (`:575-591`).
        if let Some(external_id) = data_string(data, "external_id")
            && data_truthy(data, "external_id")
            && cycle.external_id.as_deref() != Some(external_id)
        {
            let source = data_string(data, "external_source")
                .map(str::to_owned)
                .or_else(|| cycle.external_source.clone());
            let existing = client
                .query_opt(
                    queries::CYCLE_EXTERNAL_ID.sql,
                    &[&params.project_id, &params.slug, &source, &external_id],
                )
                .await
                .map_err(db_error)?;
            if existing.is_some() {
                return Ok(conflict_body(
                    "Cycle with the same external id and external source already exists",
                    pk,
                ));
            }
        }

        // `serializer.save()` → `ModelSerializer.update` then `save()`
        // (H01/H07); the columns the serializer did not validate keep their
        // stored values.
        let now = plane_http_app::write::now();
        let name = write.name.clone().unwrap_or_else(|| cycle.name.clone());
        let description = if values.contains("description") {
            write.description
        } else {
            cycle.description.clone()
        };
        let start_date = if write.start_date_present {
            write.start_date
        } else {
            cycle.start_date
        };
        let end_date = if write.end_date_present {
            write.end_date
        } else {
            cycle.end_date
        };
        let owned_by = write.owned_by.unwrap_or(cycle.owned_by);
        let external_source = if values.contains("external_source") {
            write.external_source
        } else {
            cycle.external_source.clone()
        };
        let external_id = if values.contains("external_id") {
            write.external_id
        } else {
            cycle.external_id.clone()
        };
        let timezone = if values.contains("timezone") {
            write.timezone
        } else {
            cycle.timezone.clone()
        };
        let update = queries::CYCLE_UPDATE
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &update,
                &[
                    &name,
                    &description,
                    &start_date,
                    &end_date,
                    &owned_by,
                    &external_source,
                    &external_id,
                    &timezone,
                    &now,
                    &Some(params.user_id),
                    &pk,
                ],
            )
            .await
            .map_err(integrity_or_unknown)?;

        let row = client
            .query_opt(queries::CYCLE_BY_PK.sql, &[&pk])
            .await
            .map_err(db_error)?
            .ok_or_else(|| ViewError::Unknown("the updated cycle vanished".to_owned()))?;
        let cycle = CycleRow::from_row(&row)?;
        publish_cycle_activity(
            ctx,
            &params.slug,
            pk,
            Some(current_instance),
            params.user_id,
        )
        .await?;
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_plain_cycle(ctx, &cycle)?,
        ))
    })
}

// ---------------------------------------------------------------------------
// 282 POST — bulk add work items
// ---------------------------------------------------------------------------

/// `CycleIssue.objects` request ids: `UUIDField` coercion of every element of
/// `issue_id__in` (`django/db/models/fields/__init__.py:2482-2496`).
pub(crate) fn coerce_issue_ids(value: &plane_serde::pyjson::Value) -> Result<Vec<Uuid>, ViewError> {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::List(items) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Py::Str(text) => {
                        let text = text.as_str().unwrap_or_default();
                        ids.push(Uuid::parse_str(text).map_err(|_| ViewError::Validation)?);
                    }
                    // `UUIDField.to_python(None)` is `None`, which matches no
                    // row; Django accepts it inside `__in`.
                    Py::Null => {}
                    _ => return Err(ViewError::Validation),
                }
            }
            Ok(ids)
        }
        Py::Str(text) => {
            // A string iterates its characters, each failing the coercion.
            if text.as_str().is_none_or(str::is_empty) {
                return Ok(Vec::new());
            }
            Err(ViewError::Validation)
        }
        Py::Dict(map) => {
            if map.iter().next().is_none() {
                Ok(Vec::new())
            } else {
                Err(ViewError::Validation)
            }
        }
        Py::Int(number) => {
            if number.as_i64() == Some(0) {
                Ok(Vec::new())
            } else {
                Err(ViewError::Unknown(
                    "TypeError: 'int' object is not iterable".to_owned(),
                ))
            }
        }
        Py::Null => Ok(Vec::new()),
        _ => Err(ViewError::Unknown(
            "TypeError: object is not iterable".to_owned(),
        )),
    }
}

/// `CycleIssueListCreateAPIEndpoint.post` (`api/views/cycle.py:963-1070`).
pub fn issue_add<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        // `issues = request.data.get("issues", [])`; `if not issues`
        // (`:969-975`). A missing key and a `null` are both the 400.
        let Some(raw_issues) = raw_body_field(data, "issues") else {
            return Ok(missing_work_items());
        };
        let ids = coerce_issue_ids(&raw_issues)?;
        if ids.is_empty() && !body_issues_truthy(&raw_issues) {
            return Ok(missing_work_items());
        }

        // `Cycle.objects.get(workspace__slug, project_id, pk)` (`:977`).
        let cycle = client
            .query_opt(
                queries::CYCLE_ARCHIVE_LOOKUP.sql,
                &[&cycle_id, &params.project_id, &params.slug],
            )
            .await
            .map_err(db_error)?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let end_date: Option<DateTime<Utc>> = cycle.try_get(0).map_err(db_error)?;
        if end_date.is_some_and(|end| end < plane_http_app::write::now()) {
            let body = plane_query::json::Json::object(vec![
                ("code", plane_query::json::Json::string("CYCLE_COMPLETED")),
                (
                    "message",
                    plane_query::json::Json::string(
                        "The Cycle has already been completed so no new issues can be added",
                    ),
                ),
            ]);
            return Ok(ViewResponse::raw(400, APPLICATION_JSON, body.to_drf()));
        }

        // Existing rows of these issues in **another** cycle (`:989-999`), in
        // `-created_at` order.
        let rows = client
            .query(
                queries::CYCLE_ISSUES_IN_OTHER_CYCLES.sql,
                &[&cycle_id, &ids, &params.slug, &params.project_id],
            )
            .await
            .map_err(db_error)?;
        let mut existing: Vec<(Uuid, Uuid, Uuid)> = Vec::with_capacity(rows.len());
        for row in &rows {
            existing.push((
                row.try_get(0).map_err(db_error)?,
                row.try_get(1).map_err(db_error)?,
                row.try_get(2).map_err(db_error)?,
            ));
        }
        // `existing_issues` keeps only the ids the body carried, and
        // `new_issues = list(set(issues) - set(existing_issues))`; the set is
        // immediately re-ordered by the issue query (`:1000-1010`).
        let existing_ids: std::collections::HashSet<Uuid> = existing
            .iter()
            .filter(|(_, issue_id, _)| ids.contains(issue_id))
            .map(|(_, issue_id, _)| *issue_id)
            .collect();
        let mut candidates: Vec<Uuid> = Vec::with_capacity(ids.len());
        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            if !existing_ids.contains(id) && seen.insert(*id) {
                candidates.push(*id);
            }
        }
        let new_rows = client
            .query(
                queries::CYCLE_NEW_ISSUE_IDS.sql,
                &[&params.slug, &params.project_id, &candidates],
            )
            .await
            .map_err(db_error)?;
        let new_issues: Vec<Uuid> = new_rows
            .iter()
            .map(|row| row.try_get(0).map_err(db_error))
            .collect::<Result<_, _>>()?;

        // `bulk_create(..., ignore_conflicts=True, batch_size=10)`
        // (`:1013-1025`): one atomic insert per row, `created_at` increasing
        // with the batch order (auto_now_add runs per object), which is what
        // `-created_at` reverses. An issue already in this cycle is silently
        // skipped, and every instance — inserted or not — is still part of the
        // activity payload.
        let workspace_id: Uuid = client
            .query_opt(queries::CYCLE_PROJECT.sql, &[&params.project_id])
            .await
            .map_err(db_error)?
            .ok_or(ViewError::ObjectDoesNotExist)?
            .try_get(3)
            .map_err(db_error)?;
        let mut created: Vec<CreatedCycleIssue> = Vec::with_capacity(new_issues.len());
        let now = plane_http_app::write::now();
        client.batch_execute("BEGIN").await.map_err(db_error)?;
        let mut outcome = Ok(());
        for (index, issue_id) in new_issues.iter().enumerate() {
            let created_at = now + chrono::Duration::microseconds(index as i64);
            let row_id = Uuid::new_v4();
            let result = client
                .execute(
                    queries::CYCLE_ISSUE_INSERT.sql,
                    &[
                        &row_id,
                        &created_at,
                        &created_at,
                        &params.project_id,
                        &workspace_id,
                        issue_id,
                        &cycle_id,
                    ],
                )
                .await;
            match result {
                Ok(_) => created.push(CreatedCycleIssue {
                    id: row_id,
                    created_at,
                    project_id: params.project_id,
                    workspace_id,
                    issue_id: *issue_id,
                    cycle_id,
                }),
                Err(error) => {
                    outcome = Err(integrity_or_unknown(error));
                    break;
                }
            }
        }
        match outcome {
            Ok(()) => client.batch_execute("COMMIT").await.map_err(db_error)?,
            Err(error) => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(error);
            }
        }

        // `bulk_update(rows, ["cycle_id"])` (`:1047`).
        if !existing.is_empty() {
            let row_ids: Vec<Uuid> = existing.iter().map(|(id, _, _)| *id).collect();
            client
                .execute(queries::CYCLE_ISSUE_MOVE.sql, &[&cycle_id, &row_ids])
                .await
                .map_err(db_error)?;
        }

        // The activity (`:1050-1065`).
        let updated: Vec<plane_serde::pyjson::Value> = existing
            .iter()
            .map(|(_, issue_id, old_cycle_id)| {
                plane_serde::pyjson::Value::Dict(plane_serde::pyjson::PyDict::from_iter([
                    (
                        plane_serde::pyjson::PyStr::new("old_cycle_id"),
                        plane_serde::pyjson::Value::from(old_cycle_id.to_string()),
                    ),
                    (
                        plane_serde::pyjson::PyStr::new("new_cycle_id"),
                        plane_serde::pyjson::Value::from(cycle_id.to_string()),
                    ),
                    (
                        plane_serde::pyjson::PyStr::new("issue_id"),
                        plane_serde::pyjson::Value::from(issue_id.to_string()),
                    ),
                ]))
            })
            .collect();
        let created_serialized = serialize_cycle_issues(&created)?;
        let requested =
            plane_serde::pyjson::Value::Dict(plane_serde::pyjson::PyDict::from_iter([(
                plane_serde::pyjson::PyStr::new("cycles_list"),
                raw_issues.clone(),
            )]));
        let current = plane_serde::pyjson::Value::Dict(plane_serde::pyjson::PyDict::from_iter([
            (
                plane_serde::pyjson::PyStr::new("updated_cycle_issues"),
                plane_serde::pyjson::Value::List(updated),
            ),
            (
                plane_serde::pyjson::PyStr::new("created_cycle_issues"),
                plane_serde::pyjson::Value::from(created_serialized),
            ),
        ]));
        publish_issue_activity(
            ctx,
            ActivityShape::View,
            "cycle.activity.created",
            django_dumps(&requested)?,
            Some(django_dumps(&current)?),
            None,
            params.project_id,
            params.user_id,
            plane_http_app::write::now(),
        )
        .await?;

        // `CycleIssueSerializer(self.get_queryset(), many=True)` (`:1067-1070`).
        let rows = client
            .query(
                queries::CYCLE_ISSUE_LIST_ROWS.sql,
                &[&params.slug, &params.project_id, &params.user_id, &cycle_id],
            )
            .await
            .map_err(db_error)?;
        let body = render_through_rows(&rows, true)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

fn missing_work_items() -> ViewResponse {
    let body = plane_query::json::Json::object(vec![
        (
            "error",
            plane_query::json::Json::string("Work items are required"),
        ),
        (
            "code",
            plane_query::json::Json::string("MISSING_WORK_ITEMS"),
        ),
    ]);
    ViewResponse::raw(400, APPLICATION_JSON, body.to_drf())
}

/// `if not issues` for the raw body value.
fn body_issues_truthy(value: &plane_serde::pyjson::Value) -> bool {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Null => false,
        Py::Bool(flag) => *flag,
        Py::Int(number) => number.as_i64() != Some(0),
        Py::Float(number) => *number != 0.0,
        Py::Str(text) => text.as_str().is_some_and(|text| !text.is_empty()),
        Py::List(items) => !items.is_empty(),
        Py::Dict(map) => map.iter().next().is_some(),
    }
}

/// `json.dumps(value, cls=DjangoJSONEncoder)` for the activity payloads.
pub(crate) fn django_dumps(value: &plane_serde::pyjson::Value) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// One inserted `cycle_issues` row as `serializers.serialize("json", …)`
/// renders it (`_meta.local_fields`, `db.cycleissue`).
struct CreatedCycleIssue {
    id: Uuid,
    created_at: DateTime<Utc>,
    project_id: Uuid,
    workspace_id: Uuid,
    issue_id: Uuid,
    cycle_id: Uuid,
}

/// `serializers.serialize("json", created_records)` (`cycle.py:1059`): the
/// instances came out of `bulk_create(..., ignore_conflicts=True)`, so their
/// `created_by`/`updated_by` are `NULL` (the v1 constructor sets neither) and
/// the pk is the in-memory `uuid4`.
fn serialize_cycle_issues(rows: &[CreatedCycleIssue]) -> Result<String, ViewError> {
    use plane_http_app::write::django_serialize::{
        DjangoField, DjangoObject, DjangoValue, django_serialize,
    };
    let objects: Vec<DjangoObject<'_>> = rows
        .iter()
        .map(|row| DjangoObject {
            model: "db.cycleissue",
            pk: row.id,
            fields: vec![
                DjangoField {
                    name: "created_at",
                    value: DjangoValue::DateTime(row.created_at),
                },
                DjangoField {
                    name: "updated_at",
                    value: DjangoValue::DateTime(row.created_at),
                },
                DjangoField {
                    name: "created_by",
                    value: DjangoValue::OptUuid(None),
                },
                DjangoField {
                    name: "updated_by",
                    value: DjangoValue::OptUuid(None),
                },
                DjangoField {
                    name: "deleted_at",
                    value: DjangoValue::OptDateTime(None),
                },
                DjangoField {
                    name: "project",
                    value: DjangoValue::Uuid(row.project_id),
                },
                DjangoField {
                    name: "workspace",
                    value: DjangoValue::Uuid(row.workspace_id),
                },
                DjangoField {
                    name: "issue",
                    value: DjangoValue::Uuid(row.issue_id),
                },
                DjangoField {
                    name: "cycle",
                    value: DjangoValue::Uuid(row.cycle_id),
                },
            ],
        })
        .collect();
    django_serialize(&objects)
}

/// The `issue_activity` kwarg shapes the v1 cycle/module routes use.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityShape {
    /// `notification=True`, `origin` last (`cycle.py:1050-1065`).
    View,
    /// No `notification`, no `origin` (`cycle.py:1164-1177`).
    Bare,
    /// `origin` last, no `notification` (`module.py:563-578`, `:771-785`).
    WithOrigin,
}

/// `ModuleIssueSerializer(get_queryset(), many=True)`: the same row shape as
/// the cycle through-list with the last FK renamed.
pub(crate) fn render_module_issue_rows(rows: &[tokio_postgres::Row]) -> Result<Vec<u8>, ViewError> {
    render_through_rows(rows, false)
}

/// `issue_activity.delay(...)` in the shape the call site uses.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn publish_issue_activity(
    ctx: &ViewCtx<'_>,
    shape: ActivityShape,
    activity_type: &str,
    requested_data: String,
    current_instance: Option<String>,
    issue_id: Option<Uuid>,
    project_id: Uuid,
    user: Uuid,
    epoch: DateTime<Utc>,
) -> Result<(), ViewError> {
    let base = plane_jobs::celery::tasks::IssueActivityBase::new(
        activity_type,
        plane_jobs::celery::args::TaskArg::json_text(requested_data),
        user.to_string(),
        issue_id.map_or(plane_jobs::celery::args::TaskArg::Null, |id| {
            plane_jobs::celery::args::TaskArg::str(id.to_string())
        }),
        plane_jobs::celery::args::TaskArg::str(project_id.to_string()),
        current_instance.map_or(plane_jobs::celery::args::TaskArg::Null, |text| {
            plane_jobs::celery::args::TaskArg::json_text(text)
        }),
        epoch.timestamp(),
    );
    let call = match shape {
        ActivityShape::Bare => plane_jobs::celery::tasks::IssueActivityCall::bare(base),
        ActivityShape::View | ActivityShape::WithOrigin => {
            let origin = plane_auth::base_host(
                ctx.settings,
                plane_auth::LoginFlags {
                    is_app: true,
                    ..Default::default()
                },
            )
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
            if shape == ActivityShape::View {
                plane_jobs::celery::tasks::IssueActivityCall::view(base, origin)
            } else {
                plane_jobs::celery::tasks::IssueActivityCall::with_origin(base, origin)
            }
        }
    };
    let message = plane_jobs::celery::tasks::issue_activity(call);
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    plane_http_app::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::new(task),
    )
    .await
    .map(|_| ())
}

/// Render through-table rows with `CycleIssueSerializer`
/// (`with_count`) or `ModuleIssueSerializer` (`!with_count`; the two
/// serializers differ only in the last FK).
fn render_through_rows(
    rows: &[tokio_postgres::Row],
    with_count: bool,
) -> Result<Vec<u8>, ViewError> {
    let mut body = Vec::with_capacity(rows.len() * 320 + 2);
    body.push(b'[');
    for (index, row) in rows.iter().enumerate() {
        let id: Uuid = row.try_get(0).map_err(db_error)?;
        let count: i64 = row.try_get(1).map_err(db_error)?;
        let created_at: DateTime<Utc> = row.try_get(2).map_err(db_error)?;
        let updated_at: DateTime<Utc> = row.try_get(3).map_err(db_error)?;
        let deleted_at: Option<DateTime<Utc>> = row.try_get(4).map_err(db_error)?;
        let created_by: Option<Uuid> = row.try_get(5).map_err(db_error)?;
        let updated_by: Option<Uuid> = row.try_get(6).map_err(db_error)?;
        let project: Uuid = row.try_get(7).map_err(db_error)?;
        let workspace: Uuid = row.try_get(8).map_err(db_error)?;
        let issue: Uuid = row.try_get(9).map_err(db_error)?;
        let last: Uuid = row.try_get(10).map_err(db_error)?;
        if index != 0 {
            body.push(b',');
        }
        if with_count {
            let serializer = CycleIssueSerializer {
                id,
                sub_issues_count: FieldValue::Value(count),
                created_at,
                updated_at,
                deleted_at,
                created_by,
                updated_by,
                project,
                workspace,
                issue,
                cycle: last,
            };
            body.extend(
                plane_serializers::render(&serializer, &RenderCtx::utc())
                    .map_err(ViewError::from)?,
            );
        } else {
            let serializer = ModuleIssueSerializer {
                id,
                sub_issues_count: FieldValue::Value(count),
                created_at,
                updated_at,
                deleted_at,
                created_by,
                updated_by,
                project,
                workspace,
                module: issue,
                issue: last,
            };
            body.extend(
                plane_serializers::render(&serializer, &RenderCtx::utc())
                    .map_err(ViewError::from)?,
            );
        }
    }
    body.push(b']');
    Ok(body)
}

// ---------------------------------------------------------------------------
// 283 DELETE — remove one work item
// ---------------------------------------------------------------------------

/// `CycleIssueDetailAPIEndpoint.delete` (`api/views/cycle.py:1150-1178`).
pub fn issue_remove<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let client = client(ctx)?;

        let row = client
            .query_opt(
                queries::CYCLE_ISSUE_ROW.sql,
                &[&params.slug, &params.project_id, &cycle_id, &issue_id],
            )
            .await
            .map_err(db_error)?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let row_id: Uuid = row.try_get(0).map_err(db_error)?;
        let issue: Uuid = row.try_get(8).map_err(db_error)?;

        // `cycle_issue.delete()` — H03 (`db/mixins.py:72-80`).
        let now = plane_http_app::write::now();
        plane_http_app::write::soft_delete(
            client,
            "cycle_issues",
            "id",
            row_id,
            Some(params.user_id),
            now,
        )
        .await?;
        // `soft_delete_related_objects.delay("db", "cycleissue", pk)`
        // (`db/mixins.py:72-80`).
        plane_http_app::write::publish_soft_delete_cascade(ctx, "db", "cycleissue", row_id).await?;

        let requested = plane_serde::pyjson::Value::Dict(plane_serde::pyjson::PyDict::from_iter([
            (
                plane_serde::pyjson::PyStr::new("cycle_id"),
                plane_serde::pyjson::Value::from(cycle_id.to_string()),
            ),
            (
                plane_serde::pyjson::PyStr::new("issues"),
                plane_serde::pyjson::Value::List(vec![plane_serde::pyjson::Value::from(
                    issue.to_string(),
                )]),
            ),
        ]));
        publish_issue_activity(
            ctx,
            ActivityShape::Bare,
            "cycle.activity.deleted",
            django_dumps(&requested)?,
            None,
            Some(issue),
            params.project_id,
            params.user_id,
            now,
        )
        .await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// 284 POST — transfer work items
// ---------------------------------------------------------------------------

/// `TransferCycleIssueAPIEndpoint.post` (`api/views/cycle.py:1231-1274`): the
/// two v1-only pre-checks, then the app surface's kernel.
pub fn transfer<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let cycle_id = uuid_kwarg(ctx, "cycle_id")?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        // `new_cycle_id = request.data.get("new_cycle_id", False)` (`:1237`).
        let Some(raw) = raw_body_field(data, "new_cycle_id") else {
            return Ok(error_body(400, "New Cycle Id is required"));
        };
        let _new_cycle_id = match &raw {
            plane_serde::pyjson::Value::Str(text) => {
                let text = text.as_str().unwrap_or_default();
                if text.is_empty() {
                    return Ok(error_body(400, "New Cycle Id is required"));
                }
                Uuid::parse_str(text).map_err(|_| ViewError::Validation)?
            }
            value if !body_issues_truthy(value) => {
                return Ok(error_body(400, "New Cycle Id is required"));
            }
            _ => return Err(ViewError::Validation),
        };

        // `old_cycle = Cycle.objects.get(...)` (`:1245-1249`) — a 404.
        let old = client
            .query_opt(
                queries::CYCLE_ARCHIVE_LOOKUP.sql,
                &[&cycle_id, &params.project_id, &params.slug],
            )
            .await
            .map_err(db_error)?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let end_date: Option<DateTime<Utc>> = old.try_get(0).map_err(db_error)?;
        if end_date.is_some_and(|end| end > plane_http_app::write::now()) {
            return Ok(error_body(400, "The old cycle is not completed yet"));
        }

        // `transfer_cycle_issues(...)` (`utils/cycle_transfer_issues.py:35`),
        // the same kernel the app route uses.
        plane_http_app::cycles::write_transfer::post(ctx).await
    })
}
