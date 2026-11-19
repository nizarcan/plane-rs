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
    // Phase 8 set C. `get_asset_object_metadata` moves: ten call sites, every
    // one on a Rust route (ROUTES.md rows 18-21, 24-25, 78-79, 326, 338,
    // 255-256).
    (
        "plane.bgtasks.storage_metadata_task.get_asset_object_metadata",
        &[
            Producer::RustHttp("plane/app/views/asset/v2.py:219"),
            Producer::RustHttp("plane/app/views/asset/v2.py:460"),
            Producer::RustHttp("plane/app/views/asset/v2.py:696"),
            Producer::RustHttp("plane/app/views/issue/attachment.py:277"),
            Producer::RustHttp("plane/api/views/asset.py:217"),
            Producer::RustHttp("plane/api/views/asset.py:373"),
            Producer::RustHttp("plane/api/views/asset.py:662"),
            Producer::RustHttp("plane/api/views/issue.py:2254"),
            Producer::RustHttp("plane/api/views/issue.py:2415"),
            Producer::RustHttp("plane/space/views/asset.py:197"),
        ],
    ),
    // Phase 8 set C. `crawl_work_item_link_title` moves: four call sites, two
    // `/api/` (routes 74-75) and two `/api/v1/` (rows 319-320, 331-332).
    (
        "plane.bgtasks.work_item_link_task.crawl_work_item_link_title",
        &[
            Producer::RustHttp("plane/app/views/issue/link.py:61"),
            Producer::RustHttp("plane/app/views/issue/link.py:92"),
            Producer::RustHttp("plane/api/views/issue.py:1284"),
            Producer::RustHttp("plane/api/views/issue.py:1407"),
        ],
    ),
    // Phase 8 set C. `issue_export_task` moves: its only producer is the
    // export POST, a Rust `/api/` route (`crates/plane-http-app/src/issues/
    // write_export.rs`).
    (
        "plane.bgtasks.export_task.issue_export_task",
        &[Producer::RustHttp("plane/app/views/exporter/base.py:50")],
    ),
    (
        "plane.bgtasks.analytic_plot_export.analytic_export_task",
        &[Producer::RustHttp("plane/app/views/analytic/base.py:256")],
    ),
    // Phase 8 set C: the workspace create POST (route 186) is Rust and calls
    // `workspace_seed.delay(serializer.data["id"])` positionally
    // (`plane/app/views/workspace/base.py:138`).
    (
        "plane.bgtasks.workspace_seed_task.workspace_seed",
        &[Producer::RustHttp("plane/app/views/workspace/base.py:138")],
    ),
    // Phase 8 set C: `PageDuplicateEndpoint.post` (route 133) is Rust.
    (
        "plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets",
        &[Producer::RustHttp("plane/app/views/page/base.py:617")],
    ),
    // Phase 8 set C, handler ported but not switched. The only producer is the
    // Django beat entry (twice); a task moves only after its
    // `plane_jobs.schedules` row is flipped to `owner='rust'`
    // (CUTOVER.md §2 "Beat handover").
    (
        "plane.bgtasks.exporter_expired_task.delete_old_s3_link",
        &[
            Producer::CeleryBeat("plane/celery.py:69-72"),
            Producer::CeleryBeat("plane/celery.py:97-100"),
        ],
    ),
    (
        "plane.bgtasks.file_asset_task.delete_unuploaded_file_asset",
        &[Producer::CeleryBeat("plane/celery.py:73-76")],
    ),
    // Phase 8 set D's cleanup jobs: the only producer is the Django beat entry,
    // which `plane-beat` takes over (`beat::RUST_SCHEDULES`).
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
    // Step A3's version/description tasks: five call sites, all in
    // `plane-http-app` (`TASKS.md:727-739`).
    (
        "plane.bgtasks.issue_description_version_task.issue_description_version_task",
        &[
            Producer::RustHttp("plane/app/views/issue/base.py:484 (create)"),
            Producer::RustHttp("plane/app/views/issue/base.py:710 (update)"),
            Producer::RustHttp("plane/app/views/intake/base.py:294 (intake create)"),
            Producer::RustHttp("plane/app/views/intake/base.py:456 (intake update)"),
        ],
    ),
    (
        "plane.bgtasks.page_transaction_task.page_transaction",
        &[
            Producer::RustHttp("plane/app/views/page/base.py:160 (PageViewSet.create)"),
            Producer::RustHttp("plane/app/views/page/base.py:204 (PageViewSet.partial_update)"),
            Producer::RustHttp("plane/app/views/page/base.py:558 (PagesDescriptionViewSet)"),
            Producer::RustHttp("plane/app/views/page/base.py:610 (PageDuplicateEndpoint)"),
        ],
    ),
    (
        "plane.bgtasks.page_version_task.track_page_version",
        &[Producer::RustHttp(
            "plane/app/views/page/base.py:565 (PagesDescriptionViewSet)",
        )],
    ),
    // Step A3 moved `track_page_version` to Rust. Its trim
    // (`page_version_task.py:72-74`) is the only publish of this task outside
    // the Rust HTTP hooks, and it is unreachable: both write branches raise
    // `AttributeError: 'Page' object has no attribute 'description'`
    // (`:55`, `:63`) before reaching the delete. The Python task body can no
    // longer run because no producer publishes it, so D25's producer rule is
    // satisfied and the task moves with A3. The Rust handler reproduces the
    // observable effect: no row change and no publish
    // (`plane-tasks/src/versions.rs`).
    // Phase 8 set B. `model_activity`'s 22 call sites are all Rust HTTP
    // routes (`TASKS.md:844-873`): the `/api/` viewsets and the three v1 API
    // endpoints. `webhook_activity` is published by `model_activity` (Rust)
    // and by two delete routes; `webhook_send_task` only by
    // `webhook_activity`, and the deactivation e-mail only by
    // `webhook_send_task`.
    (
        "plane.bgtasks.webhook_task.model_activity",
        &[
            Producer::RustHttp("plane/app/views/module/base.py:340"),
            Producer::RustHttp("plane/app/views/module/base.py:709"),
            Producer::RustHttp("plane/app/views/cycle/base.py:319"),
            Producer::RustHttp("plane/app/views/cycle/base.py:398"),
            Producer::RustHttp("plane/app/views/project/base.py:301"),
            Producer::RustHttp("plane/app/views/project/base.py:370"),
            Producer::RustHttp("plane/app/views/issue/comment.py:135"),
            Producer::RustHttp("plane/app/views/issue/comment.py:173"),
            Producer::RustHttp("plane/app/views/issue/base.py:474"),
            Producer::RustHttp("plane/app/views/issue/base.py:700"),
            Producer::RustHttp("plane/api/views/cycle.py:339"),
            Producer::RustHttp("plane/api/views/cycle.py:595"),
            Producer::RustHttp("plane/api/views/module.py:232"),
            Producer::RustHttp("plane/api/views/module.py:490"),
            Producer::RustHttp("plane/api/views/issue.py:592"),
            Producer::RustHttp("plane/api/views/issue.py:754"),
            Producer::RustHttp("plane/api/views/issue.py:801"),
            Producer::RustHttp("plane/api/views/issue.py:889"),
            Producer::RustHttp("plane/api/views/issue.py:1602"),
            Producer::RustHttp("plane/api/views/issue.py:1763"),
            Producer::RustHttp("plane/api/views/project.py:295"),
            Producer::RustHttp("plane/api/views/project.py:589"),
            Producer::RustHttp("plane-http-public/src/issues.rs:2190 (public routes)"),
        ],
    ),
    (
        "plane.bgtasks.webhook_task.webhook_activity",
        &[
            Producer::RustTask("plane-tasks/src/webhook/mod.rs (model_activity)"),
            Producer::RustHttp("plane/app/views/project/base.py:401"),
            Producer::RustHttp("plane/api/views/project.py:637"),
        ],
    ),
    (
        "plane.bgtasks.webhook_task.webhook_send_task",
        &[Producer::RustTask(
            "plane-tasks/src/webhook/mod.rs (webhook_activity)",
        )],
    ),
    (
        "plane.bgtasks.webhook_task.send_webhook_deactivation_email",
        &[Producer::RustTask(
            "plane-tasks/src/webhook/mod.rs (webhook_send_task)",
        )],
    ),
    // Phase 8 set B's e-mail notifications. 13's only producer is 12, whose
    // only producer is the Django beat entry `plane/celery.py:48-51`, taken
    // over through `beat::RUST_SCHEDULES`.
    (
        "plane.bgtasks.email_notification_task.stack_email_notification",
        &[Producer::RustBeat(
            "plane/celery.py:48-51 (check-every-five-minutes-to-send-email-notifications)",
        )],
    ),
    (
        "plane.bgtasks.email_notification_task.send_email_notification",
        &[Producer::RustTask(
            "plane-tasks/src/email_notification.rs (stack_email_notification)",
        )],
    ),
    // Phase 8 set B's e-mail tasks. 17 and 28 are published by the Rust
    // `/auth/` flows; 36 by the Rust sign-up adapter; 37-39 by
    // `/api/users/me/`; 32 by the project-member invite POST; 45 by the
    // workspace invitation POST.
    (
        "plane.bgtasks.forgot_password_task.forgot_password",
        &[
            Producer::RustHttp("plane-http-app/src/auth/password.rs:223"),
            Producer::RustHttp("plane-auth/src/auth_flow/tasks.rs:104"),
        ],
    ),
    (
        "plane.bgtasks.magic_link_code_task.magic_link",
        &[
            Producer::RustHttp("plane-http-app/src/auth/magic.rs:158"),
            Producer::RustHttp("plane-auth/src/auth_flow/tasks.rs:77"),
        ],
    ),
    (
        "plane.bgtasks.user_activation_email_task.user_activation_email",
        &[
            Producer::RustHook("plane-auth/src/auth_flow/adapter.rs:421"),
            Producer::RustHttp("plane-auth/src/auth_flow/tasks.rs:127"),
        ],
    ),
    (
        "plane.bgtasks.user_deactivation_email_task.user_deactivation_email",
        &[Producer::RustHttp(
            "plane-http-app/src/users_me/write_deactivate.rs:153",
        )],
    ),
    (
        "plane.bgtasks.user_email_update_task.send_email_update_magic_code",
        &[Producer::RustHttp(
            "plane-http-app/src/users_me/write_email.rs:131",
        )],
    ),
    (
        "plane.bgtasks.user_email_update_task.send_email_update_confirmation",
        &[Producer::RustHttp(
            "plane-http-app/src/users_me/write_email.rs:268",
        )],
    ),
    (
        "plane.bgtasks.project_add_user_email_task.project_add_user_email",
        &[Producer::RustHttp(
            "plane-http-app/src/projects/member_write.rs:628",
        )],
    ),
    (
        "plane.bgtasks.workspace_invitation_task.workspace_invitation",
        &[Producer::RustHttp(
            "plane-http-app/src/workspaces/invite_write.rs:576",
        )],
    ),
    (
        "plane.bgtasks.deletion_task.soft_delete_related_objects",
        &[
            Producer::RustHttp("plane-http-app::write::publish_soft_delete_cascade (H03 sites)"),
            Producer::RustTask(
                "plane-tasks/src/versions.rs (track_page_version trim; unreachable upstream)",
            ),
        ],
    ),
    // Set A1 moves the activity engine. Its 78 call sites are the three HTTP
    // surfaces (all Rust since Phase 7) plus two Rust task paths:
    // `issue_automation_task` and `cycle_transfer_issues`
    // (`plane/utils/cycle_transfer_issues.py:461`, now
    // `plane-http-app/src/cycles/write_transfer.rs`). Every enqueue goes
    // through `plane_jobs::celery::tasks::issue_activity`; the view files are
    // listed by group because `TASKS.md:631-712` already enumerates them one
    // by one.
    (
        "plane.bgtasks.issue_activities_task.issue_activity",
        &[
            Producer::RustHttp(
                "plane/app/views/{issue,cycle,module,intake,estimate,comment,link,reaction,vote,draft,workspace,sub_issue} (app API)",
            ),
            Producer::RustHttp(
                "plane/api/views/{issue,cycle,module,intake,comment,link,relation} (v1)",
            ),
            Producer::RustHttp(
                "plane/space/views/{issue,comment,reaction,vote,intake} (public API)",
            ),
            Producer::RustHttp(
                "plane-http-app/src/cycles/write_transfer.rs (transfer_cycle_issues)",
            ),
            Producer::RustTask("plane-tasks/src/automation.rs (archive/close)"),
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
