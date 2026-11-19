// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The five `plane/bgtasks/cleanup_task.py` tasks.
//!
//! All five share `process_cleanup_task` (`cleanup_task.py:34-81`): take the
//! primary keys a queryset yields, delete them in batches of `BATCH_SIZE = 500`
//! with `Model.all_objects.filter(id__in=ids).delete()` (a real `DELETE`, no
//! soft delete: `all_objects` is a plain manager), and swallow a failed batch
//! so the remaining rows are still cleaned. A failure outside the batch loop
//! (the queryset itself) propagates and fails the Celery task; the handler
//! returns a fatal error then.
//!
//! The five querysets (`cleanup_task.py:85-158`), with `all_objects` = every
//! row including soft-deleted ones:
//!
//! | task | window |
//! | --- | --- |
//! | `delete_api_logs` | `created_at <= now - API_ACTIVITY_LOG_RETENTION_DAYS` |
//! | `delete_email_notification_logs` | `sent_at <= now - EMAIL_LOG_RETENTION_DAYS` |
//! | `delete_webhook_logs` | `created_at <= now - WEBHOOK_LOG_RETENTION_DAYS`, ascending |
//! | `delete_page_versions` | every row past the newest 20 per `page_id` |
//! | `delete_issue_description_versions` | every row past the newest 20 per `issue_id` |
//!
//! `sent_at` is nullable, so `sent_at <= cutoff` never matches a log that was
//! never sent (SQL `NULL`), exactly as in Django.
//!
//! None of the five models has a reverse relation (`model_graph`: an empty
//! `children` list), so Django's collector has nothing to cascade and the
//! plain `DELETE` is the whole effect.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use uuid::Uuid;

use crate::TaskContext;

/// `cleanup_task.BATCH_SIZE`.
const BATCH_SIZE: usize = 500;

pub const DELETE_API_LOGS: &str = "plane.bgtasks.cleanup_task.delete_api_logs";
pub const DELETE_EMAIL_NOTIFICATION_LOGS: &str =
    "plane.bgtasks.cleanup_task.delete_email_notification_logs";
pub const DELETE_PAGE_VERSIONS: &str = "plane.bgtasks.cleanup_task.delete_page_versions";
pub const DELETE_ISSUE_DESCRIPTION_VERSIONS: &str =
    "plane.bgtasks.cleanup_task.delete_issue_description_versions";
pub const DELETE_WEBHOOK_LOGS: &str = "plane.bgtasks.cleanup_task.delete_webhook_logs";

/// The five task names this module registers.
pub const TASKS: &[&str] = &[
    DELETE_API_LOGS,
    DELETE_EMAIL_NOTIFICATION_LOGS,
    DELETE_PAGE_VERSIONS,
    DELETE_ISSUE_DESCRIPTION_VERSIONS,
    DELETE_WEBHOOK_LOGS,
];

/// Register the five handlers.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    for task in [
        DELETE_API_LOGS,
        DELETE_EMAIL_NOTIFICATION_LOGS,
        DELETE_PAGE_VERSIONS,
        DELETE_ISSUE_DESCRIPTION_VERSIONS,
        DELETE_WEBHOOK_LOGS,
    ] {
        let context = Arc::clone(&context);
        registry.register(task, move |job| {
            let context = Arc::clone(&context);
            handle(context, job, task)
        });
    }
}

async fn handle(
    context: Arc<TaskContext>,
    job: JobContext,
    task: &'static str,
) -> Result<(), HandlerError> {
    let limits = &context.settings.limits;
    let plan = match task {
        DELETE_API_LOGS => Plan::ApiLogs(cutoff(limits.api_activity_log_retention_days)),
        DELETE_EMAIL_NOTIFICATION_LOGS => Plan::EmailLogs(cutoff(limits.email_log_retention_days)),
        DELETE_WEBHOOK_LOGS => Plan::WebhookLogs(cutoff(limits.webhook_log_retention_days)),
        DELETE_PAGE_VERSIONS => Plan::PageVersions,
        DELETE_ISSUE_DESCRIPTION_VERSIONS => Plan::IssueDescriptionVersions,
        _ => return Err(HandlerError::fatal(format!("unknown cleanup task {task}"))),
    };
    let ids = plan
        .select(&job.client)
        .await
        .map_err(|error| HandlerError::retryable(format!("{task}: {error}")))?;
    let total = ids.len();
    let mut deleted = 0u64;
    let mut batches = 0usize;
    for batch in ids.chunks(BATCH_SIZE) {
        batches += 1;
        match delete_rows(&job.client, plan.table(), batch).await {
            Ok(count) => deleted += count,
            // `except Exception: log_exception(e)` — skip the batch.
            Err(error) => {
                tracing::warn!(
                    target: "plane.tasks",
                    task,
                    error = %error,
                    "cleanup batch failed; skipping it"
                );
            }
        }
    }
    tracing::info!(
        target: "plane.tasks",
        task,
        candidates = total,
        deleted,
        batches,
        "cleanup task completed"
    );
    Ok(())
}

fn cutoff(days: i64) -> DateTime<Utc> {
    Utc::now() - Duration::days(days)
}

#[derive(Clone, Copy)]
enum Plan {
    ApiLogs(DateTime<Utc>),
    EmailLogs(DateTime<Utc>),
    WebhookLogs(DateTime<Utc>),
    PageVersions,
    IssueDescriptionVersions,
}

impl Plan {
    fn table(self) -> &'static str {
        match self {
            Self::ApiLogs(_) => "api_activity_logs",
            Self::EmailLogs(_) => "email_notification_logs",
            Self::WebhookLogs(_) => "webhook_logs",
            Self::PageVersions => "page_versions",
            Self::IssueDescriptionVersions => "issue_description_versions",
        }
    }

    /// The ids `values_list("id", flat=True).iterator()` yields.
    async fn select(self, client: &Client) -> Result<Vec<Uuid>, tokio_postgres::Error> {
        let rows = match self {
            Self::ApiLogs(cutoff) => {
                client
                    .query(
                        "SELECT id FROM api_activity_logs WHERE created_at <= $1 \
                         ORDER BY created_at DESC",
                        &[&cutoff],
                    )
                    .await?
            }
            Self::EmailLogs(cutoff) => {
                client
                    .query(
                        "SELECT id FROM email_notification_logs WHERE sent_at <= $1 \
                         ORDER BY created_at DESC",
                        &[&cutoff],
                    )
                    .await?
            }
            Self::WebhookLogs(cutoff) => {
                client
                    .query(
                        "SELECT id FROM webhook_logs WHERE created_at <= $1 \
                         ORDER BY created_at ASC",
                        &[&cutoff],
                    )
                    .await?
            }
            // `Window(RowNumber(), partition_by=[page_id], order_by=created_at DESC)`
            // then `filter(row_num__gt=20)` (`cleanup_task.py:107-125`).
            Self::PageVersions => {
                client
                    .query(
                        "SELECT id FROM ( \
                             SELECT id, row_number() OVER ( \
                                 PARTITION BY page_id ORDER BY created_at DESC) AS row_num \
                             FROM page_versions) ranked \
                         WHERE row_num > 20 ORDER BY id",
                        &[],
                    )
                    .await?
            }
            Self::IssueDescriptionVersions => {
                client
                    .query(
                        "SELECT id FROM ( \
                             SELECT id, row_number() OVER ( \
                                 PARTITION BY issue_id ORDER BY created_at DESC) AS row_num \
                             FROM issue_description_versions) ranked \
                         WHERE row_num > 20 ORDER BY id",
                        &[],
                    )
                    .await?
            }
        };
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }
}

/// `Model.all_objects.filter(id__in=ids).delete()`: a real `DELETE`, one
/// statement per batch.
async fn delete_rows(
    client: &Client,
    table: &str,
    ids: &[Uuid],
) -> Result<u64, tokio_postgres::Error> {
    // `table` is one of the constants above, never request data.
    let sql = format!("DELETE FROM {table} WHERE id = ANY($1)");
    client.execute(&sql, &[&ids]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_graph;

    #[test]
    fn the_five_tables_have_no_cascade_children() {
        for (task, table) in [
            (DELETE_API_LOGS, "api_activity_logs"),
            (DELETE_EMAIL_NOTIFICATION_LOGS, "email_notification_logs"),
            (DELETE_PAGE_VERSIONS, "page_versions"),
            (
                DELETE_ISSUE_DESCRIPTION_VERSIONS,
                "issue_description_versions",
            ),
            (DELETE_WEBHOOK_LOGS, "webhook_logs"),
        ] {
            assert!(TASKS.contains(&task));
            let model = model_graph::models()
                .registry_models()
                .find(|model| model.table == table)
                .unwrap_or_else(|| panic!("no model for {table}"));
            assert!(
                model.children.is_empty(),
                "{table} gained a reverse relation; the cleanup DELETE must cascade like Django"
            );
        }
    }
}
