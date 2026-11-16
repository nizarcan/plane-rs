// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.state.base.StateViewSet` — the two read actions.
//!
//! Python: `apps/api/plane/app/views/state/base.py:19-117`.
//!
//! ## `list` (`GET …/projects/{project_id}/states/`)
//!
//! `plane/app/views/state/base.py:82-110`. The view serialises every state of
//! the project, then **adds an `order` key in Python** — the position of the
//! state inside its group over the number of states in that group:
//!
//! ```python
//! grouped_states = defaultdict(list)
//! for state in states:
//!     grouped_states[state["group"]].append(state)
//! for group, group_states in grouped_states.items():
//!     count = len(group_states)
//!     for index, state in enumerate(group_states, start=1):
//!         state["order"] = index / count
//! ```
//!
//! Two consequences are behaviour and are reproduced:
//!
//! * with `?grouped=true` the body is an object keyed by group, and the group
//!   order is `sorted(states, key=lambda state: state["group"])` — alphabetical,
//!   with the states inside a group in their original (sequence) order because
//!   Python's sort is stable;
//! * without it the body is the flat list, and every entry carries `order`.
//!
//! `StateSerializer` declares `order` as a `FloatField(required=False)`
//! (`plane/app/serializers/state.py:10`) and a `State` model has no `order`
//! attribute, so DRF raises `SkipField` for it on the model and the key is
//! absent until the view's own assignment appends it in its declared position
//! (last, after `sequence`).
//!
//! ## `retrieve` (`GET …/projects/{project_id}/states/{pk}/`)
//!
//! `plane/app/views/state/base.py:47-55` is `super().retrieve(...)` — DRF's
//! `GenericAPIView.get_object()` over the same queryset, so a state outside the
//! caller's projects (or archived/triage) is a 404, not a 403. No `order` key:
//! nothing assigns it.

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::state::State;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_state::StateSerializer;

use crate::common::render::RequestRender;
use crate::common::{client, lookup, render_list};
use crate::states::queries;

/// One state, with the group-relative `order` the view computes.
struct OrderedState {
    state: State,
    order: f64,
}

/// `StateViewSet.list` (`plane/app/views/state/base.py:82-110`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let member_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

        let rows = client(ctx)?
            .query(
                queries::STATES_BY_PROJECT.sql,
                &[&slug, &project_id, &member_id],
            )
            .await?;
        let mut states = Vec::with_capacity(rows.len());
        for row in &rows {
            states.push(State::from_row(row, 0)?);
        }

        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_states(ctx, states)?,
        ))
    })
}

/// `StateViewSet.retrieve` (`plane/app/views/state/base.py:47-55`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let state_id = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let member_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

        let row = client(ctx)?
            .query_opt(
                queries::STATE_IN_PROJECT.sql,
                &[&slug, &project_id, &member_id, &state_id],
            )
            .await?;
        let Some(row) = row else {
            // `get_object()` raises `Http404("No State matches the given query.")`.
            return crate::states::not_found::no_match("State");
        };
        let state = State::from_row(&row, 0)?;

        let serializer = StateSerializer {
            id: state.id,
            project_id: state.project_id,
            workspace_id: state.workspace_id,
            name: &state.name,
            color: &state.color,
            group: &state.group,
            default: state.default,
            description: &state.description,
            sequence: state.sequence,
            order: None,
        };
        let body = crate::common::render(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// The body of `list`, including the `grouped=true` shape.
fn render_states(ctx: &ViewCtx<'_>, states: Vec<State>) -> Result<Vec<u8>, ViewError> {
    // `grouped_states[state["group"]].append(state)`, in query order.
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for state in &states {
        *counts.entry(state.group.clone()).or_insert(0) += 1;
    }

    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut ordered: Vec<OrderedState> = Vec::with_capacity(states.len());
    for state in states {
        let seen_in_group = seen.entry(state.group.clone()).or_insert(0);
        *seen_in_group += 1;
        let count = counts.get(state.group.as_str()).copied().unwrap_or(0);
        // `index / count` on two Python ints is a float division.
        let order = *seen_in_group as f64 / count as f64;
        ordered.push(OrderedState { state, order });
    }

    let prepared = RequestRender::from_ctx(ctx);
    let render_ctx = prepared.render_ctx();

    // `grouped` is `request.GET.get("grouped", False)` compared to the string
    // `"true"`, so any other value (including an empty one) takes the flat path.
    let grouped = ctx
        .query()
        .get("grouped")
        .is_some_and(|value| value == "true");

    if !grouped {
        let serializers: Vec<StateSerializer<'_>> = ordered
            .iter()
            .map(|entry| serializer_for(&entry.state, Some(entry.order)))
            .collect();
        return render_list(&serializers, &render_ctx);
    }

    // `sorted(states, key=state["group"])`: Python's stable sort by the group
    // name, i.e. byte order for ASCII group slugs.
    ordered.sort_by(|left, right| left.state.group.cmp(&right.state.group));

    let mut body = Vec::with_capacity(ordered.len() * 128);
    body.push(b'{');
    let mut first_group = true;
    let mut index = 0;
    while index < ordered.len() {
        let group = ordered[index].state.group.clone();
        let start = index;
        while index < ordered.len() && ordered[index].state.group == group {
            index += 1;
        }
        if !first_group {
            body.push(b',');
        }
        first_group = false;
        let key = plane_serde::to_vec(plane_serde::Dialect::Drf, &group)?;
        body.extend_from_slice(&key);
        body.push(b':');
        let entries: Vec<StateSerializer<'_>> = ordered[start..index]
            .iter()
            .map(|entry| serializer_for(&entry.state, Some(entry.order)))
            .collect();
        body.extend_from_slice(&render_list(&entries, &render_ctx)?);
    }
    body.push(b'}');
    Ok(body)
}

/// `StateSerializer(state)`, or the model plus the view-assigned `order`.
fn serializer_for(state: &State, order: Option<f64>) -> StateSerializer<'_> {
    StateSerializer {
        id: state.id,
        project_id: state.project_id,
        workspace_id: state.workspace_id,
        name: &state.name,
        color: &state.color,
        group: &state.group,
        default: state.default,
        description: &state.description,
        sequence: state.sequence,
        order,
    }
}
