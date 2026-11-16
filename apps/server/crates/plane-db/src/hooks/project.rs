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
//! `snoozed_till` to the same calendar day's midnight in the new zone. The
//! midnight is `project_day_start`'s, i.e. pytz `localize(is_dst=False)`
//! ([`intake_issue::project_day_start`]), never chrono's `earliest()`.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

use crate::hooks::intake_issue;
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

/// H09a per row (`project.py:189`):
///
/// ```python
/// project_day_start(intake_issue.snoozed_till.astimezone(old_tz).date(), new_tz)
/// ```
///
/// The old day is Python's `datetime.astimezone`: the instant read with the old
/// zone's offset *at that instant*, which is [`DateTime::with_timezone`], not a
/// wall-clock re-localisation. Only the new midnight has pytz semantics; see
/// [`intake_issue::project_day_start`].
#[must_use]
pub fn retimed_snooze(
    snoozed_till: DateTime<Utc>,
    old_timezone: Tz,
    new_timezone: Tz,
) -> DateTime<Utc> {
    let day = snoozed_till.with_timezone(&old_timezone).date_naive();
    intake_issue::project_day_start(day, new_timezone)
}

/// H09a: `_retime_intake_snoozes` (`project.py:169-191`).
///
/// One row becomes [`retimed_snooze`]: the calendar day the stored instant has
/// in the **old** zone, at midnight in the **new** zone. The midnight uses
/// `project_day_start` (`plane/utils/timezone_converter.py:98-100`), i.e. pytz's
/// `localize(..., is_dst=False)`: an ambiguous midnight takes standard time and a
/// nonexistent one still resolves to an instant — Django never skips a row. The
/// rows are locked (`FOR UPDATE`) in the caller's transaction, exactly like
/// `select_for_update()`.
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
        updates.push((id, retimed_snooze(snoozed_till, old_timezone, new_timezone)));
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

    fn at(value: &str) -> DateTime<Utc> {
        value.parse::<DateTime<Utc>>().expect("RFC 3339")
    }

    fn zone(name: &str) -> Tz {
        name.parse().expect("known zone")
    }

    /// The expected values are the **real** retime expression
    /// `project_day_start(st.astimezone(old_tz).date(), new_tz)` in UTC, run with
    /// CPython 3.12.13 and pytz 2024.1 from `apps/api/.venv`: both functions
    /// (`project_day_start`, and `pytz.timezone(...).localize`) live in
    /// `plane/utils/timezone_converter.py`, which imports `plane.db.models`, so
    /// the function body was extracted with `ast` and executed with the module's
    /// own `datetime`/`time` names. Each case is
    /// `(snoozed_till, old zone, new zone, expected)`.
    const RETIME_GOLDEN: &[(&str, &str, &str, &str)] = &[
        // Ambiguous midnight (Havana falls back 01:00 -> 00:00 on 2026-11-01):
        // pytz's is_dst=False picks standard time, 05:00Z, where `.earliest()`
        // would have picked 04:00Z.
        (
            "2026-11-01T15:00:00Z",
            "UTC",
            "America/Havana",
            "2026-11-01T05:00:00Z",
        ),
        // Nonexistent midnight (Sao Paulo's DST began at 00:00 on 2018-11-04):
        // pytz still returns an instant, 03:00Z with the pre-transition offset;
        // the old code skipped the row and left it at the stored value.
        (
            "2018-11-04T12:00:00Z",
            "UTC",
            "America/Sao_Paulo",
            "2018-11-04T03:00:00Z",
        ),
        // The ordinary case the scenario covers: ALP's snoozed issue in UTC
        // moved to Europe/Berlin.
        (
            "2026-11-23T00:00:00Z",
            "UTC",
            "Europe/Berlin",
            "2026-11-22T23:00:00Z",
        ),
        // The same row back to UTC: the old day is Berlin's (the 23rd), because
        // `astimezone` reads the instant with that zone's offset.
        (
            "2026-11-22T23:00:00Z",
            "Europe/Berlin",
            "UTC",
            "2026-11-23T00:00:00Z",
        ),
        // The old day is the old zone's, not UTC's: 00:00Z is still the 22nd in
        // Los Angeles.
        (
            "2026-11-23T00:00:00Z",
            "America/Los_Angeles",
            "UTC",
            "2026-11-22T00:00:00Z",
        ),
        // Reading the old day never re-localises: 12:00Z is the 4th in Sao Paulo
        // (DST, -02:00 at that instant) and the new midnight is UTC's.
        (
            "2018-11-04T12:00:00Z",
            "America/Sao_Paulo",
            "UTC",
            "2018-11-04T00:00:00Z",
        ),
    ];

    #[test]
    fn h09a_retimes_to_the_new_zones_midnight_with_pytz_semantics() {
        for &(snoozed_till, old, new, expected) in RETIME_GOLDEN {
            assert_eq!(
                retimed_snooze(at(snoozed_till), zone(old), zone(new)),
                at(expected),
                "{snoozed_till} {old} -> {new}"
            );
        }
    }
}
