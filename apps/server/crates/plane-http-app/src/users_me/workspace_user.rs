// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/user.py` — the per-workspace graphs and the
//! "last visited workspace" lookup.
//!
//! * `GET /api/users/me/workspaces/{slug}/activity-graph/` —
//!   `UserActivityGraphEndpoint.get`
//! * `GET /api/users/me/workspaces/{slug}/issues-completed-graph/` —
//!   `UserIssueCompletedGraphEndpoint.get`
//! * `GET /api/users/last-visited-workspace/` —
//!   `UserLastProjectWithWorkspaceEndpoint.get`
//!
//! The two graph views are `IsAuthenticated` only and filter by the URL slug,
//! so an authenticated user sees their own activity in any workspace — the
//! upstream behaviour, ported as it is.
//!
//! `last-visited-workspace` is a 500 in Django: the view reads
//! `user.last_workspace_id`, but `last_workspace_id` lives on `Profile`, not
//! `User` (`plane/app/views/workspace/user.py:89`;
//! `plane/db/models/user.py:236`). That `AttributeError` reaches
//! `BaseAPIView.handle_exception` and becomes its generic 500 body. The port
//! returns the same error without issuing Django's `User.objects.get`, which
//! has no observable side effect.

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;
use plane_serde::values::PyDate;
use serde::Serialize;

use super::queries;
use super::support;

/// Register this module's handlers.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.workspace.user.UserActivityGraphEndpoint")?
        .method(Method::GET, activity_graph)?;
    registry
        .api_view("plane.app.views.workspace.user.UserIssueCompletedGraphEndpoint")?
        .method(Method::GET, issues_completed_graph)?;
    registry
        .api_view("plane.app.views.workspace.user.UserLastProjectWithWorkspaceEndpoint")?
        .method(Method::GET, last_visited_workspace)?;
    Ok(())
}

#[derive(Serialize)]
struct ActivityGraphRow {
    created_date: PyDate,
    activity_count: i64,
}

/// `UserActivityGraphEndpoint.get`
/// (`plane/app/views/workspace/user.py:565-579`).
fn activity_graph<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user_id = support::user_id(ctx)?;
        let since = support::months_ago(6);
        let client = crate::common::client(ctx)?;
        let statement = queries::ACTIVITY_GRAPH.prepare(client).await?;
        let rows = client.query(&statement, &[&user_id, &since, &slug]).await?;
        let items = rows
            .iter()
            .map(|row| {
                Ok::<_, tokio_postgres::Error>(ActivityGraphRow {
                    created_date: PyDate(row.try_get(0)?),
                    activity_count: row.try_get(1)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let body = plane_serde::to_vec(Dialect::Drf, &items)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

#[derive(Serialize)]
struct CompletedGraphRow {
    week: i32,
    completed_count: i64,
}

/// `UserIssueCompletedGraphEndpoint.get`
/// (`plane/app/views/workspace/user.py:582-600`).
fn issues_completed_graph<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user_id = support::user_id(ctx)?;
        let month = super::workspace_base::month_parameter(ctx)?;
        let client = crate::common::client(ctx)?;
        let statement = queries::ISSUES_COMPLETED_GRAPH.prepare(client).await?;
        let rows = client.query(&statement, &[&user_id, &month, &slug]).await?;
        let items = rows
            .iter()
            .map(|row| {
                Ok::<_, tokio_postgres::Error>(CompletedGraphRow {
                    week: row.try_get(0)?,
                    completed_count: row.try_get(1)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let body = plane_serde::to_vec(Dialect::Drf, &items)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `UserLastProjectWithWorkspaceEndpoint.get`
/// (`plane/app/views/workspace/user.py:85-106`).
fn last_visited_workspace<'a>(
    _ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `User` has no `last_workspace_id`; `Profile` does. Django raises
        // `AttributeError` and the family handler answers its 500 body.
        Err(ViewError::Unknown(
            "AttributeError: 'User' object has no attribute 'last_workspace_id'".to_string(),
        ))
    })
}
