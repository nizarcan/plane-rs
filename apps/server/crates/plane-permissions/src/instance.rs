// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `InstanceAdminPermission` (`plane/license/api/permissions/instance.py:12-17`).
//!
//! ```python
//! if request.user.is_anonymous:
//!     return False
//! instance = Instance.objects.first()
//! return InstanceAdmin.objects.filter(role__gte=15, instance=instance, user=request.user).exists()
//! ```
//!
//! `Instance.objects.first()` is soft-delete filtered and ordered by
//! `-created_at`, so a soft-deleted newer instance row is skipped. `role >= 15`
//! (the field is an `integer`, not the membership `smallint`) accepts the
//! instance ADMIN role and any value above the workspace MEMBER floor; the
//! fixture has a role-10 row to prove the boundary.

use crate::queries as catalog;
use crate::{Error, PermCtx};

pub(crate) async fn instance_admin_permission(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    let user = ctx.user_id()?;

    let statement = catalog::INSTANCE_LATEST.prepare(ctx.client).await?;
    let row = ctx.client.query_opt(&statement, &[]).await?;
    let Some(row) = row else {
        // Django filters `instance=None` → `instance_id IS NULL`, which matches
        // nothing because the column is NOT NULL.
        return Ok(false);
    };
    let instance: uuid::Uuid = row.get(0);

    let statement = catalog::INSTANCE_ADMIN_EXISTS.prepare(ctx.client).await?;
    let row = ctx
        .client
        .query_one(&statement, &[&instance, &user])
        .await?;
    Ok(row.get(0))
}
