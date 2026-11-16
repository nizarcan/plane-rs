// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/project/member.py` — the caller's project roles.
//!
//! `GET /api/users/me/workspaces/{slug}/project-roles/`
//! (`UserProjectRolesEndpoint.get`, `member.py:391-405`), behind
//! `WorkspaceUserPermission`.
//!
//! The body is a plain dict `{project_id: role}` built from a `values()` query
//! in the query's order (default `-created_at`), so the JSON key order is the
//! row order. The dictionary is rendered through `Dialect::Drf` on an
//! insertion-ordered [`plane_core::json::Object`].

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::queries;
use super::support;

/// Register this module's handler.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.project.member.UserProjectRolesEndpoint")?
        .method(Method::GET, get)?;
    Ok(())
}

/// `UserProjectRolesEndpoint.get` (`plane/app/views/project/member.py:395-405`).
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user_id = support::user_id(ctx)?;
        let client = crate::common::client(ctx)?;
        let statement = queries::USER_PROJECT_ROLES.prepare(client).await?;
        let rows = client.query(&statement, &[&slug, &user_id]).await?;
        let mut roles = Object::new();
        for row in &rows {
            let project_id: uuid::Uuid = row.try_get(0)?;
            let role: i16 = row.try_get(1)?;
            roles = roles.field(project_id.to_string(), i64::from(role));
        }
        let body = plane_serde::to_vec(plane_serde::Dialect::Drf, &roles)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
