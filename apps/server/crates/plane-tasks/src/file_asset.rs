// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/file_asset_task.py:21-26` — `delete_unuploaded_file_asset`.
//!
//! Beat-only (02:00 UTC, `plane/celery.py:73-76`). One statement:
//!
//! ```sql
//! UPDATE file_assets SET deleted_at = now()
//!  WHERE created_at < now - UNUPLOADED_ASSET_DELETE_DAYS days
//!    AND is_uploaded = false AND deleted_at IS NULL
//! ```
//!
//! Details that matter:
//!
//! * `FileAsset` is a `BaseModel` and its default manager is
//!   `SoftDeletionManager`, whose `SoftDeletionQuerySet.delete(soft=True)`
//!   rewrites the delete as `self.update(deleted_at=timezone.now())`
//!   (`plane/db/mixins.py:38-46`). This is therefore a **soft** delete — and a
//!   single `UPDATE`, so `updated_at` does **not** move and a row that was
//!   already soft-deleted is outside the manager's queryset and untouched.
//!   `is_deleted` stays `false`; the two flags are independent
//!   (`MODEL_HOOKS.md:1263-1279`).
//! * The task does **not** delete S3 objects — a row whose upload never
//!   completed has no object to delete (`TASKS.md:544`).
//! * The window is `created_at < now - N days` (strict `<`) and the threshold
//!   is `timezone.now()` computed in Python, then bound.
//! * `UNUPLOADED_ASSET_DELETE_DAYS` is read with `os.environ.get(..., "7")`
//!   and `int()`ed **inside** the task body, which has no `try`: a missing
//!   variable means 7, a non-numeric value fails the task.
//! * No reverse relations point at `file_assets`, so Django's collector
//!   deletes nothing else; there are no `post_delete` receivers.
//!
//! Ownership: the only producer is Django beat (`plane/celery.py:73-76`) until
//! the `plane_jobs.schedules` row for
//! `check-every-day-to-delete-file-asset` is flipped to `owner='rust'`
//! (`plans/CUTOVER.md` §2). The task therefore stays Celery-owned; this
//! handler and its `difftest task` case are the evidence for the flip.

use std::sync::Arc;

use chrono::{Duration, Utc};
use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};

use crate::TaskContext;

/// The Celery task name (`plane.bgtasks.file_asset_task.delete_unuploaded_file_asset`).
pub const TASK: &str = "plane.bgtasks.file_asset_task.delete_unuploaded_file_asset";

/// `os.environ.get("UNUPLOADED_ASSET_DELETE_DAYS", "7")`.
const DEFAULT_DAYS: i64 = 7;
const DAYS_VAR: &str = "UNUPLOADED_ASSET_DELETE_DAYS";

/// Register the handler; the worker's registry owns the closure.
pub fn register(registry: &mut TaskRegistry, _context: Arc<TaskContext>) {
    registry.register(TASK, handle);
}

/// The handler entry point. `int()` failures are uncaught in Python, so they
/// fail the task; every other error is a database error with the same result.
async fn handle(job: JobContext) -> Result<(), HandlerError> {
    let days = match std::env::var(DAYS_VAR) {
        // `os.environ.get` returns the empty string when the variable is set
        // to one, and `int("")` raises in Python.
        Ok(value) => value
            .trim()
            .parse::<i64>()
            .map_err(|_| HandlerError::fatal(format!("invalid literal for int(): {value:?}")))?,
        Err(std::env::VarError::NotPresent) => DEFAULT_DAYS,
        Err(error) => return Err(HandlerError::fatal(error.to_string())),
    };
    run(&job.client, Utc::now(), days).await
}

/// The one statement, with an injected clock.
async fn run(client: &Client, now: chrono::DateTime<Utc>, days: i64) -> Result<(), HandlerError> {
    let cutoff = now - Duration::days(days);
    // `QuerySet.delete()` computes its own `timezone.now()` for the update
    // (`mixins.py:41`), a second read of the clock.
    let deleted_at = Utc::now();
    client
        .execute(
            "UPDATE file_assets SET deleted_at = $2 \
             WHERE created_at < $1 AND is_uploaded = false AND deleted_at IS NULL",
            &[&cutoff, &deleted_at],
        )
        .await
        .map_err(|error| HandlerError::retryable(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_name_is_the_registered_one() {
        assert_eq!(
            TASK,
            "plane.bgtasks.file_asset_task.delete_unuploaded_file_asset"
        );
    }
}
