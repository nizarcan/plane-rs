// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H16 — `IntakeIssue.save` (`plane/db/models/intake.py:100-109`, MODEL_HOOKS
//! §2.7).
//!
//! ```python
//! def save(self, *args, **kwargs):
//!     if self.snoozed_till and (self._state.adding or self.has_changed("snoozed_till")):
//!         from plane.utils.timezone_converter import project_day_start
//!
//!         project_tz = pytz.timezone(Project.objects.values_list("timezone", flat=True).get(id=self.project_id))
//!         self.snoozed_till = project_day_start(self.snoozed_till.astimezone(project_tz).date(), project_tz)
//!     super().save(*args, **kwargs)
//! ```
//!
//! A snooze is day-granular: whatever instant the caller sent is floored to
//! **midnight of that instant's calendar day in the project's timezone**, and
//! the resulting aware datetime is what Django stores (UTC in Postgres). `null`
//! stays `null`, and an unchanged value on an update is left alone even when it
//! is not midnight.
//!
//! ## The two inputs that are not values
//!
//! * **"new or changed"** is the caller's pre-image comparison, the same shape
//!   as H10b's `state_changed` (`hooks/issue.rs`): `adding` is
//!   `self._state.adding`, `changed` is `self.has_changed("snoozed_till")`
//!   evaluated *before* the save (the change tracker resets afterwards).
//! * **the project timezone** is the project row's `timezone` column, read by
//!   the caller through the **default manager**
//!   (`Project.objects.values_list(...).get(...)`), so a soft-deleted project
//!   raises `DoesNotExist` rather than silently falling back — unlike H07,
//!   which re-reads through `_base_manager`. Parse the stored string with
//!   [`super::project::parse_timezone`]; an unknown zone is pytz's
//!   `UnknownTimeZoneError` (a 500).
//!
//! ## pytz's `localize`, not Django's `make_aware`
//!
//! `project_day_start(day, tz)` is `tz.localize(datetime.combine(day,
//! time.min))` (`plane/utils/timezone_converter.py:98-100`). pytz's
//! `localize` defaults to `is_dst=False`, which differs from chrono-tz's
//! `earliest()` on the two transition edges:
//!
//! * **ambiguous** midnight (the clock goes back over 00:00, e.g.
//!   `America/Havana` on 2026-11-01): pytz picks **standard** time, the later
//!   of the two instants;
//! * **nonexistent** midnight (the clock jumps over it, e.g.
//!   `America/Sao_Paulo` on 2018-11-04, where DST began at 00:00): pytz winds
//!   the clock back six hours, localizes that wall time and adds six hours
//!   back **with that offset** (`pytz/tzinfo.py:localize`, the
//!   `len(possible_loc_dt) == 0` branch), i.e. the pre-transition offset.
//!
//! Both cases are pinned by the tests below against the real
//! `project_day_start` source executed with the venv's pytz 2024.1.

use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

/// `project_day_start(day, tz)` (`plane/utils/timezone_converter.py:98-100`):
/// midnight of `day` in `timezone`, using pytz's `localize(..., is_dst=False)`
/// semantics (see the module comment).
#[must_use]
pub fn project_day_start(day: NaiveDate, timezone: Tz) -> DateTime<Utc> {
    let naive = day.and_hms_opt(0, 0, 0).expect("midnight is a valid time");
    localize(naive, timezone)
}

/// pytz's `tz.localize(naive, is_dst=False)`.
///
/// `Ambiguous` keeps the later (standard-time) instance; `None` reproduces the
/// six-hour wind-back described on the module, recursively, so a gap of a
/// whole skipped day (`Pacific/Apia`, 2011-12-30) resolves the same way.
fn localize(naive: NaiveDateTime, timezone: Tz) -> DateTime<Utc> {
    match timezone.from_local_datetime(&naive) {
        LocalResult::Single(value) => value.with_timezone(&Utc),
        LocalResult::Ambiguous(_, latest) => latest.with_timezone(&Utc),
        // `self.localize(dt - timedelta(hours=6), is_dst=False) + timedelta(hours=6)`:
        // adding to an aware datetime keeps its tzinfo, so the instant is
        // `naive` read with the pre-transition offset.
        LocalResult::None => localize(naive - Duration::hours(6), timezone) + Duration::hours(6),
    }
}

/// H16: the `snoozed_till` a save writes.
///
/// `adding` is `self._state.adding`; `changed` is
/// `self.has_changed("snoozed_till")` on an update (ignored when `adding`).
/// Everything else is the instance's value and the project's zone.
#[must_use]
pub fn before_save(
    snoozed_till: Option<DateTime<Utc>>,
    adding: bool,
    changed: bool,
    project_timezone: Tz,
) -> Option<DateTime<Utc>> {
    match snoozed_till {
        Some(value) if adding || changed => Some(project_day_start(
            value.with_timezone(&project_timezone).date_naive(),
            project_timezone,
        )),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The expected instants were recorded from the **real**
    /// `plane.utils.timezone_converter.project_day_start` (only that one
    /// function, extracted from the module source with `ast` because the
    /// module imports `plane.db.models` and therefore Django settings) running
    /// on CPython 3.12.13 with pytz 2024.1. Each case is
    /// `project_day_start(instant.astimezone(tz).date(), tz)` in UTC.
    ///
    /// The zones cover DST (northern and southern), half-hour and
    /// three-quarter-hour offsets, a 30-minute DST shift, an ambiguous
    /// midnight and a nonexistent one.
    const GOLDEN: &[(&str, &str, &str)] = &[
        // +05:30, no DST: the last second of the local day and the first.
        (
            "Asia/Kolkata",
            "2026-11-17T18:29:59Z",
            "2026-11-16T18:30:00Z",
        ),
        (
            "Asia/Kolkata",
            "2026-11-17T18:30:00Z",
            "2026-11-17T18:30:00Z",
        ),
        // Northern DST: spring-forward day and the day before the fall-back.
        (
            "America/New_York",
            "2026-03-09T03:00:00Z",
            "2026-03-08T05:00:00Z",
        ),
        (
            "America/New_York",
            "2026-11-01T03:30:00Z",
            "2026-10-31T04:00:00Z",
        ),
        // Ambiguous midnight (fall-back 01:00 -> 00:00): pytz's is_dst=False
        // picks the later, standard-time instant (05:00Z, not 04:00Z).
        (
            "America/Havana",
            "2026-11-01T15:00:00Z",
            "2026-11-01T05:00:00Z",
        ),
        // Nonexistent midnight (DST began at 00:00): pre-transition -03.
        (
            "America/Sao_Paulo",
            "2018-11-04T12:00:00Z",
            "2018-11-04T03:00:00Z",
        ),
        // +10:30 / +11:00, a thirty-minute DST shift.
        (
            "Australia/Lord_Howe",
            "2026-01-15T05:00:00Z",
            "2026-01-14T13:00:00Z",
        ),
        (
            "Australia/Lord_Howe",
            "2026-07-15T05:00:00Z",
            "2026-07-14T13:30:00Z",
        ),
        // +12:45 / +13:45.
        (
            "Pacific/Chatham",
            "2026-01-15T00:00:00Z",
            "2026-01-14T10:15:00Z",
        ),
        // +05:45.
        (
            "Asia/Kathmandu",
            "2026-11-17T18:14:00Z",
            "2026-11-16T18:15:00Z",
        ),
        // -09:30, on and just before local midnight.
        (
            "Pacific/Marquesas",
            "2026-06-01T09:30:00Z",
            "2026-06-01T09:30:00Z",
        ),
        (
            "Pacific/Marquesas",
            "2026-06-01T09:29:00Z",
            "2026-05-31T09:30:00Z",
        ),
        // -03:30 / -02:30.
        (
            "America/St_Johns",
            "2026-07-15T05:00:00Z",
            "2026-07-15T02:30:00Z",
        ),
        // Fall-back at 02:00: midnight is still summer time (+01).
        (
            "Europe/Dublin",
            "2026-10-25T12:00:00Z",
            "2026-10-24T23:00:00Z",
        ),
        ("Etc/UTC", "2026-11-17T23:59:59Z", "2026-11-17T00:00:00Z"),
    ];

    fn parse(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("rfc3339")
            .with_timezone(&Utc)
    }

    fn zone(name: &str) -> Tz {
        name.parse()
            .expect("chrono-tz knows every IANA zone in GOLDEN")
    }

    /// `before_save(Some(instant), adding = true, ..)` is the Python's
    /// `project_day_start(...)` for the same instant.
    #[test]
    fn flooring_matches_pytz_localize() {
        for (name, instant, expected) in GOLDEN {
            let timezone = zone(name);
            let floored = before_save(Some(parse(instant)), true, false, timezone)
                .expect("a snooze stays set");
            assert_eq!(floored, parse(expected), "{name} {instant}");
        }
    }

    /// The floor is idempotent: a stored midnight is its own day's midnight.
    #[test]
    fn flooring_twice_is_a_no_op() {
        for (name, instant, _) in GOLDEN {
            let timezone = zone(name);
            let once = before_save(Some(parse(instant)), true, false, timezone);
            let twice = before_save(once, true, false, timezone);
            assert_eq!(once, twice, "{name} {instant}");
        }
    }

    /// pytz's six-hour wind-back, applied to the two days a zone skipped
    /// entirely (`Pacific/Apia`, 2011-12-30; `Pacific/Kwajalein`,
    /// 1993-08-21). `before_save` can never see these dates — no instant maps
    /// to them — but `project_day_start` is also the primitive a bare
    /// `"YYYY-MM-DD"` snooze converts through (`snooze_day_start`,
    /// `plane/utils/timezone_converter.py:103-112`).
    ///
    /// Expected values recorded from the same real-function oracle: the
    /// wall-clock date is kept and the pre-transition offset is applied.
    #[test]
    fn a_whole_skipped_day_keeps_the_pytz_result() {
        let apia: Tz = "Pacific/Apia".parse().expect("zone");
        assert_eq!(
            project_day_start(NaiveDate::from_ymd_opt(2011, 12, 30).expect("date"), apia),
            parse("2011-12-30T10:00:00Z")
        );
        let kwajalein: Tz = "Pacific/Kwajalein".parse().expect("zone");
        assert_eq!(
            project_day_start(
                NaiveDate::from_ymd_opt(1993, 8, 21).expect("date"),
                kwajalein
            ),
            parse("1993-08-21T12:00:00Z")
        );
    }

    /// `null` is never touched, and an unchanged value on an update is kept
    /// verbatim (`self._state.adding or self.has_changed(...)`).
    #[test]
    fn null_and_unchanged_values_are_left_alone() {
        let timezone = zone("Asia/Kolkata");
        assert_eq!(before_save(None, true, false, timezone), None);
        assert_eq!(before_save(None, false, true, timezone), None);

        let not_midnight = parse("2026-11-17T10:00:00Z");
        assert_eq!(
            before_save(Some(not_midnight), false, false, timezone),
            Some(not_midnight)
        );
        // A change floors even on an existing row.
        assert_eq!(
            before_save(Some(not_midnight), false, true, timezone),
            Some(parse("2026-11-16T18:30:00Z"))
        );
    }
}
