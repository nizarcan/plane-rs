// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `DefaultAnalyticsEndpoint` and `ProjectStatsEndpoint`
//! (`plane/app/views/analytic/base.py:264-487`).
//!
//! `default-analytics` is the heaviest read of the group: Django runs ten
//! statements for it (two counts, two `SUM(point)`, two `state_group` groups,
//! the completed-month histogram and three "top users" aggregates), in the
//! order the view's eager `count()`/`aggregate()` calls and its lazy dict
//! evaluate. The port issues the same ten, in the same order.
//!
//! `project-stats` runs two: the caller's workspace role, then one statement
//! whose `SELECT` carries a correlated `COUNT` subquery per requested field.
//! `requested_fields` is a `set` in the Python, so the JSON key order of each
//! row is CPython's `str` hash order. Rust emits the same keys in ascending
//! order and the harness compares that object as an unordered key set
//! (`DECISIONS.md` D16, `scenarios/normalize.yaml` `mode: object_keys`).

use chrono::{DateTime, Datelike, NaiveDate, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::model::Relation;
use plane_query::json::Json;
use plane_query::sql::{Frag, JoinKind, Value};
use uuid::Uuid;

use crate::analytics::base::{
    count, legacy_filters, read_text, read_uuid, rendered, user_timezone,
};
use crate::analytics::queries;
use crate::analytics::sql::{self, QueryBase};
use crate::common::{client, jobs};

/// `ROLE.MEMBER.value` (`plane/app/permissions/__init__.py`).
const ROLE_MEMBER: i16 = 15;
/// `ROLE.ADMIN.value`.
const ROLE_ADMIN: i16 = 20;

/// `DefaultAnalyticsEndpoint.get` (`plane/app/views/analytic/base.py:264-407`).
pub fn default_analytics<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let timezone = user_timezone(ctx);
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;
        let legacy = legacy_filters(ctx)?;
        let now = Utc::now();

        let base = || {
            QueryBase::issue_objects()
                .workspace_slug(slug)
                .project_members(member_id, true)
                .legacy(&legacy, timezone)
        };
        let open = || {
            let mut base = base();
            base.require_relation(Relation::States, JoinKind::Inner);
            base.state_groups(&["backlog", "unstarted", "started"]);
            base
        };
        let states = || {
            let mut base = base();
            base.require_relation(Relation::States, JoinKind::Inner);
            base
        };

        // `total_issues = base_issues.count()` (eager, `base.py:276`).
        let total_issues = count(client, &base()).await?;
        // `open_issues = open_issues_queryset.count()` (eager, `base.py:287`).
        let open_issues = count(client, &open()).await?;
        // `open_estimate_sum` then `total_estimate_sum` (eager,
        // `base.py:390-391`).
        let open_estimate_sum = sum_point(client, &open()).await?;
        let total_estimate_sum = sum_point(client, &base()).await?;

        // The lazy querysets, evaluated during serialisation in the response
        // dict's order.
        let total_issues_classified = classified(client, &states()).await?;
        let open_issues_classified = classified(client, &open()).await?;
        let issue_completed_month_wise = month_wise(client, &base(), timezone, now).await?;
        let most_issue_created_user = most_issue_created_user(client, &base()).await?;
        let most_issue_closed_user = most_issue_closed_user(client, &base()).await?;
        let pending_issue_user = pending_issue_user(client, &base()).await?;

        let body = Json::Object(vec![
            ("total_issues".to_owned(), Json::Int(total_issues)),
            (
                "total_issues_classified".to_owned(),
                total_issues_classified,
            ),
            ("open_issues".to_owned(), Json::Int(open_issues)),
            ("open_issues_classified".to_owned(), open_issues_classified),
            (
                "issue_completed_month_wise".to_owned(),
                issue_completed_month_wise,
            ),
            (
                "most_issue_created_user".to_owned(),
                most_issue_created_user,
            ),
            ("most_issue_closed_user".to_owned(), most_issue_closed_user),
            ("pending_issue_user".to_owned(), pending_issue_user),
            (
                "open_estimate_sum".to_owned(),
                open_estimate_sum.map_or(Json::Null, Json::Int),
            ),
            (
                "total_estimate_sum".to_owned(),
                total_estimate_sum.map_or(Json::Null, Json::Int),
            ),
        ]);
        Ok(rendered(&body))
    })
}

/// `ProjectStatsEndpoint.get` (`plane/app/views/analytic/base.py:410-487`).
pub fn project_stats<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;

        let fields = ctx.query().get_or("fields", "");
        let requested = requested_fields(fields);
        let project_ids = ctx.query().get_or("project_ids", "");
        let project_ids = if project_ids.is_empty() {
            Vec::new()
        } else {
            let mut ids = Vec::new();
            for item in project_ids.split(',') {
                ids.push(Uuid::parse_str(item).map_err(|_| ViewError::Validation)?);
            }
            ids
        };

        // `role = WorkspaceMember.objects.filter(...).values_list("role").first()`
        // (`base.py:431-435`).
        let statement = client
            .prepare_cached(queries::WORKSPACE_MEMBER_ROLE.sql)
            .await
            .map_err(ViewError::from)?;
        let role: Option<i16> = client
            .query_opt(&statement, &[&member_id, &slug])
            .await
            .map_err(ViewError::from)?
            .map(|row| row.get(0));

        let mut frag = Frag::raw("SELECT projects.id AS id");
        for field in &requested {
            frag.push_raw(", ");
            frag.push(subquery(field));
            frag.push_raw(" AS ");
            frag.push_raw(field);
        }
        frag.push_raw(
            " FROM projects INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
             WHERE projects.deleted_at IS NULL AND workspaces.slug = ",
        );
        frag.push(Frag::bind(Value::Text(slug.to_owned())));
        if !project_ids.is_empty() {
            frag.push_raw(" AND ");
            frag.push(Frag::in_uuids("projects.id", &project_ids));
        }
        // `if role != ROLE.ADMIN.value:` — members see their projects plus the
        // public ones, guests only their projects (`base.py:436-441`).
        if role != Some(ROLE_ADMIN) {
            frag.push_raw(" AND ");
            let mut visible = Frag::raw(
                "projects.id IN (SELECT U0.project_id FROM project_members U0 \
                 INNER JOIN workspaces U2 ON (U0.workspace_id = U2.id) \
                 WHERE U0.deleted_at IS NULL AND U0.is_active AND U0.member_id = ",
            );
            visible.push(Frag::bind(Value::Uuid(member_id)));
            visible.push_raw(" AND U2.slug = ");
            visible.push(Frag::bind(Value::Text(slug.to_owned())));
            visible.push_raw(")");
            if role == Some(ROLE_MEMBER) {
                // `visible |= Q(network=2)` — one parenthesised `OR`, or the
                // `OR` would swallow every predicate before it.
                frag.push(visible.or(Frag::raw("projects.network = 2")).paren());
            } else {
                frag.push(visible);
            }
        }
        frag.push_raw(" ORDER BY projects.created_at DESC");

        let rows = sql::fetch(client, &frag.into_sql()).await?;
        let mut items = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut entries = Vec::with_capacity(requested.len() + 1);
            let id = row.try_get::<_, Uuid>(0).map_err(ViewError::from)?;
            entries.push(("id".to_owned(), Json::Str(id.to_string())));
            for (index, field) in requested.iter().enumerate() {
                let value = row.try_get::<_, i64>(index + 1).unwrap_or(0);
                entries.push(((*field).to_owned(), Json::Int(value)));
            }
            items.push(Json::Object(entries));
        }
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            Json::Array(items).to_drf(),
        ))
    })
}

/// `set(filter(None, fields)) & valid_fields` — the fields the caller asked
/// for, deduplicated, in ascending order; all five when none is named
/// (`plane/app/views/analytic/base.py:416-427`).
///
/// The Python splats a `set` into `values("id", *requested_fields)`, so the
/// row's key *order* is CPython's hash order. D16: Rust emits set-derived
/// values in ascending order, and the harness compares this object's key set
/// (`scenarios/normalize.yaml`, `mode: object_keys`), not its order.
fn requested_fields(fields: &str) -> Vec<&'static str> {
    // Ascending, and the same order as the Python's `valid_fields` set.
    const VALID: [&str; 5] = [
        "completed_issues",
        "total_cycles",
        "total_issues",
        "total_members",
        "total_modules",
    ];
    let mut requested: Vec<&'static str> = VALID
        .into_iter()
        .filter(|field| fields.split(',').any(|item| item == *field))
        .collect();
    if requested.is_empty() {
        requested.extend(VALID);
    }
    requested
}

/// The correlated subquery one `project-stats` field selects
/// (`base.py:445-484`).
fn subquery(field: &str) -> Frag {
    match field {
        "total_issues" => Frag::raw(
            "(SELECT COUNT(U0.id) FROM issues U0 \
             LEFT OUTER JOIN states U1 ON (U0.state_id = U1.id) \
             INNER JOIN projects U2 ON (U0.project_id = U2.id) \
             WHERE U0.deleted_at IS NULL \
               AND NOT (U1.group = 'triage' AND U1.group IS NOT NULL) \
               AND NOT (U0.archived_at IS NOT NULL) \
               AND NOT (U2.archived_at IS NOT NULL) \
               AND NOT U0.is_draft AND U0.project_id = projects.id)",
        ),
        "completed_issues" => Frag::raw(
            "(SELECT COUNT(U0.id) FROM issues U0 \
             INNER JOIN states U1 ON (U0.state_id = U1.id) \
             INNER JOIN projects U2 ON (U0.project_id = U2.id) \
             WHERE U0.deleted_at IS NULL \
               AND NOT (U1.group = 'triage' AND U1.group IS NOT NULL) \
               AND NOT (U0.archived_at IS NOT NULL) \
               AND NOT (U2.archived_at IS NOT NULL) \
               AND NOT U0.is_draft AND U0.project_id = projects.id \
               AND U1.group IN ('completed', 'cancelled'))",
        ),
        "total_cycles" => Frag::raw(
            "(SELECT COUNT(U0.id) FROM cycles U0 \
             WHERE U0.deleted_at IS NULL AND U0.project_id = projects.id)",
        ),
        "total_modules" => Frag::raw(
            "(SELECT COUNT(U0.id) FROM modules U0 \
             WHERE U0.deleted_at IS NULL AND U0.project_id = projects.id)",
        ),
        "total_members" => Frag::raw(
            "(SELECT COUNT(U0.id) FROM project_members U0 \
             INNER JOIN users U1 ON (U0.member_id = U1.id) \
             WHERE U0.deleted_at IS NULL AND U0.is_active AND NOT U1.is_bot \
               AND U0.project_id = projects.id)",
        ),
        _ => Frag::raw("0"),
    }
}

/// `SELECT SUM(issues.point)` — `None` when every row is NULL (or there are
/// none), which DRF renders as `null`.
async fn sum_point(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Option<i64>, ViewError> {
    let statement = base.statement(Frag::raw("SUM(issues.point)"), "");
    let rows = sql::fetch(client, &statement).await?;
    match rows.first().map(|row| row.try_get::<_, Option<i64>>(0)) {
        Some(Ok(value)) => Ok(value),
        Some(Err(error)) => Err(ViewError::from(error)),
        None => Ok(None),
    }
}

/// `values("state_group").annotate(state_count=Count("state_group")).order_by("state_group")`.
async fn classified(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Json, ViewError> {
    let projection = Frag::raw("states.group AS state_group, COUNT(states.group) AS state_count");
    let statement = base.statement(projection, " GROUP BY 1 ORDER BY 1 ASC");
    let rows = sql::fetch(client, &statement).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            ("state_group".to_owned(), read_text(row, 0)?),
            (
                "state_count".to_owned(),
                Json::Int(row.try_get::<_, i64>(1)?),
            ),
        ]));
    }
    Ok(Json::Array(items))
}

/// `EXTRACT(MONTH FROM completed_at AT TIME ZONE tz)` bucketed over the UTC
/// year of `timezone.now()` (`base.py:294-301`).
///
/// The *filter* uses the active timezone's year boundaries (`__year` is a
/// tz-aware lookup), while `current_year` itself is `timezone.now().year` on an
/// aware UTC datetime — an upstream inconsistency ported as it is.
async fn month_wise(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
    timezone: Tz,
    now: DateTime<Utc>,
) -> Result<Json, ViewError> {
    let year = now.year();
    let start = local_start(
        timezone,
        NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or_default(),
    );
    let end = local_end(
        timezone,
        NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or_default(),
    );
    let mut base = base.clone();
    let mut between = Frag::raw("issues.completed_at BETWEEN ");
    between.push(Frag::bind(Value::Timestamp(start)));
    between.push_raw(" AND ");
    between.push(Frag::bind(Value::Timestamp(end)));
    base.and(between);

    let mut projection = Frag::raw("EXTRACT(MONTH FROM issues.completed_at ");
    projection.push_raw(&sql::timezone_literal(timezone));
    projection.push_raw(")::int AS month, COUNT(*) AS count");
    let statement = base.statement(projection, " GROUP BY 1 ORDER BY 1 ASC");
    let rows = sql::fetch(client, &statement).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            (
                "month".to_owned(),
                Json::Int(i64::from(row.try_get::<_, i32>(0)?)),
            ),
            ("count".to_owned(), Json::Int(row.try_get::<_, i64>(1)?)),
        ]));
    }
    Ok(Json::Array(items))
}

/// `most_issue_created_user` (`base.py:303-332`).
async fn most_issue_created_user(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Json, ViewError> {
    let mut base = base.clone();
    sql::require_creator_users(&mut base, JoinKind::Left);
    base.and(Frag::is_not_null("issues.created_by_id"));
    let projection = format!(
        "creator_user.first_name AS created_by__first_name, \
         creator_user.last_name AS created_by__last_name, \
         creator_user.display_name AS created_by__display_name, \
         issues.created_by_id AS created_by__id, \
         COUNT(issues.id) AS count, {} AS created_by__avatar_url",
        sql::avatar_url("creator_user")
    );
    let statement = base.statement(
        Frag::raw(projection),
        " GROUP BY 1, 2, 3, 4, 6 ORDER BY 5 DESC LIMIT 5",
    );
    let rows = sql::fetch(client, &statement).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            ("created_by__first_name".to_owned(), read_text(row, 0)?),
            ("created_by__last_name".to_owned(), read_text(row, 1)?),
            ("created_by__display_name".to_owned(), read_text(row, 2)?),
            ("created_by__id".to_owned(), read_uuid(row, 3)?),
            ("count".to_owned(), Json::Int(row.try_get::<_, i64>(4)?)),
            ("created_by__avatar_url".to_owned(), read_text(row, 5)?),
        ]));
    }
    Ok(Json::Array(items))
}

/// `most_issue_closed_user` (`base.py:341-364`).
///
/// `exclude(assignees=None)` is Django's anti-join subquery; the port keeps the
/// same correlated `NOT EXISTS`.
async fn most_issue_closed_user(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Json, ViewError> {
    let mut base = base.clone();
    base.require_relation(Relation::IssueAssignees, JoinKind::Left);
    sql::require_assignee_users(&mut base, JoinKind::Left);
    base.and(Frag::is_not_null("issues.completed_at"));
    base.and(Frag::raw(
        "NOT (EXISTS(SELECT 1 AS a FROM issues U0 \
         LEFT OUTER JOIN issue_assignees U1 ON (U0.id = U1.issue_id) \
         WHERE (U1.assignee_id IS NULL AND U0.id = issues.id) LIMIT 1))",
    ));
    let projection = format!(
        "assignee_user.first_name AS assignees__first_name, \
         assignee_user.last_name AS assignees__last_name, \
         assignee_user.display_name AS assignees__display_name, \
         issue_assignees.assignee_id AS assignees__id, \
         {} AS assignees__avatar_url, COUNT(issues.id) AS count",
        sql::avatar_url("assignee_user")
    );
    let statement = base.statement(
        Frag::raw(projection),
        " GROUP BY 1, 2, 3, 4, 5 ORDER BY 6 DESC LIMIT 5",
    );
    let rows = sql::fetch(client, &statement).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            ("assignees__first_name".to_owned(), read_text(row, 0)?),
            ("assignees__last_name".to_owned(), read_text(row, 1)?),
            ("assignees__display_name".to_owned(), read_text(row, 2)?),
            ("assignees__id".to_owned(), read_uuid(row, 3)?),
            ("assignees__avatar_url".to_owned(), read_text(row, 4)?),
            ("count".to_owned(), Json::Int(row.try_get::<_, i64>(5)?)),
        ]));
    }
    Ok(Json::Array(items))
}

/// `pending_issue_user` (`base.py:366-388`).
async fn pending_issue_user(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Json, ViewError> {
    let mut base = base.clone();
    base.require_relation(Relation::IssueAssignees, JoinKind::Left);
    sql::require_assignee_users(&mut base, JoinKind::Left);
    base.and(Frag::is_null("issues.completed_at"));
    let projection = format!(
        "assignee_user.first_name AS assignees__first_name, \
         assignee_user.last_name AS assignees__last_name, \
         assignee_user.display_name AS assignees__display_name, \
         issue_assignees.assignee_id AS assignees__id, \
         COUNT(issues.id) AS count, {} AS assignees__avatar_url",
        sql::avatar_url("assignee_user")
    );
    let statement = base.statement(
        Frag::raw(projection),
        " GROUP BY 1, 2, 3, 4, 6 ORDER BY 5 DESC",
    );
    let rows = sql::fetch(client, &statement).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            ("assignees__first_name".to_owned(), read_text(row, 0)?),
            ("assignees__last_name".to_owned(), read_text(row, 1)?),
            ("assignees__display_name".to_owned(), read_text(row, 2)?),
            ("assignees__id".to_owned(), read_uuid(row, 3)?),
            ("count".to_owned(), Json::Int(row.try_get::<_, i64>(4)?)),
            ("assignees__avatar_url".to_owned(), read_text(row, 5)?),
        ]));
    }
    Ok(Json::Array(items))
}

/// `datetime.combine(day, time.min)` in `tz`, converted to UTC.
///
/// Django converts a naive datetime with `timezone.make_aware(value, tz)`, i.e.
/// `value.replace(tzinfo=tz)`: for a DST gap that is the *pre-transition*
/// offset (`fold=0`), which is what the fallback reproduces.
pub(crate) fn local_start(timezone: Tz, day: NaiveDate) -> DateTime<Utc> {
    let naive = day.and_hms_opt(0, 0, 0).unwrap_or_default();
    match timezone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(value) => value.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(value, _) => value.with_timezone(&Utc),
        chrono::LocalResult::None => gap_fallback(timezone, naive),
    }
}

/// `datetime.combine(day, datetime.max.time())` in `tz`, converted to UTC.
pub(crate) fn local_end(timezone: Tz, day: NaiveDate) -> DateTime<Utc> {
    let naive = day
        .and_hms_micro_opt(23, 59, 59, 999_999)
        .unwrap_or_default();
    match timezone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(value) => value.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(value, _) => value.with_timezone(&Utc),
        chrono::LocalResult::None => gap_fallback(timezone, naive),
    }
}

/// The wall time falls in a DST gap: use the offset in force before the
/// transition, which is what a `fold=0` zoneinfo attachment does.
fn gap_fallback(timezone: Tz, naive: chrono::NaiveDateTime) -> DateTime<Utc> {
    let previous = naive - chrono::Duration::days(1);
    let offset = timezone.offset_from_utc_datetime(&previous).fix();
    DateTime::<Utc>::from_naive_utc_and_offset(naive - offset, Utc)
}

#[cfg(test)]
mod tests {
    use super::requested_fields;

    #[test]
    fn project_stats_fields_are_deduplicated_and_ascending() {
        // `set(filter(None, fields)) & valid_fields`: unknown names and empty
        // entries drop out, duplicates collapse, and an empty intersection means
        // every valid field (`base.py:416-427`). Rust's order is ascending
        // (D16); the harness compares the key set, not the order.
        assert_eq!(
            requested_fields("total_issues,total_issues,completed_issues"),
            vec!["completed_issues", "total_issues"]
        );
        assert_eq!(
            requested_fields("nonsense,,total_members"),
            vec!["total_members"]
        );
        assert_eq!(
            requested_fields(""),
            vec![
                "completed_issues",
                "total_cycles",
                "total_issues",
                "total_members",
                "total_modules"
            ]
        );
        assert_eq!(
            requested_fields("total_modules,total_cycles,total_issues"),
            vec!["total_cycles", "total_issues", "total_modules"]
        );
    }
}
