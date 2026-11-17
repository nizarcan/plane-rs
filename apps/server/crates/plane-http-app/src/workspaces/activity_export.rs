// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `POST /api/workspaces/{slug}/user-activity/{user_id}/export/` — route 203,
//! `ExportWorkspaceUserActivityEndpoint.post`
//! (`plane/app/views/workspace/base.py:340-391`).
//!
//! The one route in the app surface that answers a CSV synchronously. It writes
//! nothing and enqueues nothing: one `IssueActivity` query (at most 10 000
//! rows) and the document.
//!
//! ## Order (`workspace/base.py:351-391`)
//!
//! 1. `WorkspaceEntityPermission` — the runtime's policy layer (`routes.json`).
//!    Its non-safe branch is the ADMIN/MEMBER test, so a guest is 403.
//! 2. `if not request.data.get("date")` → 400 `{"error": "Date is required"}`.
//! 3. The activity query. `date` goes to the ORM **unparsed**
//!    (`created_at__date=request.data.get("date")`): a bad string is
//!    `DateField.to_python`'s `ValidationError`, i.e. the family
//!    `400 {"error": "Please provide valid detail"}`, while a non-string is
//!    `date.fromisoformat`'s `TypeError` → 500.
//! 4. Nine columns per row, `sanitize_csv_row` and
//!    `csv.writer(quoting=csv.QUOTE_ALL)`; `Content-Type: text/csv` exactly
//!    (`HttpResponse(content_type=…)` adds no charset, `django/http/
//!    response.py:117-131`) and the attachment filename.
//!
//! ## Upstream quirks kept (plan §4)
//!
//! * No `archived_at` filter: activity in an archived project is exported
//!   (`workspace/base.py:355-361` has no `project__archived_at__isnull`).
//! * `user_id` is never checked against the workspace: it is only the
//!   `actor_id` filter, and `_visible_issues_q(request.user)` scopes the
//!   *issues*.
//! * `date` is only required to be truthy. `0`, `false`, `[]` and `""` are all
//!   "Date is required"; `123` and `["…"]` are the 500 above.
//! * The actor's `display_name`, the activity's own `project` identifier and
//!   the issue's `sequence_id` are read through `select_related("actor",
//!   "workspace", "issue", "project")`; an activity whose `issue_id` is NULL is
//!   filtered out by `_visible_issues_q`'s inner joins, so the
//!   `if activity.issue else ''` branch is unreachable.

use chrono::NaiveDate;
use plane_core::csv::{sanitize_csv_value, write_row};
use plane_core::py::decimal_value;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::sql::Frag;
use plane_serde::pyjson::Value as Json;

use crate::common::client;
use crate::views::write_support::{Body, body_get};
use crate::workspaces::rows;
use crate::workspaces::user::{self, ActivityRecord, ActivityScope};
use crate::write::pyiso;

/// The header row the view writes (`workspace/base.py:363-373`).
pub const HEADER: [&str; 9] = [
    "Actor name",
    "Issue ID",
    "Project",
    "Created at",
    "Updated at",
    "Action",
    "Field",
    "Old value",
    "New value",
];

/// The attachment header the view sets (`workspace/base.py:390`).
const CONTENT_DISPOSITION: &str = "attachment; filename=\"workspace-user-activity.csv\"";

/// `[:10000]` — the view's slice (`workspace/base.py:361`).
const LIMIT: i64 = 10_000;

/// `ExportWorkspaceUserActivityEndpoint.post`.
pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = user::caller(ctx)?;
        let target = user::target_user(ctx)?;
        let timezone = rows::timezone(ctx);
        let client = client(ctx)?;

        // `if not request.data.get("date")` (`:351-353`). `request.data` is
        // read before anything else in the view body.
        let data = ctx.data()?;
        let Some(date) = activity_date(data)? else {
            return Ok(crate::write::error_body(400, "Date is required"));
        };

        let mut select = user::activity_select(&ActivityScope {
            slug,
            user,
            target,
            projects: &[],
            // 203 has no `project__archived_at__isnull=True` (`:355-361`).
            live_projects_only: false,
            created_on: Some(date),
            timezone,
        });
        // `IssueActivity.Meta.ordering = ("-created_at",)`
        // (`plane/db/models/issue.py:444`) with `[:10000]`.
        select.order_by = vec![Frag::raw("issue_activities.created_at DESC")];
        select.limit = Some(LIMIT);

        let rows = user::run(client, select).await?;
        let mut document = String::with_capacity(rows.len() * 160 + 256);
        write_row(&mut document, HEADER.map(Some));
        for row in &rows {
            let record = ActivityRecord::from_row(row)?;
            let activity = &record.activity;
            // `f"{activity.project.identifier} - {activity.issue.sequence_id
            // if activity.issue else ''}"` (`:377`).
            let issue_id = format!(
                "{} - {}",
                record.project.identifier, record.issue.sequence_id
            );
            // `str(activity.created_at)`: `isoformat(" ")`, microseconds only
            // when non-zero (the shared helper `projects::write_support` added
            // for `ProjectArchiveUnarchiveEndpoint.post`).
            let created_at =
                crate::projects::write_support::python_str_datetime(activity.created_at);
            let updated_at =
                crate::projects::write_support::python_str_datetime(activity.updated_at);
            // `sanitize_csv_row` (`plane/utils/csv_utils.py:24-26`).
            let actor = sanitize_csv_value(&record.actor.display_name);
            let issue_id = sanitize_csv_value(&issue_id);
            let project = sanitize_csv_value(&record.project.name);
            let verb = sanitize_csv_value(&activity.verb);
            let field = activity.field.as_deref().map(sanitize_csv_value);
            let old_value = activity.old_value.as_deref().map(sanitize_csv_value);
            let new_value = activity.new_value.as_deref().map(sanitize_csv_value);
            write_row(
                &mut document,
                [
                    Some(actor.as_ref()),
                    Some(issue_id.as_ref()),
                    Some(project.as_ref()),
                    Some(created_at.as_str()),
                    Some(updated_at.as_str()),
                    Some(verb.as_ref()),
                    field.as_deref(),
                    old_value.as_deref(),
                    new_value.as_deref(),
                ],
            );
        }

        Ok(ViewResponse::raw(200, "text/csv", document.into_bytes())
            .with_header("Content-Disposition", CONTENT_DISPOSITION))
    })
}

/// `request.data.get("date")` through `DateField.get_prep_value`'s
/// `to_python` (`django/db/models/fields/__init__.py:1479-1508`).
///
/// `None` covers every falsy value (the caller answers "Date is required"
/// before this runs). A non-string raises `TypeError` inside
/// `date.fromisoformat` → the family 500. A string that is not a date is the
/// `ValidationError` → the family 400. Django's fallback regex
/// (`django/utils/dateparse.py:13`) accepts one-digit month/day, and its `\d`
/// is Unicode-aware.
fn activity_date(data: &plane_drf::parsers::Data) -> Result<Option<NaiveDate>, ViewError> {
    let raw = match body_get(data, "date")? {
        // `not request.data.get("date")`: `None`, `0`, `0.0`, `false`, `""`,
        // `[]` and `{}` are all the "Date is required" branch.
        Body::Missing => return Ok(None),
        Body::Json(value) if !plane_db::hooks::issue_view::truthy(value) => return Ok(None),
        Body::Form("") => return Ok(None),
        Body::Form(text) => text.to_owned(),
        Body::Json(Json::Str(text)) => text.as_str().unwrap_or_default().to_owned(),
        // `date.fromisoformat(<int|float|list|dict>)` raises `TypeError`.
        Body::Json(other) => {
            return Err(ViewError::Unknown(format!(
                "TypeError: fromisoformat: argument must be str ({})",
                crate::views::write_support::python_type_name(other)
            )));
        }
    };
    if let Some(date) = pyiso::fromisoformat_date(&raw) {
        return Ok(Some(date));
    }
    // Django's second chance: `date_re` (`django/utils/dateparse.py:13`).
    django_date_re(&raw).map(Some).ok_or(ViewError::Validation)
}

/// `(?P<year>\d{4})-(?P<month>\d{1,2})-(?P<day>\d{1,2})$` with CPython's
/// `\d` (Unicode `Nd`) and `int()` on the captured text.
fn django_date_re(text: &str) -> Option<NaiveDate> {
    // Python's `$` also matches before a single trailing newline.
    let text = text.strip_suffix('\n').unwrap_or(text);
    let mut parts = text.split('-');
    let year = parts.next()?;
    let month = parts.next()?;
    let day = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let year = digits(year, 4, 4)?;
    let month = digits(month, 1, 2)?;
    let day = digits(day, 1, 2)?;
    NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day)
}

/// A run of `min..=max` Unicode decimal digits, as `int()` would read it.
fn digits(text: &str, min: usize, max: usize) -> Option<u32> {
    let len = text.chars().count();
    if len < min || len > max {
        return None;
    }
    let mut value: u32 = 0;
    for ch in text.chars() {
        value = value.checked_mul(10)?.checked_add(decimal_value(ch)?)?;
    }
    Some(value)
}

/// `HttpResponse(csv, content_type="text/csv")` for a fixed body, used by the
/// unit tests to pin the writer's bytes against the view's header list.
#[cfg(test)]
fn expected_header_line() -> String {
    let mut line = String::new();
    write_row(&mut line, HEADER.map(Some));
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn django_accepts_a_one_digit_month_and_day() {
        assert_eq!(
            django_date_re("2026-1-5"),
            NaiveDate::from_ymd_opt(2026, 1, 5)
        );
        assert_eq!(
            django_date_re("2026-01-05"),
            NaiveDate::from_ymd_opt(2026, 1, 5)
        );
        // `\d` is Unicode `Nd`, exactly as Python's `re` reads a str.
        assert_eq!(
            django_date_re("٢٠٢٦-٠١-٠٥"),
            NaiveDate::from_ymd_opt(2026, 1, 5)
        );
        // `$` matches before a trailing newline.
        assert_eq!(
            django_date_re("2026-1-5\n"),
            NaiveDate::from_ymd_opt(2026, 1, 5)
        );
        assert_eq!(django_date_re("2026-13-01"), None);
        assert_eq!(django_date_re("2026-01-055"), None);
        assert_eq!(django_date_re("2026/01/05"), None);
        // `\d{4}` is exactly four digits, so a two-digit year is not a date.
        assert_eq!(django_date_re("26-1-5"), None);
        assert_eq!(django_date_re("20265-1-5"), None);
    }

    #[test]
    fn the_csv_header_is_the_views_header() {
        assert_eq!(
            expected_header_line(),
            "\"Actor name\",\"Issue ID\",\"Project\",\"Created at\",\"Updated at\",\
             \"Action\",\"Field\",\"Old value\",\"New value\"\r\n"
        );
    }
}
