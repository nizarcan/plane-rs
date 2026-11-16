// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/module.py` — `WorkspaceModulesEndpoint.get`
//! (`GET /api/workspaces/{slug}/modules/`).
//!
//! `ModuleSerializer(modules, many=True)`: the fields the queryset does not
//! annotate (`total_estimate_points`, `completed_estimate_points`,
//! `is_favorite`) are skipped by DRF's `SkipField` rule, so the body has none
//! of them. Datetimes render in the request user's profile timezone.

use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;
use plane_serializers::generated::app_module::ModuleSerializer;

use crate::common::{client, jobs};
use crate::modules::base::{ModuleAnnotations, decode_workspace_row, fetch};
use crate::modules::queries;

/// `WorkspaceModulesEndpoint.get` (`workspace/module.py:25-129`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user_id = jobs::user_id(ctx)?;
        let rows = fetch(client, &queries::WORKSPACE_MODULES, &[&slug, &user_id]).await?;
        let decoded = rows
            .iter()
            .map(decode_workspace_row)
            .collect::<Result<Vec<_>, _>>()?;
        let modules: Vec<ModuleSerializer<'_>> = decoded
            .iter()
            .map(|row| row.serializer(ModuleAnnotations::Workspace))
            .collect();
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let bytes = plane_serializers::render_list(&modules, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(
            200,
            plane_core::error::APPLICATION_JSON,
            bytes,
        ))
    })
}
