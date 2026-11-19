// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/storage_metadata_task.py:15-30` — `get_asset_object_metadata`.
//!
//! Called after an asset upload completes (ten call sites, `TASKS.md:804-830`),
//! with `asset_id` either as a keyword or as the single positional argument
//! (`TASKS.md:351`). It fills `FileAsset.storage_metadata` from one S3 `HEAD`:
//!
//! 1. `FileAsset.objects.get(pk=asset_id)`: the soft-delete manager hides
//!    deleted rows; `DoesNotExist` returns silently.
//! 2. `S3Storage().get_object_metadata(asset.asset.name)` — the five fields in
//!    `plane/settings/storage.py:153-169`. A missing object (`404`/`NoSuchKey`)
//!    is expected and yields `None` without a log; any other `ClientError` is
//!    logged and also yields `None`; a non-`ClientError` propagates.
//! 3. `asset.storage_metadata = <dict or None>; asset.save(update_fields=
//!    ["storage_metadata"])`. Only that column is written, so `updated_at`
//!    does **not** move (`BaseModel.save` would null the audit columns, but
//!    they are not in `update_fields`).
//!
//! Every exception is swallowed by the outer `except Exception`
//! (`storage_metadata_task.py:27-30`), so a malformed `asset_id` and a broken
//! storage client both end as a successful no-op.
//!
//! Ownership: all ten producers are Rust HTTP routes — eight `/api/`, three
//! `/api/v1/` (one row shared) and one `/api/public/` (`ROUTES.md:109-116,
//! 169-170, 326, 338, 435-436`) — so the task moves with this change.

use std::sync::Arc;

use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_storage::StorageRequest;
use uuid::Uuid;

use crate::{TaskContext, args, kwargs, swallow, uuid_value};

/// The Celery task name (`plane.bgtasks.storage_metadata_task.get_asset_object_metadata`).
pub const TASK: &str = "plane.bgtasks.storage_metadata_task.get_asset_object_metadata";

/// Register the handler; the worker's registry owns the closure.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    registry.register(TASK, move |job| {
        let context = Arc::clone(&context);
        Box::pin(async move { handle(&context, job).await })
    });
}

/// `asset_id` from either the keyword argument or the first positional one.
fn asset_id(payload: &serde_json::Value) -> Option<Uuid> {
    if let Some(value) = kwargs(payload).and_then(|kw| uuid_value(kw.get("asset_id"))) {
        return Some(value);
    }
    args(payload).and_then(|items| uuid_value(items.first()))
}

/// The handler entry point. Always `Ok(())`: see the module docs.
async fn handle(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    let Some(asset_id) = asset_id(&job.payload) else {
        // `ValidationError` from the UUID lookup / `TypeError` from a missing
        // argument: the outer `except Exception` logs and returns.
        swallow("invalid get_asset_object_metadata payload", TASK);
        return Ok(());
    };
    let Some(storage) = context.storage.as_ref() else {
        // `S3Storage()` succeeds without credentials in Python; the failure
        // happens inside `get_object_metadata` and is not a `ClientError`, so
        // the task's outer `except` swallows it.
        swallow("object storage is not configured for this worker", TASK);
        return Ok(());
    };
    match run(&job.client, storage.worker(), asset_id).await {
        Ok(()) => Ok(()),
        Err(error) => {
            swallow(error, TASK);
            Ok(())
        }
    }
}

/// The body, with every failure mapped to the outer `except`.
async fn run(
    client: &Client,
    storage: StorageRequest<'_>,
    asset_id: Uuid,
) -> Result<(), HandlerError> {
    let row = client
        .query_opt(
            "SELECT asset FROM file_assets WHERE id = $1 AND deleted_at IS NULL",
            &[&asset_id],
        )
        .await
        .map_err(db)?;
    // `FileAsset.DoesNotExist: return` — no log.
    let Some(row) = row else {
        return Ok(());
    };
    let key: String = row.try_get(0).map_err(db)?;

    // `storage.get_object_metadata(...)`: both a missing object and a logged
    // `ClientError` return `None`, which is written as SQL `NULL`.
    let metadata = storage
        .head_object(&key)
        .await
        .map_err(|error| HandlerError::retryable(error.to_string()))?;
    // `tokio-postgres` has `with-serde_json-1` (plane-db), so a JSON value
    // binds straight to the `jsonb` column.
    let value = match &metadata {
        Some(metadata) => Some(
            serde_json::to_value(metadata)
                .map_err(|error| HandlerError::fatal(error.to_string()))?,
        ),
        None => None,
    };
    // `save(update_fields=["storage_metadata"])`: one column, no `updated_at`.
    client
        .execute(
            "UPDATE file_assets SET storage_metadata = $2::jsonb WHERE id = $1",
            &[&asset_id, &value],
        )
        .await
        .map_err(db)?;
    Ok(())
}

/// A database failure is caught by the outer `except Exception`.
fn db<E: std::fmt::Display>(error: E) -> HandlerError {
    HandlerError::retryable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(value: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"args": [], "kwargs": value})
    }

    #[test]
    fn reads_the_keyword_and_the_positional_form() {
        let id = Uuid::nil();
        assert_eq!(
            asset_id(&payload(serde_json::json!({"asset_id": id.to_string()}))),
            Some(id)
        );
        assert_eq!(
            asset_id(&serde_json::json!({"args": [id.to_string()], "kwargs": {}})),
            Some(id)
        );
        assert_eq!(
            asset_id(&payload(serde_json::json!({"asset_id": "not-a-uuid"}))),
            None
        );
        assert_eq!(asset_id(&payload(serde_json::json!({}))), None);
    }

    #[test]
    fn the_metadata_json_uses_plane_keys() {
        let metadata = plane_storage::ObjectMetadata {
            content_type: Some("image/png".to_string()),
            content_length: Some(3),
            last_modified: Some("2026-11-19T00:00:00+00:00".to_string()),
            etag: Some("\"abc\"".to_string()),
            metadata: std::collections::BTreeMap::new(),
        };
        assert_eq!(
            serde_json::to_string(&metadata).unwrap(),
            "{\"ContentType\":\"image/png\",\"ContentLength\":3,\
             \"LastModified\":\"2026-11-19T00:00:00+00:00\",\"ETag\":\"\\\"abc\\\"\",\
             \"Metadata\":{}}"
        );
    }
}
