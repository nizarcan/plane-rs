// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Tasks the Rust backend no longer publishes to RabbitMQ.
//!
//! During the Phase 8 cutover a task moves one at a time
//! (`apps/server/plans/CUTOVER.md` §2): its producer inserts a `plane_jobs` row
//! instead of a Celery message. A differential scenario therefore sees one
//! Celery delivery on target A (Django) and none on target B (Rust) for every
//! switched task. That is the intended transport change, not a side-effect
//! difference, so the harness drops those messages from A's drain before
//! comparing.
//!
//! The list mirrors `plane-jobs/src/ownership.rs::RUST_TASKS`. The harness is
//! a separate Cargo workspace and cannot depend on `plane-jobs` (its features
//! would leak into the server build, `apps/server/Cargo.toml`), so the names
//! are duplicated. `plane-jobs`' `policy::every_rust_task_has_only_rust_producers`
//! fails when a task moves without its producer table, and `difftest task` is
//! the handler comparison; this list only keeps the HTTP sweeps green for the
//! same moves.
//!
//! `plane.jobs.echo` is test-only and never published by an HTTP scenario.

use crate::celery::CeleryMessage;

/// Registered task names whose producer is Rust-owned.
pub const RUST_OWNED_TASKS: &[&str] = &[
    "plane.jobs.echo",
    "plane.bgtasks.recent_visited_task.recent_visited_task",
];

/// Whether `task` is published to `plane_jobs` rather than to RabbitMQ.
#[must_use]
pub fn rust_owned(task: &str) -> bool {
    RUST_OWNED_TASKS.contains(&task)
}

/// Remove the deliveries that only Django's producer can make.
///
/// The caller passes target A's messages; a Rust-owned task's absence on
/// target B is not a difference. Messages stay in their drained order, so
/// every other comparison is unchanged.
pub fn drop_rust_owned(messages: &mut Vec<CeleryMessage>) {
    messages.retain(|message| message.task.as_deref().is_none_or(|task| !rust_owned(task)));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn message(task: &str) -> CeleryMessage {
        CeleryMessage {
            queue: "celery".to_string(),
            task: Some(task.to_string()),
            headers: BTreeMap::new(),
            properties: BTreeMap::new(),
            args: serde_json::Value::Array(Vec::new()),
            kwargs: serde_json::Value::Object(serde_json::Map::new()),
            body_extra: None,
        }
    }

    #[test]
    fn only_switched_tasks_are_dropped() {
        let mut messages = vec![
            message("plane.bgtasks.recent_visited_task.recent_visited_task"),
            message("plane.bgtasks.logger_task.process_logs"),
        ];
        drop_rust_owned(&mut messages);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].task.as_deref(),
            Some("plane.bgtasks.logger_task.process_logs")
        );
    }
}
