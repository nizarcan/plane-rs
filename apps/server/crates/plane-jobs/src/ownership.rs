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
    // Phase 8 set C: ExportAnalyticsEndpoint.post is a Rust route.
    "plane.bgtasks.analytic_plot_export.analytic_export_task",
    // Phase 8 set D: the only producer is a Django beat entry, which
    // `plane-beat` takes over (`beat::RUST_SCHEDULES`).
    "plane.bgtasks.cleanup_task.delete_api_logs",
    "plane.bgtasks.cleanup_task.delete_email_notification_logs",
    "plane.bgtasks.cleanup_task.delete_issue_description_versions",
    "plane.bgtasks.cleanup_task.delete_page_versions",
    "plane.bgtasks.cleanup_task.delete_webhook_logs",
    // Phase 8 set C: PageDuplicateEndpoint.post (route 133) is a Rust route.
    "plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets",
    // Phase 8 set D: `check-every-day-to-delete-hard-delete`.
    "plane.bgtasks.deletion_task.hard_delete",
    // Phase 8 set C: the only producer is ExportIssuesEndpoint.post, a Rust route.
    "plane.bgtasks.export_task.issue_export_task",
    // Phase 8 set D: `check-every-day-to-archive-and-close` and
    // `check-every-five-minutes-to-unsnooze-intake-issues`.
    "plane.bgtasks.issue_automation_task.archive_and_close_old_issues",
    "plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues",
    // Phase 8 set D: `APITokenLogMiddleware`.
    "plane.bgtasks.logger_task.process_logs",
    "plane.bgtasks.issue_description_version_task.issue_description_version_task",
    "plane.bgtasks.page_transaction_task.page_transaction",
    "plane.bgtasks.page_version_task.track_page_version",
    "plane.bgtasks.deletion_task.soft_delete_related_objects",
    "plane.bgtasks.recent_visited_task.recent_visited_task",
    // Phase 8 set C: all ten producers are Rust HTTP routes.
    "plane.bgtasks.storage_metadata_task.get_asset_object_metadata",
    // Phase 8 set C: four producers, all Rust HTTP routes.
    "plane.bgtasks.work_item_link_task.crawl_work_item_link_title",
    // Phase 8 set C: WorkspaceViewSet.create (route 186) is a Rust route.
    "plane.bgtasks.workspace_seed_task.workspace_seed",
    // Phase 8 set B: the webhook chain moves in its producer order.
    "plane.bgtasks.webhook_task.model_activity",
    "plane.bgtasks.webhook_task.webhook_activity",
    "plane.bgtasks.webhook_task.webhook_send_task",
    "plane.bgtasks.webhook_task.send_webhook_deactivation_email",
    // Phase 8 set B: the auth and user e-mails. Every producer is a Rust route
    // or the Rust auth adapter (`PRODUCERS` below).
    "plane.bgtasks.forgot_password_task.forgot_password",
    "plane.bgtasks.magic_link_code_task.magic_link",
    "plane.bgtasks.project_add_user_email_task.project_add_user_email",
    "plane.bgtasks.user_activation_email_task.user_activation_email",
    "plane.bgtasks.user_deactivation_email_task.user_deactivation_email",
    "plane.bgtasks.user_email_update_task.send_email_update_magic_code",
    "plane.bgtasks.user_email_update_task.send_email_update_confirmation",
    "plane.bgtasks.workspace_invitation_task.workspace_invitation",
    // Phase 8 set A1: every producer is a Rust HTTP route, the Rust
    // `archive_and_close_old_issues`, or `transfer_cycle_issues`.
    "plane.bgtasks.issue_activities_task.issue_activity",
    // Phase 8 set B: 13 is published only by 12, which is published only by
    // the beat entry `check-every-five-minutes-to-send-email-notifications`
    // (`beat::RUST_SCHEDULES`).
    "plane.bgtasks.email_notification_task.stack_email_notification",
    "plane.bgtasks.email_notification_task.send_email_notification",
    // Phase 8 set A2: its only producer is the Rust `issue_activity` handler's
    // `notification=True` re-publish (`issue_activities_task.py:1632-1645`).
    "plane.bgtasks.notification_task.notifications",
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
        // `push_instance_metrics` is never ported (owner decision 5,
        // `RUST_REWRITE.md:237`), so it stays a Celery-owned task for good.
        assert_eq!(
            owner_with(
                "plane.license.bgtasks.telemetry_metrics.push_instance_metrics",
                &[]
            ),
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
