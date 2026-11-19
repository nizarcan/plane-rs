// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Which side owns each task: Celery or the Postgres queue.
//!
//! During coexistence a task has exactly one owner. The default is Celery, so
//! a task that is missing from this file stays on the wire-identical Celery
//! publisher; a task moves by adding it to [`RUST_TASKS`] (and to the
//! `TASKS.md` §10 table) in the same change that ports its handler.
//!
//! `PLANE_RS_JOBS_RUST_TASKS` is a comma-separated dev/test override. It exists
//! so the Phase 4 exit check can make the test-only `plane.jobs.echo` task
//! Rust-owned without editing the production table; it is documented in
//! `ENV.md` and is not for deployments.

use std::sync::LazyLock;

/// The production ownership table.
///
/// `plane.jobs.echo` exists only for the worker/beat tests. Phase 8 P0 moves
/// `recent_visited_task`, the first real task: all ten of its producers are
/// already Rust (`policy::PRODUCERS`), which is the safety rule in
/// `plans/CUTOVER.md` §2.
pub const RUST_TASKS: &[&str] = &[
    "plane.jobs.echo",
    "plane.bgtasks.recent_visited_task.recent_visited_task",
    "plane.bgtasks.cleanup_task.delete_api_logs",
    "plane.bgtasks.cleanup_task.delete_email_notification_logs",
    "plane.bgtasks.cleanup_task.delete_page_versions",
    "plane.bgtasks.cleanup_task.delete_issue_description_versions",
    "plane.bgtasks.cleanup_task.delete_webhook_logs",
    "plane.bgtasks.deletion_task.hard_delete",
    "plane.bgtasks.issue_automation_task.archive_and_close_old_issues",
    "plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues",
    "plane.bgtasks.logger_task.process_logs",
];

/// A task's owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Celery,
    Rust,
}

impl Owner {
    #[must_use]
    pub fn is_rust(self) -> bool {
        matches!(self, Self::Rust)
    }
}

fn env_overrides() -> &'static [String] {
    static OVERRIDES: LazyLock<Vec<String>> = LazyLock::new(|| {
        std::env::var("PLANE_RS_JOBS_RUST_TASKS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect()
    });
    &OVERRIDES
}

/// The owner of one task. Unknown tasks are Celery, never silently Rust.
#[must_use]
pub fn owner(task: &str) -> Owner {
    owner_with(task, env_overrides())
}

/// [`owner`] with an explicit override list, for tests and for callers that
/// loaded the table before the process environment was frozen.
#[must_use]
pub fn owner_with(task: &str, overrides: &[String]) -> Owner {
    if RUST_TASKS.contains(&task) || overrides.iter().any(|entry| entry == task) {
        Owner::Rust
    } else {
        Owner::Celery
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_celery() {
        assert_eq!(
            owner_with("plane.bgtasks.issue_activities_task.issue_activity", &[]),
            Owner::Celery
        );
        assert_eq!(owner_with("plane.bgtasks.unknown", &[]), Owner::Celery);
    }

    #[test]
    fn the_test_task_is_rust_and_an_override_still_works() {
        assert_eq!(owner_with("plane.jobs.echo", &[]), Owner::Rust);
        assert_eq!(
            owner_with("plane.bgtasks.recent_visited_task.recent_visited_task", &[]),
            Owner::Rust
        );
        assert_eq!(
            owner_with("plane.jobs.echo", &["plane.jobs.echo".to_owned()]),
            Owner::Rust
        );
        assert_eq!(
            owner_with("plane.jobs.other", &["plane.jobs.other".to_owned()]),
            Owner::Rust
        );
    }
}
