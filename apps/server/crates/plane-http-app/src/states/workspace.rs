// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.workspace.state.WorkspaceStatesEndpoint`
//! (`GET /api/workspaces/{slug}/states/`).
//!
//! Python: `apps/api/plane/app/views/workspace/state.py:18-41`. One query, no
//! serialiser arguments, `WorkspaceEntityPermission` (workspace member,
//! enforced by the runtime).
//!
//! The view assigns `state.order = index / count` **on the model instance**
//! before serialising, so `order` is the serializer's own field here (declared
//! last, after `sequence`) rather than a key appended by the view — the two
//! paths agree on the key's position by coincidence, not by construction.
//!
//! Unlike `StateViewSet`, this endpoint does not filter
//! `project__deleted_at__isnull=True`: states of a *soft-deleted* project are
//! returned for its active members (`workspace/state.py:20-26`, ported as-is;
//! see `queries::WORKSPACE_STATES`).

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::state::State;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_state::StateSerializer;

use crate::common::render::RequestRender;
use crate::common::{client, render_list};
use crate::states::queries;

/// `WorkspaceStatesEndpoint.get` (`plane/app/views/workspace/state.py:21-41`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let member_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

        let rows = client(ctx)?
            .query(queries::WORKSPACE_STATES.sql, &[&member_id, &slug])
            .await?;

        let mut states = Vec::with_capacity(rows.len());
        for row in &rows {
            states.push(State::from_row(row, 0)?);
        }

        // Build `grouped_states[state.group]` in query order to get each group's
        // count, then `order = index / count` per state, exactly as the Python
        // loops do (`workspace/state.py:28-35`).
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for state in &states {
            *counts.entry(state.group.as_str()).or_insert(0) += 1;
        }
        let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        let mut orders = Vec::with_capacity(states.len());
        for state in &states {
            let seen_in_group = seen.entry(state.group.as_str()).or_insert(0);
            *seen_in_group += 1;
            let count = counts.get(state.group.as_str()).copied().unwrap_or(0);
            orders.push(*seen_in_group as f64 / count as f64);
        }

        let serializers: Vec<StateSerializer<'_>> = states
            .iter()
            .zip(orders)
            .map(|(state, order)| StateSerializer {
                id: state.id,
                project_id: state.project_id,
                workspace_id: state.workspace_id,
                name: &state.name,
                color: &state.color,
                group: &state.group,
                default: state.default,
                description: &state.description,
                sequence: state.sequence,
                order: Some(order),
            })
            .collect();

        let prepared = RequestRender::from_ctx(ctx);
        let body = render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
