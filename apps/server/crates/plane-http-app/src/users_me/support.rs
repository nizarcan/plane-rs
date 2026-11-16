// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Small helpers the `users-me` handlers share.
//!
//! They reproduce a Python rule that is not a serializer: CPython's `int()` in
//! the paginator, the paginator's cursor parsing, `date.today()`, and the
//! request accessors.
//!
//! The `FileAsset.asset_url` family (`User.avatar_url`, `User.cover_image_url`,
//! `Workspace.logo_url`, `Project.cover_image_url`) used to live here; it now
//! lives in [`crate::common::asset_url`] so this group and `assets` share one
//! implementation (`HANDLERS.md` §7); the domains that still carry their own
//! copy are listed in the refactor report rather than switched silently.

use chrono::{DateTime, NaiveDate, Utc};
use plane_core::ApiError;
use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;
use uuid::Uuid;

/// Python's `int(text)` for the handful of integer query parameters these
/// views read (`month`).
///
/// Delegates to `plane_core::py::int`, CPython's `int()`: surrounding
/// whitespace, an optional sign, single underscores between digits and Unicode
/// decimal digits are accepted.
pub fn python_int(text: &str) -> Option<i64> {
    plane_core::py::int(text).ok()
}

/// `BasePaginator.paginate`'s cursor step (`plane/utils/paginator.py:672-681`),
/// including `get_per_page` (`paginator.py:642-657`).
///
/// The parsing itself lives in `plane_query::PageQuery`, which is the same code
/// the issue lists run. In particular a present-but-empty `?per_page=` is the
/// `ValueError` Django's `int()` raises, not the default.
pub fn page_query(
    query: &plane_drf::QueryDict,
    default_per_page: u32,
    max_per_page: u32,
) -> Result<plane_query::PageQuery, ViewError> {
    plane_query::PageQuery::parse_with(query, default_per_page, max_per_page, None, None).map_err(
        |error| match error {
            plane_query::QueryError::Api(error) => {
                ViewError::Api(ApiError::ParseError(error.message))
            }
            other => ViewError::Unknown(other.to_string()),
        },
    )
}

/// `date.today()`, evaluated in Django's process timezone.
///
/// Django sets the process `TZ` to `settings.TIME_ZONE` and calls
/// `time.tzset()` as soon as the settings load (`django/conf/__init__.py:195-205`,
/// in the pinned 5.2), and Plane's `TIME_ZONE` is `"UTC"`
/// (`plane/settings/common.py:319`). `date.today()` is therefore the current
/// UTC date, not the host's local one — at UTC+3 the two differ between 00:00
/// and 03:00 local.
pub fn today() -> NaiveDate {
    today_at(plane_core::clock::now())
}

/// [`today`] at a given instant, so tests can pin one where the host's date
/// and the UTC date differ.
pub fn today_at(now: DateTime<Utc>) -> NaiveDate {
    now.date_naive()
}

/// `date.today() + relativedelta(months=-months)`.
///
/// `relativedelta` clamps the day to the last day of the target month, which
/// `chrono`'s `checked_sub_months` also does.
pub fn months_ago(months: u32) -> NaiveDate {
    let today = today();
    today
        .checked_sub_months(chrono::Months::new(months))
        .unwrap_or(today)
}

/// The authenticated user's id. Every route here is behind `IsAuthenticated`,
/// so a missing user is a bug in the runtime rather than a request outcome.
pub fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user.id().ok_or(ViewError::Key)
}

/// The email `request.user.email` the settings/invitation views filter on.
pub fn user_email(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    ctx.user
        .user()
        .map(|user| user.email.clone())
        .ok_or(ViewError::Key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_int_matches_the_common_cases() {
        assert_eq!(python_int("42"), Some(42));
        assert_eq!(python_int(" 42 "), Some(42));
        assert_eq!(python_int("+42"), Some(42));
        assert_eq!(python_int("-7"), Some(-7));
        assert_eq!(python_int("1_000"), Some(1000));
        // CPython's `int()` accepts Unicode decimal digits (`int("٠") == 0`).
        assert_eq!(python_int("١٢"), Some(12));
        assert_eq!(python_int(""), None);
        assert_eq!(python_int("1.0"), None);
        assert_eq!(python_int("_1"), None);
        assert_eq!(python_int("1_"), None);
        assert_eq!(python_int("1__0"), None);
        assert_eq!(python_int("abc"), None);
        assert_eq!(python_int("0x10"), None);
    }

    #[test]
    fn today_is_the_utc_date_not_the_host_date() {
        // 2026-11-16T23:30Z is already 2026-11-17 at UTC+3: a `Local::now`
        // would answer the 17th there. Django's process timezone is UTC, so
        // `date.today()` answers the 16th.
        let instant = DateTime::parse_from_rfc3339("2026-11-16T23:30:00Z")
            .expect("instant")
            .with_timezone(&Utc);
        assert_eq!(
            today_at(instant),
            NaiveDate::from_ymd_opt(2026, 11, 16).expect("date")
        );
        assert_ne!(
            today_at(instant),
            instant
                .with_timezone(&chrono::FixedOffset::east_opt(3 * 3600).expect("offset"))
                .date_naive()
        );
    }

    #[test]
    fn page_query_rejects_a_present_but_empty_per_page() {
        // `int(request.GET.get("per_page", 1000))` (`paginator.py:645`): absent
        // is the default, `?per_page=` is a `ValueError` → `ParseError`.
        let absent = plane_drf::QueryDict::parse("", None).expect("query");
        let page = page_query(&absent, 1000, 1000).expect("page");
        assert_eq!(page.per_page, 1000);

        let empty = plane_drf::QueryDict::parse("per_page=", None).expect("query");
        assert_eq!(
            page_query(&empty, 1000, 1000).expect_err("error"),
            ViewError::Api(ApiError::ParseError(
                "Invalid per_page parameter.".to_string()
            ))
        );
    }
}
