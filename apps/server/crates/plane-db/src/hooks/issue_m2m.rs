// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `IssueAssignee` / `IssueLabel` through-row writes
//! (`plane/app/serializers/issue.py:209-333`).
//!
//! Both serializers write the rows themselves, not through the ORM hook:
//!
//! * **create** (`:226`, `:267`): `bulk_create([...], batch_size=10)` with a
//!   plain INSERT (no `ignore_conflicts`), audit ids copied from the saved
//!   issue, wrapped in `try/except IntegrityError: pass`;
//! * **update** (`:299`, `:320`): `IssueAssignee.objects.filter(issue=…)
//!   .delete()` first — a soft `UPDATE … SET deleted_at=now` with no audit
//!   columns, in autocommit — then `bulk_create(..., batch_size=10,
//!   ignore_conflicts=True)`, i.e. `ON CONFLICT DO NOTHING`;
//! * **`bulk_create` is one atomic over every batch.** Django wraps the whole
//!   `_batched_insert` loop in `with transaction.atomic(using=self.db,
//!   savepoint=False):` (`django/db/models/query.py`, `bulk_create`), so in
//!   autocommit mode it is an outermost `BEGIN … COMMIT/ROLLBACK`. A unique
//!   violation in **any** batch of a create call rolls back **all** batches of
//!   that call; the `except IntegrityError: pass` then swallows the exception,
//!   leaving **zero** rows from that call. An error other than `23505`
//!   propagates after the rollback. The update call is also one atomic, so its
//!   batches are all-or-nothing (`ignore_conflicts` gives it no unique
//!   violation to hit);
//! * `savepoint=False` only matters when the call is already inside an outer
//!   atomic: Django then creates **no savepoint**, so an `IntegrityError` marks
//!   the outer block for rollback and the caller must roll it back. No current
//!   caller nests this way (a future draft-to-issue path might); these helpers
//!   always run as the outermost transaction.
//! * the audit ids are the **issue's** ids read before `super().update()`
//!   (`:292-296`), so on a PATCH they are the pre-update values (often
//!   `updated_by_id = NULL` on a just-created issue);
//! * `IssueAssignee` has a partial unique index on `(issue, assignee)` where
//!   `deleted_at IS NULL`; `IssueLabel` has no constraint, so its
//!   `ignore_conflicts` only suppresses nothing (port as-is).
//!
//! Django's `bulk_create` calls each field's `pre_save`, so `created_at` and
//! `updated_at` are both the publish instant; the caller passes one `now`.
//!
//! ## Default assignee (`issue.py:236-266`)
//!
//! When `assignee_ids` is absent/empty the serializer falls back to the
//! project's `default_assignee_id` **if** it is an active project member with
//! `role >= 15`; otherwise no row is written. [`default_assignee`] is that
//! decision.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use tokio_postgres::error::SqlState;
use uuid::Uuid;

use crate::models::issue::Issue;
use crate::pool::DbError;

/// `bulk_create(..., batch_size=10)` (`issue.py:226`, `:267`).
pub const BATCH_SIZE: usize = 10;

/// Which write shape a bulk insert uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum M2mMode {
    /// `create`: plain INSERT, a unique violation drops that batch.
    Create,
    /// `update`: `ON CONFLICT DO NOTHING`.
    Update,
}

/// `IssueAssignee.objects.filter(issue=…).delete()` (`plane/serializers/issue.py:292`)
/// then `bulk_create(..., ignore_conflicts=True)` (`:299`).
///
/// # Errors
///
/// A database failure (unique violations are swallowed, like the Python
/// `except IntegrityError: pass`).
pub async fn replace_assignees(
    client: &Client,
    issue: &Issue,
    assignee_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    soft_delete_rows(client, "issue_assignees", issue.id, now).await?;
    insert_rows(
        client,
        "issue_assignees",
        "assignee_id",
        issue,
        assignee_ids,
        now,
        M2mMode::Update,
    )
    .await
}

/// `IssueAssignee.objects.bulk_create(..., batch_size=10)` in the create
/// serializer. No delete first.
///
/// # Errors
///
/// A database failure (a unique violation silently drops the batch it occurs
/// in, exactly like `except IntegrityError: pass`).
pub async fn insert_assignees(
    client: &Client,
    issue: &Issue,
    assignee_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    insert_rows(
        client,
        "issue_assignees",
        "assignee_id",
        issue,
        assignee_ids,
        now,
        M2mMode::Create,
    )
    .await
}

/// The `IssueLabel` twin of [`replace_assignees`].
///
/// # Errors
///
/// A database failure.
pub async fn replace_labels(
    client: &Client,
    issue: &Issue,
    label_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    soft_delete_rows(client, "issue_labels", issue.id, now).await?;
    insert_rows(
        client,
        "issue_labels",
        "label_id",
        issue,
        label_ids,
        now,
        M2mMode::Update,
    )
    .await
}

/// The `IssueLabel` twin of [`insert_assignees`].
///
/// # Errors
///
/// A database failure.
pub async fn insert_labels(
    client: &Client,
    issue: &Issue,
    label_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    insert_rows(
        client,
        "issue_labels",
        "label_id",
        issue,
        label_ids,
        now,
        M2mMode::Create,
    )
    .await
}

/// The create serializer's default-assignee fallback (`issue.py:236-266`):
/// `project.default_assignee_id` if it is an active project member with
/// `role >= 15`, else `None`.
///
/// `ProjectMember.objects.filter(member_id=…, project_id=…, role__gte=15,
/// is_active=True)` uses the soft-deletion manager.
///
/// # Errors
///
/// A database failure.
pub async fn default_assignee(
    client: &Client,
    project_id: Uuid,
    candidate: Option<Uuid>,
) -> Result<Option<Uuid>, DbError> {
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let row = client
        .query_opt(
            "SELECT 1 FROM project_members \
             WHERE project_id = $1 AND member_id = $2 AND role >= 15 \
               AND is_active AND deleted_at IS NULL \
             LIMIT 1",
            &[&project_id, &candidate],
        )
        .await?;
    Ok(row.map(|_| candidate))
}

/// `Model.objects.filter(issue=…).delete()` for a through table: a bare
/// `deleted_at` update (`plane/db/mixins.py:49-53`).
async fn soft_delete_rows(
    client: &Client,
    table: &str,
    issue_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    let sql =
        format!("UPDATE {table} SET deleted_at = $1 WHERE issue_id = $2 AND deleted_at IS NULL");
    client.execute(&sql, &[&now, &issue_id]).await?;
    Ok(())
}

/// Every batch of one `bulk_create` call, inside one `BEGIN … COMMIT`.
///
/// Django's `bulk_create` raises `IntegrityError` out of the whole atomic, so a
/// create-mode call that hits a unique violation must roll back every batch;
/// the serializer's `except IntegrityError: pass` then leaves no rows.
async fn insert_rows(
    client: &Client,
    table: &str,
    member_column: &str,
    issue: &Issue,
    member_ids: &[Uuid],
    now: DateTime<Utc>,
    mode: M2mMode,
) -> Result<(), DbError> {
    // `bulk_create([])` returns before the atomic (`query.py`, `if not objs`).
    if member_ids.is_empty() {
        return Ok(());
    }
    client.batch_execute("BEGIN").await?;
    let outcome: Result<(), tokio_postgres::Error> = async {
        for batch in member_ids.chunks(BATCH_SIZE) {
            let mut sql = format!(
                "INSERT INTO {table} (created_at, updated_at, created_by_id, updated_by_id, \
                 id, project_id, workspace_id, {member_column}, issue_id) VALUES "
            );
            let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> =
                Vec::with_capacity(batch.len() * 9);
            for (index, member_id) in batch.iter().enumerate() {
                if index != 0 {
                    sql.push_str(", ");
                }
                let base = index * 9;
                sql.push_str(&format!(
                    "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                    base + 1,
                    base + 2,
                    base + 3,
                    base + 4,
                    base + 5,
                    base + 6,
                    base + 7,
                    base + 8,
                    base + 9
                ));
                params.push(Box::new(now));
                params.push(Box::new(now));
                params.push(Box::new(issue.created_by_id));
                params.push(Box::new(issue.updated_by_id));
                params.push(Box::new(Uuid::new_v4()));
                params.push(Box::new(issue.project_id));
                params.push(Box::new(issue.workspace_id));
                params.push(Box::new(*member_id));
                params.push(Box::new(issue.id));
            }
            if mode == M2mMode::Update {
                sql.push_str(" ON CONFLICT DO NOTHING");
            }
            let statement = client.prepare(&sql).await?;
            let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                params.iter().map(|value| value.as_ref()).collect();
            client.execute(&statement, &refs).await?;
        }
        Ok(())
    }
    .await;

    match outcome {
        Ok(()) => {
            client.batch_execute("COMMIT").await?;
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            // `except IntegrityError: pass` (`issue.py:235`, `:264`, `:305`,
            // `:326`): the whole call is gone, and the exception is swallowed.
            if mode == M2mMode::Create && error.code() == Some(&SqlState::UNIQUE_VIOLATION) {
                Ok(())
            } else {
                Err(error.into())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The generic through-row insert (cycle issues, module issues, …)
// ---------------------------------------------------------------------------

/// One `ProjectBaseModel` through row: the nine columns every membership table
/// shares, with the second endpoint's column named by the caller.
#[derive(Clone, Copy, Debug)]
pub struct MembershipRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub issue_id: Uuid,
    /// `assignee_id`, `label_id`, `cycle_id`, `module_id`: the column the
    /// caller names.
    pub other_id: Uuid,
}

/// `bulk_create([...], batch_size=10)` for a through table, **without**
/// `ignore_conflicts` and **without** a catch: one outermost
/// `BEGIN`/`COMMIT`, every batch inside it, and a unique violation rolls the
/// whole call back and propagates.
///
/// `CycleIssueViewSet.create` (`plane/app/views/cycle/issue.py:272-284`) is
/// the first caller; `module/issue.py` and `draft.py` have the same shape.
///
/// # Errors
///
/// A database failure, including the unique violation `bulk_create` raises
/// when the caller did not pass `ignore_conflicts`.
pub async fn insert_membership(
    client: &Client,
    table: &str,
    other_column: &str,
    rows: &[MembershipRow],
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    // `bulk_create([])` returns before the atomic (`query.py`, `if not objs`).
    if rows.is_empty() {
        return Ok(());
    }
    client.batch_execute("BEGIN").await?;
    let outcome: Result<(), tokio_postgres::Error> = async {
        for (batch_index, batch) in rows.chunks(BATCH_SIZE).enumerate() {
            let mut sql = format!(
                "INSERT INTO {table} (created_at, updated_at, created_by_id, updated_by_id, \
                 id, project_id, workspace_id, issue_id, {other_column}) VALUES "
            );
            let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> =
                Vec::with_capacity(batch.len() * 9);
            for (index, row) in batch.iter().enumerate() {
                // Django's `pre_save` calls `timezone.now()` per object, so
                // `created_at` — and the `-created_at` ordering the handlers
                // read back — is strictly increasing in insertion order. One
                // shared timestamp would make every row a tie and let
                // Postgres choose.
                if index != 0 {
                    sql.push_str(", ");
                }
                let base = index * 9;
                sql.push_str(&format!(
                    "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                    base + 1,
                    base + 2,
                    base + 3,
                    base + 4,
                    base + 5,
                    base + 6,
                    base + 7,
                    base + 8,
                    base + 9
                ));
                let created_at =
                    now + chrono::Duration::microseconds((batch_index * BATCH_SIZE + index) as i64);
                params.push(Box::new(created_at));
                params.push(Box::new(created_at));
                params.push(Box::new(row.created_by_id));
                params.push(Box::new(row.updated_by_id));
                params.push(Box::new(row.id));
                params.push(Box::new(row.project_id));
                params.push(Box::new(row.workspace_id));
                params.push(Box::new(row.issue_id));
                params.push(Box::new(row.other_id));
            }
            let statement = client.prepare(&sql).await?;
            let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
                .iter()
                .map(|value| value.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
                .collect();
            client.execute(&statement, &refs).await?;
        }
        Ok(())
    }
    .await;

    match outcome {
        Ok(()) => {
            client.batch_execute("COMMIT").await?;
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(error.into())
        }
    }
}
