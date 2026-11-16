// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two *paginated* issue lists that serialize every row by hand:
//!
//! * `GET /api/workspaces/{slug}/projects/{project_id}/issues-detail/` —
//!   `IssueDetailEndpoint.get` (`plane/app/views/issue/base.py:968-1088`), whose
//!   results are `IssueListDetailSerializer` rows;
//! * `GET /api/workspaces/{slug}/issues/` — `WorkspaceViewIssuesViewSet.list`
//!   (`plane/app/views/view/base.py:173-288`), whose results are
//!   `ViewIssueListSerializer` rows scoped by the caller's project memberships.
//!
//! Both use `OffsetPaginator`: `total_count` is the *unannotated* filtered
//! queryset's `count()` (so a relation filter can make it larger than the
//! number of issues), `count` is the page's row count, `next_page_results` is
//! "the slice held `per_page + 1` rows", and the ordering is
//! `key ASC|DESC NULLS LAST, created_at DESC`.
//!
//! The three id arrays come from the prefetched through-table managers, which
//! are ordered by `-created_at` (`IssueLabel`/`IssueAssignee`/`ModuleIssue`
//! `Meta.ordering`), *not* the ascending `ARRAY_AGG(DISTINCT)` order the values
//! projection uses. A correlated `ARRAY(SELECT … ORDER BY created_at DESC)`
//! reproduces it in the page query, so this route is two statements where
//! Django runs the page, three prefetches, the count and the page-count
//! (`exists`).

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use http::Method;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::model::{ValueField, manager_predicate};
use plane_query::issue::query::IssueScope;
use plane_query::json::Json;
use plane_query::paginator::{Cursor, MAX_LIMIT, PageQuery};
use plane_query::sql::{Frag, Select, Value};
use uuid::Uuid;

use super::list_common::{
    ListRequest, build_issue_query, client, kwarg_text, kwarg_uuid, project_with_membership,
    rendered, user_id, user_timezone,
};

/// Register both routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.base.IssueDetailEndpoint")?
        .method(Method::GET, detail_endpoint)?;
    registry
        .viewset("plane.app.views.view.base.WorkspaceViewIssuesViewSet")?
        .action("list", workspace_issues)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared statement pieces
// ---------------------------------------------------------------------------

/// The three through-table arrays, in the prefetched managers' order.
fn arrays() -> Vec<(&'static str, String)> {
    vec![
        (
            "label_ids",
            "ARRAY(SELECT issue_labels.label_id FROM issue_labels \
              WHERE issue_labels.deleted_at IS NULL AND issue_labels.issue_id = issues.id \
              ORDER BY issue_labels.created_at DESC)"
                .to_owned(),
        ),
        (
            "assignee_ids",
            "ARRAY(SELECT issue_assignees.assignee_id FROM issue_assignees \
              WHERE issue_assignees.deleted_at IS NULL \
                AND issue_assignees.issue_id = issues.id \
              ORDER BY issue_assignees.created_at DESC)"
                .to_owned(),
        ),
        (
            "module_ids",
            "ARRAY(SELECT module_issues.module_id FROM module_issues \
              WHERE module_issues.deleted_at IS NULL \
                AND module_issues.issue_id = issues.id \
              ORDER BY module_issues.created_at DESC)"
                .to_owned(),
        ),
    ]
}

const CYCLE_ID: &str = "(SELECT cycle_issues.cycle_id FROM cycle_issues \
     WHERE cycle_issues.deleted_at IS NULL AND cycle_issues.issue_id = issues.id \
     ORDER BY cycle_issues.created_at DESC LIMIT 1)";

const SUB_ISSUES_COUNT: &str = "(SELECT COUNT(sub_issues.id) FROM issues sub_issues \
     LEFT OUTER JOIN states sub_states ON sub_issues.state_id = sub_states.id \
     INNER JOIN projects sub_projects ON sub_issues.project_id = sub_projects.id \
     WHERE sub_issues.deleted_at IS NULL \
       AND (sub_states.group IS NULL OR sub_states.group <> 'triage') \
       AND sub_issues.archived_at IS NULL AND sub_projects.archived_at IS NULL \
       AND NOT sub_issues.is_draft AND sub_issues.parent_id = issues.id)";

const LINK_COUNT: &str = "(SELECT COUNT(issue_links.id) FROM issue_links \
     WHERE issue_links.deleted_at IS NULL AND issue_links.issue_id = issues.id)";

const ATTACHMENT_COUNT: &str = "(SELECT COUNT(file_assets.id) FROM file_assets \
     WHERE file_assets.deleted_at IS NULL \
       AND file_assets.entity_type = 'ISSUE_ATTACHMENT' \
       AND file_assets.issue_id = issues.id)";

const BASE_COLUMNS: &str = "issues.id, issues.name, issues.state_id, issues.sort_order, \
     issues.completed_at, issues.estimate_point_id, issues.priority, issues.start_date, \
     issues.target_date, issues.sequence_id, issues.project_id, issues.parent_id, \
     issues.created_at, issues.updated_at, issues.created_by_id, issues.updated_by_id, \
     issues.is_draft, issues.archived_at";

// ---------------------------------------------------------------------------
// IssueDetailEndpoint
// ---------------------------------------------------------------------------

/// `IssueDetailEndpoint.get`.
fn detail_endpoint<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = user_timezone(ctx);

        let project = project_with_membership(client, slug, project_id, user).await?;
        let created_by = if project.is_guest() && !project.guest_view_all_features {
            Some(user)
        } else {
            None
        };

        let request = ListRequest {
            query: ctx.query(),
            scope: IssueScope {
                workspace_slug: slug.to_owned(),
                project_id: Some(project_id),
                // `IssueDetailEndpoint` never applies `.distinct()`.
                distinct: false,
                created_by,
            },
            timezone,
            now: plane_query::global::now(),
            extra: Frag::empty(),
            options: plane_query::ListOptions {
                // Neither of these two views calls `issue_queryset_grouper`:
                // `?group_by=` is ignored, so the through-table join it would
                // add must not reach the page.
                grouping: false,
                ..plane_query::ListOptions::issue_viewset()
            },
        };
        let query = match build_issue_query(&request) {
            Ok(query) => query,
            Err(error) => {
                return match super::list_common::query_error_response(&error) {
                    Some(response) => Ok(response),
                    None => Err(ViewError::Unknown(error.message())),
                };
            }
        };
        let page = match PageQuery::parse(ctx.query(), None, None) {
            Ok(page) => page,
            Err(error) => return super::list_common::map_query_error(error),
        };

        let expand = ctx.expand().unwrap_or_default();

        let body = page_serialized(
            client,
            &query,
            &page,
            Serializer::IssueListDetail,
            &request.options,
            &expand,
            timezone,
        )
        .await?;
        Ok(rendered(200, &body))
    })
}

// ---------------------------------------------------------------------------
// WorkspaceViewIssuesViewSet.list
// ---------------------------------------------------------------------------

/// `WorkspaceViewIssuesViewSet.list`.
fn workspace_issues<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = user_timezone(ctx);

        let request = ListRequest {
            query: ctx.query(),
            scope: IssueScope {
                workspace_slug: slug.to_owned(),
                project_id: None,
                // `get_queryset()` has no `.distinct()`.
                distinct: false,
                created_by: None,
            },
            timezone,
            now: plane_query::global::now(),
            extra: Frag::empty(),
            options: plane_query::ListOptions {
                // Neither of these two views calls `issue_queryset_grouper`:
                // `?group_by=` is ignored, so the through-table join it would
                // add must not reach the page.
                grouping: false,
                // `WorkspaceViewIssuesViewSet.get_queryset` is
                // `Issue.issue_objects.filter(workspace__slug=…)` plus
                // `_get_project_permission_filters()`
                // (`plane/app/views/view/base.py:177-248`).
                scope: Some(workspace_scope(user)),
                ..plane_query::ListOptions::issue_viewset()
            },
        };
        let query = match build_issue_query(&request) {
            Ok(query) => query,
            Err(error) => {
                return match super::list_common::query_error_response(&error) {
                    Some(response) => Ok(response),
                    None => Err(ViewError::Unknown(error.message())),
                };
            }
        };
        let page = match PageQuery::parse(ctx.query(), None, None) {
            Ok(page) => page,
            Err(error) => return super::list_common::map_query_error(error),
        };

        let body = page_serialized(
            client,
            &query,
            &page,
            Serializer::ViewIssueList,
            &request.options,
            &[],
            timezone,
        )
        .await?;
        Ok(rendered(200, &body))
    })
}

/// `_get_project_permission_filters` (`plane/app/views/view/base.py:177-197`):
/// an `INNER JOIN project_members` with the caller's membership row and one of
/// three role branches. The join is what can duplicate a row when a
/// (project, member) pair has more than one matching row, exactly as Django's
/// join does — it has no `deleted_at` condition, so neither does this.
fn workspace_scope(user: Uuid) -> plane_query::issue::query::IssueScopeFilter {
    let mut predicate = Frag::raw(
        "(((projects.guest_view_all_features AND project_members.role = 5) \
         OR (issues.created_by_id = ",
    );
    predicate.push(Frag::bind(Value::Uuid(user)));
    predicate.push_raw(
        " AND NOT projects.guest_view_all_features AND project_members.role = 5) \
         OR project_members.role > 5) \
         AND project_members.is_active AND project_members.member_id = ",
    );
    predicate.push(Frag::bind(Value::Uuid(user)));
    predicate.push_raw(")");
    plane_query::issue::query::IssueScopeFilter::new(manager_predicate().and(predicate)).with_join(
        plane_query::issue::model::Relation::ProjectMembers,
        plane_query::sql::JoinKind::Inner,
    )
}

#[derive(Clone, Copy)]
enum Serializer {
    /// `IssueListDetailSerializer` (issues-detail).
    IssueListDetail,
    /// `ViewIssueListSerializer` (workspace issues).
    ViewIssueList,
}

/// One page plus its `OffsetPaginator` envelope.
#[allow(clippy::too_many_arguments)]
async fn page_serialized(
    client: &Client,
    query: &plane_query::issue::query::IssueQuery,
    page: &PageQuery,
    serializer: Serializer,
    options: &plane_query::ListOptions,
    expand: &[&str],
    timezone: chrono_tz::Tz,
) -> Result<Json, ViewError> {
    let limit = i64::from(page.per_page.min(MAX_LIMIT));
    let page_index = page.cursor.offset;
    let offset = page_index * limit;
    let scope = options.scope.as_ref();

    // The page: the filtered queryset plus the paginator's ordering.
    let mut select = query.filtered_select_with(scope);
    select.projection = projection(serializer);
    // `OffsetPaginator.get_result` re-applies the key with `NULLS LAST` and
    // `-created_at` as the tiebreak.
    if let Serializer::ViewIssueList = serializer {
        select.order_by = query.ordering.paginator_order();
    } else {
        select.order_by = query.ordering.paginator_order();
    }
    if query.ordering.group_by_issue {
        select.group_by = vec!["issues.id".to_owned()];
    }
    select.limit = Some(limit + 1);
    select.offset = Some(offset);
    let rows = fetch(client, &select).await?;
    let has_results = rows.len() as i64 > limit;
    let rows: Vec<&tokio_postgres::Row> = rows.iter().take(limit as usize).collect();

    // `total_count = total_count_queryset.count()`: the *unannotated* filtered
    // queryset, before the serializer's annotations and without `DISTINCT`.
    let count_select = query.filtered_base_with(scope);
    let total = count(client, &count_select).await?;

    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(match serializer {
            Serializer::IssueListDetail => issue_list_detail_row(row, timezone)?,
            Serializer::ViewIssueList => view_issue_list_row(row, timezone)?,
        });
    }
    if let Serializer::IssueListDetail = serializer {
        add_expand_relations(client, &rows, expand, &mut results, timezone).await?;
    }

    let count = results.len() as i64;
    let total_pages = if limit == 0 {
        0
    } else {
        (total + limit - 1) / limit
    };
    Ok(envelope(
        total,
        Cursor::new(limit, page_index + 1, false, has_results),
        Cursor::new(limit, page_index - 1, true, page_index > 0),
        count,
        total_pages,
        Json::Array(results),
    ))
}

/// The projection of one serializer, in its `to_representation` key order.
fn projection(serializer: Serializer) -> Vec<Frag> {
    let mut projection: Vec<Frag> = Vec::new();
    for column in BASE_COLUMNS.split(',') {
        projection.push(Frag::raw(column.trim().to_owned()));
    }
    projection.push(Frag::raw(format!("{CYCLE_ID} AS cycle_id")));
    match serializer {
        Serializer::IssueListDetail => {
            let mut entries = arrays();
            for (key, sql) in entries.drain(..) {
                projection.push(Frag::raw(format!("{sql} AS {key}")));
            }
            projection.push(Frag::raw(format!("{SUB_ISSUES_COUNT} AS sub_issues_count")));
            projection.push(Frag::raw(format!("{ATTACHMENT_COUNT} AS attachment_count")));
            projection.push(Frag::raw(format!("{LINK_COUNT} AS link_count")));
        }
        Serializer::ViewIssueList => {
            projection.push(Frag::raw(format!("{SUB_ISSUES_COUNT} AS sub_issues_count")));
            projection.push(Frag::raw("states.group AS state__group"));
            let mut entries = arrays();
            for (key, sql) in entries.drain(..) {
                projection.push(Frag::raw(format!("{sql} AS {key}")));
            }
            projection.push(Frag::raw(format!("{ATTACHMENT_COUNT} AS attachment_count")));
            projection.push(Frag::raw(format!("{LINK_COUNT} AS link_count")));
        }
    }
    projection
}

/// `IssueListDetailSerializer.to_representation` (`plane/app/serializers/issue.py:851-880`).
fn issue_list_detail_row(
    row: &tokio_postgres::Row,
    _timezone: chrono_tz::Tz,
) -> Result<Json, ViewError> {
    let labels: Vec<Uuid> = row.get(19);
    let assignees: Vec<Uuid> = row.get(20);
    let modules: Vec<Uuid> = row.get(21);
    Ok(Json::object(vec![
        ("id", Json::uuid(row.get(0))),
        ("name", Json::string(row.get::<_, String>(1))),
        ("state_id", Json::opt_uuid(row.get(2))),
        ("sort_order", Json::Float(row.get(3))),
        ("completed_at", Json::opt_datetime(row.get(4))),
        ("estimate_point", Json::opt_uuid(row.get(5))),
        ("priority", Json::string(row.get::<_, String>(6))),
        ("start_date", Json::opt_date(row.get(7))),
        ("target_date", Json::opt_date(row.get(8))),
        ("sequence_id", Json::Int(i64::from(row.get::<_, i32>(9)))),
        ("project_id", Json::uuid(row.get(10))),
        ("parent_id", Json::opt_uuid(row.get(11))),
        ("created_at", Json::DateTime(row.get(12))),
        ("updated_at", Json::DateTime(row.get(13))),
        ("created_by", Json::opt_uuid(row.get(14))),
        ("updated_by", Json::opt_uuid(row.get(15))),
        ("is_draft", Json::Bool(row.get(16))),
        ("archived_at", Json::opt_date(row.get(17))),
        ("cycle_id", Json::opt_uuid(row.get(18))),
        (
            "module_ids",
            Json::Array(modules.into_iter().map(Json::uuid).collect()),
        ),
        (
            "label_ids",
            Json::Array(labels.into_iter().map(Json::uuid).collect()),
        ),
        (
            "assignee_ids",
            Json::Array(assignees.into_iter().map(Json::uuid).collect()),
        ),
        ("sub_issues_count", opt_count(row, 22)),
        ("attachment_count", opt_count(row, 23)),
        ("link_count", opt_count(row, 24)),
    ]))
}

/// `ViewIssueListSerializer.to_representation` (`plane/app/serializers/view.py:24-53`).
fn view_issue_list_row(
    row: &tokio_postgres::Row,
    _timezone: chrono_tz::Tz,
) -> Result<Json, ViewError> {
    let sub_issues_count = opt_count(row, 19);
    let state_group: Option<String> = row.get(20);
    let labels: Vec<Uuid> = row.get(21);
    let assignees: Vec<Uuid> = row.get(22);
    let modules: Vec<Uuid> = row.get(23);
    Ok(Json::object(vec![
        ("id", Json::uuid(row.get(0))),
        ("name", Json::string(row.get::<_, String>(1))),
        ("state_id", Json::opt_uuid(row.get(2))),
        ("sort_order", Json::Float(row.get(3))),
        ("completed_at", Json::opt_datetime(row.get(4))),
        ("estimate_point", Json::opt_uuid(row.get(5))),
        ("priority", Json::string(row.get::<_, String>(6))),
        ("start_date", Json::opt_date(row.get(7))),
        ("target_date", Json::opt_date(row.get(8))),
        ("sequence_id", Json::Int(i64::from(row.get::<_, i32>(9)))),
        ("project_id", Json::uuid(row.get(10))),
        ("parent_id", Json::opt_uuid(row.get(11))),
        ("cycle_id", Json::opt_uuid(row.get(18))),
        ("sub_issues_count", sub_issues_count),
        ("created_at", Json::DateTime(row.get(12))),
        ("updated_at", Json::DateTime(row.get(13))),
        ("created_by", Json::opt_uuid(row.get(14))),
        ("updated_by", Json::opt_uuid(row.get(15))),
        ("attachment_count", opt_count(row, 24)),
        ("link_count", opt_count(row, 25)),
        ("is_draft", Json::Bool(row.get(16))),
        ("archived_at", Json::opt_date(row.get(17))),
        ("state__group", Json::opt_string(state_group)),
        (
            "assignee_ids",
            Json::Array(assignees.into_iter().map(Json::uuid).collect()),
        ),
        (
            "label_ids",
            Json::Array(labels.into_iter().map(Json::uuid).collect()),
        ),
        (
            "module_ids",
            Json::Array(modules.into_iter().map(Json::uuid).collect()),
        ),
    ]))
}

/// A `COUNT(*)` subquery's value. The annotation has no `GROUP BY` (the view
/// calls `.order_by()` first), so an empty set is `0`, not `null`.
fn opt_count(row: &tokio_postgres::Row, index: usize) -> Json {
    Json::Int(row.get::<_, i64>(index))
}

/// `expand=issue_relation` / `expand=issue_related` (`:883-932`).
async fn add_expand_relations(
    client: &Client,
    rows: &[&tokio_postgres::Row],
    expand: &[&str],
    results: &mut [Json],
    _timezone: chrono_tz::Tz,
) -> Result<(), ViewError> {
    let wants_relation = expand.contains(&"issue_relation");
    let wants_related = expand.contains(&"issue_related");
    if !wants_relation && !wants_related {
        return Ok(());
    }
    let ids: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();
    let sql = "SELECT issue_relations.id, issue_relations.issue_id, \
         issue_relations.related_issue_id, issue_relations.relation_type, \
         related.project_id, related.sequence_id, related.name, related.state_id, \
         related.priority, related.created_by_id, related.created_at, \
         related.updated_at, related.updated_by_id, related.deleted_at \
       FROM issue_relations \
       LEFT OUTER JOIN issues related ON issue_relations.related_issue_id = related.id \
       WHERE issue_relations.deleted_at IS NULL AND issue_relations.issue_id = ANY($1) \
       ORDER BY issue_relations.created_at DESC";
    let statement = client.prepare(sql).await.map_err(ViewError::from)?;
    let relation_rows = client
        .query(&statement, &[&ids])
        .await
        .map_err(ViewError::from)?;

    for (index, result) in results.iter_mut().enumerate() {
        if !wants_relation {
            continue;
        }
        let Some(issue_id) = rows.get(index).map(|row| row.get::<_, Uuid>(0)) else {
            continue;
        };
        let mut relations: Vec<Json> = Vec::new();
        for row in &relation_rows {
            let owner: Uuid = row.get(1);
            if owner != issue_id {
                continue;
            }
            let deleted_at: Option<DateTime<Utc>> = row.get(13);
            if deleted_at.is_some() {
                continue;
            }
            relations.push(Json::object(vec![
                ("id", Json::uuid(row.get(2))),
                ("project_id", Json::uuid(row.get(4))),
                ("sequence_id", Json::Int(i64::from(row.get::<_, i32>(5)))),
                ("name", Json::string(row.get::<_, String>(6))),
                ("relation_type", Json::string(row.get::<_, String>(3))),
                ("state_id", Json::opt_uuid(row.get(7))),
                ("priority", Json::string(row.get::<_, String>(8))),
                ("created_by", Json::opt_uuid(row.get(9))),
                ("created_at", Json::DateTime(row.get(10))),
                ("updated_at", Json::DateTime(row.get(11))),
                ("updated_by", Json::opt_uuid(row.get(12))),
            ]));
        }
        if let Json::Object(entries) = result {
            entries.push(("issue_relation".to_owned(), Json::Array(relations)));
        }
    }
    Ok(())
}

fn envelope(
    total: i64,
    next: Cursor,
    prev: Cursor,
    count: i64,
    total_pages: i64,
    results: Json,
) -> Json {
    Json::object(vec![
        ("grouped_by", Json::Null),
        ("sub_grouped_by", Json::Null),
        ("total_count", Json::Int(total)),
        ("next_cursor", Json::string(next.to_string())),
        ("prev_cursor", Json::string(prev.to_string())),
        ("next_page_results", Json::Bool(next.has_results)),
        ("prev_page_results", Json::Bool(prev.has_results)),
        ("count", Json::Int(count)),
        ("total_pages", Json::Int(total_pages)),
        ("total_results", Json::Int(total)),
        ("extra_stats", Json::Null),
        ("results", results),
    ])
}

async fn count(client: &Client, select: &Select) -> Result<i64, ViewError> {
    let distinct = if select.distinct {
        Some("issues.id")
    } else {
        None
    };
    let sql = select.count_sql(distinct, None);
    let statement = client.prepare(&sql.text).await.map_err(ViewError::from)?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    let row = client
        .query_one(&statement, &params)
        .await
        .map_err(ViewError::from)?;
    row.try_get(0).map_err(ViewError::from)
}

async fn fetch(client: &Client, select: &Select) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let sql = select.clone().into_sql();
    let statement = client.prepare(&sql.text).await.map_err(ViewError::from)?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    client
        .query(&statement, &params)
        .await
        .map_err(ViewError::from)
}

/// `Ordering::resolve` needs the same `order_by` default; kept for symmetry with
/// the values-based lists.
const _: fn(&ValueField) = |_| {};

/// `Value` is the predicate parameter type used by `build_issue_query`.
const _: fn(Uuid) -> Value = Value::Uuid;

/// `NaiveDate` is part of the decoded row shape.
const _: Option<NaiveDate> = None;
