// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H10/H10a/H10b/H11/H12 — `Issue.save` (`plane/db/models/issue.py:180-255`).
//!
//! ```python
//! def save(self, *args, **kwargs):
//!     self._ensure_default_state()          # H10a
//!     kwargs = self._sync_completed_at(kwargs)  # H10b
//!     if self._state.adding:
//!         with transaction.atomic():
//!             lock_key = convert_uuid_to_integer(self.project.id)
//!             cursor.execute("SELECT pg_advisory_xact_lock(%s)", [lock_key])
//!             last_sequence = IssueSequence.objects.filter(project=...).aggregate(Max("sequence"))["largest"]
//!             self.sequence_id = last_sequence + 1 if last_sequence else 1
//!             self.description_stripped = strip_tags(self.description_html) ...
//!             largest_sort_order = Issue.objects.filter(project=..., state=...).aggregate(Max("sort_order"))["largest"]
//!             if largest_sort_order is not None: self.sort_order = largest_sort_order + 10000
//!             super().save()
//!             IssueSequence.objects.create(issue=self, sequence=self.sequence_id, project=self.project)
//!     else:
//!         self.description_stripped = strip_tags(...)
//!         super().save()
//! ```
//!
//! ## Statement order (create)
//!
//! 1. `SELECT pg_advisory_xact_lock(<key>)` — the key is
//!    `int.from_bytes(sha256(str(project_id).encode()).digest()[:8], "big",
//!    signed=True)` (`plane/utils/uuid.py:19-26`), byte-equal to Django's so
//!    the two implementations interlock during coexistence.
//! 2. `State` lookup for H10a (only when `state_id` is NULL) and H10b's group.
//! 3. `MAX(issue_sequences.sequence)` (soft-delete filtered) → `+1`.
//! 4. `MAX(issues.sort_order)` for `project+state` over `Issue.objects`
//!    (`deleted_at IS NULL` only; archived/triage/draft count) → `+10000`,
//!    otherwise the supplied/default 65535 stays.
//! 5. `projects.workspace_id` (H07/`ProjectBaseModel.save`).
//! 6. `INSERT issues`.
//! 7. `INSERT issue_sequences` (H11; one row per issue, audit columns from the
//!    acting user).
//!
//! Everything above runs in **one transaction on the caller's connection**;
//! `create_issue_row` opens it with `BEGIN` because the request only exposes a
//! `&Client` (the pooled object cannot hand out `&mut`). A failure rolls the
//! whole block back, so a failed create never burns a sequence.
//!
//! `description_stripped` is computed by the caller (H12's `strip_tags` lives
//! in `hooks::issue_strip`, owned by the HTML piece): the value is bound as-is.
//! On update H12 always runs and H10b runs only when `state_id` changed.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::models::JsonText;
use crate::models::issue::{Issue, IssueSequence};
use crate::pool::DbError;

/// Why a create could not be prepared.
#[derive(Debug, thiserror::Error)]
pub enum IssueHookError {
    /// `self.project` raised `Project.DoesNotExist` (`project.py:237-239`).
    #[error("the project does not exist")]
    ProjectNotFound,
    /// A database failure.
    #[error(transparent)]
    Db(#[from] DbError),
    /// A raw statement failure inside the transaction.
    #[error(transparent)]
    Postgres(#[from] tokio_postgres::Error),
}

// ---------------------------------------------------------------------------
// H10's advisory lock key
// ---------------------------------------------------------------------------

/// `convert_uuid_to_integer(project_id)` (`plane/utils/uuid.py:19-26`):
/// the first eight SHA-256 bytes as a **signed big-endian** `int64`.
#[must_use]
pub fn advisory_lock_key(project_id: Uuid) -> i64 {
    let digest = Sha256::digest(project_id.to_string().as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

// ---------------------------------------------------------------------------
// H10a / H10b
// ---------------------------------------------------------------------------

/// H10a: the state a new issue gets when the body supplied none.
///
/// `State.objects.filter(~Q(is_triage=True), project=…, default=True).first()`
/// then `State.objects.filter(~Q(is_triage=True), project=…).first()`
/// (`issue.py:228-238`). `State.objects` is `StateManager`, so triage-group and
/// soft-deleted rows are excluded; the ordering is `Meta.ordering=("sequence",)`.
///
/// # Errors
///
/// A database failure.
pub async fn ensure_default_state(
    client: &Client,
    project_id: Uuid,
) -> Result<Option<Uuid>, DbError> {
    let row = client
        .query_opt(
            "SELECT id FROM states \
             WHERE deleted_at IS NULL AND NOT (\"group\" = 'triage') \
               AND NOT is_triage AND project_id = $1 AND \"default\" \
             ORDER BY sequence ASC LIMIT 1",
            &[&project_id],
        )
        .await?;
    if let Some(row) = row {
        return Ok(Some(row.get(0)));
    }
    let row = client
        .query_opt(
            "SELECT id FROM states \
             WHERE deleted_at IS NULL AND NOT (\"group\" = 'triage') \
               AND NOT is_triage AND project_id = $1 \
             ORDER BY sequence ASC LIMIT 1",
            &[&project_id],
        )
        .await?;
    Ok(row.map(|row| row.get(0)))
}

/// The `group` of a state the way an FK access reads it: the base manager, so
/// a soft-deleted or triage state still resolves (`issue.py:242-252`).
///
/// # Errors
///
/// A database failure.
pub async fn state_group(client: &Client, state_id: Uuid) -> Result<Option<String>, DbError> {
    let row = client
        .query_opt("SELECT \"group\" FROM states WHERE id = $1", &[&state_id])
        .await?;
    Ok(row.map(|row| row.get(0)))
}

/// H10b's value: `now` for the completed group, `NULL` otherwise.
#[must_use]
pub fn sync_completed_at(group: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if group == "completed" {
        Some(now)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// H10 / H11 — the create pipeline
// ---------------------------------------------------------------------------

/// `Issue.save()` on create: H10a, H10b, the advisory lock, H10's sequence and
/// sort order, H07's workspace, the `issues` INSERT and H11's
/// `issue_sequences` INSERT, in one transaction.
///
/// The caller fills the model columns the serializer validated (`id`,
/// `project_id`, `name`, description columns, dates, FKs, `is_draft`, …,
/// `description_stripped`, and `sort_order` when the body supplied one);
/// this function fills the rest and returns them in `issue`.
///
/// # Errors
///
/// A database failure. On error the transaction is rolled back and no row or
/// sequence is left behind.
pub async fn create_issue_row(
    client: &Client,
    issue: &mut Issue,
    user: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), IssueHookError> {
    client.batch_execute("BEGIN").await?;
    match create_issue_row_inner(client, issue, user, now).await {
        Ok(()) => {
            client.batch_execute("COMMIT").await?;
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(error)
        }
    }
}

async fn create_issue_row_inner(
    client: &Client,
    issue: &mut Issue,
    user: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), IssueHookError> {
    // 1. The project advisory lock, before any read that the sequence depends
    //    on (`issue.py:185-190`).
    client
        .execute(
            "SELECT pg_advisory_xact_lock($1)",
            &[&advisory_lock_key(issue.project_id)],
        )
        .await?;

    // 2. H10a: a missing state is resolved before H10b and the sort-order
    //    aggregate (which filters on state).
    if issue.state_id.is_none() {
        issue.state_id = ensure_default_state(client, issue.project_id).await?;
    }
    // H10b: `completed_at` from the state group (create always runs it).
    issue.completed_at = match issue.state_id {
        Some(state_id) => match state_group(client, state_id).await? {
            Some(group) => sync_completed_at(&group, now),
            None => None,
        },
        None => None,
    };

    // 3. `MAX(IssueSequence.sequence) + 1`; a caller-supplied sequence_id is
    //    always overwritten (`issue.py:191-194`).
    let row = client
        .query_one(
            "SELECT MAX(sequence) FROM issue_sequences \
             WHERE project_id = $1 AND deleted_at IS NULL",
            &[&issue.project_id],
        )
        .await?;
    let last_sequence: Option<i64> = row.get(0);
    issue.sequence_id = last_sequence.map_or(1, |sequence| (sequence + 1) as i32);

    // 4. `MAX(Issue.sort_order)` over `Issue.objects` (soft-delete only) for
    //    project+state; otherwise the supplied/default value stays
    //    (`issue.py:198-203`).
    let row = client
        .query_one(
            "SELECT MAX(sort_order) FROM issues \
             WHERE project_id = $1 AND state_id = $2 AND deleted_at IS NULL",
            &[&issue.project_id, &issue.state_id],
        )
        .await?;
    let largest_sort_order: Option<f64> = row.get(0);
    if let Some(largest) = largest_sort_order {
        issue.sort_order = largest + 10_000.0;
    }

    // 5. H07: `workspace = project.workspace` (`project.py:237-239`).
    let row = client
        .query_opt(
            "SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL",
            &[&issue.project_id],
        )
        .await?;
    let Some(row) = row else {
        // `self.project` raises `Project.DoesNotExist`.
        return Err(IssueHookError::ProjectNotFound);
    };
    issue.workspace_id = row.get(0);

    // 6. H01 + auto_now/auto_now_add.
    issue.created_at = now;
    issue.updated_at = now;
    issue.created_by_id = user;
    issue.updated_by_id = None;
    issue.deleted_at = None;

    insert_issue(client, issue).await?;

    // 7. H11: one `issue_sequences` row, same transaction.
    insert_issue_sequence(client, issue, user, now).await?;
    Ok(())
}

async fn insert_issue(client: &Client, issue: &Issue) -> Result<(), DbError> {
    client
        .execute(
            "INSERT INTO issues (created_at, updated_at, created_by_id, updated_by_id, \
             deleted_at, id, project_id, workspace_id, parent_id, state_id, point, \
             estimate_point_id, name, description_json, description_html, \
             description_stripped, description_binary, priority, start_date, target_date, \
             sequence_id, sort_order, completed_at, archived_at, is_draft, external_source, \
             external_id, type_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
             $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)",
            &[
                &issue.created_at,
                &issue.updated_at,
                &issue.created_by_id,
                &issue.updated_by_id,
                &issue.deleted_at,
                &issue.id,
                &issue.project_id,
                &issue.workspace_id,
                &issue.parent_id,
                &issue.state_id,
                &issue.point,
                &issue.estimate_point_id,
                &issue.name,
                &issue.description_json,
                &issue.description_html,
                &issue.description_stripped,
                &issue.description_binary,
                &issue.priority,
                &issue.start_date,
                &issue.target_date,
                &issue.sequence_id,
                &issue.sort_order,
                &issue.completed_at,
                &issue.archived_at,
                &issue.is_draft,
                &issue.external_source,
                &issue.external_id,
                &issue.type_id,
            ],
        )
        .await?;
    Ok(())
}

async fn insert_issue_sequence(
    client: &Client,
    issue: &Issue,
    user: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    let sequence = IssueSequence {
        created_at: now,
        updated_at: now,
        created_by_id: user,
        updated_by_id: None,
        deleted_at: None,
        id: Uuid::new_v4(),
        project_id: issue.project_id,
        workspace_id: issue.workspace_id,
        issue_id: Some(issue.id),
        sequence: i64::from(issue.sequence_id),
        deleted: false,
    };
    client
        .execute(
            "INSERT INTO issue_sequences (created_at, updated_at, created_by_id, \
             updated_by_id, deleted_at, id, project_id, workspace_id, issue_id, sequence, \
             deleted) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            &[
                &sequence.created_at,
                &sequence.updated_at,
                &sequence.created_by_id,
                &sequence.updated_by_id,
                &sequence.deleted_at,
                &sequence.id,
                &sequence.project_id,
                &sequence.workspace_id,
                &sequence.issue_id,
                &sequence.sequence,
                &sequence.deleted,
            ],
        )
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// H10b / H12 — the update path
// ---------------------------------------------------------------------------

/// `Issue.save()` on update (`issue.py:214-222`): H10b only when `state_id`
/// changed, then H12 and the full row save. No sequence or sort-order work.
///
/// # Errors
///
/// A database failure.
pub async fn update_issue_row(
    client: &Client,
    issue: &mut Issue,
    state_changed: bool,
    user: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), DbError> {
    // H10a runs on every save, update included (`issue.py:214`):
    // `_ensure_default_state()` replaces a NULL state before H10b.
    if issue.state_id.is_none() {
        issue.state_id = ensure_default_state(client, issue.project_id).await?;
    }
    // H10b returns early when `self.state` is falsy (`issue.py:240-246`), so a
    // PATCH that sets `state_id` to NULL leaves `completed_at` untouched.
    if state_changed && let Some(state_id) = issue.state_id {
        issue.completed_at = match state_group(client, state_id).await? {
            Some(group) => sync_completed_at(&group, now),
            None => None,
        };
    }
    issue.updated_at = now;
    issue.updated_by_id = user;
    client
        .execute(
            "UPDATE issues SET created_at = $1, updated_at = $2, created_by_id = $3, \
             updated_by_id = $4, deleted_at = $5, project_id = $6, workspace_id = $7, \
             parent_id = $8, state_id = $9, point = $10, estimate_point_id = $11, \
             name = $12, description_json = $13, description_html = $14, \
             description_stripped = $15, description_binary = $16, priority = $17, \
             start_date = $18, target_date = $19, sequence_id = $20, sort_order = $21, \
             completed_at = $22, archived_at = $23, is_draft = $24, external_source = $25, \
             external_id = $26, type_id = $27 \
             WHERE id = $28",
            &[
                &issue.created_at,
                &issue.updated_at,
                &issue.created_by_id,
                &issue.updated_by_id,
                &issue.deleted_at,
                &issue.project_id,
                &issue.workspace_id,
                &issue.parent_id,
                &issue.state_id,
                &issue.point,
                &issue.estimate_point_id,
                &issue.name,
                &issue.description_json,
                &issue.description_html,
                &issue.description_stripped,
                &issue.description_binary,
                &issue.priority,
                &issue.start_date,
                &issue.target_date,
                &issue.sequence_id,
                &issue.sort_order,
                &issue.completed_at,
                &issue.archived_at,
                &issue.is_draft,
                &issue.external_source,
                &issue.external_id,
                &issue.type_id,
                &issue.id,
            ],
        )
        .await?;
    Ok(())
}

/// A row for the serializer's default JSON columns (`description_json`).
#[must_use]
pub fn empty_json() -> JsonText {
    JsonText::new("{}")
}

/// The model default for `description_html` (`issue.py:104`).
pub const DEFAULT_DESCRIPTION_HTML: &str = "<p></p>";

/// The allowed priority values (`PRIORITY_CHOICES`, `issue.py`).
pub const PRIORITIES: [&str; 5] = ["urgent", "high", "medium", "low", "none"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_lock_key_matches_python() {
        // `int.from_bytes(hashlib.sha256(str(uuid).encode()).digest()[:8],
        // "big", signed=True)` for the same ids.
        for (uuid, expected) in [
            ("00000000-0000-4000-8000-000000000001", 1289600646178507792),
            ("11111111-2222-3333-4444-555555555555", 7381389673175399431),
            ("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", -7280555155463651731),
            ("d44ae570-3de4-4413-af08-f686bf5b7235", -8360935549403877905),
            ("ffffffff-ffff-4fff-8fff-ffffffffffff", 3290751847814871087),
        ] {
            let id = Uuid::parse_str(uuid).expect("uuid");
            assert_eq!(advisory_lock_key(id), expected, "{uuid}");
        }
    }

    #[test]
    fn completed_at_uses_the_completed_group_only() {
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("timestamp");
        assert_eq!(sync_completed_at("completed", now), Some(now));
        assert_eq!(sync_completed_at("started", now), None);
        assert_eq!(sync_completed_at("cancelled", now), None);
    }
}
