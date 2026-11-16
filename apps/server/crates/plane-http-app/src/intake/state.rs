// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.state.base.IntakeStateEndpoint.get`
//! (`plane/app/views/state/base.py:151-161`).
//!
//! The route lives under `intake` in `deploy/route-groups.yaml`
//! (`/api/workspaces/*/projects/*/intake-state/`) even though the class is in
//! `state/base.py`, so it is registered here.
//!
//! `State.triage_objects.filter(workspace__slug=…, project_id=…).first()` uses
//! the triage manager (`deleted_at IS NULL AND group = 'triage'`,
//! `plane/db/models/state.py:66-68`) and `Meta.ordering = ("sequence",)`. No
//! triage state is the view's own `404 {"error": "Triage state not found"}`.
//!
//! The serializer is `StateSerializer`, whose `order` field
//! (`FloatField(required=False)`) has no model attribute, so DRF raises
//! `SkipField` and the key is absent.

use plane_core::error::APPLICATION_JSON;
use plane_db::models::state::State;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::FieldValue;
use plane_serializers::generated::app_state::StateSerializer;

use super::queries;
use super::render::plain_context;
use crate::issues::lists::list_common::{client, error_body, kwarg_text, kwarg_uuid};

/// Register the route.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.state.base.IntakeStateEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

/// `IntakeStateEndpoint.get`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(queries::TRIAGE_STATE.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&statement, &[&project_id, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Ok(error_body(404, "Triage state not found"));
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
            // `State` has no `order` attribute and nothing assigns one on this
            // route, so DRF skips the key (`rest_framework/fields.py:421-446`).
            order: FieldValue::absent(),
        };
        let context = plain_context(ctx);
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            plane_serializers::render(&serializer, &context)?,
        ))
    })
}
