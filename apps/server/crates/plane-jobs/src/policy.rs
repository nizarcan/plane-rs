// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Per-task policy: retries, and the producer table that decides when a task
//! may move from Celery to the Postgres queue.
//!
//! Two rules from `plans/CUTOVER.md` §2 live here:
//!
//! * **Retries.** Only `webhook_send_task` retries upstream: `max_retries=5`
//!   with a uniform 0–600 s jitter (`TASKS.md:498-515`). Every other Plane task
//!   swallows its exception (`log_exception` + `return`), so the Rust policy is
//!   zero retries, record `last_error` and `dead`. Retrying would create rows
//!   Django never created.
//! * **Producer before consumer.** A Python producer still calls `.delay()`,
//!   which only RabbitMQ delivers (`TASKS.md:943-945`). A task may therefore
//!   move only when every producer is Rust. [`PRODUCERS`] records that; the
//!   test at the bottom fails if a Rust-owned task names a non-Rust producer.
//!
//! The table is deliberately small: only tasks that have moved (or are about to
//! move) need a row, and a Rust-owned task with no row fails the test. That
//! makes the check as strong as the ownership table without pretending the
//! Phase 7 surface is finished.

/// What a task's retry policy is when the queue executes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskPolicy {
    /// Retries after the first attempt; `0` = dead on first failure.
    pub max_retries: i32,
    /// Delay before a retry, in seconds.
    pub retry_backoff_seconds: i32,
    /// Pick uniformly from `[0, retry_backoff_seconds]` instead of the full
    /// delay (`celery`'s `retry_backoff=True` with `retry_jitter=True`).
    pub retry_jitter: bool,
}

impl TaskPolicy {
    /// The default: one attempt, then `dead`.
    pub const DEAD_ON_FAILURE: Self = Self {
        max_retries: 0,
        retry_backoff_seconds: 0,
        retry_jitter: false,
    };

    const fn retry(max_retries: i32, backoff: i32, jitter: bool) -> Self {
        Self {
            max_retries,
            retry_backoff_seconds: backoff,
            retry_jitter: jitter,
        }
    }
}

/// The retry policy table. A task that is missing keeps
/// [`TaskPolicy::DEAD_ON_FAILURE`], which is Django's behaviour for every task
/// except the one below (`TASKS.md:517-521`).
#[must_use]
pub fn policy(task: &str) -> TaskPolicy {
    match task {
        // `plane/bgtasks/webhook_task.py:242-386`: `@shared_task(max_retries=5,
        // retry_backoff=True)` — Celery's backoff is `2 ** retries` *without*
        // jitter unless `retry_jitter` is set; Plane sets
        // `retry_backoff=600, retry_jitter=True`, a uniform 0–600 s
        // (`TASKS.md:498-515`).
        "plane.bgtasks.webhook_task.webhook_send_task" => TaskPolicy::retry(5, 600, true),
        _ => TaskPolicy::DEAD_ON_FAILURE,
    }
}

/// Who publishes a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Producer {
    /// A Rust HTTP handler (`plane-http-app`).
    RustHttp(&'static str),
    /// Another Rust task handler.
    RustTask(&'static str),
    /// A Rust `plane-beat` schedule.
    RustBeat(&'static str),
    /// A Rust signal receiver or model hook.
    RustHook(&'static str),
    /// A Django view or task that still calls `.delay()`.
    Celery(&'static str),
    /// A Django beat entry.
    CeleryBeat(&'static str),
}

impl Producer {
    /// Whether this producer publishes to the Postgres queue. Only Rust ones
    /// do; `.delay()` has no Postgres path (`TASKS.md:943-945`).
    #[must_use]
    pub fn is_rust(self) -> bool {
        matches!(
            self,
            Self::RustHttp(_) | Self::RustTask(_) | Self::RustBeat(_) | Self::RustHook(_)
        )
    }
}

/// The producers of each task that is Rust-owned or about to move.
///
/// `recent_visited_task`'s ten call sites are the GET handlers listed in
/// `plane-jobs/src/celery/tasks.rs:365-437`; all ten are in
/// `plane-http-app`, so the task can move first (`plans/CUTOVER.md` §6 P0).
pub const PRODUCERS: &[(&str, &[Producer])] = &[
    (
        "plane.jobs.echo",
        &[Producer::RustHook("bins/plane-worker (test-only)")],
    ),
    (
        "plane.bgtasks.recent_visited_task.recent_visited_task",
        &[
            Producer::RustHttp("plane/app/views/issue/base.py:165"),
            Producer::RustHttp("plane/app/views/issue/base.py:304"),
            Producer::RustHttp("plane/app/views/issue/base.py:617"),
            Producer::RustHttp("plane/app/views/issue/base.py:1359"),
            Producer::RustHttp("plane/app/views/project/base.py:247"),
            Producer::RustHttp("plane/app/views/view/base.py:140"),
            Producer::RustHttp("plane/app/views/view/base.py:376"),
            Producer::RustHttp("plane/app/views/cycle/base.py:469"),
            Producer::RustHttp("plane/app/views/module/base.py:642"),
            Producer::RustHttp("plane/app/views/page/base.py:232"),
        ],
    ),
    // Step D's cleanup jobs: the only producer is the Django beat entry, which
    // `plane-beat` takes over (`beat::RUST_SCHEDULES`).
    (
        "plane.bgtasks.cleanup_task.delete_api_logs",
        &[Producer::RustBeat(
            "plane/celery.py:77-80 (check-every-day-to-delete-api-logs)",
        )],
    ),
    (
        "plane.bgtasks.cleanup_task.delete_email_notification_logs",
        &[Producer::RustBeat(
            "plane/celery.py:81-84 (check-every-day-to-delete-email-notification-logs)",
        )],
    ),
    (
        "plane.bgtasks.cleanup_task.delete_page_versions",
        &[Producer::RustBeat(
            "plane/celery.py:85-88 (check-every-day-to-delete-page-versions)",
        )],
    ),
    (
        "plane.bgtasks.cleanup_task.delete_issue_description_versions",
        &[Producer::RustBeat(
            "plane/celery.py:89-92 (check-every-day-to-delete-issue-description-versions)",
        )],
    ),
    (
        "plane.bgtasks.cleanup_task.delete_webhook_logs",
        &[Producer::RustBeat(
            "plane/celery.py:93-96 (check-every-day-to-delete-webhook-logs)",
        )],
    ),
    (
        "plane.bgtasks.deletion_task.hard_delete",
        &[Producer::RustBeat(
            "plane/celery.py:61-64 (check-every-day-to-delete-hard-delete)",
        )],
    ),
    (
        "plane.bgtasks.issue_automation_task.archive_and_close_old_issues",
        &[Producer::RustBeat(
            "plane/celery.py:65-68 (check-every-day-to-archive-and-close)",
        )],
    ),
    (
        "plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues",
        &[Producer::RustBeat(
            "plane/celery.py:52-55 (check-every-five-minutes-to-unsnooze-intake-issues)",
        )],
    ),
    (
        "plane.bgtasks.logger_task.process_logs",
        &[Producer::RustHttp(
            "bins/plane-api/src/middleware/extensions.rs:56 (APITokenLogMiddleware)",
        )],
    ),
    // Not Rust-owned yet: the handler is ported and `difftest task` proves it,
    // but `page_version_task` is still a Django task (set A) whose
    // `PageVersion.objects...first().delete()` publishes this task through
    // `.delay()` (`plane/bgtasks/page_version_task.py:74`). D25 keeps the task
    // on Celery until that producer is Rust.
    (
        "plane.bgtasks.deletion_task.soft_delete_related_objects",
        &[
            Producer::RustHttp("plane-http-app::write::publish_soft_delete_cascade (H03 sites)"),
            Producer::Celery("plane/bgtasks/page_version_task.py:74"),
        ],
    ),
];

/// The producers recorded for `task`, or `None` when the table has no row.
#[must_use]
pub fn producers(task: &str) -> Option<&'static [Producer]> {
    PRODUCERS
        .iter()
        .find(|(name, _)| *name == task)
        .map(|(_, producers)| *producers)
}

/// The safety rule as a function, so the test can prove that a mixed producer
/// set is rejected. Returns the offending producers.
#[must_use]
pub fn non_rust_producers(producers: &[Producer]) -> Vec<Producer> {
    producers.iter().copied().filter(|p| !p.is_rust()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ownership::RUST_TASKS;

    #[test]
    fn every_rust_task_has_only_rust_producers() {
        for task in RUST_TASKS {
            let producers = producers(task).unwrap_or_else(|| {
                panic!("Rust-owned task {task} has no PRODUCERS row; add it before moving the task")
            });
            assert!(
                !producers.is_empty(),
                "Rust-owned task {task} has an empty producer list"
            );
            let offenders = non_rust_producers(producers);
            assert!(
                offenders.is_empty(),
                "Rust-owned task {task} still has non-Rust producers: {offenders:?}"
            );
        }
    }

    #[test]
    fn a_mixed_producer_set_is_rejected() {
        let mixed = [
            Producer::RustHttp("rust"),
            Producer::Celery("plane/app/views/old.py:1"),
        ];
        assert_eq!(non_rust_producers(&mixed), vec![mixed[1]]);
    }

    #[test]
    fn only_the_webhook_send_task_retries() {
        assert_eq!(
            policy("plane.bgtasks.webhook_task.webhook_send_task"),
            TaskPolicy::retry(5, 600, true)
        );
        assert_eq!(
            policy("plane.bgtasks.recent_visited_task.recent_visited_task"),
            TaskPolicy::DEAD_ON_FAILURE
        );
        assert_eq!(policy("plane.bgtasks.unknown"), TaskPolicy::DEAD_ON_FAILURE);
    }
}
