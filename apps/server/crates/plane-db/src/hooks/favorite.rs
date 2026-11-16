// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H25 — `UserFavorite.save` (`plane/db/models/favorite.py:52-72`).
//!
//! ```python
//! if self._state.adding:
//!     if self.project:
//!         largest_sequence = UserFavorite.objects.filter(workspace=self.project.workspace).aggregate(Max("sequence"))["largest"]
//!     else:
//!         largest_sequence = UserFavorite.objects.filter(workspace=self.workspace).aggregate(Max("sequence"))["largest"]
//!     if largest_sequence is not None: self.sequence = largest_sequence + 10000
//! ```
//!
//! The model default is 65535. The workspace is always known to the caller
//! (the project's workspace for a project favourite), so the query is one
//! `MAX(sequence)` on live favourites in that workspace.

use deadpool_postgres::Client;
use uuid::Uuid;

use crate::pool::DbError;

/// H25: the `sequence` a new favourite row is written with.
///
/// # Errors
///
/// A database failure.
pub async fn before_insert(client: &Client, workspace_id: Uuid) -> Result<f64, DbError> {
    let row = client
        .query_one(
            "SELECT MAX(sequence) FROM user_favorites \
             WHERE workspace_id = $1 AND deleted_at IS NULL",
            &[&workspace_id],
        )
        .await?;
    let largest: Option<f64> = row.get(0);
    Ok(largest.map_or(65535.0, |largest| largest + 10_000.0))
}
