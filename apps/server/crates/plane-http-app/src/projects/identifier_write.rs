// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.project.base.ProjectIdentifierEndpoint.delete`
//! (`DELETE /api/workspaces/{slug}/project-identifiers/`) — workspace-core
//! set A, route 138.
//!
//! Python: `apps/api/plane/app/views/project/base.py:457-472`.
//!
//! ```python
//! name = request.data.get("name", "").strip().upper()
//! if name == "":
//!     return Response({"error": "Name is required"}, 400)
//! if Project.objects.filter(identifier=name, workspace__slug=slug).exists():
//!     return Response({"error": "Cannot delete an identifier of an existing project"}, 400)
//! ProjectIdentifier.objects.filter(name=name, workspace__slug=slug).delete()
//! return Response(204)
//! ```
//!
//! Three details that are behaviour:
//!
//! * `request.data.get` on a body that is not a mapping (a list, a scalar,
//!   `null`) is an `AttributeError`, i.e. the family 500 — 138 is one of the
//!   raw-read handlers (`plans/WORKSPACES_CORE.md` §4);
//! * a `name` that is not a string (`null`, a number, a list) has no
//!   `.strip()`: another 500, before the empty-name 400;
//! * the delete is H04, a queryset `delete()`: **only** `deleted_at` is
//!   written (no `updated_at`, no audit column) and no cascade task is
//!   published (`plane/db/mixins.py:33-38`). The statement is
//!   `workspaces::write_queries::PROJECT_IDENTIFIERS_SOFT_DELETE`, the P0's.
//!
//! The permission is `allow_permission([ADMIN, MEMBER], level="WORKSPACE")`
//! (`:457`), which the runtime enforces before this handler.

use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;

use crate::common::client;
use crate::views::write_support::{Body, body_get};
use crate::workspaces::write_a_queries as queries;
use crate::workspaces::write_queries as shared_queries;
use crate::write;

/// `ProjectIdentifierEndpoint.delete` (`plane/app/views/project/base.py:457-472`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let client = client(ctx)?;

        // `request.data.get("name", "").strip().upper()` (`:459`).
        let name = match body_get(ctx.data()?, "name")? {
            Body::Missing => String::new(),
            Body::Form(text) => text.trim().to_uppercase(),
            Body::Json(Json::Str(text)) => text.as_str().unwrap_or_default().trim().to_uppercase(),
            Body::Json(other) => {
                return Err(ViewError::Unknown(format!(
                    "AttributeError: '{}' object has no attribute 'strip'",
                    python_type_name(other)
                )));
            }
        };
        if name.is_empty() {
            return Ok(write::error_body(400, "Name is required"));
        }

        let statement = queries::PROJECT_IDENTIFIER_NAME_TAKEN
            .prepare(client)
            .await?;
        let row = client.query_one(&statement, &[&name, &slug]).await?;
        let taken: bool = row.get(0);
        if taken {
            return Ok(write::error_body(
                400,
                "Cannot delete an identifier of an existing project",
            ));
        }

        // H04 for `ProjectIdentifier.objects.filter(name=name,
        // workspace__slug=slug).delete()` (`:470`).
        plane_db::hooks::soft_delete::delete_where(
            client,
            &shared_queries::PROJECT_IDENTIFIERS_SOFT_DELETE,
            &[&name, &slug],
            write::now(),
        )
        .await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `type(value).__name__`, for the `AttributeError` above.
fn python_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::List(_) => "list",
        Json::Dict(_) => "dict",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JSON `null` is `None.strip()`, not the empty-name 400: the handler's
    /// 500 arm. The body value is what `request.data.get("name")` returned.
    #[test]
    fn the_type_name_matches_python() {
        assert_eq!(python_type_name(&Json::Null), "NoneType");
        assert_eq!(python_type_name(&Json::from(3i64)), "int");
        assert_eq!(python_type_name(&Json::from("x")), "str");
    }
}
