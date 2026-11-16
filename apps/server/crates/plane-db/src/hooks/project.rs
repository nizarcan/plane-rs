// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H08/H09 — `Project.save` (`plane/db/models/project.py:193-227`).
//!
//! ```python
//! self.identifier = self.identifier.strip().upper()
//! if is_creating and not self.is_timezone_provided:
//!     self.timezone = Workspace.objects.get(id=self.workspace_id).timezone
//! ...
//! if old_timezone != self.timezone: with transaction.atomic(): save(); self._retime_intake_snoozes(old_timezone)
//! ```
//!
//! `is_timezone_provided` is `Project.__init__`'s
//! `kwargs.get("timezone") is not None` (`project.py:126-129`): it is true only
//! when the serializer's validated data carried `timezone`, so a create without
//! one copies the workspace's zone, and an update without one leaves the stored
//! zone alone.
//!
//! H09a (`_retime_intake_snoozes`) runs in the same transaction as the timezone
//! update: it locks the project's snoozed `intake_issues` and moves
//! `snoozed_till` to the same calendar day's midnight in the new zone.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

use crate::pool::DbError;

/// H08: the `identifier` and `timezone` a create writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInsert {
    pub identifier: String,
    pub timezone: String,
}

/// H08 (`project.py:196-201`). `timezone` is `Some` when the body supplied one.
#[must_use]
pub fn before_insert(
    identifier: &str,
    timezone: Option<&str>,
    workspace_timezone: &str,
) -> ProjectInsert {
    ProjectInsert {
        identifier: identifier.trim().to_uppercase(),
        timezone: timezone.unwrap_or(workspace_timezone).to_owned(),
    }
}

/// H09 (`project.py:196`, `:209-227`): the identifier is normalised on every
/// save; the timezone branch is the caller's, because it needs the stored value
/// and (for a change) H09a.
#[must_use]
pub fn before_update(identifier: &str) -> String {
    identifier.trim().to_uppercase()
}

/// H09a: `_retime_intake_snoozes` (`project.py:169-191`).
///
/// `project_day_start(date, tz)` is midnight of `date` in `tz`
/// (`plane/utils/timezone_converter.py`). The rows are locked (`FOR UPDATE`) in
/// the caller's transaction, exactly like `select_for_update()`.
///
/// # Errors
///
/// A database failure.
pub async fn retime_intake_snoozes(
    transaction: &deadpool_postgres::Transaction<'_>,
    project_id: Uuid,
    old_timezone: Tz,
    new_timezone: Tz,
) -> Result<(), DbError> {
    retime_intake_snoozes_with(transaction, project_id, old_timezone, new_timezone).await
}

/// H09a for any client — the same body as [`retime_intake_snoozes`], for a
/// caller that opened its transaction with `BEGIN` on the request's own
/// connection (a `&mut deadpool_postgres::Client` is not available there).
///
/// # Errors
///
/// A database failure.
pub async fn retime_intake_snoozes_with<C>(
    client: &C,
    project_id: Uuid,
    old_timezone: Tz,
    new_timezone: Tz,
) -> Result<(), DbError>
where
    C: deadpool_postgres::GenericClient + ?Sized,
{
    let rows = client
        .query(
            "SELECT id, snoozed_till FROM intake_issues \
             WHERE project_id = $1 AND status = 0 AND snoozed_till IS NOT NULL \
             FOR UPDATE",
            &[&project_id],
        )
        .await?;
    let mut updates: Vec<(Uuid, DateTime<Utc>)> = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.get(0);
        let snoozed_till: DateTime<Utc> = row.get(1);
        let local_date: NaiveDate = snoozed_till.with_timezone(&old_timezone).date_naive();
        let naive = local_date.and_hms_opt(0, 0, 0).expect("midnight");
        let Some(start) = new_timezone
            .from_local_datetime(&naive)
            .earliest()
            .map(|value| value.with_timezone(&Utc))
        else {
            continue;
        };
        updates.push((id, start));
    }
    if updates.is_empty() {
        return Ok(());
    }
    let statement = client
        .prepare("UPDATE intake_issues SET snoozed_till = $1 WHERE id = $2")
        .await?;
    for (id, snoozed_till) in updates {
        client.execute(&statement, &[&snoozed_till, &id]).await?;
    }
    Ok(())
}

/// The project's stored timezone, read inside the caller's transaction.
///
/// # Errors
///
/// A database failure or a missing row.
pub async fn stored_timezone(
    transaction: &deadpool_postgres::Transaction<'_>,
    project_id: Uuid,
) -> Result<String, DbError> {
    let row = transaction
        .query_one(
            "SELECT timezone FROM projects WHERE id = $1",
            &[&project_id],
        )
        .await?;
    Ok(row.get(0))
}

/// The parse used by `retime_intake_snoozes`'s callers; an unknown zone is
/// Python's `pytz.UnknownTimeZoneError` (500).
#[must_use]
pub fn parse_timezone(name: &str) -> Option<Tz> {
    name.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h08_normalises_the_identifier_and_copies_the_workspace_timezone() {
        let prepared = before_insert(" alp ", None, "Asia/Kolkata");
        assert_eq!(prepared.identifier, "ALP");
        assert_eq!(prepared.timezone, "Asia/Kolkata");
        let prepared = before_insert("bet", Some("Europe/Berlin"), "Asia/Kolkata");
        assert_eq!(prepared.identifier, "BET");
        assert_eq!(prepared.timezone, "Europe/Berlin");
    }

    #[test]
    fn h09_normalises_the_identifier_on_update() {
        assert_eq!(before_update(" gam "), "GAM");
        assert_eq!(before_update("θ"), "Θ");
    }
}
