// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/recent_visited_task.py:17-61`.
//!
//! The task is an upsert with a trim:
//!
//! 1. `Workspace.objects.get(slug=slug)` — the default manager hides
//!    soft-deleted workspaces; `.get()` with two live rows is
//!    `MultipleObjectsReturned`, which the outer `except` swallows.
//! 2. A row for `(entity_name, entity_identifier, user_id, project_id,
//!    workspace_id)` exists: bump `visited_at` with `update_fields`, so
//!    `updated_at` does **not** move.
//! 3. Otherwise count the user's live visits in the workspace. At exactly
//!    **20**, soft-delete the oldest (`order_by("created_at").first()`), then
//!    insert the new row. The exact `== 20` comparison is the upstream bug the
//!    brief says to keep: a workspace with 21 or more visits never trims
//!    (`recent_visited_task.py:37-44`, `plans/CUTOVER.md` §6 risks).
//!
//! # Details that the SQL has to reproduce
//!
//! * `UserRecentVisit` is a `BaseModel` → `SoftDeleteModel`, so
//!   `recent_visited.delete()` is a **soft** delete. `BaseModel.save` with no
//!   current user sets `created_by`/`updated_by` to `None` and then writes
//!   every column, so the row's `deleted_at`, `updated_at`, `created_by_id` and
//!   `updated_by_id` all change (`plane/db/models/base.py:23-50`,
//!   `plane/db/mixins.py:70-84`).
//! * The task's own `created_by_id = user_id` / `updated_by_id = user_id`
//!   followed by `save(update_fields=[...])` is a **no-op**: `BaseModel.save`
//!   overwrites both attributes with `None` before writing them. The row keeps
//!   `NULL` (`recent_visited_task.py:54-56`). The oracle run in `difftest task`
//!   proves it.
//! * The trim's `delete()` also publishes `soft_delete_related_objects.delay`
//!   (`plane/db/mixins.py:78`). For this model `_meta.get_fields()` has no
//!   auto-created reverse relation, so that task only re-saves the
//!   already-soft-deleted row and changes nothing
//!   (`plane/bgtasks/deletion_task.py:17-105`). The message is still
//!   observable on the broker while the task is Celery-owned, so the handler
//!   publishes exactly what Django publishes (`DECISIONS.md` D25) through
//!   [`crate::publish_or_enqueue`]; once task 9 is Rust-owned the same call
//!   becomes a Rust→Rust enqueue.
//! * A failed publish is the Python `except Exception` branch: the soft delete
//!   has already committed, the new row is **not** inserted, and the task
//!   succeeds (`recent_visited_task.py:59-61`).
//! * Every exception, including `DatabaseError`, is swallowed by the outer
//!   `except Exception` (`recent_visited_task.py:59-61`), so the handler
//!   returns `Ok(())` after logging. The queue must not retry it.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_jobs::queue::Job;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use uuid::Uuid;

use crate::{TaskContext, kwargs, publish_or_enqueue, string_value, swallow, uuid_value};

/// The Celery task name (`plane.bgtasks.recent_visited_task.recent_visited_task`).
pub const TASK: &str = "plane.bgtasks.recent_visited_task.recent_visited_task";

/// Register the handler; the worker's registry owns the closure.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    registry.register(TASK, move |job| handle(Arc::clone(&context), job));
}

/// One `recent_visited_task.delay(...)` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Visit {
    slug: String,
    entity_name: String,
    entity_identifier: Uuid,
    user_id: Uuid,
    project_id: Option<Uuid>,
}

impl Visit {
    /// Decode the payload. `None` is Django's `TypeError`/`ValidationError`
    /// inside the task's `try`: logged, no rows changed, task succeeds.
    fn from_payload(payload: &serde_json::Value) -> Option<Self> {
        let kwargs = kwargs(payload)?;
        let slug = string_value(kwargs.get("slug"))?;
        let entity_name = string_value(kwargs.get("entity_name"))?;
        // `entity_identifier` is a UUID field; a malformed string raises
        // `ValidationError` in the filter.
        let entity_identifier = uuid_value(kwargs.get("entity_identifier"))?;
        let user_id = uuid_value(kwargs.get("user_id"))?;
        let project_id = match kwargs.get("project_id") {
            None | Some(serde_json::Value::Null) => None,
            value => Some(uuid_value(value)?),
        };
        Some(Self {
            slug,
            entity_name,
            entity_identifier,
            user_id,
            project_id,
        })
    }
}

/// The handler entry point. Always `Ok(())`: see the module docs.
async fn handle(context: Arc<TaskContext>, job: JobContext) -> Result<(), HandlerError> {
    let Some(visit) = Visit::from_payload(&job.payload) else {
        swallow("invalid recent_visited_task payload", TASK);
        return Ok(());
    };
    match run(&context, &job.client, &visit).await {
        Ok(()) => Ok(()),
        // `except Exception as e: log_exception(e); return`.
        Err(error) => {
            swallow(error, TASK);
            Ok(())
        }
    }
}

/// The body, with every failure mapped to the outer `except`.
async fn run(context: &TaskContext, client: &Client, visit: &Visit) -> Result<(), HandlerError> {
    // `Workspace.objects.get(slug=slug)`. `.get()` on more than one live row
    // raises and is swallowed like any other exception.
    let workspace = client
        .query_opt(
            "SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL",
            &[&visit.slug],
        )
        .await
        .map_err(db)?;
    let Some(workspace) = workspace else {
        return Err(HandlerError::retryable("workspace does not exist"));
    };
    let workspace_id: Uuid = workspace.try_get(0).map_err(db)?;
    let more = client
        .query_opt(
            "SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL OFFSET 1 LIMIT 1",
            &[&visit.slug],
        )
        .await
        .map_err(db)?;
    if more.is_some() {
        return Err(HandlerError::retryable(
            "MultipleObjectsReturned: more than one workspace with this slug",
        ));
    }

    // `.filter(...).first()`: the model's `-created_at` ordering applies.
    let existing = client
        .query_opt(
            "SELECT id FROM user_recent_visits \
             WHERE entity_name = $1 AND entity_identifier = $2 AND user_id = $3 \
               AND project_id IS NOT DISTINCT FROM $4 AND workspace_id = $5 \
               AND deleted_at IS NULL \
             ORDER BY created_at DESC LIMIT 1",
            &[
                &visit.entity_name,
                &visit.entity_identifier,
                &visit.user_id,
                &visit.project_id,
                &workspace_id,
            ],
        )
        .await
        .map_err(db)?;

    if let Some(row) = existing {
        let id: Uuid = row.try_get(0).map_err(db)?;
        // `save(update_fields=["visited_at"])`: only that column moves;
        // `updated_at` is untouched.
        client
            .execute(
                "UPDATE user_recent_visits SET visited_at = now() WHERE id = $1",
                &[&id],
            )
            .await
            .map_err(db)?;
        return Ok(());
    }

    // `UserRecentVisit.objects.filter(user_id=..., workspace_id=...).count()`.
    let row = client
        .query_one(
            "SELECT count(*) FROM user_recent_visits \
             WHERE user_id = $1 AND workspace_id = $2 AND deleted_at IS NULL",
            &[&visit.user_id, &workspace_id],
        )
        .await
        .map_err(db)?;
    let count: i64 = row.try_get(0).map_err(db)?;
    if count == 20 {
        // `.order_by("created_at").first()` overrides the model ordering.
        let oldest = client
            .query_opt(
                "SELECT id FROM user_recent_visits \
                 WHERE user_id = $1 AND workspace_id = $2 AND deleted_at IS NULL \
                 ORDER BY created_at ASC LIMIT 1",
                &[&visit.user_id, &workspace_id],
            )
            .await
            .map_err(db)?;
        if let Some(oldest) = oldest {
            let id: Uuid = oldest.try_get(0).map_err(db)?;
            // `SoftDeleteModel.delete()` -> `self.deleted_at = now();
            // self.save()`. `BaseModel.save` with no request user nulls
            // `created_by`/`updated_by` and writes every field, so `auto_now`
            // moves both `updated_at` **and** `visited_at`
            // (`plane/db/models/recent_visit.py:30`; `base.py:23-50`).
            client
                .execute(
                    "UPDATE user_recent_visits \
                     SET deleted_at = now(), updated_at = now(), visited_at = now(), \
                         created_by_id = NULL, updated_by_id = NULL \
                     WHERE id = $1",
                    &[&id],
                )
                .await
                .map_err(db)?;
            // `soft_delete_related_objects.delay(app_label, model_name, pk,
            // using=None)` (`plane/db/mixins.py:78`): the same message Django
            // publishes, routed by the ownership table.
            let task = plane_jobs::celery::task_name::SOFT_DELETE_RELATED_OBJECTS;
            let message = plane_jobs::celery::tasks::soft_delete_related_objects(
                "db".to_string(),
                "userrecentvisit".to_string(),
                id,
                None,
            );
            let queued = Job::from_message(&message);
            publish_or_enqueue(context, &***client, task, &message, queued).await?;
        }
    }

    // `UserRecentVisit.objects.create(...)`: `visited_at` explicit, the audit
    // columns from `auto_now_add`/`auto_now`, `created_by`/`updated_by` NULL
    // because there is no current user in a task.
    let id = Uuid::new_v4();
    let now: DateTime<Utc> = Utc::now();
    client
        .execute(
            "INSERT INTO user_recent_visits \
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
              workspace_id, project_id, entity_identifier, entity_name, user_id, visited_at) \
             VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, $8, $9)",
            &[
                &id,
                &now,
                &now,
                &workspace_id,
                &visit.project_id,
                &visit.entity_identifier,
                &visit.entity_name,
                &visit.user_id,
                &now,
            ],
        )
        .await
        .map_err(db)?;
    Ok(())
}

/// Any SQL failure is Django's `DatabaseError`, which the outer `except`
/// swallows. The message is kept for the worker log.
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
    fn decodes_both_keyword_orders() {
        let id = Uuid::nil();
        let project = Uuid::max();
        let decoded = Visit::from_payload(&payload(serde_json::json!({
            "slug": "acme",
            "project_id": project.to_string(),
            "entity_name": "project",
            "entity_identifier": id.to_string(),
            "user_id": id.to_string(),
        })))
        .expect("project-keyed");
        assert_eq!(decoded.project_id, Some(project));

        let decoded = Visit::from_payload(&payload(serde_json::json!({
            "slug": "acme",
            "entity_name": "issue",
            "entity_identifier": id.to_string(),
            "user_id": id.to_string(),
            "project_id": null,
        })))
        .expect("entity-keyed");
        assert_eq!(decoded.project_id, None);
    }

    #[test]
    fn malformed_payloads_are_a_swallowed_error() {
        assert!(Visit::from_payload(&payload(serde_json::json!({"slug": "acme"}))).is_none());
        assert!(
            Visit::from_payload(&payload(serde_json::json!({
                "slug": "acme",
                "entity_name": "project",
                "entity_identifier": "not-a-uuid",
                "user_id": Uuid::nil().to_string(),
            })))
            .is_none()
        );
    }
}
