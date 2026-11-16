// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H21 — `Module.save` (`plane/db/models/module.py:115-124`).
//!
//! ```python
//! def save(self, *args, **kwargs):
//!     if self._state.adding:
//!         smallest_sort_order = Module.objects.filter(project=self.project).aggregate(
//!             smallest=models.Min("sort_order")
//!         )["smallest"]
//!
//!         if smallest_sort_order is not None:
//!             self.sort_order = smallest_sort_order - 10000
//!
//!     super(Module, self).save(*args, **kwargs)
//! ```
//!
//! Only a create runs it; the project is the URL project and the default
//! manager filters soft-deleted modules. When the project has no live module
//! the instance keeps the value the serializer produced (the model default
//! 65535 when the body did not carry `sort_order`).

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
            "SELECT MIN(sort_order) FROM modules \
             WHERE deleted_at IS NULL AND project_id = $1",
            &[&project_id],
        )
        .await?;
    let smallest: Option<f64> = row.get(0);
    Ok(smallest.map_or(requested, |smallest| smallest - 10_000.0))
}
