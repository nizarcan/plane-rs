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
//! H04 (`Model.objects.filter(...).delete()`, the queryset form) is different:
//! it is a bare `UPDATE ... SET deleted_at=now` with no save hook, no
//! `updated_at` and no task. No GET route needs it yet; a write route that
//! deletes a collection adds it here.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
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
         WHERE {key} = $3 AND deleted_at IS NULL"
    );
    let statement = client.prepare(&sql).await?;
    client
        .execute(&statement, &[&now, &updated_by, &pk])
        .await?;
    Ok(())
}
