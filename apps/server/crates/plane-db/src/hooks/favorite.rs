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
//! The model default is 65535, so the hook returns
//!
//! * `max + 10000` when the workspace has a live favourite,
//! * else the caller's `requested` value when the body carried one,
//! * else 65535.
//!
//! Only the five per-entity favourite creates (`cycles`, `projects`,
//! `modules`, `views`, `pages`) write a fixed sequence and pass `None`; the
//! workspace favourites POST (`views/workspace/favorite.py:58-72`) forwards the
//! serializer's `sequence`, which survives **only** when the aggregate is
//! `NULL` (`db/models/favorite.py:62-63`).
//!
//! The workspace is always known to the caller (the project's workspace for a
//! project favourite), so the query is one `MAX(sequence)` on live favourites in
//! that workspace. It races: two concurrent creates can share a value, as in
//! Django (no lock).

use deadpool_postgres::Client;
use uuid::Uuid;

use crate::pool::DbError;

/// H25: the `sequence` a new favourite row is written with.
///
/// `requested` is the value the request body carried (`None` when absent, and
/// always `None` for the per-entity creates, whose views never send one).
///
/// # Errors
///
/// A database failure.
pub async fn before_insert(
    client: &Client,
    workspace_id: Uuid,
    requested: Option<f64>,
) -> Result<f64, DbError> {
    let row = client
        .query_one(
            "SELECT MAX(sequence) FROM user_favorites \
             WHERE workspace_id = $1 AND deleted_at IS NULL",
            &[&workspace_id],
        )
        .await?;
    let largest: Option<f64> = row.get(0);
    Ok(match largest {
        Some(largest) => largest + 10_000.0,
        // `if largest_sequence is not None` — otherwise the instance keeps what
        // the serializer set, or the model default (`db/models/favorite.py:24`).
        None => requested.unwrap_or(65535.0),
    })
}
