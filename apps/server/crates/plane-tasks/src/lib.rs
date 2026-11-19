// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Rust handlers for Plane's Celery tasks (`plans/CUTOVER.md` §1).
//!
//! A task moves here in the same change that adds it to
//! `plane_jobs::ownership::RUST_TASKS` and to `plane_jobs::policy::PRODUCERS`;
//! `TaskRegistry` registration in [`registry`] is the other half. The worker
//! gives each handler a [`TaskContext`] (the long-lived resources) and a
//! [`JobContext`] (its own database connection and the decoded payload).
//!
//! **Payload shape.** [`plane_jobs::Job::from_message`] stores
//! `{"args": [...], "kwargs": {...}}` with Python runtime values. A handler
//! reads its keyword arguments from `payload["kwargs"]`, exactly as the
//! Celery task received them, so a handler test can run against either
//! representation.
//!
//! **Failure policy.** Django's tasks swallow every exception
//! (`log_exception` + `return`, `TASKS.md:517-521`), so a handler returns
//! `Ok(())` after logging a data error. Only `webhook_send_task` retries
//! (`plane_jobs::policy`).

pub mod analytic_export;
pub mod bs4;
mod bs4_entities;
pub mod copy_s3_object;
pub mod export_task;
pub mod exporter_expired;
pub mod file_asset;
pub mod html;
pub mod recent_visited;
pub mod storage_metadata;
pub mod work_item_link;

use std::sync::Arc;

use plane_cache::Redis;
use plane_core::config::Settings;
use plane_db::DbPools;
use plane_jobs::worker::{HandlerError, TaskRegistry};
use plane_mail::EmailConfiguration;
use plane_net::HttpEngine;
use plane_storage::Storage;

/// The resources a task handler shares for the life of the worker.
///
/// Constructing the optional clients is lazy in effect: `Redis` and `Storage`
/// build handles, not connections. A task that needs one and finds it `None`
/// reports a handler error; no task silently skips work because a deployment
/// left a component unconfigured.
pub struct TaskContext {
    /// The database pools, including the read replica once one is configured.
    pub db: DbPools,
    /// The parsed environment; later sets read `WEB_URL`, `LIVE_URL`,
    /// `SECRET_KEY` and the webhook policy from it.
    pub settings: Settings,
    /// `REDIS_URL`, for the tasks that hold locks (`email_notification_task`'s
    /// `SET NX EX 300`) or write origin keys.
    pub redis: Option<Redis>,
    /// The S3 client (`plane-storage`).
    pub storage: Option<Storage>,
    /// The SSRF-pinned HTTP client (`plane-net`) for webhook and link tasks.
    pub http: HttpEngine,
}

impl TaskContext {
    /// Build the context once, in the worker's `main`.
    #[must_use]
    pub fn new(db: DbPools, settings: Settings) -> Self {
        let redis = Redis::from_settings(&settings)
            .map_err(|error| {
                tracing::warn!(target: "plane.tasks", %error, "redis client unavailable");
                error
            })
            .ok();
        let storage = Storage::from_process_env(&settings.storage)
            .map_err(|error| {
                tracing::warn!(target: "plane.tasks", %error, "object storage unavailable");
                error
            })
            .ok();
        Self {
            db,
            settings,
            redis,
            storage,
            http: HttpEngine::system(),
        }
    }

    /// `get_email_configuration()` for a task's connection
    /// (`plane/license/utils/instance_value.py:42-59`).
    ///
    /// The caller passes its own database connection: the configuration is
    /// seven `InstanceConfiguration` rows, read on every task send, as Django
    /// does.
    ///
    /// # Errors
    ///
    /// A database failure or an invalid `EMAIL_PORT`, both of which Django
    /// raises outside the task's `try` (it does not: every e-mail task wraps
    /// the whole body in `try`/`except Exception`; the error is logged and the
    /// task returns).
    pub async fn email_configuration(
        &self,
        client: &deadpool_postgres::Client,
    ) -> Result<EmailConfiguration, plane_mail::MailError> {
        EmailConfiguration::load(client, &self.settings).await
    }
}

/// Register every Rust-owned task handler.
///
/// This is the consumer half of the ownership table: a task in `RUST_TASKS`
/// with no handler here is dead-lettered by the worker
/// (`plane-jobs/src/worker.rs:295-306`), so the test at the bottom fails
/// instead.
#[must_use]
pub fn registry(context: Arc<TaskContext>) -> TaskRegistry {
    let mut registry = TaskRegistry::new();
    recent_visited::register(&mut registry, Arc::clone(&context));
    analytic_export::register(&mut registry, Arc::clone(&context));
    export_task::register(&mut registry, Arc::clone(&context));
    exporter_expired::register(&mut registry, Arc::clone(&context));
    file_asset::register(&mut registry, Arc::clone(&context));
    storage_metadata::register(&mut registry, Arc::clone(&context));
    work_item_link::register(&mut registry, Arc::clone(&context));
    copy_s3_object::register(&mut registry, Arc::clone(&context));
    registry
}

/// The tasks [`registry`] installs, for the ownership/handler consistency
/// test. Keep in step with the `register` calls above.
pub const REGISTERED_TASKS: &[&str] = &[
    recent_visited::TASK,
    analytic_export::TASK,
    export_task::TASK,
    exporter_expired::TASK,
    file_asset::TASK,
    storage_metadata::TASK,
    work_item_link::TASK,
    copy_s3_object::TASK,
];

/// Decode `payload["kwargs"]`, the shape `Job::from_message` writes.
///
/// A missing or non-object payload is Django's `TypeError` inside the task's
/// `try`: logged, no rows changed, task succeeds. The handler turns the
/// `None` into its own no-op return.
#[must_use]
pub fn kwargs(payload: &serde_json::Value) -> Option<&serde_json::Map<String, serde_json::Value>> {
    payload.get("kwargs")?.as_object()
}

/// The task's `args` array, for the rare positional call site.
#[must_use]
pub fn args(payload: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    payload.get("args")?.as_array()
}

/// A UUID argument as Django's ORM coerces it: a UUID object or its string.
#[must_use]
pub fn uuid_value(value: Option<&serde_json::Value>) -> Option<uuid::Uuid> {
    match value? {
        serde_json::Value::String(text) => uuid::Uuid::parse_str(text).ok(),
        _ => None,
    }
}

/// A string argument.
#[must_use]
pub fn string_value(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Log and succeed, Django's `except Exception` branch.
pub fn swallow(error: impl std::fmt::Display, task: &str) {
    tracing::warn!(target: "plane.tasks", task, error = %error, "task failed; Django logs and returns");
}

/// A handler that cannot proceed because the worker's context lacks a
/// component. Retrying cannot create a Redis URL, so it is fatal.
pub fn missing_component(component: &str) -> HandlerError {
    HandlerError::fatal(format!("{component} is not configured for this worker"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_jobs::ownership::RUST_TASKS;

    #[test]
    fn every_rust_task_has_a_registered_handler() {
        for task in RUST_TASKS {
            // `plane.jobs.echo` is the worker's test-only handler.
            if *task == "plane.jobs.echo" {
                continue;
            }
            assert!(
                REGISTERED_TASKS.contains(task),
                "Rust-owned task {task} has no handler in plane_tasks::registry"
            );
        }
    }

    #[test]
    fn kwargs_are_read_from_the_shared_payload_shape() {
        let payload = serde_json::json!({
            "args": [],
            "kwargs": {"slug": "acme", "entity_name": "project"},
        });
        let kwargs = kwargs(&payload).expect("kwargs");
        assert_eq!(string_value(kwargs.get("slug")).as_deref(), Some("acme"));
        assert!(args(&payload).is_some_and(Vec::is_empty));
    }
}
