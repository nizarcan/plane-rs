// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `DraftIssue.save` (`plane/db/models/draft.py:110-170`).
//!
//! On every save: pick the project's default non-triage state when none was
//! sent, derive `completed_at` from that state's group, and recompute
//! `description_stripped` with `strip_tags`. On create only: `sort_order =`
//! `MAX(sort_order)` over the live drafts of the same project and state, plus
//! 10000; an empty project keeps the serializer/model default.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use uuid::Uuid;

use crate::pool::DbError;

/// The values the model `save()` replaces before the row is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub state_id: Option<Uuid>,
    pub completed_at: Option<DateTime<Utc>>,
    pub description_stripped: Option<String>,
    /// Create only; the stored value is kept on update.
    pub sort_order: f64,
}

/// `DraftIssue.save()` for a new row.
///
/// # Errors
///
/// A database failure from the state or `MAX(sort_order)` lookups.
pub async fn before_create(
    client: &Client,
    project_id: Option<Uuid>,
    state_id: Option<Uuid>,
    description_html: &str,
    default_sort_order: f64,
    now: DateTime<Utc>,
) -> Result<Prepared, DbError> {
    let state = resolve_state(client, project_id, state_id).await?;
    let group = state_group(client, state).await?;
    let description_stripped = strip(description_html);
    let sort_order = match (project_id, state) {
        (Some(project_id), Some(state_id)) => {
            let statement = client
                .prepare(
                    "SELECT MAX(sort_order) FROM draft_issues \
                     WHERE project_id = $1 AND state_id = $2 AND deleted_at IS NULL",
                )
                .await?;
            let row = client
                .query_one(&statement, &[&project_id, &state_id])
                .await?;
            let largest: Option<f64> = row.get(0);
            largest.map_or(default_sort_order, |largest| largest + 10_000.0)
        }
        _ => default_sort_order,
    };
    Ok(Prepared {
        state_id: state,
        completed_at: completed_at(group.as_deref(), now),
        description_stripped,
        sort_order,
    })
}

/// `DraftIssue.save()` for an existing row: state/default, `completed_at` and
/// the stripped description; `sort_order` is untouched.
///
/// # Errors
///
/// A database failure from the state lookup.
pub async fn before_update(
    client: &Client,
    project_id: Option<Uuid>,
    state_id: Option<Uuid>,
    description_html: &str,
    now: DateTime<Utc>,
) -> Result<Prepared, DbError> {
    let state = resolve_state(client, project_id, state_id).await?;
    let group = state_group(client, state).await?;
    Ok(Prepared {
        state_id: state,
        completed_at: completed_at(group.as_deref(), now),
        description_stripped: strip(description_html),
        sort_order: 0.0,
    })
}

/// `if self.state is None: default_state = …filter(default=True).first() or
/// …filter().first()` (`draft.py:114-131`).
async fn resolve_state(
    client: &Client,
    project_id: Option<Uuid>,
    state_id: Option<Uuid>,
) -> Result<Option<Uuid>, DbError> {
    if state_id.is_some() {
        return Ok(state_id);
    }
    let Some(project_id) = project_id else {
        return Ok(None);
    };
    for sql in [
        "SELECT id FROM states WHERE project_id = $1 AND is_triage = false \
         AND deleted_at IS NULL AND \"default\" = true ORDER BY id LIMIT 1",
        "SELECT id FROM states WHERE project_id = $1 AND is_triage = false \
         AND deleted_at IS NULL ORDER BY id LIMIT 1",
    ] {
        let statement = client.prepare(sql).await?;
        let row = client.query_opt(&statement, &[&project_id]).await?;
        if let Some(row) = row {
            return Ok(Some(row.get(0)));
        }
    }
    Ok(None)
}

async fn state_group(client: &Client, state_id: Option<Uuid>) -> Result<Option<String>, DbError> {
    let Some(state_id) = state_id else {
        return Ok(None);
    };
    let statement = client
        .prepare("SELECT \"group\" FROM states WHERE id = $1")
        .await?;
    let row = client.query_opt(&statement, &[&state_id]).await?;
    Ok(row.map(|row| row.get(0)))
}

/// `completed_at = now` for a completed state, `None` otherwise
/// (`draft.py:132-140`).
fn completed_at(group: Option<&str>, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    (group == Some("completed")).then_some(now)
}

/// `None if description_html == "" or None else strip_tags(...)`.
#[must_use]
pub fn strip(description_html: &str) -> Option<String> {
    if description_html.is_empty() {
        None
    } else {
        Some(super::html::strip_tags(description_html))
    }
}
