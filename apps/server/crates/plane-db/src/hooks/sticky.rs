// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H23 — `Sticky.save` (`plane/db/models/sticky.py:38-54`, MODEL_HOOKS §2.13).
//!
//! ```python
//! self.description_stripped = (
//!     None if (self.description_html == "" or self.description_html is None)
//!     else strip_tags(self.description_html)
//! )
//! if self._state.adding:
//!     last_id = Sticky.objects.filter(workspace=self.workspace).aggregate(Max("sort_order"))["largest"]
//!     if last_id is not None: self.sort_order = last_id + 10000
//! ```
//!
//! Two facts a write path must reproduce:
//!
//! * `description_stripped` is recomputed on **every** save, including a PATCH
//!   that only changes `name` and the soft delete, so a client cannot set it;
//! * `sort_order` is only assigned on create, and only when the workspace
//!   already has a live sticky: `MAX(sort_order) + 10000`. An empty workspace
//!   keeps the field's default 65535. Soft-deleted rows are excluded
//!   (`Sticky.objects` is the soft-deletion manager).

use deadpool_postgres::Client;
use uuid::Uuid;

use crate::pool::DbError;

/// The values H23 replaces before the row is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub description_stripped: Option<String>,
    /// The sort order to write on create; `default` when the workspace is
    /// empty. Ignored on update (the stored value is kept).
    pub sort_order: f64,
}

/// `Sticky.save()` for a new row: strip the HTML and derive `sort_order`.
///
/// # Errors
///
/// A database failure from the `MAX(sort_order)` lookup.
pub async fn before_create(
    client: &Client,
    workspace_id: Uuid,
    description_html: &str,
    sort_order: f64,
) -> Result<Prepared, DbError> {
    let description_stripped = strip(description_html);
    let statement = client
        .prepare(
            "SELECT MAX(sort_order) FROM stickies \
             WHERE workspace_id = $1 AND deleted_at IS NULL",
        )
        .await?;
    let row = client.query_one(&statement, &[&workspace_id]).await?;
    let largest: Option<f64> = row.get(0);
    Ok(Prepared {
        description_stripped,
        sort_order: largest.map_or(sort_order, |largest| largest + 10_000.0),
    })
}

/// `Sticky.save()` for an existing row: only `description_stripped` changes.
#[must_use]
pub fn before_update(description_html: &str) -> Option<String> {
    strip(description_html)
}

/// The `description_stripped` assignment shared by both branches.
#[must_use]
pub fn strip(description_html: &str) -> Option<String> {
    if description_html.is_empty() {
        None
    } else {
        Some(super::html::strip_tags(description_html))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_html_is_null() {
        assert_eq!(strip(""), None);
        assert_eq!(strip("<p></p>"), Some(String::new()));
    }
}
