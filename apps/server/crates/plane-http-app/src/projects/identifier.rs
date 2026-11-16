// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.project.base.ProjectIdentifierEndpoint.get`
//! (`GET /api/workspaces/{slug}/project-identifiers/`).
//!
//! Python: `apps/api/plane/app/views/project/base.py:445-455`.
//!
//! The body is built in the view (`{"exists": len(exists), "identifiers":
//! exists}`) from a `.values("id", "name", "project")` queryset, so the keys are
//! in the view's order and each identifier row is a plain object. `name` is
//! `request.GET.get("name", "").strip().upper()`; an empty one is a 400 with
//! `{"error": "Name is required"}`.

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::DrfUuid;
use uuid::Uuid;

use crate::common::client;
use crate::projects::queries;

/// One `.values("id", "name", "project")` row.
#[derive(serde::Serialize)]
struct IdentifierRow {
    id: i64,
    name: String,
    project: DrfUuid,
}

/// `{"exists": n, "identifiers": [...]}`.
#[derive(serde::Serialize)]
struct Body {
    exists: i64,
    identifiers: Vec<IdentifierRow>,
}

/// `{"error": …}`.
#[derive(serde::Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

/// `ProjectIdentifierEndpoint.get` (`plane/app/views/project/base.py:446-455`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        // `request.GET.get("name", "").strip().upper()`.
        let name = ctx.query().get("name").unwrap_or("").trim().to_uppercase();

        if name.is_empty() {
            let rendered = plane_serde::to_vec(
                plane_serde::Dialect::Drf,
                &ErrorBody {
                    error: "Name is required",
                },
            )?;
            return Ok(ViewResponse::raw(400, APPLICATION_JSON, rendered));
        }

        let statement = queries::PROJECT_IDENTIFIERS.prepare(client(ctx)?).await?;
        let rows = client(ctx)?.query(&statement, &[&name, &slug]).await?;

        // `len(exists)` evaluates the queryset once, so the count is the row
        // count of the one query.
        let identifiers: Vec<IdentifierRow> = rows
            .iter()
            .map(|row| IdentifierRow {
                id: row.get(0),
                name: row.get(1),
                project: DrfUuid(row.get::<_, Uuid>(2)),
            })
            .collect();
        let body = Body {
            exists: identifiers.len() as i64,
            identifiers,
        };
        let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &body)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, rendered))
    })
}
