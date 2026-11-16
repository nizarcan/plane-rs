// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H20 — `Label.save` (`plane/db/models/label.py:46-54`).
//!
//! ```python
//! if self._state.adding:
//!     last_id = Label.objects.filter(project=self.project)
//!         .aggregate(largest=Max("sort_order"))["largest"]
//!     if last_id is not None:
//!         self.sort_order = last_id + 10000
//! super().save()
//! ```
//!
//! The `project` filter is the default manager's (`deleted_at IS NULL`), and a
//! workspace-level label (`project is None`) aggregates the workspace labels.
//! Only a create runs it: an update keeps the stored `sort_order`.
//!
//! `bulk_create` bypasses `save()`, so the bulk route writes the model default
//! 65535 for every row (`LabelViewSet.create` is the only H20 caller).

use deadpool_postgres::Client;
use uuid::Uuid;

use crate::pool::DbError;

/// The `sort_order` a create writes.
///
/// # Errors
///
/// A database failure.
pub async fn before_insert(client: &Client, project_id: Option<Uuid>) -> Result<f64, DbError> {
    let row = client
        .query_one(
            "SELECT MAX(sort_order) FROM labels \
             WHERE deleted_at IS NULL AND project_id IS NOT DISTINCT FROM $1",
            &[&project_id],
        )
        .await?;
    let largest: Option<f64> = row.get(0);
    Ok(largest.map_or(65535.0, |largest| largest + 10_000.0))
}
