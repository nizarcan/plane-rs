// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET …/issues/<issue_id>/history/` — `IssueActivityEndpoint.get` (route 80).
//!
//! Python: `plane/app/views/issue/activity.py:28-104`.
//!
//! The view is unusual in three ways, and all three are part of the response:
//!
//! 1. it **merges** two serializers — `IssueActivitySerializer` rows and
//!    `IssueCommentSerializer` rows — and sorts the combined list by the
//!    *rendered* `created_at` string (`sorted(chain(...), key=lambda i:
//!    i["created_at"])`). Sorting on the string instead of the instant only
//!    differs around a DST fall-back; the port sorts on the rendered string so
//!    it cannot differ at all;
//! 2. the activity queryset excludes `field` in `comment/vote/reaction/draft`
//!    with `~Q(field__in=[...])`, which Django compiles to
//!    `NOT (field IN (…) AND field IS NOT NULL)` — NULL-field rows are kept;
//! 3. `activity_type=issue-property` and `activity_type=issue-comment` short
//!    circuit the merge and return only one side.
//!
//! `use_read_replica = True` on the view; `routes.json` records it and the
//! runtime picks the replica connection.
//!
//! Round trips: Django costs 29 queries on the fixture (one avatar-asset query
//! per activity/comment actor and per reaction actor). This port reads the two
//! sides, the reactions and the intake sources in at most four statements.

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use tokio_postgres::Row;
use uuid::Uuid;

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::issue::IssueActivity;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_issue::{IssueActivitySerializer, IssueFlatSerializer};
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use plane_serializers::{DrfDateTime, DrfValue};

use super::sub_common::{
    self as shared, CommentEntry, CommentReactionRow, CommentRow, IssueFlatRow, ProjectLiteRow,
    UserLiteRow, WorkspaceLiteRow,
};
use super::sub_queries as queries;
use crate::common;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.activity.IssueActivityEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

struct ActivityRow {
    activity: IssueActivity,
    issue: Option<IssueFlatRow>,
    project: Option<ProjectLiteRow>,
    workspace: Option<WorkspaceLiteRow>,
    actor: Option<UserLiteRow>,
}

impl ActivityRow {
    const ISSUE_AT: usize = 20;
    const PROJECT_AT: usize = Self::ISSUE_AT + IssueFlatRow::COLUMN_COUNT;
    const WORKSPACE_AT: usize = Self::PROJECT_AT + ProjectLiteRow::COLUMN_COUNT;
    const ACTOR_AT: usize = Self::WORKSPACE_AT + WorkspaceLiteRow::COLUMN_COUNT;

    fn decode(row: &Row) -> Result<Self, ViewError> {
        Ok(Self {
            activity: IssueActivity::from_row(row, 0)?,
            issue: IssueFlatRow::from_row(row, Self::ISSUE_AT)?,
            project: ProjectLiteRow::from_row(row, Self::PROJECT_AT)?,
            workspace: WorkspaceLiteRow::from_row(row, Self::WORKSPACE_AT)?,
            actor: UserLiteRow::from_row(row, Self::ACTOR_AT)?,
        })
    }
}

struct ActivityEntry<'a> {
    activity: &'a IssueActivity,
    issue: Option<IssueFlatSerializer<'a>>,
    project: Option<ProjectLiteSerializer<'a>>,
    workspace: Option<WorkspaceLiteSerializer<'a>>,
    actor: Option<UserLiteSerializer<'a>>,
    attachments: Vec<&'a str>,
    source_data: DrfValue<'a>,
}

impl<'a> ActivityEntry<'a> {
    fn new(row: &'a ActivityRow, source: Option<&'a IntakeSourceRow>) -> Self {
        let source_data = match source {
            Some(source) => DrfValue::Object(vec![
                (
                    "source",
                    source
                        .source
                        .as_deref()
                        .map_or(DrfValue::Null, DrfValue::Str),
                ),
                (
                    "source_email",
                    source
                        .source_email
                        .as_deref()
                        .map_or(DrfValue::Null, DrfValue::Str),
                ),
                ("extra", DrfValue::JsonText(&source.extra)),
            ]),
            None => DrfValue::Null,
        };
        Self {
            activity: &row.activity,
            issue: row.issue.as_ref().map(IssueFlatRow::serializer),
            project: row.project.as_ref().map(ProjectLiteRow::serializer),
            workspace: row.workspace.as_ref().map(WorkspaceLiteRow::serializer),
            actor: row.actor.as_ref().map(UserLiteRow::serializer),
            attachments: row
                .activity
                .attachments
                .iter()
                .map(String::as_str)
                .collect(),
            source_data,
        }
    }

    fn serializer(&self) -> IssueActivitySerializer<'_> {
        IssueActivitySerializer {
            id: self.activity.id,
            actor_detail: self.actor.as_ref(),
            issue_detail: self.issue.as_ref(),
            project_detail: self.project.as_ref(),
            workspace_detail: self.workspace.as_ref(),
            source_data: self.source_data.clone(),
            created_at: self.activity.created_at,
            updated_at: self.activity.updated_at,
            deleted_at: self.activity.deleted_at,
            verb: &self.activity.verb,
            field: self.activity.field.as_deref(),
            old_value: self.activity.old_value.as_deref(),
            new_value: self.activity.new_value.as_deref(),
            comment: &self.activity.comment,
            attachments: &self.attachments,
            old_identifier: self.activity.old_identifier,
            new_identifier: self.activity.new_identifier,
            epoch: self.activity.epoch,
            created_by: self.activity.created_by_id,
            updated_by: self.activity.updated_by_id,
            project: self.activity.project_id,
            workspace: self.activity.workspace_id,
            issue: self.activity.issue_id,
            issue_comment: self.activity.issue_comment_id,
            actor: self.activity.actor_id,
        }
    }
}

struct IntakeSourceRow {
    source: Option<String>,
    source_email: Option<String>,
    extra: JsonText,
}

// ---------------------------------------------------------------------------
// Request handling
// ---------------------------------------------------------------------------

/// `IssueActivityEndpoint.get` (`activity.py:34-104`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user_id = shared::user_id(ctx)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);

        // `filters = {"created_at__gt": request.GET.get("created_at__gt")}`
        // when the parameter is *present* (`activity.py:35-37`).
        let created_at_gt = match ctx.query().get("created_at__gt") {
            None => None,
            Some(value) => Some(parse_filter_datetime(value, timezone)?),
        };

        if !shared::issue_in_project(client, issue_id, project_id, slug).await? {
            return Ok(shared::issue_not_found_response());
        }
        if shared::issue_hidden_from_guest(client, slug, project_id, issue_id, user_id).await? {
            return Ok(shared::hidden_response());
        }

        let activity_type = ctx.query().get("activity_type");

        if activity_type == Some("issue-property") {
            let rows = activities(ctx, slug, project_id, issue_id, user_id, created_at_gt).await?;
            let source = if rows.is_empty() {
                None
            } else {
                intake_source(ctx, issue_id).await?
            };
            let entries: Vec<ActivityEntry<'_>> = rows
                .iter()
                .map(|row| ActivityEntry::new(row, source.as_ref()))
                .collect();
            let prepared = common::RequestRender::from_ctx(ctx);
            let serializers: Vec<IssueActivitySerializer<'_>> =
                entries.iter().map(ActivityEntry::serializer).collect();
            let body = common::render_list(&serializers, &prepared.render_ctx())?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        }

        if activity_type == Some("issue-comment") {
            let (rows, reactions, grouped) =
                comments_with_reactions(ctx, slug, project_id, issue_id, user_id, created_at_gt)
                    .await?;
            return render_comments(ctx, &rows, &reactions, &grouped);
        }

        let activity_rows =
            activities(ctx, slug, project_id, issue_id, user_id, created_at_gt).await?;
        let (comment_rows, reactions, grouped) =
            comments_with_reactions(ctx, slug, project_id, issue_id, user_id, created_at_gt)
                .await?;

        let entries: Vec<ActivityEntry<'_>> = activity_rows
            .iter()
            .map(|row| ActivityEntry::new(row, None))
            .collect();
        let grouped_refs: Vec<Vec<&CommentReactionRow>> = grouped
            .iter()
            .map(|indexes| indexes.iter().map(|index| &reactions[*index]).collect())
            .collect();
        let comment_entries: Vec<CommentEntry<'_>> = comment_rows
            .iter()
            .enumerate()
            .map(|(position, row)| {
                CommentEntry::new(
                    row,
                    grouped_refs
                        .get(position)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                )
            })
            .collect();

        // `sorted(chain(activities, comments), key=lambda i: i["created_at"])`:
        // Python sorts the *rendered* strings, and `sorted` is stable.
        enum HistoryEntry<'a> {
            Activity(&'a ActivityEntry<'a>),
            Comment(&'a CommentEntry<'a>),
        }
        let mut ordered: Vec<(String, HistoryEntry<'_>)> = entries
            .iter()
            .map(|entry| {
                rendered_datetime(entry.activity.created_at, timezone)
                    .map(|key| (key, HistoryEntry::Activity(entry)))
            })
            .chain(comment_entries.iter().map(|entry| {
                rendered_datetime(entry.created_at(), timezone)
                    .map(|key| (key, HistoryEntry::Comment(entry)))
            }))
            .collect::<Result<_, _>>()?;
        ordered.sort_by(|left, right| left.0.cmp(&right.0));

        let prepared = common::RequestRender::from_ctx(ctx);
        let render_ctx = prepared.render_ctx();
        let mut body = Vec::new();
        body.push(b'[');
        for (position, (_, entry)) in ordered.iter().enumerate() {
            if position > 0 {
                body.push(b',');
            }
            let item = match entry {
                HistoryEntry::Activity(entry) => {
                    plane_serializers::render(&entry.serializer(), &render_ctx)
                        .map_err(ViewError::from)?
                }
                HistoryEntry::Comment(entry) => {
                    plane_serializers::render(&entry.serializer(), &render_ctx)
                        .map_err(ViewError::from)?
                }
            };
            body.extend_from_slice(&item);
        }
        body.push(b']');
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

fn render_comments(
    ctx: &ViewCtx<'_>,
    rows: &[CommentRow],
    reactions: &[CommentReactionRow],
    grouped: &[Vec<usize>],
) -> Result<ViewResponse, ViewError> {
    let grouped_refs: Vec<Vec<&CommentReactionRow>> = grouped
        .iter()
        .map(|indexes| indexes.iter().map(|index| &reactions[*index]).collect())
        .collect();
    let entries: Vec<CommentEntry<'_>> = rows
        .iter()
        .enumerate()
        .map(|(position, row)| {
            CommentEntry::new(
                row,
                grouped_refs
                    .get(position)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            )
        })
        .collect();
    let prepared = common::RequestRender::from_ctx(ctx);
    let serializers: Vec<_> = entries.iter().map(CommentEntry::serializer).collect();
    let body = common::render_list(&serializers, &prepared.render_ctx())?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

async fn activities(
    ctx: &ViewCtx<'_>,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    user_id: Uuid,
    created_at_gt: Option<DateTime<Utc>>,
) -> Result<Vec<ActivityRow>, ViewError> {
    let client = common::client(ctx)?;
    let statement = queries::ACTIVITIES.prepare(client).await?;
    let rows = client
        .query(
            &statement,
            &[&issue_id, &user_id, &project_id, &slug, &created_at_gt],
        )
        .await?;
    rows.iter().map(ActivityRow::decode).collect()
}

async fn intake_source(
    ctx: &ViewCtx<'_>,
    issue_id: Uuid,
) -> Result<Option<IntakeSourceRow>, ViewError> {
    let client = common::client(ctx)?;
    let statement = queries::INTAKE_SOURCES.prepare(client).await?;
    let rows = client.query(&statement, &[&issue_id]).await?;
    match rows.first() {
        Some(row) => Ok(Some(IntakeSourceRow {
            source: row.try_get(0)?,
            source_email: row.try_get(1)?,
            extra: row.try_get(2)?,
        })),
        None => Ok(None),
    }
}

async fn comments_with_reactions(
    ctx: &ViewCtx<'_>,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    user_id: Uuid,
    created_at_gt: Option<DateTime<Utc>>,
) -> Result<(Vec<CommentRow>, Vec<CommentReactionRow>, Vec<Vec<usize>>), ViewError> {
    let client = common::client(ctx)?;
    let statement = queries::HISTORY_COMMENTS.prepare(client).await?;
    let rows = client
        .query(
            &statement,
            &[&issue_id, &user_id, &project_id, &slug, &created_at_gt],
        )
        .await?;
    let comments: Vec<CommentRow> = rows
        .iter()
        .map(|row| CommentRow::from_row(row, 0, false))
        .collect::<Result<_, _>>()?;
    if comments.is_empty() {
        return Ok((comments, Vec::new(), Vec::new()));
    }
    let ids: Vec<Uuid> = comments.iter().map(|row| row.comment.id).collect();
    let statement = queries::COMMENT_REACTIONS.prepare(client).await?;
    let rows = client.query(&statement, &[&ids]).await?;
    let reactions: Vec<CommentReactionRow> = rows
        .iter()
        .map(|row| CommentReactionRow::from_row(row, 0))
        .collect::<Result<_, _>>()?;
    let mut index: std::collections::HashMap<Uuid, usize> = std::collections::HashMap::new();
    for (position, comment) in comments.iter().enumerate() {
        index.insert(comment.comment.id, position);
    }
    let mut grouped: Vec<Vec<usize>> = vec![Vec::new(); comments.len()];
    for (position, reaction) in reactions.iter().enumerate() {
        if let Some(slot) = index.get(&reaction.reaction.comment_id) {
            grouped[*slot].push(position);
        }
    }
    Ok((comments, reactions, grouped))
}

/// The `created_at` string DRF writes, used as the sort key.
fn rendered_datetime(value: DateTime<Utc>, timezone: Tz) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(
        plane_serde::Dialect::Drf,
        &DrfDateTime(value.with_timezone(&timezone)),
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(String::from_utf8_lossy(&bytes)
        .trim_matches('"')
        .to_string())
}

// ---------------------------------------------------------------------------
// `DateTimeField.to_python` + `get_prep_value` for the `created_at__gt` filter
// ---------------------------------------------------------------------------

/// Parse the filter value the way Django's `DateTimeField` does
/// (`django/db/models/fields/__init__.py`, `django/utils/dateparse.py`): an ISO
/// datetime with an optional timezone, or a bare date (midnight in the active
/// timezone). An unparsable value is Django's `ValidationError`, which the
/// family handler renders as a 400.
fn parse_filter_datetime(value: &str, timezone: Tz) -> Result<DateTime<Utc>, ViewError> {
    let mut rest = value;
    let year = take_digits(&mut rest, 4, 4).ok_or(ViewError::Validation)?;
    take_char(&mut rest, '-').ok_or(ViewError::Validation)?;
    let month = take_digits(&mut rest, 1, 2).ok_or(ViewError::Validation)?;
    take_char(&mut rest, '-').ok_or(ViewError::Validation)?;
    let day = take_digits(&mut rest, 1, 2).ok_or(ViewError::Validation)?;
    let date = NaiveDate::from_ymd_opt(
        i32::try_from(year).map_err(|_| ViewError::Validation)?,
        u32::try_from(month).map_err(|_| ViewError::Validation)?,
        u32::try_from(day).map_err(|_| ViewError::Validation)?,
    )
    .ok_or(ViewError::Validation)?;

    if rest.is_empty() {
        let naive = date.and_hms_opt(0, 0, 0).ok_or(ViewError::Validation)?;
        return localize(naive, None, timezone);
    }

    let separator = rest.chars().next().ok_or(ViewError::Validation)?;
    if separator != 'T' && separator != ' ' {
        return Err(ViewError::Validation);
    }
    rest = &rest[1..];
    let hour = take_digits(&mut rest, 1, 2).ok_or(ViewError::Validation)?;
    take_char(&mut rest, ':').ok_or(ViewError::Validation)?;
    let minute = take_digits(&mut rest, 1, 2).ok_or(ViewError::Validation)?;
    let mut second = 0;
    let mut microsecond = 0;
    if rest.starts_with(':') {
        rest = &rest[1..];
        second = take_digits(&mut rest, 1, 2).ok_or(ViewError::Validation)?;
        if rest.starts_with('.') {
            rest = &rest[1..];
            let digits = take_digits_raw(&mut rest);
            if digits.len() > 12 {
                return Err(ViewError::Validation);
            }
            // `.microsecond` keeps 6 digits; the rest are truncated
            // (`django/utils/dateparse.py`).
            let first_six: String = digits.chars().take(6).collect();
            let parsed: i64 = first_six.parse().map_err(|_| ViewError::Validation)?;
            microsecond = parsed * 10_i64.pow(6 - u32::try_from(first_six.len()).unwrap_or(6));
        }
    }
    let naive = date
        .and_hms_opt(
            u32::try_from(hour).map_err(|_| ViewError::Validation)?,
            u32::try_from(minute).map_err(|_| ViewError::Validation)?,
            u32::try_from(second).map_err(|_| ViewError::Validation)?,
        )
        .ok_or(ViewError::Validation)?
        .checked_add_signed(Duration::microseconds(microsecond))
        .ok_or(ViewError::Validation)?;

    let offset = if rest.is_empty() {
        None
    } else if rest == "Z" {
        Some(FixedOffset::east_opt(0).ok_or(ViewError::Validation)?)
    } else {
        let sign = match rest.chars().next().ok_or(ViewError::Validation)? {
            '+' => 1,
            '-' => -1,
            _ => return Err(ViewError::Validation),
        };
        let mut clock = &rest[1..];
        let hours = take_digits(&mut clock, 2, 2).ok_or(ViewError::Validation)?;
        let minutes = if clock.is_empty() {
            0
        } else {
            let mut clock = clock.strip_prefix(':').unwrap_or(clock);
            let minutes = take_digits(&mut clock, 2, 2).ok_or(ViewError::Validation)?;
            if !clock.is_empty() {
                return Err(ViewError::Validation);
            }
            minutes
        };
        let total = sign * (hours * 60 + minutes);
        let seconds = i32::try_from(total * 60).map_err(|_| ViewError::Validation)?;
        Some(FixedOffset::east_opt(seconds).ok_or(ViewError::Validation)?)
    };
    localize(naive, offset, timezone)
}

fn localize(
    naive: chrono::NaiveDateTime,
    offset: Option<FixedOffset>,
    timezone: Tz,
) -> Result<DateTime<Utc>, ViewError> {
    match offset {
        Some(offset) => offset
            .from_local_datetime(&naive)
            .single()
            .map(|value| value.with_timezone(&Utc))
            .ok_or(ViewError::Validation),
        None => match timezone.from_local_datetime(&naive) {
            chrono::LocalResult::Single(local) => Ok(local.with_timezone(&Utc)),
            // `timezone.make_aware` uses `localize(is_dst=None)` and raises for
            // a nonexistent or ambiguous local time; nothing catches it, so the
            // request is a 500.
            chrono::LocalResult::Ambiguous(_, _) | chrono::LocalResult::None => Err(
                ViewError::Unknown("AmbiguousTimeError or NonExistentTimeError".to_string()),
            ),
        },
    }
}

fn take_char(rest: &mut &str, expected: char) -> Option<()> {
    let mut chars = rest.char_indices();
    match chars.next() {
        Some((_, character)) if character == expected => {
            *rest = &rest[expected.len_utf8()..];
            Some(())
        }
        _ => None,
    }
}

fn take_digits_raw<'a>(rest: &mut &'a str) -> &'a str {
    let count = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (digits, remaining) = rest.split_at(count);
    *rest = remaining;
    digits
}

fn take_digits(rest: &mut &str, minimum: usize, maximum: usize) -> Option<i64> {
    let digits = take_digits_raw(rest);
    if digits.len() < minimum || digits.len() > maximum {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_datetimes_parse_like_django() {
        let utc = chrono_tz::UTC;
        assert_eq!(
            parse_filter_datetime("2026-01-01T00:00:00Z", utc).expect("utc"),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            parse_filter_datetime("2026-01-01 00:00", utc).expect("minutes only"),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            parse_filter_datetime("2026-1-1", utc).expect("single digit date"),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            parse_filter_datetime("2026-01-01T05:30:00+05:30", utc).expect("offset"),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            parse_filter_datetime("2026-01-01T00:00:00.5Z", utc).expect("micro"),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::microseconds(500_000)
        );
    }

    #[test]
    fn invalid_values_are_a_validation_error() {
        let utc = chrono_tz::UTC;
        for value in [
            "",
            "garbage",
            "2026-13-01",
            "2026-01-01T25:00:00Z",
            "2026-01-01T00:00:00+24:00",
            "2026-01-01T",
            "2026-01-01T00:00:00Zx",
        ] {
            assert!(
                matches!(
                    parse_filter_datetime(value, utc),
                    Err(ViewError::Validation)
                ),
                "{value}"
            );
        }
    }

    #[test]
    fn a_naive_value_is_interpreted_in_the_active_timezone() {
        let berlin: Tz = "Europe/Berlin".parse().expect("zone");
        assert_eq!(
            parse_filter_datetime("2026-07-01T12:00:00", berlin).expect("berlin"),
            Utc.with_ymd_and_hms(2026, 7, 1, 10, 0, 0).unwrap()
        );
    }
}
