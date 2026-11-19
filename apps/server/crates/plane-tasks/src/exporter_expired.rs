// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/exporter_expired_task.py:23-53` — `delete_old_s3_link`.
//!
//! Beat-only, and scheduled **twice** (`plane/celery.py:69-72,97-100`, 01:30
//! and 03:45 UTC). The second run is a database no-op because the first run
//! sets `url = NULL`; both entries stay (`plans/CUTOVER.md` §6 risks, upstream
//! bug kept as-is).
//!
//! The body, in Django's order:
//!
//! 1. `ExporterHistory.objects.filter(Q(url__isnull=False) &
//!    Q(created_at__lte=now-8d)).values_list("key", "id")`. The default
//!    manager is `SoftDeletionManager`, so soft-deleted rows are excluded; the
//!    model's ordering is `-created_at`.
//! 2. One S3 client for the whole loop. Constructing it happens **outside**
//!    any `try`, so a configuration error fails the task (Celery `FAILURE`,
//!    never retried: `TASKS.md:517-521`).
//! 3. For every row: delete the object when `key` is non-empty (an empty
//!    string is falsy in Python and skipped), then
//!    `ExporterHistory.objects.filter(id=...).update(url=None)` — a queryset
//!    `update`, so `updated_at` does **not** move.
//!
//! The S3 delete is uncaught too (`exporter_expired_task.py:47-50`): a real
//! `ClientError` aborts the task and leaves the remaining rows for the next
//! tick. Deleting a key that does not exist succeeds (S3 answers 204), which
//! `plane_storage::delete_object` already reproduces.
//!
//! Ownership: this task's only producer is Django beat
//! (`plane/celery.py:69-72,97-100`) until an operator flips the
//! `plane_jobs.schedules` row for both entries to `owner='rust'`
//! (`plans/CUTOVER.md` §2 "Beat handover"). It therefore stays Celery-owned;
//! the handler and the `difftest task` case are the evidence for the flip.

use std::sync::Arc;

use chrono::{Duration, Utc};
use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use uuid::Uuid;

use crate::{TaskContext, missing_component};

/// The Celery task name (`plane.bgtasks.exporter_expired_task.delete_old_s3_link`).
pub const TASK: &str = "plane.bgtasks.exporter_expired_task.delete_old_s3_link";

/// Register the handler; the worker's registry owns the closure.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    registry.register(TASK, move |job| {
        let context = Arc::clone(&context);
        Box::pin(async move { handle(&context, job).await })
    });
}

/// The handler entry point. Unlike the e-mail tasks this one is **not**
/// wrapped in `try`/`except`: an error is a real failed task.
async fn handle(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    let storage = context
        .storage
        .as_ref()
        .ok_or_else(|| missing_component("object storage"))?;
    run(&job.client, storage.worker(), Utc::now()).await
}

/// The body, with an injected clock for the 8-day window.
async fn run(
    client: &Client,
    storage: plane_storage::StorageRequest<'_>,
    now: chrono::DateTime<Utc>,
) -> Result<(), HandlerError> {
    let cutoff = now - Duration::days(8);
    // `.values_list("key", "id")`: the model's `-created_at` ordering and the
    // soft-delete manager.
    let rows = client
        .query(
            "SELECT key, id FROM exporters \
             WHERE url IS NOT NULL AND created_at <= $1 AND deleted_at IS NULL \
             ORDER BY created_at DESC",
            &[&cutoff],
        )
        .await
        .map_err(db)?;

    for row in rows {
        let key: String = row.try_get(0).map_err(db)?;
        let id: Uuid = row.try_get(1).map_err(db)?;
        // `if file_name:` — `TextField(blank=True)` may hold `""`.
        if !key.is_empty() {
            storage
                .delete_object(&key)
                .await
                .map_err(|error| HandlerError::retryable(error.to_string()))?;
        }
        // `.filter(id=exporter_id).update(url=None)`: one column, no
        // `updated_at` movement and no soft-delete manager involvement for the
        // write itself.
        client
            .execute("UPDATE exporters SET url = NULL WHERE id = $1", &[&id])
            .await
            .map_err(db)?;
    }
    Ok(())
}

/// An uncaught Python exception: the task fails.
fn db<E: std::fmt::Display>(error: E) -> HandlerError {
    HandlerError::retryable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_name_is_the_registered_one() {
        assert_eq!(
            TASK,
            "plane.bgtasks.exporter_expired_task.delete_old_s3_link"
        );
    }

    #[test]
    fn the_window_is_eight_days_ending_now() {
        let now = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 11, 19, 12, 0, 0)
            .single()
            .unwrap();
        assert_eq!(
            now - Duration::days(8),
            chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 11, 11, 12, 0, 0)
                .single()
                .unwrap()
        );
    }
}
