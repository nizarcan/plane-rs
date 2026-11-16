// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/analytic/advance.py` and
//! `plane/utils/date_utils.py:12-191` — the workspace "advance analytics"
//! endpoints.
//!
//! | Route | Python |
//! | --- | --- |
//! | `GET …/advance-analytics/` | `AdvanceAnalyticsEndpoint.get` (`advance.py:43-121`) |
//! | `GET …/advance-analytics-stats/` | `AdvanceAnalyticsStatsEndpoint.get` (`advance.py:124-171`) |
//! | `GET …/advance-analytics-charts/` | `AdvanceAnalyticsChartEndpoint.get` (`advance.py:174-320`) |
//!
//! ## Dates are the caller's, counts are `created_at`
//!
//! `get_analytics_date_range` builds **naive** datetimes
//! (`datetime.combine(today, time.min)`, `date_utils.py:38,45,56,67,82`), and
//! Django interprets a naive datetime in the active timezone — the caller's
//! `user_timezone`. The UTC instants therefore depend on the user: for
//! `Asia/Kolkata` the "last 3 months" window starts at `2026-08-17T18:30Z`.
//! `RUST_REWRITE.md` "Timezones" is exactly this.
//!
//! `get_chart_period_range` returns plain `date`s instead, and
//! `created_at__date__gte/lte` compiles to `(created_at AT TIME ZONE tz)::date`.
//!
//! `date_filter=custom` never filters anything: `get_analytics_date_range` is
//! called with `start_date`/`end_date` left at their `None` defaults
//! (`advance.py:34-40`), so its `custom` branch cannot be entered. Ported as it
//! is.
//!
//! ## Round trips
//!
//! `tab=overview` is 4 + 1 + 1 + 1 + 1 = 8 counts (`advance.py:80-93`);
//! `tab=work-items` is 5; `advance-analytics-stats` is 1;
//! `advance-analytics-charts?type=projects` is 7, `type=work-items` is a
//! workspace lookup plus one aggregate, and `type=custom-work-items` is one.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::model::Relation;
use plane_query::json::Json;
use plane_query::sql::{Frag, JoinKind, Value};
use uuid::Uuid;

use crate::analytics::base::{count, message_body, rendered, user_timezone};
use crate::analytics::chart::build_analytics_chart;
use crate::analytics::default::{local_end, local_start};
use crate::analytics::sql::{self, QueryBase};
use crate::common::{client, jobs, lookup};

/// The filters `get_analytics_filters` returns (`date_utils.py:125-191`).
#[derive(Debug, Clone, Default)]
pub(crate) struct AnalyticsFilters {
    /// `None` when the parameter is absent — which is not the same as an empty
    /// list for the members query (`advance.py:71-78`).
    pub(crate) project_ids: Option<Vec<Uuid>>,
    /// `{"current": {"gte", "lte"}}` (`date_utils.py:12-87`).
    pub(crate) analytics_date_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub(crate) chart_period_range: Option<(NaiveDate, NaiveDate)>,
}

/// `get_analytics_filters(...)` (`date_utils.py:125-191`).
pub(crate) fn analytics_filters(
    ctx: &ViewCtx<'_>,
    kind: &str,
    timezone: Tz,
    now: DateTime<Utc>,
) -> Result<AnalyticsFilters, ViewError> {
    let query = ctx.query();
    let project_ids = match query.get("project_ids") {
        Some(raw) if !raw.is_empty() => {
            let mut ids = Vec::new();
            for item in raw.split(',') {
                ids.push(Uuid::parse_str(item).map_err(|_| ViewError::Validation)?);
            }
            Some(ids)
        }
        _ => None,
    };
    let date_filter = query.get("date_filter").filter(|value| !value.is_empty());
    let analytics_date_range = if kind == "analytics" {
        analytics_date_range(date_filter, timezone, now)
    } else {
        None
    };
    let chart_period_range = if kind == "chart" {
        chart_period_range(date_filter, now)
    } else {
        None
    };
    Ok(AnalyticsFilters {
        project_ids,
        analytics_date_range,
        chart_period_range,
    })
}

/// `get_analytics_date_range(date_filter)` with `start_date`/`end_date` unset.
fn analytics_date_range(
    date_filter: Option<&str>,
    timezone: Tz,
    now: DateTime<Utc>,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let today = now.date_naive();
    let (start, end) = match date_filter? {
        "yesterday" => (
            today - chrono::Duration::days(1),
            today - chrono::Duration::days(1),
        ),
        "last_7_days" => (today - chrono::Duration::days(7), today),
        "last_30_days" => (today - chrono::Duration::days(30), today),
        "last_3_months" => (today - chrono::Duration::days(90), today),
        _ => return None,
    };
    Some((local_start(timezone, start), local_end(timezone, end)))
}

/// `get_chart_period_range(date_filter)` (`date_utils.py:90-122`).
fn chart_period_range(
    date_filter: Option<&str>,
    now: DateTime<Utc>,
) -> Option<(NaiveDate, NaiveDate)> {
    let today = now.date_naive();
    match date_filter? {
        "yesterday" => Some((
            today - chrono::Duration::days(1),
            today - chrono::Duration::days(1),
        )),
        "last_7_days" => Some((today - chrono::Duration::days(7), today)),
        "last_30_days" => Some((today - chrono::Duration::days(30), today)),
        "last_3_months" => Some((today - chrono::Duration::days(90), today)),
        _ => None,
    }
}

/// The `base_filters` of `get_analytics_filters` as an `Issue` queryset
/// (`date_utils.py:153-174`).
pub(crate) fn issue_base(slug: &str, member_id: Uuid, filters: &AnalyticsFilters) -> QueryBase {
    let mut base = QueryBase::issue_objects()
        .workspace_slug(slug)
        .project_members(member_id, false);
    base.and(Frag::is_null("projects.archived_at"));
    base.and(Frag::is_null("projects.deleted_at"));
    if let Some(project_ids) = &filters.project_ids {
        base.and(Frag::in_uuids("issues.project_id", project_ids));
    }
    base
}

/// `get_filtered_counts`' `created_at__gte/__lte` (`advance.py:44-64`).
pub(crate) fn with_range(base: &mut QueryBase, column: &str, filters: &AnalyticsFilters) {
    let Some((gte, lte)) = filters.analytics_date_range else {
        return;
    };
    let mut predicate = Frag::raw(column);
    predicate.push_raw(" >= ");
    predicate.push(Frag::bind(Value::Timestamp(gte)));
    predicate.push_raw(" AND ");
    predicate.push_raw(column);
    predicate.push_raw(" <= ");
    predicate.push(Frag::bind(Value::Timestamp(lte)));
    base.and(predicate);
}

/// The chart endpoints' `created_at__date__gte/lte`.
pub(crate) fn with_date_range(
    base: &mut QueryBase,
    column: &str,
    filters: &AnalyticsFilters,
    timezone: Tz,
) {
    if let Some((start, end)) = filters.chart_period_range {
        base.and(date_range_predicate(column, start, end, timezone));
    }
}

/// `(column AT TIME ZONE tz)::date >= start AND … <= end`.
pub(crate) fn date_range_predicate(
    column: &str,
    start: NaiveDate,
    end: NaiveDate,
    timezone: Tz,
) -> Frag {
    let zone = sql::timezone_literal(timezone);
    let mut predicate = Frag::raw("(");
    predicate.push_raw(column);
    predicate.push_raw(" ");
    predicate.push_raw(&zone);
    predicate.push_raw(")::date >= ");
    predicate.push(Frag::bind(Value::Date(start)));
    predicate.push_raw(" AND (");
    predicate.push_raw(column);
    predicate.push_raw(" ");
    predicate.push_raw(&zone);
    predicate.push_raw(")::date <= ");
    predicate.push(Frag::bind(Value::Date(end)));
    predicate
}

/// `AdvanceAnalyticsEndpoint.get` (`advance.py:106-121`).
pub fn advance_analytics<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let timezone = user_timezone(ctx);
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;
        let now = Utc::now();
        let filters = analytics_filters(ctx, "analytics", timezone, now)?;
        let tab = ctx.query().get_or("tab", "overview");

        let body = match tab {
            "overview" => {
                let mut entries: Vec<(String, Json)> = Vec::with_capacity(8);
                let counts = member_counts(client, slug, &filters, member_id).await?;
                for (key, value) in [
                    ("total_users", counts.total),
                    ("total_admins", counts.admins),
                    ("total_members", counts.members),
                    ("total_guests", counts.guests),
                ] {
                    entries.push((key.to_owned(), count_body(value)));
                }
                entries.push((
                    "total_projects".to_owned(),
                    count_body(project_count(client, slug, &filters, member_id).await?),
                ));
                entries.push((
                    "total_work_items".to_owned(),
                    count_body(
                        entity_count(client, Entity::Issue, slug, &filters, member_id, timezone)
                            .await?,
                    ),
                ));
                entries.push((
                    "total_cycles".to_owned(),
                    count_body(
                        entity_count(client, Entity::Cycle, slug, &filters, member_id, timezone)
                            .await?,
                    ),
                ));
                entries.push((
                    "total_intake".to_owned(),
                    count_body(
                        intake_count(client, slug, &filters, member_id, true, timezone).await?,
                    ),
                ));
                Json::Object(entries)
            }
            "work-items" => {
                let total =
                    entity_count(client, Entity::Issue, slug, &filters, member_id, timezone)
                        .await?;
                let mut entries: Vec<(String, Json)> = Vec::with_capacity(5);
                entries.push(("total_work_items".to_owned(), count_body(total)));
                for (key, group) in [
                    ("started_work_items", "started"),
                    ("backlog_work_items", "backlog"),
                    ("un_started_work_items", "unstarted"),
                    ("completed_work_items", "completed"),
                ] {
                    let value = issue_group_count(client, slug, &filters, member_id, group).await?;
                    entries.push((key.to_owned(), count_body(value)));
                }
                Json::Object(entries)
            }
            _ => return Ok(message_body(400, "Invalid tab")),
        };
        Ok(rendered(&body))
    })
}

/// `AdvanceAnalyticsStatsEndpoint.get` (`advance.py:160-171`).
pub fn advance_analytics_stats<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let timezone = user_timezone(ctx);
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;
        let now = Utc::now();
        let filters = analytics_filters(ctx, "chart", timezone, now)?;
        if ctx.query().get_or("type", "work-items") != "work-items" {
            return Ok(message_body(400, "Invalid type"));
        }
        // `get_work_items_stats` never applies `chart_period_range`
        // (`advance.py:146-158`), so `date_filter` has no effect here.
        let base = issue_base(slug, member_id, &filters);
        let projection = Frag::raw(
            "issues.project_id AS project_id, projects.name AS project__name, \
             COUNT(issues.id) FILTER (WHERE states.group = 'cancelled') AS cancelled_work_items, \
             COUNT(issues.id) FILTER (WHERE states.group = 'completed') AS completed_work_items, \
             COUNT(issues.id) FILTER (WHERE states.group = 'backlog') AS backlog_work_items, \
             COUNT(issues.id) FILTER (WHERE states.group = 'unstarted') AS un_started_work_items, \
             COUNT(issues.id) FILTER (WHERE states.group = 'started') AS started_work_items",
        );
        let statement = base.statement(projection, " GROUP BY 1, 2 ORDER BY 1 ASC");
        let rows = sql::fetch(client, &statement).await?;
        let mut items = Vec::with_capacity(rows.len());
        for row in &rows {
            items.push(Json::Object(vec![
                (
                    "project_id".to_owned(),
                    crate::analytics::base::read_uuid(row, 0)?,
                ),
                (
                    "project__name".to_owned(),
                    crate::analytics::base::read_text(row, 1)?,
                ),
                ("cancelled_work_items".to_owned(), read_int(row, 2)?),
                ("completed_work_items".to_owned(), read_int(row, 3)?),
                ("backlog_work_items".to_owned(), read_int(row, 4)?),
                ("un_started_work_items".to_owned(), read_int(row, 5)?),
                ("started_work_items".to_owned(), read_int(row, 6)?),
            ]));
        }
        Ok(rendered(&Json::Array(items)))
    })
}

/// `AdvanceAnalyticsChartEndpoint.get` (`advance.py:287-320`).
pub fn advance_analytics_chart<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let timezone = user_timezone(ctx);
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;
        let now = Utc::now();
        let filters = analytics_filters(ctx, "chart", timezone, now)?;
        let kind = ctx.query().get_or("type", "projects");
        let group_by = ctx
            .query()
            .get("group_by")
            .filter(|value| !value.is_empty());
        let x_axis = ctx.query().get_or("x_axis", "PRIORITY");

        let body = match kind {
            "projects" => {
                let work_items =
                    entity_count(client, Entity::Issue, slug, &filters, member_id, timezone)
                        .await?;
                let cycles =
                    entity_count(client, Entity::Cycle, slug, &filters, member_id, timezone)
                        .await?;
                let modules =
                    entity_count(client, Entity::Module, slug, &filters, member_id, timezone)
                        .await?;
                let intake =
                    intake_count(client, slug, &filters, member_id, false, timezone).await?;
                // `WorkspaceMember.objects.filter(...)` without the bot
                // filter (`advance.py:194-196`).
                let members = workspace_member_count(client, slug, &filters, timezone).await?;
                let pages =
                    entity_count(client, Entity::Page, slug, &filters, member_id, timezone).await?;
                let views =
                    entity_count(client, Entity::View, slug, &filters, member_id, timezone).await?;
                let mut items = Vec::with_capacity(7);
                for (key, value) in [
                    ("work_items", work_items),
                    ("cycles", cycles),
                    ("modules", modules),
                    ("intake", intake),
                    ("members", members),
                    ("pages", pages),
                    ("views", views),
                ] {
                    items.push(Json::Object(vec![
                        ("key".to_owned(), Json::Str(key.to_owned())),
                        ("name".to_owned(), Json::Str(title(key))),
                        ("count".to_owned(), Json::Int(value)),
                    ]));
                }
                Json::Array(items)
            }
            "custom-work-items" => {
                let mut base = issue_base(slug, member_id, &filters);
                with_date_range(&mut base, "issues.created_at", &filters, timezone);
                build_analytics_chart(client, base, x_axis, group_by, timezone).await?
            }
            "work-items" => {
                workspace_completion_chart(client, slug, &filters, member_id, timezone, now).await?
            }
            _ => return Ok(message_body(400, "Invalid type")),
        };
        Ok(rendered(&body))
    })
}

fn read_int(row: &tokio_postgres::Row, index: usize) -> Result<Json, ViewError> {
    match row.try_get::<_, i64>(index) {
        Ok(value) => Ok(Json::Int(value)),
        Err(error) => Err(ViewError::from(error)),
    }
}

/// `base_queryset.filter(state__group=…)` counts (`advance.py:95-104`).
async fn issue_group_count(
    client: &deadpool_postgres::Client,
    slug: &str,
    filters: &AnalyticsFilters,
    member_id: Uuid,
    group: &str,
) -> Result<i64, ViewError> {
    let mut base = issue_base(slug, member_id, filters);
    base.require_relation(Relation::States, JoinKind::Inner);
    base.state_groups(&[group]);
    with_range(&mut base, "issues.created_at", filters);
    count(client, &base).await
}

/// The non-issue tables `advance` counts under `base_filters`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entity {
    Issue,
    Cycle,
    Module,
    Page,
    View,
}

impl Entity {
    fn table(self) -> &'static str {
        match self {
            Self::Issue => "issues",
            Self::Cycle => "cycles",
            Self::Module => "modules",
            Self::Page => "project_pages",
            Self::View => "issue_views",
        }
    }

    fn date_column(self) -> &'static str {
        match self {
            Self::Issue => "issues.created_at",
            Self::Cycle => "cycles.created_at",
            Self::Module => "modules.created_at",
            Self::Page => "project_pages.created_at",
            Self::View => "issue_views.created_at",
        }
    }
}

/// `Entity.objects.filter(**base_filters).count()` (`advance.py:85-87`,
/// `advance.py:188-198`).
pub(crate) async fn entity_count(
    client: &deadpool_postgres::Client,
    entity: Entity,
    slug: &str,
    filters: &AnalyticsFilters,
    member_id: Uuid,
    timezone: Tz,
) -> Result<i64, ViewError> {
    let mut base = if entity == Entity::Issue {
        issue_base(slug, member_id, filters)
    } else {
        related_base(entity, slug, member_id, filters)
    };
    // `advance` filters `created_at` (a datetime); every chart branch filters
    // `created_at__date`, in the caller's timezone (`advance.py:182-198`).
    with_range(&mut base, entity.date_column(), filters);
    with_date_range(&mut base, entity.date_column(), filters, timezone);
    count(client, &base).await
}

/// `base_filters` over `cycles`, `modules`, `project_pages` or `issue_views`:
/// all four carry `workspace` and `project` foreign keys.
fn related_base(
    entity: Entity,
    slug: &str,
    member_id: Uuid,
    filters: &AnalyticsFilters,
) -> QueryBase {
    let table = entity.table();
    let mut base = QueryBase::new(table);
    base.join(
        JoinKind::Inner,
        "projects",
        "projects",
        Frag::raw(format!("{table}.project_id = projects.id")),
    );
    base.join(
        JoinKind::Inner,
        "project_members",
        "project_members",
        Frag::raw("projects.id = project_members.project_id"),
    );
    base.join(
        JoinKind::Inner,
        "workspaces",
        "workspaces",
        Frag::raw(format!("{table}.workspace_id = workspaces.id")),
    );
    base.and(Frag::is_null(&format!("{table}.deleted_at")));
    base.and(Frag::is_null("projects.archived_at"));
    base.and(Frag::is_null("projects.deleted_at"));
    base.and(Frag::raw("project_members.is_active"));
    base.and(Frag::compare(
        "project_members.member_id",
        "=",
        Value::Uuid(member_id),
    ));
    if let Some(project_ids) = &filters.project_ids {
        base.and(Frag::in_uuids(&format!("{table}.project_id"), project_ids));
    }
    base.and(Frag::compare(
        "workspaces.slug",
        "=",
        Value::Text(slug.to_owned()),
    ));
    base
}

/// `Issue.objects.filter(**base_filters).filter(issue_intake__status__in=[...])`
/// (`advance.py:88-92`) or `.filter(issue_intake__isnull=False)`
/// (`advance.py:191-193`).
pub(crate) async fn intake_count(
    client: &deadpool_postgres::Client,
    slug: &str,
    filters: &AnalyticsFilters,
    member_id: Uuid,
    by_status: bool,
    timezone: Tz,
) -> Result<i64, ViewError> {
    // `Issue.objects`, not `issue_objects`: no manager predicate, no `states`
    // join (`advance.py:89`).
    let mut base = QueryBase::issue_objects_off();
    base.join(
        JoinKind::Inner,
        "projects",
        "projects",
        Frag::raw("issues.project_id = projects.id"),
    );
    base.join(
        JoinKind::Inner,
        "project_members",
        "project_members",
        Frag::raw("projects.id = project_members.project_id"),
    );
    base.join(
        JoinKind::Inner,
        "workspaces",
        "workspaces",
        Frag::raw("issues.workspace_id = workspaces.id"),
    );
    base.join(
        JoinKind::Inner,
        "intake_issues",
        "intake_issues",
        Frag::raw("issues.id = intake_issues.issue_id"),
    );
    base.and(Frag::is_null("projects.archived_at"));
    base.and(Frag::is_null("projects.deleted_at"));
    base.and(Frag::raw("project_members.is_active"));
    base.and(Frag::compare(
        "project_members.member_id",
        "=",
        Value::Uuid(member_id),
    ));
    if let Some(project_ids) = &filters.project_ids {
        base.and(Frag::in_uuids("issues.project_id", project_ids));
    }
    base.and(Frag::compare(
        "workspaces.slug",
        "=",
        Value::Text(slug.to_owned()),
    ));
    if by_status {
        base.and(Frag::raw("intake_issues.status IN (-2, -1, 0, 1, 2)"));
    } else {
        base.and(Frag::is_not_null("intake_issues.id"));
    }
    with_date_range(&mut base, "issues.created_at", filters, timezone);
    with_range(&mut base, "issues.created_at", filters);
    count(client, &base).await
}

/// `WorkspaceMember.objects.filter(workspace__slug=slug, is_active=True, **date_filter).count()`
/// (`advance.py:194-196`) — note: no `member__is_bot` filter here.
async fn workspace_member_count(
    client: &deadpool_postgres::Client,
    slug: &str,
    filters: &AnalyticsFilters,
    timezone: Tz,
) -> Result<i64, ViewError> {
    let mut base = QueryBase::new("workspace_members");
    base.join(
        JoinKind::Inner,
        "workspaces",
        "workspaces",
        Frag::raw("workspace_members.workspace_id = workspaces.id"),
    );
    base.and(Frag::is_null("workspace_members.deleted_at"));
    base.and(Frag::raw("workspace_members.is_active"));
    base.and(Frag::compare(
        "workspaces.slug",
        "=",
        Value::Text(slug.to_owned()),
    ));
    with_date_range(&mut base, "workspace_members.created_at", filters, timezone);
    let rows = sql::fetch(client, &base.statement(Frag::raw("COUNT(*)"), "")).await?;
    Ok(rows.first().map(|row| row.get(0)).unwrap_or(0))
}

/// The four member counts of `get_overview_data` (`advance.py:66-84`).
#[derive(Debug, Clone, Copy)]
struct MemberCounts {
    total: i64,
    admins: i64,
    members: i64,
    guests: i64,
}

async fn member_counts(
    client: &deadpool_postgres::Client,
    slug: &str,
    filters: &AnalyticsFilters,
    member_id: Uuid,
) -> Result<MemberCounts, ViewError> {
    let total = member_count(client, slug, filters, member_id, None).await?;
    let admins = member_count(client, slug, filters, member_id, Some(20)).await?;
    let members = member_count(client, slug, filters, member_id, Some(15)).await?;
    let guests = member_count(client, slug, filters, member_id, Some(5)).await?;
    Ok(MemberCounts {
        total,
        admins,
        members,
        guests,
    })
}

async fn member_count(
    client: &deadpool_postgres::Client,
    slug: &str,
    filters: &AnalyticsFilters,
    member_id: Uuid,
    role: Option<i32>,
) -> Result<i64, ViewError> {
    let mut base;
    let table;
    if filters.project_ids.is_some() {
        // `ProjectMember.objects.filter(project_id__in=projects.values("id"),
        // is_active=True, member__is_bot=False)` (`advance.py:76-78`).
        table = "project_members";
        base = QueryBase::new(table);
        base.join(
            JoinKind::Inner,
            "users",
            "users",
            Frag::raw("project_members.member_id = users.id"),
        );
        base.and(Frag::is_null("project_members.deleted_at"));
        base.and(Frag::raw("project_members.is_active"));
        base.and(Frag::raw("NOT users.is_bot"));
        let mut subquery = Frag::raw(
            "project_members.project_id IN (SELECT U0.id FROM projects U0 \
             INNER JOIN project_members U1 ON (U0.id = U1.project_id) \
             INNER JOIN workspaces U3 ON (U0.workspace_id = U3.id) \
             WHERE U0.deleted_at IS NULL AND U0.archived_at IS NULL \
               AND U0.deleted_at IS NULL",
        );
        if let Some(ids) = &filters.project_ids {
            subquery.push_raw(" AND ");
            subquery.push(Frag::in_uuids("U0.id", ids));
        }
        subquery.push_raw(" AND U1.deleted_at IS NULL AND U1.is_active AND U1.member_id = ");
        subquery.push(Frag::bind(Value::Uuid(member_id)));
        subquery.push_raw(" AND U3.slug = ");
        subquery.push(Frag::bind(Value::Text(slug.to_owned())));
        subquery.push_raw(")");
        base.and(subquery);
    } else {
        // `WorkspaceMember.objects.filter(workspace__slug=slug, is_active=True,
        // member__is_bot=False)` (`advance.py:67-69`).
        table = "workspace_members";
        base = QueryBase::new(table);
        base.join(
            JoinKind::Inner,
            "users",
            "users",
            Frag::raw("workspace_members.member_id = users.id"),
        );
        base.join(
            JoinKind::Inner,
            "workspaces",
            "workspaces",
            Frag::raw("workspace_members.workspace_id = workspaces.id"),
        );
        base.and(Frag::is_null("workspace_members.deleted_at"));
        base.and(Frag::raw("workspace_members.is_active"));
        base.and(Frag::raw("NOT users.is_bot"));
        base.and(Frag::compare(
            "workspaces.slug",
            "=",
            Value::Text(slug.to_owned()),
        ));
    }
    if let Some(role) = role {
        // Django emits the role as a literal; the column is `int2`, which a
        // bound `int4` cannot be compared with.
        base.and(Frag::raw(format!("{table}.role = {role}")));
    }
    if let Some((gte, lte)) = filters.analytics_date_range {
        let mut predicate = Frag::raw(table);
        predicate.push_raw(".created_at >= ");
        predicate.push(Frag::bind(Value::Timestamp(gte)));
        predicate.push_raw(" AND ");
        predicate.push_raw(table);
        predicate.push_raw(".created_at <= ");
        predicate.push(Frag::bind(Value::Timestamp(lte)));
        base.and(predicate);
    }
    let rows = sql::fetch(client, &base.statement(Frag::raw("COUNT(*)"), "")).await?;
    Ok(rows.first().map(|row| row.get(0)).unwrap_or(0))
}

/// `Project.objects.filter(**project_filters).count()` (`advance.py:85`).
pub(crate) async fn project_count(
    client: &deadpool_postgres::Client,
    slug: &str,
    filters: &AnalyticsFilters,
    member_id: Uuid,
) -> Result<i64, ViewError> {
    let mut base = QueryBase::new("projects");
    base.join(
        JoinKind::Inner,
        "project_members",
        "project_members",
        Frag::raw("projects.id = project_members.project_id"),
    );
    base.join(
        JoinKind::Inner,
        "workspaces",
        "workspaces",
        Frag::raw("projects.workspace_id = workspaces.id"),
    );
    base.and(Frag::is_null("projects.deleted_at"));
    base.and(Frag::is_null("projects.archived_at"));
    base.and(Frag::is_null("projects.deleted_at"));
    if let Some(project_ids) = &filters.project_ids {
        base.and(Frag::in_uuids("projects.id", project_ids));
    }
    base.and(Frag::raw("project_members.is_active"));
    base.and(Frag::compare(
        "project_members.member_id",
        "=",
        Value::Uuid(member_id),
    ));
    base.and(Frag::compare(
        "workspaces.slug",
        "=",
        Value::Text(slug.to_owned()),
    ));
    if let Some((gte, lte)) = filters.analytics_date_range {
        let mut predicate = Frag::raw("projects.created_at >= ");
        predicate.push(Frag::bind(Value::Timestamp(gte)));
        predicate.push_raw(" AND projects.created_at <= ");
        predicate.push(Frag::bind(Value::Timestamp(lte)));
        base.and(predicate);
    }
    let rows = sql::fetch(client, &base.statement(Frag::raw("COUNT(*)"), "")).await?;
    Ok(rows.first().map(|row| row.get(0)).unwrap_or(0))
}

/// `{"count": value}` — the shape every count in the overview has.
fn count_body(value: i64) -> Json {
    Json::Object(vec![("count".to_owned(), Json::Int(value))])
}

/// `key.replace("_", " ").title()` (`advance.py:213`).
fn title(key: &str) -> String {
    key.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `AdvanceAnalyticsChartEndpoint.work_item_completion_chart`
/// (`advance.py:219-285`).
///
/// `start_date` is the workspace's creation month (UTC) unless a chart period
/// is active; the loop always ends at the *UTC* current month
/// (`timezone.now().date()`, `advance.py:258`). `TruncMonth` groups in the
/// caller's timezone, so the two can disagree and produce zero-filled months —
/// which is what the fixture shows for `date_filter=last_3_months`.
pub(crate) async fn workspace_completion_chart(
    client: &deadpool_postgres::Client,
    slug: &str,
    filters: &AnalyticsFilters,
    member_id: Uuid,
    timezone: Tz,
    now: DateTime<Utc>,
) -> Result<Json, ViewError> {
    let workspace = lookup::workspace_by_slug(client, slug).await?;
    let mut start_date = workspace
        .created_at
        .date_naive()
        .with_day(1)
        .ok_or_else(|| ViewError::Unknown("ValueError: day is out of range".to_owned()))?;

    let mut base = issue_base(slug, member_id, filters);
    if let Some((start, end)) = filters.chart_period_range {
        start_date = start;
        base.and(date_range_predicate(
            "issues.created_at",
            start,
            end,
            timezone,
        ));
    }

    let mut projection = Frag::raw("DATE_TRUNC('month', issues.created_at ");
    projection.push_raw(&sql::timezone_literal(timezone));
    projection.push_raw(
        ") AS month, COUNT(issues.id) AS created_count, \
         COUNT(issues.id) FILTER (WHERE states.group = 'completed') AS completed_count",
    );
    let statement = base.statement(projection, " GROUP BY 1 ORDER BY 1 ASC");
    let rows = sql::fetch(client, &statement).await?;

    let mut stats: Vec<(String, i64, i64)> = Vec::with_capacity(rows.len());
    for row in &rows {
        let month: chrono::NaiveDateTime = row.try_get(0).map_err(ViewError::from)?;
        stats.push((
            month.format("%Y-%m-%d").to_string(),
            row.try_get::<_, i64>(1).map_err(ViewError::from)?,
            row.try_get::<_, i64>(2).map_err(ViewError::from)?,
        ));
    }

    // `last_month = timezone.now().date().replace(day=1)`.
    let last_month = now
        .date_naive()
        .with_day(1)
        .ok_or_else(|| ViewError::Unknown("ValueError: day is out of range".to_owned()))?;
    let mut data = Vec::new();
    let mut current_month = start_date;
    while current_month <= last_month {
        let key = current_month.format("%Y-%m-%d").to_string();
        let (created, completed) = stats
            .iter()
            .find(|(existing, _, _)| *existing == key)
            .map(|(_, created, completed)| (*created, *completed))
            .unwrap_or((0, 0));
        data.push(Json::Object(vec![
            ("key".to_owned(), Json::Str(key.clone())),
            ("name".to_owned(), Json::Str(key)),
            ("count".to_owned(), Json::Int(created)),
            ("completed_issues".to_owned(), Json::Int(completed)),
            ("created_issues".to_owned(), Json::Int(created)),
        ]));
        current_month = next_month(current_month)?;
    }

    Ok(Json::Object(vec![
        ("data".to_owned(), Json::Array(data)),
        (
            "schema".to_owned(),
            Json::Object(vec![
                (
                    "completed_issues".to_owned(),
                    Json::Str("completed_issues".to_owned()),
                ),
                (
                    "created_issues".to_owned(),
                    Json::Str("created_issues".to_owned()),
                ),
            ]),
        ),
    ]))
}

/// `date.replace(month=month + 1)` / `.replace(year=year + 1, month=1)`.
///
/// A day that does not exist in the next month is a `ValueError` upstream, i.e.
/// Plane's family 500 (`advance.py:275-278`).
pub(crate) fn next_month(date: NaiveDate) -> Result<NaiveDate, ViewError> {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, date.day())
        .ok_or_else(|| ViewError::Unknown("ValueError: day is out of range for month".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_keys_are_title_cased_like_python() {
        assert_eq!(title("work_items"), "Work Items");
        assert_eq!(title("intake"), "Intake");
        assert_eq!(title("views"), "Views");
    }

    #[test]
    fn date_filters_map_to_the_python_ranges() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-16T05:00:00Z")
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        // `custom` is documented as inert: the view never passes the window.
        assert!(chart_period_range(Some("custom"), now).is_none());
        assert!(analytics_date_range(Some("custom"), chrono_tz::UTC, now).is_none());
        assert_eq!(
            chart_period_range(Some("last_3_months"), now),
            Some((
                chrono::NaiveDate::from_ymd_opt(2026, 8, 18).unwrap_or_default(),
                chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap_or_default()
            ))
        );
        let range = analytics_date_range(Some("yesterday"), chrono_tz::UTC, now)
            .expect("yesterday is a range");
        assert_eq!(range.0.to_rfc3339(), "2026-11-15T00:00:00+00:00");
        assert_eq!(range.1.to_rfc3339(), "2026-11-15T23:59:59.999999+00:00");
    }

    #[test]
    fn a_month_step_on_the_31st_overflows_like_python() {
        let january = chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap_or_default();
        assert!(next_month(january).is_err());
        let december = chrono::NaiveDate::from_ymd_opt(2026, 12, 1).unwrap_or_default();
        assert_eq!(
            next_month(december).ok(),
            chrono::NaiveDate::from_ymd_opt(2027, 1, 1)
        );
    }
}
