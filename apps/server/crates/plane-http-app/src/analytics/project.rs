// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/analytic/project_analytics.py` — the project-scoped
//! analytics endpoints.
//!
//! | Route | Python |
//! | --- | --- |
//! | `GET …/projects/{project_id}/advance-analytics/` | `ProjectAdvanceAnalyticsEndpoint.get` (`:84-94`) |
//! | `GET …/advance-analytics-stats/` | `ProjectAdvanceAnalyticsStatsEndpoint.get` (`:165-179`) |
//! | `GET …/advance-analytics-charts/` | `ProjectAdvanceAnalyticsChartEndpoint.get` (`:317-367`) |
//!
//! Two behaviours worth naming:
//!
//! * `advance-analytics-stats` ignores `date_filter` entirely: only
//!   `get_project_issues_stats` applies `chart_period_range`, and the `GET`
//!   calls `get_work_items_stats` (`:165-179`).
//! * the chart's `cycle_id`/`module_id` branches count the *link* rows
//!   (`CycleIssue.created_at`, `ModuleIssue.created_at`), not the issues', and
//!   the daily loop runs from the cycle's (or module's) own dates.
//!
//! Round trips: `advance-analytics` is 5 counts; `advance-analytics-stats` is 1
//! aggregate; `advance-analytics-charts` is one lookup (project, cycle or
//! module) plus one aggregate — or just the aggregate for
//! `type=custom-work-items`.

use chrono::{Datelike, Utc};
use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::model::Relation;
use plane_query::json::Json;
use plane_query::sql::{Frag, JoinKind, Value};
use uuid::Uuid;

use crate::analytics::advance::{
    AnalyticsFilters, analytics_filters, date_range_predicate, issue_base, next_month, with_range,
};
use crate::analytics::base::{count, message_body, read_text, read_uuid, rendered, user_timezone};
use crate::analytics::chart::build_analytics_chart;
use crate::analytics::queries;
use crate::analytics::sql::{self, QueryBase};
use crate::common::{client, jobs, lookup};

/// `ProjectAdvanceAnalyticsEndpoint.get` (`project_analytics.py:84-94`).
pub fn advance_analytics<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;
        let now = Utc::now();
        let filters = analytics_filters(ctx, "analytics", now)?;
        let (cycle_id, module_id) = cycle_or_module(ctx)?;

        let base = || work_items_base(project_id, cycle_id, module_id, slug, member_id, &filters);
        let entries = vec![
            (
                "total_work_items".to_owned(),
                count_body(count_with_range(client, &base(), &filters).await?),
            ),
            (
                "started_work_items".to_owned(),
                count_body(count_group(client, &base(), &filters, "started").await?),
            ),
            (
                "backlog_work_items".to_owned(),
                count_body(count_group(client, &base(), &filters, "backlog").await?),
            ),
            (
                "un_started_work_items".to_owned(),
                count_body(count_group(client, &base(), &filters, "unstarted").await?),
            ),
            (
                "completed_work_items".to_owned(),
                count_body(count_group(client, &base(), &filters, "completed").await?),
            ),
        ];
        Ok(rendered(&Json::Object(entries)))
    })
}

/// `ProjectAdvanceAnalyticsStatsEndpoint.get` (`project_analytics.py:165-179`).
pub fn advance_analytics_stats<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;
        let now = Utc::now();
        // `initialize_workspace(slug, type="chart")` — but the stats method it
        // calls never reads `chart_period_range`.
        let filters = analytics_filters(ctx, "chart", now)?;
        if ctx.query().get_or("type", "work-items") != "work-items" {
            return Ok(message_body(400, "Invalid type"));
        }
        let (cycle_id, module_id) = cycle_or_module(ctx)?;
        let mut base = work_items_base(project_id, cycle_id, module_id, slug, member_id, &filters);
        // `annotate(display_name=F("assignees__display_name"))` and friends,
        // then `.values(...)` with `distinct=True` counts (`:133-163`).
        base.require_relation(Relation::IssueAssignees, JoinKind::Left);
        sql::require_assignee_users(&mut base, JoinKind::Left);
        let projection = format!(
            "assignee_user.display_name AS display_name, \
             issue_assignees.assignee_id AS assignee_id, \
             {} AS avatar_url, \
             COUNT(DISTINCT issues.id) FILTER (WHERE states.group = 'cancelled') AS cancelled_work_items, \
             COUNT(DISTINCT issues.id) FILTER (WHERE states.group = 'completed') AS completed_work_items, \
             COUNT(DISTINCT issues.id) FILTER (WHERE states.group = 'backlog') AS backlog_work_items, \
             COUNT(DISTINCT issues.id) FILTER (WHERE states.group = 'unstarted') AS un_started_work_items, \
             COUNT(DISTINCT issues.id) FILTER (WHERE states.group = 'started') AS started_work_items",
            sql::avatar_url("assignee_user")
        );
        let statement = base.statement(Frag::raw(projection), " GROUP BY 1, 2, 3 ORDER BY 1 ASC");
        let rows = sql::fetch(client, &statement).await?;
        let mut items = Vec::with_capacity(rows.len());
        for row in &rows {
            items.push(Json::Object(vec![
                ("display_name".to_owned(), read_text(row, 0)?),
                ("assignee_id".to_owned(), read_uuid(row, 1)?),
                ("avatar_url".to_owned(), read_text(row, 2)?),
                ("cancelled_work_items".to_owned(), read_int(row, 3)?),
                ("completed_work_items".to_owned(), read_int(row, 4)?),
                ("backlog_work_items".to_owned(), read_int(row, 5)?),
                ("un_started_work_items".to_owned(), read_int(row, 6)?),
                ("started_work_items".to_owned(), read_int(row, 7)?),
            ]));
        }
        Ok(rendered(&Json::Array(items)))
    })
}

/// `ProjectAdvanceAnalyticsChartEndpoint.get` (`project_analytics.py:317-367`).
pub fn advance_analytics_chart<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let timezone = user_timezone(ctx);
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;
        let now = Utc::now();
        let filters = analytics_filters(ctx, "chart", now)?;
        let kind = ctx.query().get_or("type", "projects");
        let group_by = ctx
            .query()
            .get("group_by")
            .filter(|value| !value.is_empty());
        let x_axis = ctx.query().get_or("x_axis", "PRIORITY");
        let (cycle_id, module_id) = cycle_or_module(ctx)?;

        let body = match kind {
            "custom-work-items" => {
                let mut base = issue_base(slug, member_id, &filters);
                base.and(Frag::compare(
                    "issues.project_id",
                    "=",
                    Value::Uuid(project_id),
                ));
                if let Some(cycle_id) = cycle_id {
                    base.and(link_subquery(
                        LinkTable::Cycle,
                        cycle_id,
                        slug,
                        member_id,
                        &filters,
                    ));
                } else if let Some(module_id) = module_id {
                    base.and(link_subquery(
                        LinkTable::Module,
                        module_id,
                        slug,
                        member_id,
                        &filters,
                    ));
                }
                if let Some((start, end)) = filters.chart_period_range {
                    base.and(date_range_predicate(
                        "issues.created_at",
                        start,
                        end,
                        timezone,
                    ));
                }
                build_analytics_chart(client, base, x_axis, group_by, timezone).await?
            }
            "work-items" => {
                completion_chart(
                    client, project_id, cycle_id, module_id, slug, member_id, &filters, timezone,
                    now,
                )
                .await?
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

fn count_body(value: i64) -> Json {
    Json::Object(vec![("count".to_owned(), Json::Int(value))])
}

/// `request.GET.get("cycle_id"|"module_id", None)`.
///
/// An empty value is *present*, so the Python takes the branch and the UUID
/// lookup rejects `""` — `400 {"error": "Please provide valid detail"}`.
fn cycle_or_module(ctx: &ViewCtx<'_>) -> Result<(Option<Uuid>, Option<Uuid>), ViewError> {
    let parse = |key: &str| -> Result<Option<Uuid>, ViewError> {
        match ctx.query().get(key) {
            Some(raw) => Ok(Some(
                Uuid::parse_str(raw).map_err(|_| ViewError::Validation)?,
            )),
            None => Ok(None),
        }
    };
    Ok((parse("cycle_id")?, parse("module_id")?))
}

/// `get_work_items_stats`' `base_queryset` (`project_analytics.py:58-74`).
fn work_items_base(
    project_id: Uuid,
    cycle_id: Option<Uuid>,
    module_id: Option<Uuid>,
    slug: &str,
    member_id: Uuid,
    filters: &AnalyticsFilters,
) -> QueryBase {
    if let Some(cycle_id) = cycle_id {
        // `Issue.issue_objects.filter(id__in=cycle_issues)` — the base filters
        // live inside the subquery (`:63-67`).
        let mut base = QueryBase::issue_objects();
        base.and(link_subquery(
            LinkTable::Cycle,
            cycle_id,
            slug,
            member_id,
            filters,
        ));
        return base;
    }
    if let Some(module_id) = module_id {
        let mut base = QueryBase::issue_objects();
        base.and(link_subquery(
            LinkTable::Module,
            module_id,
            slug,
            member_id,
            filters,
        ));
        return base;
    }
    let mut base = issue_base(slug, member_id, filters);
    base.and(Frag::compare(
        "issues.project_id",
        "=",
        Value::Uuid(project_id),
    ));
    base
}

/// The two m2m through tables the project endpoints read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkTable {
    Cycle,
    Module,
}

impl LinkTable {
    fn table(self) -> &'static str {
        match self {
            Self::Cycle => "cycle_issues",
            Self::Module => "module_issues",
        }
    }

    fn id_column(self) -> &'static str {
        match self {
            Self::Cycle => "cycle_issues.cycle_id",
            Self::Module => "module_issues.module_id",
        }
    }
}

/// `CycleIssue.objects.filter(**base_filters, cycle_id=…).values_list("issue_id")`
/// as an `issues.id IN (…)` predicate (`project_analytics.py:64-67`).
fn link_subquery(
    link: LinkTable,
    id: Uuid,
    slug: &str,
    member_id: Uuid,
    filters: &AnalyticsFilters,
) -> Frag {
    let table = link.table();
    let mut frag = Frag::raw("issues.id IN (SELECT ");
    frag.push_raw(table);
    frag.push_raw(".issue_id FROM ");
    frag.push_raw(table);
    frag.push_raw(" INNER JOIN projects ON (");
    frag.push_raw(table);
    frag.push_raw(".project_id = projects.id) INNER JOIN project_members ON (projects.id = project_members.project_id) \
         INNER JOIN workspaces ON (");
    frag.push_raw(table);
    frag.push_raw(".workspace_id = workspaces.id) WHERE ");
    frag.push_raw(table);
    frag.push_raw(".deleted_at IS NULL AND ");
    frag.push_raw(link.id_column());
    frag.push_raw(" = ");
    frag.push(Frag::bind(Value::Uuid(id)));
    frag.push_raw(" AND projects.archived_at IS NULL AND projects.deleted_at IS NULL");
    frag.push_raw(" AND project_members.is_active AND project_members.member_id = ");
    frag.push(Frag::bind(Value::Uuid(member_id)));
    if let Some(project_ids) = &filters.project_ids {
        frag.push_raw(" AND ");
        frag.push(Frag::in_uuids(&format!("{table}.project_id"), project_ids));
    }
    frag.push_raw(" AND workspaces.slug = ");
    frag.push(Frag::bind(Value::Text(slug.to_owned())));
    frag.push_raw(")");
    frag
}

/// `get_filtered_counts(base_queryset)` (`project_analytics.py:45-56`).
async fn count_with_range(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
    filters: &AnalyticsFilters,
) -> Result<i64, ViewError> {
    let mut base = base.clone();
    with_range(&mut base, "issues.created_at", filters);
    count(client, &base).await
}

async fn count_group(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
    filters: &AnalyticsFilters,
    group: &str,
) -> Result<i64, ViewError> {
    let mut base = base.clone();
    base.require_relation(Relation::States, JoinKind::Inner);
    base.state_groups(&[group]);
    with_range(&mut base, "issues.created_at", filters);
    count(client, &base).await
}

/// `work_item_completion_chart` (`project_analytics.py:182-315`).
#[allow(clippy::too_many_arguments)]
async fn completion_chart(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    cycle_id: Option<Uuid>,
    module_id: Option<Uuid>,
    slug: &str,
    member_id: Uuid,
    filters: &AnalyticsFilters,
    timezone: Tz,
    now: chrono::DateTime<Utc>,
) -> Result<Json, ViewError> {
    let mut base = issue_base(slug, member_id, filters);
    base.and(Frag::compare(
        "issues.project_id",
        "=",
        Value::Uuid(project_id),
    ));

    if let Some(cycle_id) = cycle_id {
        let statement = client
            .prepare_cached(queries::CYCLE_WINDOW.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_opt(&statement, &[&cycle_id])
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Ok(empty_chart());
        };
        let start_date: Option<chrono::DateTime<Utc>> = row.try_get(0).map_err(ViewError::from)?;
        let end_date: Option<chrono::DateTime<Utc>> = row.try_get(1).map_err(ViewError::from)?;
        if start_date.is_none() {
            return Ok(empty_chart());
        }
        // `cycle.end_date.date()` on a NULL end date is an `AttributeError`.
        let (Some(start), Some(end)) = (start_date, end_date) else {
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'date'".to_owned(),
            ));
        };
        let (start, end) = (start.date_naive(), end.date_naive());
        let stats = daily_stats(
            client,
            LinkTable::Cycle,
            cycle_id,
            slug,
            member_id,
            filters,
            timezone,
        )
        .await?;
        return Ok(daily_chart(&stats, start, end));
    }
    if let Some(module_id) = module_id {
        let statement = client
            .prepare_cached(queries::MODULE_WINDOW.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_opt(&statement, &[&module_id])
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Ok(empty_chart());
        };
        let start_date: Option<chrono::NaiveDate> = row.try_get(0).map_err(ViewError::from)?;
        let target_date: Option<chrono::NaiveDate> = row.try_get(1).map_err(ViewError::from)?;
        if start_date.is_none() {
            return Ok(empty_chart());
        }
        let (Some(start), Some(end)) = (start_date, target_date) else {
            return Err(ViewError::Unknown(
                "TypeError: '<=' not supported between instances of 'datetime.date' and 'NoneType'"
                    .to_owned(),
            ));
        };
        let stats = daily_stats(
            client,
            LinkTable::Module,
            module_id,
            slug,
            member_id,
            filters,
            timezone,
        )
        .await?;
        return Ok(daily_chart(&stats, start, end));
    }

    // `project = Project.objects.filter(id=project_id).first()`.
    let statement = client
        .prepare_cached(queries::PROJECT_CREATED_AT.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await
        .map_err(ViewError::from)?;
    let Some(row) = row else {
        return Ok(empty_chart());
    };
    let created_at: chrono::DateTime<Utc> = row.try_get(0).map_err(ViewError::from)?;
    let mut start_date = created_at
        .date_naive()
        .with_day(1)
        .ok_or_else(|| ViewError::Unknown("ValueError: day is out of range".to_owned()))?;

    if let Some((start, end)) = filters.chart_period_range {
        start_date = start;
        base.and(date_range_predicate(
            "issues.created_at",
            start,
            end,
            timezone,
        ));
    }
    let stats = monthly_stats(client, base, timezone).await?;
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
    Ok(chart_body(data))
}

/// The daily rows of the cycle/module branch.
type Stats = Vec<(String, i64, i64)>;

async fn daily_stats(
    client: &deadpool_postgres::Client,
    link: LinkTable,
    id: Uuid,
    slug: &str,
    member_id: Uuid,
    filters: &AnalyticsFilters,
    timezone: Tz,
) -> Result<Stats, ViewError> {
    let table = link.table();
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
    // `Count("id", filter=Q(issue__state__group="completed"))` joins the issue
    // and its state (`project_analytics.py:229`).
    base.join(
        JoinKind::Inner,
        "issues",
        "issues",
        Frag::raw(format!("{table}.issue_id = issues.id")),
    );
    base.require_relation(Relation::States, JoinKind::Left);
    base.and(Frag::is_null(&format!("{table}.deleted_at")));
    base.and(Frag::compare(link.id_column(), "=", Value::Uuid(id)));
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

    let mut projection = Frag::raw("(");
    projection.push_raw(table);
    projection.push_raw(".created_at ");
    projection.push_raw(&sql::timezone_literal(timezone));
    projection.push_raw(")::date AS created_at__date, COUNT(");
    projection.push_raw(table);
    projection.push_raw(".id) AS created_count, COUNT(");
    projection.push_raw(table);
    projection.push_raw(".id) FILTER (WHERE states.group = 'completed') AS completed_count");
    let statement = base.statement(projection, " GROUP BY 1 ORDER BY 1 ASC");
    let rows = sql::fetch(client, &statement).await?;
    let mut stats = Vec::with_capacity(rows.len());
    for row in &rows {
        let date: chrono::NaiveDate = row.try_get(0).map_err(ViewError::from)?;
        stats.push((
            date.format("%Y-%m-%d").to_string(),
            row.try_get::<_, i64>(1).map_err(ViewError::from)?,
            row.try_get::<_, i64>(2).map_err(ViewError::from)?,
        ));
    }
    Ok(stats)
}

/// `annotate(month=TruncMonth("created_at")).values("month")…`
/// (`project_analytics.py:266-274`).
async fn monthly_stats(
    client: &deadpool_postgres::Client,
    base: QueryBase,
    timezone: Tz,
) -> Result<Stats, ViewError> {
    let mut projection = Frag::raw("DATE_TRUNC('month', issues.created_at ");
    projection.push_raw(&sql::timezone_literal(timezone));
    projection.push_raw(
        ") AS month, COUNT(issues.id) AS created_count, \
         COUNT(issues.id) FILTER (WHERE states.group = 'completed') AS completed_count",
    );
    let statement = base.statement(projection, " GROUP BY 1 ORDER BY 1 ASC");
    let rows = sql::fetch(client, &statement).await?;
    let mut stats = Vec::with_capacity(rows.len());
    for row in &rows {
        let month: chrono::NaiveDateTime = row.try_get(0).map_err(ViewError::from)?;
        stats.push((
            month.format("%Y-%m-%d").to_string(),
            row.try_get::<_, i64>(1).map_err(ViewError::from)?,
            row.try_get::<_, i64>(2).map_err(ViewError::from)?,
        ));
    }
    Ok(stats)
}

/// The daily loop of `project_analytics.py:244-258`.
fn daily_chart(stats: &Stats, start: chrono::NaiveDate, end: chrono::NaiveDate) -> Json {
    let mut data = Vec::new();
    let mut current = start;
    while current <= end {
        let key = current.format("%Y-%m-%d").to_string();
        let (created, completed) = stats
            .iter()
            .find(|(existing, _, _)| *existing == key)
            .map(|(_, created, completed)| (*created, *completed))
            .unwrap_or((0, 0));
        data.push(Json::Object(vec![
            ("key".to_owned(), Json::Str(key.clone())),
            ("name".to_owned(), Json::Str(key)),
            ("count".to_owned(), Json::Int(created + completed)),
            ("completed_issues".to_owned(), Json::Int(completed)),
            ("created_issues".to_owned(), Json::Int(created)),
        ]));
        current += chrono::Duration::days(1);
    }
    chart_body(data)
}

/// `{"data": [...], "schema": {"completed_issues": …, "created_issues": …}}`.
fn chart_body(data: Vec<Json>) -> Json {
    Json::Object(vec![
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
    ])
}

/// `{"data": [], "schema": {}}` (`project_analytics.py:201,213,221`).
fn empty_chart() -> Json {
    Json::Object(vec![
        ("data".to_owned(), Json::Array(Vec::new())),
        ("schema".to_owned(), Json::Object(Vec::new())),
    ])
}
