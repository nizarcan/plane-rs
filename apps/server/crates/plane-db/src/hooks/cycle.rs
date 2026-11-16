// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H22 — `Cycle.save` (`plane/db/models/cycle.py:88-97`).
//!
//! ```python
//! if self._state.adding:
//!     smallest_sort_order = Cycle.objects.filter(project=self.project)
//!         .aggregate(smallest=Min("sort_order"))["smallest"]
//!     if smallest_sort_order is not None:
//!         self.sort_order = smallest_sort_order - 10000
//! super().save()
//! ```
//!
//! Only a create runs it; the project is the URL project and the default
//! manager filters soft-deleted cycles. When the project has no live cycle the
//! instance keeps the value the serializer produced (the model default 65535
//! when the body did not carry `sort_order`).

use deadpool_postgres::Client;
use uuid::Uuid;

use crate::pool::DbError;

/// The `sort_order` a create writes.
///
/// # Errors
///
/// A database failure.
pub async fn before_insert(
    client: &Client,
    project_id: Uuid,
    requested: f64,
) -> Result<f64, DbError> {
    let row = client
        .query_one(
            "SELECT MIN(sort_order) FROM cycles \
             WHERE deleted_at IS NULL AND project_id = $1",
            &[&project_id],
        )
        .await?;
    let smallest: Option<f64> = row.get(0);
    Ok(smallest.map_or(requested, |smallest| smallest - 10_000.0))
}
