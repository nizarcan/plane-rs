// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H06 — `WorkspaceBaseModel.save` (`plane/db/models/workspace.py:198-206`,
//! MODEL_HOOKS.md:2133).
//!
//! ```python
//! class WorkspaceBaseModel(BaseModel):
//!     workspace = models.ForeignKey("db.Workspace", ...)
//!     project = models.ForeignKey("db.Project", ..., null=True)
//!
//!     def save(self, *args, **kwargs):
//!         if self.project:
//!             self.workspace = self.project.workspace
//!         super(WorkspaceBaseModel, self).save(*args, **kwargs)
//! ```
//!
//! Two facts a write path must reproduce:
//!
//! * a body `project` **re-homes** the row: `workspace_id` becomes the
//!   project's workspace, so a `QuickLink` created on workspace A with a
//!   project of workspace B is stored under B (`QuickLinkViewSet.create` passes
//!   the URL workspace first and the serializer's project second);
//! * the two reads are Django FK dereferences, which use `_base_manager`, so a
//!   **soft-deleted** project (or a project in a soft-deleted workspace) still
//!   resolves. Only DRF's field validation, against `Project.objects`, hides
//!   deleted projects from the body.
//!
//! H07 (`ProjectBaseModel.save`, MODEL_HOOKS.md:2134) is the same assignment
//! with `project` always set; it shares this helper.
//!
//! The lookup is deliberately not catalogued: it is one hook-internal
//! statement with no result columns a handler reads (the same shape as
//! [`super::sticky`]'s `MAX(sort_order)`).

use deadpool_postgres::Client;
use uuid::Uuid;

use crate::pool::DbError;

/// `SELECT workspace_id FROM projects WHERE id = $1`.
///
/// Django's FK fetch is `_base_manager.get(pk=…)` with all columns selected;
/// only `workspace_id` is read here.
pub const WORKSPACE_OF_PROJECT_SQL: &str = "SELECT workspace_id FROM projects WHERE id = $1";

/// `self.workspace = self.project.workspace` when the body carried a project.
///
/// `Some(workspace_id)` is the value to write; `None` means the project row
/// does not exist, which in Python is `Project.DoesNotExist` from the FK
/// dereference — a family 500 **after** the serializer accepted the value (it
/// can only happen when the row disappears between validation and save).
///
/// # Errors
///
/// A database failure.
pub async fn workspace_from_project(
    client: &Client,
    project_id: Uuid,
) -> Result<Option<Uuid>, DbError> {
    let statement = client.prepare_cached(WORKSPACE_OF_PROJECT_SQL).await?;
    let row = client.query_opt(&statement, &[&project_id]).await?;
    Ok(row.map(|row| row.get(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lookup_uses_the_base_manager_so_deleted_projects_resolve() {
        // No `deleted_at IS NULL`: `self.project` is a FK dereference, which
        // never runs the related model's default manager.
        assert_eq!(
            WORKSPACE_OF_PROJECT_SQL,
            "SELECT workspace_id FROM projects WHERE id = $1"
        );
    }
}
