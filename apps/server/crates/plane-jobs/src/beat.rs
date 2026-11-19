// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-beat`: the DatabaseScheduler replacement.
//!
//! The 13 `apps/api/plane/celery.py:46-101` entries are seeded into
//! `plane_jobs.schedules` once; `owner` and `enabled` are the operator's
//! columns. A Rust-owned entry is fired by this process and, before the first
//! fire, its Django `django_celery_beat_periodictask` row is disabled. That
//! flag survives every `DatabaseScheduler` restart (JOBS.md).
//!
//! Exactly once across replicas is two independent guards in one transaction:
//! a transaction-scoped advisory lock keyed by schedule+tick, and the
//! `ticks` primary key. A crash before commit leaves neither; a crash after
//! commit leaves both. A missed tick fires once for the latest due tick, as
//! Celery's `reserve` (`celery/beat.py:388-390`) sets `last_run_at = now`.

use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Timelike, Utc};
use plane_db::DbPools;
use serde_json::json;
use tokio_postgres::GenericClient;

use crate::queue::{self, Job, QueueError};

/// One of the 13 static entries. `cron` is the five-field form; the parser
/// only accepts the fields the real entries use (minute/hour, `*` for the
/// rest), so an operator cannot accidentally create an unsupported schedule.
#[derive(Clone, Copy, Debug)]
pub struct ScheduleSeed {
    pub name: &'static str,
    pub task: &'static str,
    pub cron: Option<&'static str>,
    pub interval_seconds: Option<i64>,
}

pub const REAL_SCHEDULES: &[ScheduleSeed] = &[
    ScheduleSeed {
        name: "check-every-five-minutes-to-send-email-notifications",
        task: "plane.bgtasks.email_notification_task.stack_email_notification",
        cron: Some("*/5 * * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-five-minutes-to-unsnooze-intake-issues",
        task: "plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues",
        cron: Some("*/5 * * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: METRICS_SCHEDULE,
        task: "plane.license.bgtasks.telemetry_metrics.push_instance_metrics",
        cron: None,
        interval_seconds: Some(360 * 60),
    },
    ScheduleSeed {
        name: "check-every-day-to-delete-hard-delete",
        task: "plane.bgtasks.deletion_task.hard_delete",
        cron: Some("0 0 * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-day-to-archive-and-close",
        task: "plane.bgtasks.issue_automation_task.archive_and_close_old_issues",
        cron: Some("0 1 * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-day-to-delete_exporter_history",
        task: "plane.bgtasks.exporter_expired_task.delete_old_s3_link",
        cron: Some("30 1 * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-day-to-delete-file-asset",
        task: "plane.bgtasks.file_asset_task.delete_unuploaded_file_asset",
        cron: Some("0 2 * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-day-to-delete-api-logs",
        task: "plane.bgtasks.cleanup_task.delete_api_logs",
        cron: Some("30 2 * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-day-to-delete-email-notification-logs",
        task: "plane.bgtasks.cleanup_task.delete_email_notification_logs",
        cron: Some("45 2 * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-day-to-delete-page-versions",
        task: "plane.bgtasks.cleanup_task.delete_page_versions",
        cron: Some("0 3 * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-day-to-delete-issue-description-versions",
        task: "plane.bgtasks.cleanup_task.delete_issue_description_versions",
        cron: Some("15 3 * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-day-to-delete-webhook-logs",
        task: "plane.bgtasks.cleanup_task.delete_webhook_logs",
        cron: Some("30 3 * * *"),
        interval_seconds: None,
    },
    ScheduleSeed {
        name: "check-every-day-to-delete-exporter-history",
        task: "plane.bgtasks.exporter_expired_task.delete_old_s3_link",
        cron: Some("45 3 * * *"),
        interval_seconds: None,
    },
];

/// The interval schedule whose length is `METRICS_PUSH_INTERVAL_MINUTES`
/// (`celery.py:27-38`); `seed_schedules` takes it from the settings.
pub const METRICS_SCHEDULE: &str = "push-instance-metrics";

pub const TEST_SCHEDULE: &str = "plane.jobs.test-tick";
pub const TEST_TASK: &str = "plane.jobs.echo";

/// The `plane/celery.py` entries whose task is Rust-owned.
///
/// `plans/CUTOVER.md` §2 orders the handover per entry: the task moves first
/// (`ownership::RUST_TASKS`), then its schedule. `seed_schedules` performs that
/// second step on every start — a row whose name is listed here and still has
/// `owner = 'celery'` is flipped to `'rust'`. The quiet-window gate in
/// [`Beat::run`] still decides *when* the entry may fire, and the Django
/// `django_celery_beat_periodictask` row is disabled inside the same
/// transaction (`Beat::handover`).
///
/// A rollback means removing the name here and setting the row back to
/// `'celery'`; the Django row is re-enabled by an operator, as the handover
/// notes in `JOBS.md` describe.
pub const RUST_SCHEDULES: &[&str] = &[
    "check-every-five-minutes-to-send-email-notifications",
    "check-every-five-minutes-to-unsnooze-intake-issues",
    "check-every-day-to-delete-hard-delete",
    "check-every-day-to-archive-and-close",
    "check-every-day-to-delete-api-logs",
    "check-every-day-to-delete-email-notification-logs",
    "check-every-day-to-delete-page-versions",
    "check-every-day-to-delete-issue-description-versions",
    "check-every-day-to-delete-webhook-logs",
    METRICS_SCHEDULE,
];

/// A parsed cron with minute/hour fields. The real entries do not constrain
/// day/month/weekday, so those are accepted only as `*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Field {
    all: bool,
    step: Option<u32>,
    exact: Option<u32>,
}

impl Field {
    fn parse(raw: &str) -> Option<Self> {
        if raw == "*" {
            return Some(Self {
                all: true,
                step: None,
                exact: None,
            });
        }
        if let Some(step) = raw.strip_prefix("*/") {
            return Some(Self {
                all: false,
                step: Some(step.parse().ok()?),
                exact: None,
            });
        }
        Some(Self {
            all: false,
            step: None,
            exact: Some(raw.parse().ok()?),
        })
    }

    fn matches(self, value: u32) -> bool {
        if self.all {
            return true;
        }
        if let Some(exact) = self.exact {
            return value == exact;
        }
        self.step
            .is_some_and(|step| step > 0 && value.is_multiple_of(step))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CronSpec {
    minute: Field,
    hour: Field,
}

impl CronSpec {
    /// Parse the five-field cron strings the static schedule uses.
    ///
    /// # Errors
    ///
    /// `None` for a day/month/weekday field other than `*` or an unparsable
    /// minute/hour; the migrator/beat should fail loudly rather than silently
    /// skip a schedule.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let fields: Vec<&str> = raw.split_whitespace().collect();
        if fields.len() != 5 || fields[2..].iter().any(|field| *field != "*") {
            return None;
        }
        Some(Self {
            minute: Field::parse(fields[0])?,
            hour: Field::parse(fields[1])?,
        })
    }

    fn matches(self, at: DateTime<Utc>) -> bool {
        self.minute.matches(at.minute()) && self.hour.matches(at.hour())
    }

    /// The latest tick at or before `now`, scanning backwards at most eight
    /// days. That is enough for the `*/5` and daily entries; a longer outage
    /// still fires the latest tick once, like Celery.
    fn latest_tick(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut at = now.with_second(0)?.with_nanosecond(0)?;
        for _ in 0..(8 * 24 * 60) {
            if self.matches(at) {
                return Some(at);
            }
            at -= ChronoDuration::minutes(1);
        }
        None
    }

    /// The first tick strictly after `now`, scanning forward at most eight
    /// days. Starting one minute past the current minute avoids returning the
    /// tick that just fired when `now` is on it.
    fn next_tick(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut at = now.with_second(0)?.with_nanosecond(0)? + ChronoDuration::minutes(1);
        for _ in 0..(8 * 24 * 60) {
            if self.matches(at) {
                return Some(at);
            }
            at += ChronoDuration::minutes(1);
        }
        None
    }
}

/// The previous and next due ticks around `now`. Handover is only performed
/// when both are far enough away (see [`QuietWindow`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuietWindow {
    pub previous: DateTime<Utc>,
    pub next: DateTime<Utc>,
}

/// Compute the quiet window for one schedule.
///
/// For a crontab the ticks are absolute (UTC, minute resolution). For an
/// interval the phase comes from `anchor` (the schedule's `last_run_at`), so
/// the sequence is `anchor + k * interval`. `None` means the schedule cannot
/// be evaluated; the caller must not hand it over.
#[must_use]
pub fn quiet_window(
    kind: &str,
    cron: Option<&str>,
    interval_seconds: Option<i64>,
    anchor: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<QuietWindow> {
    match kind {
        "cron" => {
            let spec = CronSpec::parse(cron?)?;
            Some(QuietWindow {
                previous: spec.latest_tick(now)?,
                next: spec.next_tick(now)?,
            })
        }
        "interval" => {
            let interval = interval_seconds?;
            if interval <= 0 {
                return None;
            }
            let anchor = anchor.unwrap_or(now);
            let elapsed = (now - anchor).num_seconds().max(0);
            let steps = elapsed / interval;
            let previous = anchor + ChronoDuration::seconds(steps * interval);
            Some(QuietWindow {
                previous,
                next: previous + ChronoDuration::seconds(interval),
            })
        }
        _ => None,
    }
}

impl QuietWindow {
    /// `now - previous >= gap && next - now >= margin`.
    #[must_use]
    pub fn is_quiet(self, now: DateTime<Utc>, gap: ChronoDuration, margin: ChronoDuration) -> bool {
        now - self.previous >= gap && self.next - now >= margin
    }
}

/// The next tick that should fire, given the schedule's last run. `None` means
/// not due yet. A new row (`last_run_at` NULL) waits for the next tick rather
/// than firing immediately, matching `Entry.__init__`'s `date_changed` seed.
#[must_use]
pub fn due_tick(
    kind: &str,
    cron: Option<&str>,
    interval_seconds: Option<i64>,
    last_run_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let last = last_run_at?;
    match kind {
        "cron" => {
            let spec = CronSpec::parse(cron?)?;
            let latest = spec.latest_tick(now)?;
            (latest > last).then_some(latest)
        }
        "interval" => {
            let next = last + ChronoDuration::seconds(interval_seconds?);
            (next <= now).then_some(next)
        }
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct BeatConfig {
    pub poll: Duration,
    /// Minimum age of the previous due tick at handover (`G`). Celery must
    /// have had time to fire it; 30 s is six times its 5 s loop.
    pub handover_gap: Duration,
    /// Minimum distance to the next due tick at handover (`M`). Celery must
    /// have time to observe the tracker bump before it; 15 s is three times
    /// its 5 s loop.
    pub handover_margin: Duration,
    /// `METRICS_PUSH_INTERVAL_MINUTES` for the `push-instance-metrics` entry
    /// (`celery.py:27-38`). The seeded default is 360.
    pub metrics_push_interval_minutes: i64,
}

impl Default for BeatConfig {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(1),
            handover_gap: Duration::from_secs(30),
            handover_margin: Duration::from_secs(15),
            metrics_push_interval_minutes: 360,
        }
    }
}

pub struct Beat {
    pool: DbPools,
    config: BeatConfig,
    django_table: bool,
}

impl Beat {
    /// Seed the static schedule and detect whether the Django tracker table
    /// exists. Handover itself happens in the run loop, inside a quiet window.
    pub async fn new(
        pool: DbPools,
        config: BeatConfig,
        test_interval: Option<i64>,
    ) -> Result<Self, QueueError> {
        let client = pool.write_pool().get().await?;
        seed_schedules(
            &**client,
            test_interval,
            config.metrics_push_interval_minutes * 60,
        )
        .await?;
        let django_table: bool = client
            .query_one(
                "SELECT to_regclass('django_celery_beat_periodictask') IS NOT NULL",
                &[],
            )
            .await?
            .try_get(0)?;
        Ok(Self {
            pool,
            config,
            django_table,
        })
    }

    /// Run until `shutdown` resolves. Pending Rust-owned schedules wait for a
    /// quiet window; handed-over schedules fire from `last_run_at`.
    pub async fn run<S>(self, shutdown: S) -> Result<(), QueueError>
    where
        S: Future<Output = ()>,
    {
        tokio::pin!(shutdown);
        loop {
            let client = self.pool.write_pool().get().await?;
            let sql = if self.django_table {
                "SELECT s.name, s.task, s.kind, s.cron, s.interval_seconds, \
                        s.last_run_at, s.handover_at, p.enabled AS django_enabled \
                 FROM plane_jobs.schedules s \
                 LEFT JOIN django_celery_beat_periodictask p ON p.name = s.name \
                 WHERE s.enabled AND s.owner = 'rust'"
            } else {
                "SELECT name, task, kind, cron, interval_seconds, last_run_at, \
                        handover_at, NULL::boolean AS django_enabled \
                 FROM plane_jobs.schedules \
                 WHERE enabled AND owner = 'rust'"
            };
            let rows = client.query(sql, &[]).await?;
            drop(client);
            let now = Utc::now();
            let gap = chrono::Duration::from_std(self.config.handover_gap)
                .unwrap_or_else(|_| chrono::Duration::seconds(30));
            let margin = chrono::Duration::from_std(self.config.handover_margin)
                .unwrap_or_else(|_| chrono::Duration::seconds(15));

            for row in rows {
                let name: String = row.try_get(0)?;
                let task: String = row.try_get(1)?;
                let kind: String = row.try_get(2)?;
                let cron: Option<String> = row.try_get(3)?;
                let interval: Option<i64> = row.try_get(4)?;
                let last: Option<DateTime<Utc>> = row.try_get(5)?;
                let handover_at: Option<DateTime<Utc>> = row.try_get(6)?;
                let django_enabled: Option<bool> = row.try_get(7)?;

                if handover_at.is_none() {
                    let Some(window) = quiet_window(&kind, cron.as_deref(), interval, last, now)
                    else {
                        continue;
                    };
                    // No Django row means no live Celery entry: there is
                    // nothing to wait for. A row (enabled or already disabled)
                    // gets the quiet-window gate.
                    if django_enabled.is_some() && !window.is_quiet(now, gap, margin) {
                        continue;
                    }
                    self.handover(&name, window.previous).await?;
                    continue;
                }

                // An operator re-enabled the row after handover: go back to
                // pending and let the quiet-window path take it over again.
                if self.django_table && django_enabled == Some(true) {
                    self.reset_handover(&name).await?;
                    continue;
                }

                if let Some(tick) = due_tick(&kind, cron.as_deref(), interval, last, now) {
                    self.fire(&name, &task, tick).await?;
                }
            }

            tokio::select! {
                () = &mut shutdown => return Ok(()),
                () = tokio::time::sleep(self.config.poll) => {}
            }
        }
    }

    /// Disable the Django row and record the handover atomically. The caller
    /// has already proved `previous` is old enough (G) and `next` is far
    /// enough away (M). `last_run_at = previous` makes plane-beat start at
    /// `next`, never re-running the previous tick.
    async fn handover(&self, name: &str, previous: DateTime<Utc>) -> Result<(), QueueError> {
        let mut client = self.pool.write_pool().get().await?;
        let tx = client.transaction().await?;
        if self.django_table {
            let enabled: Option<bool> = tx
                .query_opt(
                    "SELECT enabled FROM django_celery_beat_periodictask WHERE name = $1",
                    &[&name],
                )
                .await?
                .map(|row| row.try_get(0))
                .transpose()?;
            if enabled == Some(true) {
                tx.execute(
                    "UPDATE django_celery_beat_periodictask \
                     SET enabled = false WHERE name = $1",
                    &[&name],
                )
                .await?;
                bump_tracker(&*tx).await?;
            }
        }
        tx.execute(
            "UPDATE plane_jobs.schedules \
             SET handover_at = now(), last_run_at = $2, updated_at = now() \
             WHERE name = $1",
            &[&name, &previous],
        )
        .await?;
        tx.commit().await?;
        tracing::info!(
            target: "plane.beat",
            schedule = name,
            %previous,
            "handed schedule over in a quiet window"
        );
        Ok(())
    }

    async fn reset_handover(&self, name: &str) -> Result<(), QueueError> {
        let client = self.pool.write_pool().get().await?;
        client
            .execute(
                "UPDATE plane_jobs.schedules SET handover_at = NULL, updated_at = now() \
                 WHERE name = $1",
                &[&name],
            )
            .await?;
        tracing::warn!(
            target: "plane.beat",
            schedule = name,
            "Django row re-enabled; returning to handover-pending"
        );
        Ok(())
    }

    async fn fire(&self, name: &str, task: &str, tick: DateTime<Utc>) -> Result<bool, QueueError> {
        let mut client = self.pool.write_pool().get().await?;
        let tx = client.transaction().await?;
        if !queue::try_tick_lock(&*tx, name, tick).await? {
            return Ok(false);
        }
        let job = Job::new(task).payload(json!({
            "schedule": name,
            "tick": tick.to_rfc3339(),
        }));
        let Some(job_id) = queue::enqueue(&*tx, &job).await? else {
            return Ok(false);
        };
        if !queue::claim_tick(&*tx, name, tick, job_id).await? {
            return Ok(false);
        }
        tx.execute(
            "UPDATE plane_jobs.schedules \
             SET last_run_at = now(), updated_at = now() WHERE name = $1",
            &[&name],
        )
        .await?;
        tx.commit().await?;
        tracing::info!(
            target: "plane.beat",
            schedule = name,
            task,
            %tick,
            %job_id,
            "fired schedule"
        );
        Ok(true)
    }
}

/// `PeriodicTasks.update_changed()`: the row Django's `post_save` signal
/// writes so a running `DatabaseScheduler` notices the change
/// (`django_celery_beat/models.py:415-422`). The table is
/// `django_celery_beat_periodictasks` (`ident smallint primary key`,
/// `last_update timestamptz`).
async fn bump_tracker<C: GenericClient + Sync>(client: &C) -> Result<(), QueueError> {
    client
        .execute(
            "INSERT INTO django_celery_beat_periodictasks (ident, last_update) \
             VALUES (1, now()) \
             ON CONFLICT (ident) DO UPDATE SET last_update = EXCLUDED.last_update",
            &[],
        )
        .await?;
    Ok(())
}

/// Seed/refresh the static 13 and (when configured) the test-only entry.
/// `last_run_at` is set only on insert, so a restart does not re-fire a tick.
pub async fn seed_schedules<C: GenericClient + Sync>(
    client: &C,
    test_interval: Option<i64>,
    metrics_interval_seconds: i64,
) -> Result<(), QueueError> {
    for seed in REAL_SCHEDULES {
        let kind = if seed.cron.is_some() {
            "cron"
        } else {
            "interval"
        };
        let interval_seconds = if seed.name == METRICS_SCHEDULE {
            Some(metrics_interval_seconds)
        } else {
            seed.interval_seconds
        };
        client
            .execute(
                "INSERT INTO plane_jobs.schedules \
                 (name, task, owner, kind, cron, interval_seconds, enabled, last_run_at) \
                 VALUES ($1, $2, 'celery', $3, $4, $5, true, now()) \
                 ON CONFLICT (name) DO UPDATE SET \
                     task = EXCLUDED.task, kind = EXCLUDED.kind, \
                     cron = EXCLUDED.cron, interval_seconds = EXCLUDED.interval_seconds, \
                     updated_at = now()",
                &[&seed.name, &seed.task, &kind, &seed.cron, &interval_seconds],
            )
            .await?;
    }
    if let Some(seconds) = test_interval {
        client
            .execute(
                "INSERT INTO plane_jobs.schedules \
                 (name, task, owner, kind, interval_seconds, enabled, last_run_at) \
                 VALUES ($1, $2, 'rust', 'interval', $3, true, now()) \
                 ON CONFLICT (name) DO UPDATE SET \
                     task = EXCLUDED.task, kind = 'interval', \
                     interval_seconds = EXCLUDED.interval_seconds, \
                     owner = 'rust', enabled = true, updated_at = now()",
                &[&TEST_SCHEDULE, &TEST_TASK, &seconds],
            )
            .await?;
    }
    // `plans/CUTOVER.md` §2's per-entry handover: flip the entries whose task
    // is already Rust-owned. The quiet-window logic in `Beat::run` is what
    // actually performs the handover; this only makes the row eligible.
    let names: Vec<String> = RUST_SCHEDULES
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    let flipped = client
        .execute(
            "UPDATE plane_jobs.schedules SET owner = 'rust', updated_at = now() \
             WHERE owner = 'celery' AND name = ANY($1)",
            &[&names],
        )
        .await?;
    if flipped > 0 {
        tracing::info!(
            target: "plane.beat",
            schedules = flipped,
            "marked Rust-owned schedules for handover"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, year, month, day, hour, minute, 0)
            .single()
            .expect("valid time")
    }

    #[test]
    fn every_five_minutes_matches_the_real_entries() {
        let cron = CronSpec::parse("*/5 * * * *").expect("cron");
        let now = at(2026, 11, 16, 12, 7);
        assert_eq!(cron.latest_tick(now), Some(at(2026, 11, 16, 12, 5)));
        assert_eq!(
            cron.latest_tick(at(2026, 11, 16, 12, 10)),
            Some(at(2026, 11, 16, 12, 10))
        );
    }

    #[test]
    fn daily_entries_do_not_match_every_minute() {
        let cron = CronSpec::parse("30 1 * * *").expect("cron");
        assert_eq!(
            cron.latest_tick(at(2026, 11, 16, 2, 0)),
            Some(at(2026, 11, 16, 1, 30))
        );
    }

    #[test]
    fn a_new_schedule_waits_for_the_next_tick() {
        let now = at(2026, 11, 16, 12, 7);
        assert_eq!(due_tick("cron", Some("*/5 * * * *"), None, None, now), None);
        assert_eq!(
            due_tick(
                "cron",
                Some("*/5 * * * *"),
                None,
                Some(at(2026, 11, 16, 12, 4)),
                now
            ),
            Some(at(2026, 11, 16, 12, 5))
        );
    }

    #[test]
    fn interval_fires_the_latest_due_tick_once() {
        let now = at(2026, 11, 16, 12, 7);
        assert_eq!(
            due_tick(
                "interval",
                None,
                Some(120),
                Some(at(2026, 11, 16, 12, 4)),
                now
            ),
            Some(at(2026, 11, 16, 12, 6))
        );
        assert_eq!(
            due_tick(
                "interval",
                None,
                Some(120),
                Some(at(2026, 11, 16, 12, 6)),
                now
            ),
            None
        );
    }

    #[test]
    fn unsupported_cron_fields_are_rejected() {
        assert!(CronSpec::parse("0 0 * * 1").is_none());
        assert!(CronSpec::parse("0 0 1 * *").is_none());
        assert!(CronSpec::parse("0 0 * *").is_none());
    }

    fn at_secs(day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 11, day, hour, minute, second)
            .single()
            .expect("valid time")
    }

    #[test]
    fn quiet_window_for_a_five_minute_crontab() {
        let now = at_secs(16, 12, 7, 0);
        let window = quiet_window("cron", Some("*/5 * * * *"), None, None, now).expect("window");
        assert_eq!(window.previous, at_secs(16, 12, 5, 0));
        assert_eq!(window.next, at_secs(16, 12, 10, 0));
        assert!(window.is_quiet(
            now,
            ChronoDuration::seconds(30),
            ChronoDuration::seconds(15)
        ));

        let too_close_to_next = at_secs(16, 12, 4, 50);
        let window = quiet_window("cron", Some("*/5 * * * *"), None, None, too_close_to_next)
            .expect("window");
        assert!(!window.is_quiet(
            too_close_to_next,
            ChronoDuration::seconds(30),
            ChronoDuration::seconds(15)
        ));

        let too_close_to_previous = at_secs(16, 12, 5, 20);
        let window = quiet_window(
            "cron",
            Some("*/5 * * * *"),
            None,
            None,
            too_close_to_previous,
        )
        .expect("window");
        assert!(!window.is_quiet(
            too_close_to_previous,
            ChronoDuration::seconds(30),
            ChronoDuration::seconds(15)
        ));
    }

    #[test]
    fn quiet_window_for_a_daily_crontab_defers_the_imminent_tick() {
        let one_second_before = at_secs(16, 1, 29, 59);
        let window = quiet_window("cron", Some("30 1 * * *"), None, None, one_second_before)
            .expect("window");
        assert_eq!(window.next, at_secs(16, 1, 30, 0));
        assert!(!window.is_quiet(
            one_second_before,
            ChronoDuration::seconds(30),
            ChronoDuration::seconds(15)
        ));

        let after_the_tick = at_secs(16, 1, 30, 31);
        let window =
            quiet_window("cron", Some("30 1 * * *"), None, None, after_the_tick).expect("window");
        assert_eq!(window.previous, at_secs(16, 1, 30, 0));
        assert_eq!(window.next, at_secs(17, 1, 30, 0));
        assert!(window.is_quiet(
            after_the_tick,
            ChronoDuration::seconds(30),
            ChronoDuration::seconds(15)
        ));
    }

    #[test]
    fn every_rust_schedule_names_a_real_entry_and_a_rust_task() {
        use crate::ownership::RUST_TASKS;
        for name in RUST_SCHEDULES {
            let seed = REAL_SCHEDULES
                .iter()
                .find(|seed| seed.name == *name)
                .unwrap_or_else(|| panic!("{name} is not one of the seeded entries"));
            assert!(
                RUST_TASKS.contains(&seed.task),
                "{name} owns task {} but the task is not Rust-owned",
                seed.task
            );
        }
    }

    #[test]
    fn quiet_window_for_a_ten_second_interval() {
        let anchor = at_secs(16, 12, 0, 0);
        let one_second_before = at_secs(16, 12, 0, 9);
        let window = quiet_window("interval", None, Some(10), Some(anchor), one_second_before)
            .expect("window");
        assert_eq!(window.previous, anchor);
        assert_eq!(window.next, at_secs(16, 12, 0, 10));
        assert!(!window.is_quiet(
            one_second_before,
            ChronoDuration::seconds(3),
            ChronoDuration::seconds(3)
        ));

        let after_the_tick = at_secs(16, 12, 0, 13);
        let window =
            quiet_window("interval", None, Some(10), Some(anchor), after_the_tick).expect("window");
        assert_eq!(window.previous, at_secs(16, 12, 0, 10));
        assert_eq!(window.next, at_secs(16, 12, 0, 20));
        assert!(window.is_quiet(
            after_the_tick,
            ChronoDuration::seconds(3),
            ChronoDuration::seconds(3)
        ));
    }
}
