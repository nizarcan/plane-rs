// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/issue_automation_task.py`.
//!
//! Two tasks:
//!
//! * `archive_and_close_old_issues` (`:26-29`): for every project with
//!   `archive_in > 0`/`close_in > 0`, archive or close the issues that have not
//!   moved for `archive_in * 30`/`close_in * 30` days and that are not held by
//!   an open cycle or module, then publish one `issue_activity` per affected
//!   row (`:71-86`, `:134-149`).
//! * `unsnooze_expired_intake_issues` (`:156-175`): one bulk `UPDATE`.
//!
//! # The querysets, and why the join shape matters
//!
//! `archive_old_issues`/`close_old_issues` filter a reverse relation three
//! times in one `.filter()` call (`issue_cycle`, `issue_module`,
//! `issue_intake`). Django renders those as **LEFT OUTER JOINs without
//! `DISTINCT`**, so an issue with two matching `cycle_issues` (or two matching
//! `intake_issues`) is returned twice: `bulk_update` writes it twice
//! (harmless) and `issue_activity` is **published twice** (`:73-86`). The SQL
//! below keeps the same joins and therefore the same multiplicity; adding
//! `DISTINCT` would be a behaviour change.
//!
//! # Swallowed failures
//!
//! Both functions, and the snooze task, wrap their body in
//! `try/except Exception: log_exception(e); return` (`:88-90`, `:151-153`,
//! `:173-175`), so every handler here returns `Ok(())` after logging.
//!
//! # What the published messages carry
//!
//! `requested_data` and `current_instance` are `json.dumps(...)` **strings**
//! (`:76,80,139`), `actor_id` is `str(project.created_by_id)` — the literal
//! `"None"` when the project has no creator — and `issue_id`/`project_id` are
//! `uuid.UUID` objects, which is why the typed constructor keeps the kombu
//! uuid envelope for them. Both shapes are `IssueActivityShape::Automation`
//! (`subscriber=False`, `epoch`, `notification=True`).

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use deadpool_postgres::Client;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_jobs::queue::Job;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use uuid::Uuid;

use crate::{TaskContext, publish_or_enqueue};

pub const ARCHIVE_AND_CLOSE: &str =
    "plane.bgtasks.issue_automation_task.archive_and_close_old_issues";
pub const UNSNOOZE: &str = "plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues";
pub const ISSUE_ACTIVITY: &str = "plane.bgtasks.issue_activities_task.issue_activity";

/// The two task names this module registers.
pub const TASKS: &[&str] = &[ARCHIVE_AND_CLOSE, UNSNOOZE];

/// `IntakeIssueStatus` values (`plane/db/models/intake.py:40-45`).
const SNOOZED: i32 = 0;
const PENDING: i32 = -2;

/// Register the two handlers.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    let archive = Arc::clone(&context);
    registry.register(ARCHIVE_AND_CLOSE, move |job| {
        handle_archive(Arc::clone(&archive), job)
    });
    registry.register(UNSNOOZE, move |job| {
        handle_unsnooze(Arc::clone(&context), job)
    });
}

/// `archive_and_close_old_issues`: every failure is Django's `except Exception`.
async fn handle_archive(context: Arc<TaskContext>, job: JobContext) -> Result<(), HandlerError> {
    if let Err(error) = archive_old_issues(&context, &job.client).await {
        crate::swallow(format!("archive_old_issues: {error}"), ARCHIVE_AND_CLOSE);
    }
    if let Err(error) = close_old_issues(&context, &job.client).await {
        crate::swallow(format!("close_old_issues: {error}"), ARCHIVE_AND_CLOSE);
    }
    Ok(())
}

struct ProjectSettings {
    id: Uuid,
    interval_days: i64,
    created_by: Option<Uuid>,
    default_state: Option<Uuid>,
}

/// `archive_old_issues` (`issue_automation_task.py:32-90`).
async fn archive_old_issues(context: &TaskContext, client: &Client) -> Result<(), HandlerError> {
    let rows = client
        .query(
            "SELECT id, archive_in, created_by_id FROM projects \
             WHERE archive_in > 0 AND deleted_at IS NULL ORDER BY created_at DESC",
            &[],
        )
        .await
        .map_err(db)?;
    for row in rows {
        let project = ProjectSettings {
            id: row.get(0),
            interval_days: i64::from(row.get::<_, i32>(1)),
            created_by: row.get(2),
            default_state: None,
        };
        let now = Utc::now();
        let cutoff = now - Duration::days(project.interval_days * 30);
        let ids = select_issues(
            client,
            &project.id,
            cutoff,
            now,
            &["completed", "cancelled"],
        )
        .await
        .map_err(db)?;
        if ids.is_empty() {
            continue;
        }
        let archive_at = now.date_naive();
        bulk_update_issues(client, &ids, "archived_at", BulkValue::Date(archive_at))
            .await
            .map_err(db)?;
        for issue_id in &ids {
            publish_activity(
                context,
                client,
                issue_id,
                &project,
                "issue.activity.updated",
                &format!("{{\"archived_at\": \"{archive_at}\", \"automation\": true}}"),
                "{\"archived_at\": null}",
            )
            .await?;
        }
    }
    Ok(())
}

/// `close_old_issues` (`issue_automation_task.py:93-153`).
async fn close_old_issues(context: &TaskContext, client: &Client) -> Result<(), HandlerError> {
    let rows = client
        .query(
            "SELECT id, close_in, created_by_id, default_state_id FROM projects \
             WHERE close_in > 0 AND deleted_at IS NULL ORDER BY created_at DESC",
            &[],
        )
        .await
        .map_err(db)?;
    for row in rows {
        let project = ProjectSettings {
            id: row.get(0),
            interval_days: i64::from(row.get::<_, i32>(1)),
            created_by: row.get(2),
            default_state: row.get(3),
        };
        let now = Utc::now();
        let cutoff = now - Duration::days(project.interval_days * 30);
        let ids = select_issues(
            client,
            &project.id,
            cutoff,
            now,
            &["backlog", "unstarted", "started"],
        )
        .await
        .map_err(db)?;
        if ids.is_empty() {
            continue;
        }
        // `project.default_state` comes from the `select_related`; otherwise
        // the first cancelled state by `State.Meta.ordering = ("sequence",)`
        // (`State.objects` hides triage and soft-deleted rows).
        let close_state = match project.default_state {
            Some(state) => Some(state),
            None => client
                .query_opt(
                    "SELECT id FROM states WHERE \"group\" = 'cancelled' \
                     AND deleted_at IS NULL ORDER BY sequence ASC LIMIT 1",
                    &[],
                )
                .await
                .map_err(db)?
                .map(|row| row.get(0)),
        };
        bulk_update_issues(client, &ids, "state_id", BulkValue::Uuid(close_state))
            .await
            .map_err(db)?;
        let closed_to = close_state.map_or_else(|| "None".to_string(), |id| id.to_string());
        for issue_id in &ids {
            publish_activity(
                context,
                client,
                issue_id,
                &project,
                "issue.activity.updated",
                &format!("{{\"closed_to\": \"{closed_to}\"}}"),
                "null",
            )
            .await?;
        }
    }
    Ok(())
}

/// The issue queryset of `archive_old_issues`/`close_old_issues`, including
/// its duplicate rows. See the module docs for the joins.
async fn select_issues(
    client: &Client,
    project: &Uuid,
    cutoff: DateTime<Utc>,
    now: DateTime<Utc>,
    groups: &[&str],
) -> Result<Vec<Uuid>, tokio_postgres::Error> {
    let group_list = groups
        .iter()
        .map(|group| format!("'{group}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT issues.id FROM issues \
         INNER JOIN states ON issues.state_id = states.id \
         INNER JOIN projects ON issues.project_id = projects.id \
         LEFT OUTER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
         LEFT OUTER JOIN cycles ON cycle_issues.cycle_id = cycles.id \
         LEFT OUTER JOIN module_issues ON issues.id = module_issues.issue_id \
         LEFT OUTER JOIN modules ON module_issues.module_id = modules.id \
         LEFT OUTER JOIN intake_issues ON issues.id = intake_issues.issue_id \
         WHERE issues.deleted_at IS NULL \
           AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
           AND NOT (issues.archived_at IS NOT NULL) \
           AND NOT (projects.archived_at IS NOT NULL) \
           AND NOT issues.is_draft \
           AND issues.archived_at IS NULL \
           AND issues.project_id = $1 \
           AND states.\"group\" IN ({group_list}) \
           AND issues.updated_at <= $2 \
           AND (cycle_issues.id IS NULL \
                OR (cycles.end_date < $3 AND cycle_issues.id IS NOT NULL)) \
           AND (module_issues.id IS NULL \
                OR (modules.target_date < $3 AND module_issues.id IS NOT NULL)) \
           AND (intake_issues.status IN (1, -1, 2) OR intake_issues.id IS NULL) \
         ORDER BY issues.created_at DESC"
    );
    let rows = client.query(&sql, &[project, &cutoff, &now]).await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

enum BulkValue {
    Date(chrono::NaiveDate),
    Uuid(Option<Uuid>),
}

/// `Issue.objects.bulk_update(rows, [column], batch_size=100)`: a column-only
/// `UPDATE` in batches of 100, with the `SoftDeletionManager`'s
/// `deleted_at IS NULL` predicate the manager adds (`plane/db/mixins.py:56-58`).
async fn bulk_update_issues(
    client: &Client,
    ids: &[Uuid],
    column: &str,
    value: BulkValue,
) -> Result<(), tokio_postgres::Error> {
    // `column` is one of the two constants above, never request data.
    let sql = format!("UPDATE issues SET {column} = $1 WHERE id = ANY($2) AND deleted_at IS NULL");
    for batch in ids.chunks(100) {
        match value {
            BulkValue::Date(date) => {
                client.execute(&sql, &[&date, &batch]).await?;
            }
            BulkValue::Uuid(state) => {
                client.execute(&sql, &[&state, &batch]).await?;
            }
        }
    }
    Ok(())
}

/// Publish one `issue_activity.delay(...)`.
#[allow(clippy::too_many_arguments)]
async fn publish_activity(
    context: &TaskContext,
    client: &Client,
    issue_id: &Uuid,
    project: &ProjectSettings,
    activity_type: &str,
    requested_data: &str,
    current_instance: &str,
) -> Result<(), HandlerError> {
    let base = IssueActivityBase::new(
        activity_type,
        plane_jobs::celery::TaskArg::json_text(requested_data),
        project
            .created_by
            .map_or_else(|| "None".to_string(), |id| id.to_string()),
        plane_jobs::celery::TaskArg::uuid(*issue_id),
        plane_jobs::celery::TaskArg::uuid(project.id),
        if current_instance == "null" {
            plane_jobs::celery::TaskArg::Null
        } else {
            plane_jobs::celery::TaskArg::json_text(current_instance)
        },
        Utc::now().timestamp(),
    );
    let call = IssueActivityCall::automation(base);
    let message = tasks::issue_activity(call);
    // The published task is Celery-owned today (`policy::PRODUCERS` has it as
    // `CeleryTask`); `publish_or_enqueue` routes by ownership, so this becomes
    // a Rust->Rust enqueue when set A moves it.
    let job = Job::from_message(&message);
    publish_or_enqueue(context, &***client, ISSUE_ACTIVITY, &message, job).await
}

/// `unsnooze_expired_intake_issues` (`issue_automation_task.py:156-175`).
async fn handle_unsnooze(_context: Arc<TaskContext>, job: JobContext) -> Result<(), HandlerError> {
    // `IntakeIssue.objects.filter(...)` is a `SoftDeletionQuerySet`, so the
    // manager's `deleted_at IS NULL` is part of the UPDATE.
    let result = job
        .client
        .execute(
            "UPDATE intake_issues SET status = $1, snoozed_till = NULL, updated_at = now() \
             WHERE status = $2 AND snoozed_till <= now() AND deleted_at IS NULL",
            &[&PENDING, &SNOOZED],
        )
        .await;
    match result {
        Ok(updated) => {
            if updated > 0 {
                tracing::info!(
                    target: "plane.tasks",
                    updated,
                    "unsnooze_expired_intake_issues: returned intake issues to pending"
                );
            }
            Ok(())
        }
        // `except Exception: log_exception(e); return 0`.
        Err(error) => {
            crate::swallow(error, UNSNOOZE);
            Ok(())
        }
    }
}

fn db(error: tokio_postgres::Error) -> HandlerError {
    HandlerError::retryable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_close_requested_data_matches_json_dumps() {
        let id = Uuid::parse_str("33333333-3333-3333-3333-333333333333").expect("uuid");
        assert_eq!(
            format!("{{\"closed_to\": \"{}\"}}", id),
            "{\"closed_to\": \"33333333-3333-3333-3333-333333333333\"}"
        );
    }

    #[test]
    fn the_snooze_statuses_are_djangos() {
        assert_eq!(PENDING, -2);
        assert_eq!(SNOOZED, 0);
    }
}
