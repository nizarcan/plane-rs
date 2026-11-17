// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H03 — `SoftDeleteModel.delete(soft=True)` (`plane/db/mixins.py:65-77`).
//!
//! ```python
//! self.deleted_at = timezone.now()
//! self.save(using=using)                       # full save: the model hook + H01 run
//! soft_delete_related_objects.delay(app_label, model_name, pk, using=using)
//! ```
//!
//! The request's part is one full-row `UPDATE`: `deleted_at` and `updated_at`
//! are the same `now`, `updated_by_id` is the request user, and every other
//! column keeps its value. The cascade itself runs in the worker (H35), so the
//! request only publishes the task; `plane-http-app`'s
//! `write::publish_soft_delete_cascade` does that with `plane-jobs`.
//!
//! The statement keys on the primary key only, like Django's instance save: a
//! `SoftDeleteModel.delete()` is **not** a queryset delete, so the manager's
//! `deleted_at IS NULL` predicate is not part of the SQL.
//!
//! H04 (`Model.objects.filter(...).delete()`, the queryset form) is different:
//! it is a bare `UPDATE ... SET deleted_at=now` with no save hook, no
//! `updated_at` and no task — see [`delete_where`].

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use tokio_postgres::types::ToSql;
use uuid::Uuid;

use crate::pool::DbError;

/// `self.deleted_at = timezone.now(); self.save()` for one row.
///
/// `table` and `key` are constants chosen by the caller (never request data);
/// the identity columns are bound.
///
/// # Errors
///
/// A database failure.
pub async fn delete_instance(
    client: &Client,
    table: &str,
    key: &str,
    pk: Uuid,
    updated_by: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    let sql = format!(
        "UPDATE {table} SET deleted_at = $1, updated_at = $1, updated_by_id = $2 \
         WHERE {key} = $3"
    );
    let statement = client.prepare(&sql).await?;
    client
        .execute(&statement, &[&now, &updated_by, &pk])
        .await?;
    Ok(())
}

/// The `UPDATE` H04 issues, for a predicate the caller supplies.
///
/// Only `deleted_at` is assigned: no `updated_at`, no `updated_by_id`, no model
/// hook. Django's `QuerySet.update()` writes exactly the named column
/// (`django/db/models/query.py:1200-1220`).
#[must_use]
pub fn delete_where_sql(table: &str, predicate: &str) -> String {
    format!("UPDATE {table} SET deleted_at = $1 WHERE {predicate}")
}

/// H04 — `SoftDeletionQuerySet.delete(soft=True)` (`plane/db/mixins.py:33-38`):
/// `self.update(deleted_at=timezone.now())`.
///
/// The queryset's own manager predicate (`deleted_at IS NULL`, from
/// `SoftDeletionManager.get_queryset`) is part of it, so an already deleted row
/// keeps its timestamp; nothing else is written and no
/// `soft_delete_related_objects` task is published — the cascade is H03's, for
/// one instance, not the queryset's.
///
/// `query` is the catalogued statement, whose **`$1` is `deleted_at`** and
/// whose remaining placeholders are `params`, in the order the caller's
/// predicate declares. `table`/predicate text come from constants, never from
/// request data.
///
/// Returns the row count Django's `.delete()` reports (its result tuple's
/// first element).
///
/// # Errors
///
/// A database failure.
pub async fn delete_where(
    client: &Client,
    query: &'static crate::Query,
    params: &[&(dyn ToSql + Sync)],
    deleted_at: DateTime<Utc>,
) -> Result<u64, DbError> {
    let statement = query.prepare(client).await?;
    let mut values: Vec<&(dyn ToSql + Sync)> = Vec::with_capacity(params.len() + 1);
    values.push(&deleted_at);
    values.extend_from_slice(params);
    Ok(client.execute(&statement, &values).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h04_writes_only_deleted_at() {
        // `ProjectIdentifier.objects.filter(name=name, workspace__slug=slug)
        // .delete()` (`plane/app/views/project/base.py:470`).
        assert_eq!(
            delete_where_sql(
                "project_identifiers",
                "deleted_at IS NULL AND name = $2 AND workspace_id IN (\
                 SELECT id FROM workspaces WHERE slug = $3)"
            ),
            "UPDATE project_identifiers SET deleted_at = $1 \
             WHERE deleted_at IS NULL AND name = $2 AND workspace_id IN (\
             SELECT id FROM workspaces WHERE slug = $3)"
        );
    }
}
