// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H32 — `ProjectMember.save` (`plane/db/models/project.py:276-293`).
//!
//! ```python
//! if self._state.adding and self.member:
//!     min_sort_order = ProjectUserProperty.objects.filter(
//!         workspace_id=self.project.workspace_id, user=self.member
//!     ).aggregate(Min("sort_order"))["min_sort_order"]
//!     ProjectUserProperty.objects.create(
//!         workspace_id=self.project.workspace_id, project=self.project,
//!         user=self.member, sort_order=(min - 10000 if min is not None else 65535))
//! super().save()
//! ```
//!
//! Every create goes through this hook (the view creates one membership for the
//! caller and one for a different `project_lead`), so the hook is called once
//! per membership and each call runs its own `MIN(sort_order)` query.
//!
//! `ProjectMember.save` also sets `workspace = project.workspace`
//! (`ProjectBaseModel.save`, `project.py:237-239`); the caller binds that value.

use deadpool_postgres::Client;
use uuid::Uuid;

use crate::pool::DbError;

/// The `ProjectUserProperty` H32 creates before the membership row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemberProperty {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub sort_order: f64,
}

/// H32: `MIN(sort_order)` for this user in the workspace, then
/// `min - 10000` or the model default 65535.
///
/// # Errors
///
/// A database failure.
pub async fn before_insert(
    client: &Client,
    workspace_id: Uuid,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<MemberProperty, DbError> {
    let row = client
        .query_one(
            "SELECT MIN(sort_order) FROM project_user_properties \
             WHERE workspace_id = $1 AND user_id = $2 AND deleted_at IS NULL",
            &[&workspace_id, &user_id],
        )
        .await?;
    let min: Option<f64> = row.get(0);
    Ok(MemberProperty {
        workspace_id,
        project_id,
        user_id,
        sort_order: min.map_or(65535.0, |min| min - 10_000.0),
    })
}
