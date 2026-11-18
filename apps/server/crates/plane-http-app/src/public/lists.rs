// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The five `{anchor}` list endpoints that return a plain `.values()` array:
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | 250 `anchor/{anchor}/cycles/` | `ProjectCyclesEndpoint` | `plane/space/views/cycle.py:15-28` |
//! | 251 `anchor/{anchor}/modules/` | `ProjectModulesEndpoint` | `module.py:15-28` |
//! | 252 `anchor/{anchor}/states/` | `ProjectStatesEndpoint` | `state.py:18-32` |
//! | 253 `anchor/{anchor}/labels/` | `ProjectLabelsEndpoint` | `label.py:15-28` |
//! | 254 `anchor/{anchor}/members/` | `ProjectMembersEndpoint` | `project.py:65-86` |
//!
//! All five are `AllowAny`, all five resolve the board with
//! `.filter(anchor=anchor).first()` (**no** `entity_name` filter) and answer
//! `{"error": "Invalid anchor"}` when it is missing.
//!
//! The rows are `.values()` dicts, so the JSON key order is the argument order
//! of the `values()` call and every value is the raw column: `parent` is the
//! FK column (`parent_id`) and `sequence` is the float column. Nothing here is
//! `?fields=`-aware: a `Response(queryset)` renders the dicts as they are.
//!
//! `DeployBoard.project_id` may be NULL, and the cycle/module/state/label
//! lookups then filter `project_id IS NULL`; the members endpoint compares the
//! FK objects by id, which is the same predicate.

use deadpool_postgres::Client;
use http::Method;
use plane_db::models::deploy_board::DeployBoard;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::json::Json;
use tokio_postgres::types::Type;
use uuid::Uuid;

use super::board;
use super::queries;
use super::{ok, plain_context};
use crate::common::client;
use crate::write::error_body;

/// Register the five list routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.space.views.cycle.ProjectCyclesEndpoint")?
        .method(Method::GET, cycles)?;
    registry
        .api_view("plane.space.views.module.ProjectModulesEndpoint")?
        .method(Method::GET, modules)?;
    registry
        .api_view("plane.space.views.state.ProjectStatesEndpoint")?
        .method(Method::GET, states)?;
    registry
        .api_view("plane.space.views.label.ProjectLabelsEndpoint")?
        .method(Method::GET, labels)?;
    registry
        .api_view("plane.space.views.project.ProjectMembersEndpoint")?
        .method(Method::GET, members)?;
    Ok(())
}

/// The two values every statement below binds, and the 404 the view writes
/// when its `.first()` lookup misses.
struct BoardScope {
    slug: String,
    project_id: Option<Uuid>,
}

/// `<view> = DeployBoard.objects.filter(anchor=anchor).first()`; `None` is the
/// view's `{"error": "Invalid anchor"}` body.
async fn board_scope(ctx: &ViewCtx<'_>) -> Result<Option<BoardScope>, ViewError> {
    let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
    let client = client(ctx)?;
    let Some(board) = board::board_by_anchor(client, anchor).await? else {
        return Ok(None);
    };
    let slug = super::workspace_slug(client, board.workspace_id).await?;
    Ok(Some(BoardScope {
        slug,
        project_id: board.project_id,
    }))
}

/// Render one `.values()` array: one object per row, `keys` in `values()`
/// order, every value the raw column.
fn render_rows(rows: &[tokio_postgres::Row], keys: &[&str]) -> Json {
    Json::Array(
        rows.iter()
            .map(|row| {
                Json::Object(
                    keys.iter()
                        .enumerate()
                        .map(|(index, key)| ((*key).to_owned(), scalar(row, index)))
                        .collect(),
                )
            })
            .collect(),
    )
}

/// One scalar decoded by its SQL type, as DRF's JSON encoder writes it.
fn scalar(row: &tokio_postgres::Row, index: usize) -> Json {
    let sql_type = row.columns().get(index).map(tokio_postgres::Column::type_);
    match sql_type {
        Some(&Type::UUID) => Json::opt_uuid(row.try_get::<_, Option<Uuid>>(index).ok().flatten()),
        Some(&Type::FLOAT8) => Json::Float(row.try_get::<_, f64>(index).unwrap_or_default()),
        Some(&Type::INT4) => Json::Int(i64::from(row.try_get::<_, i32>(index).unwrap_or_default())),
        Some(&Type::INT8) => Json::Int(row.try_get::<_, i64>(index).unwrap_or_default()),
        Some(&Type::DATE) => Json::opt_date(
            row.try_get::<_, Option<chrono::NaiveDate>>(index)
                .ok()
                .flatten(),
        ),
        _ => Json::opt_string(row.try_get::<_, Option<String>>(index).ok().flatten()),
    }
}

/// Run one statement whose binds are `(slug, project_id)`.
async fn list_rows(
    client: &Client,
    query: &plane_db::Query,
    scope: &BoardScope,
    keys: &[&str],
) -> Result<Json, ViewError> {
    let statement = query.prepare(client).await?;
    let rows = client
        .query(&statement, &[&scope.slug, &scope.project_id])
        .await?;
    Ok(render_rows(&rows, keys))
}

/// One {@link list_rows} handler: the anchor's rows, or "Invalid anchor".
macro_rules! list_handler {
    ($name:ident, $statement:path, [$($key:literal),* $(,)?]) => {
        fn $name<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
            Box::pin(async move {
                let Some(scope) = board_scope(ctx).await? else {
                    return Ok(error_body(404, "Invalid anchor"));
                };
                let client = client(ctx)?;
                let body = list_rows(client, &$statement, &scope, &[$($key),*]).await?;
                let _ = plain_context(ctx);
                Ok(ok(body.to_drf()))
            })
        }
    };
}

list_handler!(cycles, queries::CYCLE_NAMES, ["id", "name"]);
list_handler!(modules, queries::MODULE_NAMES, ["id", "name"]);
list_handler!(
    states,
    queries::STATE_ROWS,
    ["name", "group", "color", "id", "sequence"]
);
list_handler!(
    labels,
    queries::LABEL_ROWS,
    ["id", "name", "color", "parent"]
);

/// `ProjectMembersEndpoint.get` (`plane/space/views/project.py:68-86`).
fn members<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let client = client(ctx)?;
        let Some(board) = board::board_by_anchor(client, anchor).await? else {
            return Ok(error_body(404, "Invalid anchor"));
        };
        let statement = queries::MEMBER_ROWS.prepare(client).await?;
        let rows = client
            .query(&statement, &[&board.project_id, &board.workspace_id])
            .await?;
        let body = render_rows(
            &rows,
            &["id", "member", "member__display_name", "member__avatar"],
        );
        Ok(ok(body.to_drf()))
    })
}

/// `DeployBoard` is only named by [`members`]; keep the import meaningful for
/// readers of the module docs.
#[allow(dead_code)]
fn _board_type(_: &DeployBoard) {}
