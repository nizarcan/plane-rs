// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `CycleIssueViewSet.list` and `ModuleIssueViewSet.list`, the two issue lists
//! this port owns.
//!
//! Both are the `IssueViewSet.list` pipeline of `crates/plane-query` with a
//! different scope:
//!
//! ```text
//! Issue.issue_objects
//!   .filter(issue_cycle__cycle_id=…, issue_cycle__deleted_at__isnull=True)
//! (.filter(project_id=…).filter(workspace__slug=…) for the cycle list;
//!  ModuleIssueViewSet builds all three in get_queryset)
//!   .filter(<ComplexFilterBackend: ?filters= only>)
//!   .filter(**issue_filters(…))
//!   .annotate(cycle_id, link_count, attachment_count, sub_issues_count)
//!   order_issue_queryset(…)
//!   issue_queryset_grouper(…)
//!   paginate(…)
//! ```
//!
//! Two facts about these views decide rows and are reproduced here:
//!
//! * **No `.distinct()`.** `IssueViewSet.get_queryset` ends in `.distinct()`;
//!   these two build `Issue.issue_objects.filter(...)` directly, so
//!   `total_count_queryset.count()` counts joined rows and two issues sharing a
//!   filter row can appear twice.
//! * **Only `?filters=` filters.** `filter_backends` is `(ComplexFilterBackend,)`
//!   and `ComplexFilterBackend.filter_queryset` reads just the `filters` JSON
//!   tree (`plane/utils/filters/filter_backend.py:31-53`); the
//!   `IssueFilterSet`'s plain parameters (`assignee_id=…`, `state_id=…`) are
//!   ignored. Legacy `issue_filters` still reads `priority`, `state`, `labels`
//!   and friends, which is what makes those parameters work.
//!
//! The module list defaults `order_by` to `"created_at"` (`module/issue.py:113`)
//! while the cycle list defaults to `"-created_at"` (`cycle/issue.py:130`), and
//! only the module list passes a queryset to `issue_group_values`, so
//! `group_by=start_date|target_date|created_by` on a cycle list is an
//! `AttributeError` (500) there.

use chrono::Utc;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{ViewCtx, ViewResponse};
use plane_query::error::QueryError;
use plane_query::issue::filter::{FilterParams, LegacyFilters};
use plane_query::issue::group_values::{self, GroupValues, GroupValuesRequest};
use plane_query::issue::model::GroupField;
use plane_query::issue::order::Ordering;
use plane_query::issue::query::{GroupPlan, IssueQuery, IssueScope, render_group_value};
use plane_query::issue::rich::{self, RichFilters};
use plane_query::paginator::{PageQuery, issue_list_page};
use plane_query::sql::{Frag, Select, Value};
use uuid::Uuid;

use crate::cycles::base::Params;

/// Which through table scopes the list.
#[derive(Clone, Copy)]
pub(crate) enum ScopeKind {
    /// `issue_cycle__cycle_id=…, issue_cycle__deleted_at IS NULL`.
    Cycle(Uuid),
    /// `issue_module__module_id=…, issue_module__deleted_at IS NULL`.
    Module(Uuid),
}

impl ScopeKind {
    fn table(self) -> (&'static str, &'static str) {
        match self {
            Self::Cycle(_) => ("cycle_issues", "cycle_id"),
            Self::Module(_) => ("module_issues", "module_id"),
        }
    }

    fn id(self) -> Uuid {
        match self {
            Self::Cycle(id) | Self::Module(id) => id,
        }
    }
}

/// The request rows a list needs from `ViewCtx`.
pub(crate) struct Request<'a> {
    pub(crate) params: Params,
    pub(crate) scope: ScopeKind,
    /// `request.GET.get("order_by", …)`: `-created_at` for cycles,
    /// `created_at` for modules.
    pub(crate) default_order: &'static str,
    /// Whether `issue_group_values` receives the filtered issue queryset. The
    /// module list does (`module/issue.py:151`), the cycle list does not
    /// (`cycle/issue.py:162`).
    pub(crate) group_values_issues: bool,
    pub(crate) timezone: chrono_tz::Tz,
    pub(crate) query: &'a plane_drf::QueryDict,
}

/// Render a `QueryError` the way the view's own exception path would.
///
/// * `Api` — a DRF `ParseError`/`ValidationError`: a plain message becomes
///   `{"detail": …}`, a coded filter error its `{"message","code"[,"errors"]}`
///   dict (`plane/utils/filters/filter_backend.py:155-180`).
/// * `BadRequest` — `Response({"error": …}, 400)` (`issue.py:147-150`).
/// * everything else — an upstream 500.
pub(crate) fn query_error(error: QueryError) -> Result<ViewResponse, ViewError> {
    match error {
        QueryError::Api(api) => {
            let body = match api.detail() {
                plane_query::Json::Str(message) => {
                    plane_query::Json::object(vec![("detail", plane_query::Json::Str(message))])
                }
                other => other,
            };
            Ok(ViewResponse::raw(
                api.status,
                APPLICATION_JSON,
                body.to_drf(),
            ))
        }
        QueryError::BadRequest(message) => Ok(crate::cycles::base::error_body(400, &message)),
        other => Err(ViewError::Unknown(other.to_string())),
    }
}

/// Wrap the scope predicate in the `rich` filter slot, which is the only part
/// of [`IssueQuery`] a handler can contribute to.
fn scoped_rich(rich: Option<RichFilters>, scope: ScopeKind) -> RichFilters {
    let (table, column) = scope.table();
    let mut predicate = Frag::raw(format!(
        "EXISTS (SELECT 1 FROM {table} WHERE {table}.issue_id = issues.id \
         AND {table}.{column} = "
    ));
    predicate.push(Frag::bind(Value::Uuid(scope.id())));
    predicate.push_raw(&format!(" AND {table}.deleted_at IS NULL)"));
    match rich {
        Some(existing) => RichFilters {
            joins: existing.joins,
            predicate: existing.predicate.and(predicate),
        },
        None => RichFilters {
            joins: Vec::new(),
            predicate,
        },
    }
}

/// The group-key list for one field, with the trailing `"None"` the Python
/// appends for the nullable fields and the duplicate collapse its dict does.
async fn group_values_for(
    client: &deadpool_postgres::Client,
    request: GroupValuesRequest<'_>,
    field: GroupField,
) -> Result<Vec<String>, QueryError> {
    match group_values::values(&request)? {
        GroupValues::Static(values) => Ok(values),
        GroupValues::Query { sql, append_none } => {
            let statement = client.prepare(&sql.text).await.map_err(QueryError::from)?;
            let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
                .params
                .iter()
                .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
                .collect();
            let rows = client.query(&statement, &params).await?;
            let mut values = Vec::with_capacity(rows.len() + 1);
            for row in &rows {
                values.push(render_group_value(row, field)?);
            }
            if append_none {
                values.push("None".to_owned());
            }
            let mut seen = std::collections::HashSet::new();
            values.retain(|value| seen.insert(value.clone()));
            Ok(values)
        }
    }
}

/// `CycleIssueViewSet.list` / `ModuleIssueViewSet.list`.
pub(crate) async fn run(
    ctx: &ViewCtx<'_>,
    request: Request<'_>,
) -> Result<ViewResponse, ViewError> {
    let client = crate::common::client(ctx)?;
    let query = request.query;
    let group_by_param = query.get("group_by").filter(|value| !value.is_empty());
    let sub_group_by_param = query.get("sub_group_by").filter(|value| !value.is_empty());

    let page = match PageQuery::parse(query, group_by_param, sub_group_by_param) {
        Ok(page) => page,
        Err(error) => return query_error(error),
    };
    let legacy = match LegacyFilters::parse(&FilterParams::Get(query), Utc::now().date_naive()) {
        Ok(filters) => filters,
        Err(error) => return query_error(error),
    };
    let rich = match rich::parse_complex_query(query, request.timezone) {
        Ok(rich) => rich,
        Err(error) => return query_error(error),
    };
    let ordering = Ordering::resolve(query.get_or("order_by", request.default_order));
    let group_by = group_by_param.and_then(GroupField::parse);
    let sub_group_by = sub_group_by_param.and_then(GroupField::parse);

    if group_by.is_some() && group_by == sub_group_by {
        // `issue.py:146-150`: a bare `{"error": …}` body.
        return query_error(QueryError::BadRequest(
            "Group by and sub group by cannot have same parameters".to_owned(),
        ));
    }

    let issue_query = IssueQuery {
        scope: IssueScope {
            workspace_slug: request.params.slug.clone(),
            project_id: Some(request.params.project_id),
            // `CycleIssueViewSet`/`ModuleIssueViewSet` never call `.distinct()`.
            distinct: false,
            created_by: None,
        },
        filters: legacy,
        rich: Some(scoped_rich(rich, request.scope)),
        ordering,
        group_by,
        sub_group_by,
        timezone: request.timezone,
    };

    let plan = match group_by {
        Some(field) => {
            let filtered: Select = issue_query.filtered_base();
            let issues = if request.group_values_issues {
                Some(&filtered)
            } else {
                None
            };
            let group_request = GroupValuesRequest {
                field,
                workspace_slug: &request.params.slug,
                project_id: Some(request.params.project_id),
                user: None,
                issues,
            };
            let fields = match group_values_for(client, group_request, field).await {
                Ok(values) => values,
                Err(error) => return query_error(error),
            };
            let (sub_group_by, sub_group_fields) = match sub_group_by {
                Some(sub) => {
                    let sub_request = GroupValuesRequest {
                        field: sub,
                        workspace_slug: &request.params.slug,
                        project_id: Some(request.params.project_id),
                        user: None,
                        issues,
                    };
                    let values = match group_values_for(client, sub_request, sub).await {
                        Ok(values) => values,
                        Err(error) => return query_error(error),
                    };
                    (Some(sub), values)
                }
                None => (None, Vec::new()),
            };
            Some(GroupPlan {
                group_by: field,
                group_fields: fields,
                sub_group_by,
                sub_group_fields,
            })
        }
        None => None,
    };

    match issue_list_page(client, &issue_query, plan.as_ref(), &page).await {
        Ok(mut body) => {
            zero_counts(&mut body);
            // `plane-query`'s grouped counts replay `IssueViewSet`'s
            // `.distinct()` model projection; these two views have no
            // `.distinct()`, so a base join that duplicates an issue makes
            // Django count joined rows. One statement replaces both numbers
            // (Django issues the same two counts as separate queries).
            if let Some(field) = plan.as_ref() {
                let limit = i64::from(page.per_page.min(plane_query::paginator::MAX_LIMIT));
                let offset = page.cursor.offset * limit;
                if let Some((hits, page_count)) = joined_counts(
                    client,
                    &issue_query,
                    field.group_by,
                    field.sub_group_by,
                    limit,
                    offset,
                )
                .await?
                {
                    patch_counts(&mut body, hits, page_count);
                }
            }
            // `GroupedOffsetPaginator.process_results` returns `{}` (not the
            // pre-populated group dict) when the *page* is empty
            // (`paginator.py:380-389`); `plane-query` always shapes the groups.
            if plan.is_some() && envelope_count(&body) == Some(0) {
                clear_results(&mut body);
            }
            Ok(ViewResponse::raw(200, APPLICATION_JSON, body.to_drf()))
        }
        Err(error) => query_error(error),
    }
}

/// The two counts Django's grouped paginator reports when the queryset has no
/// `.distinct()`:
///
/// * `total_count`/`total_results` — `queryset.count()` over the windowed
///   queryset (every joined row);
/// * `count` — `len(CursorResult)`, the joined rows inside the page window.
///
/// One statement, because both are the same inner select with a different row
/// number predicate. `None` when the query orders by a min-value field, whose
/// `GROUP BY issues.id` makes the join rows collapse — `plane-query`'s counts
/// are already right there (both sides count groups).
async fn joined_counts(
    client: &deadpool_postgres::Client,
    query: &IssueQuery,
    group: GroupField,
    sub_group: Option<GroupField>,
    limit: i64,
    offset: i64,
) -> Result<Option<(i64, i64)>, ViewError> {
    if query.ordering.group_by_issue {
        return Ok(None);
    }
    let mut inner = query.filtered_select();
    let mut projection = vec![Frag::raw("issues.id")];
    let mut window = Frag::raw("ROW_NUMBER() OVER (PARTITION BY ");
    let partitions: Vec<GroupField> = [Some(group), sub_group].into_iter().flatten().collect();
    for (index, field) in partitions.iter().enumerate() {
        if index > 0 {
            window.push_raw(", ");
        }
        window.push(field.expression());
    }
    window.push_raw(" ORDER BY ");
    for (index, expression) in query.ordering.paginator_order().iter().enumerate() {
        if index > 0 {
            window.push_raw(", ");
        }
        window.push(expression.clone());
    }
    window.push_raw(")");
    projection.push(window.append(Frag::raw(" AS \"__row_number\"")));
    inner.projection = projection;
    let inner = inner.to_frag();
    let stop = offset + limit + 1;
    let mut outer = plane_query::sql::Select::from_frag(Frag {
        sql: format!("({}) AS \"qualify\"", inner.sql),
        params: inner.params,
    });
    outer.projection = vec![
        Frag::raw("count(*) AS \"__hits\""),
        Frag::raw(format!(
            "count(*) FILTER (WHERE \"__row_number\" > {offset} AND \"__row_number\" < {stop}) AS \"__page\""
        )),
    ];
    let sql = outer.into_sql();
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
    let hits: i64 = row.try_get(0).map_err(ViewError::from)?;
    let page_count: i64 = row.try_get(1).map_err(ViewError::from)?;
    Ok(Some((hits, page_count)))
}

/// The envelope's `count` (rows on this page).
fn envelope_count(body: &plane_query::Json) -> Option<i64> {
    let plane_query::Json::Object(entries) = body else {
        return None;
    };
    entries
        .iter()
        .find_map(|(key, value)| match (key.as_str(), value) {
            ("count", plane_query::Json::Int(value)) => Some(*value),
            _ => None,
        })
}

/// `results: {}` for an empty grouped page.
fn clear_results(body: &mut plane_query::Json) {
    let plane_query::Json::Object(entries) = body else {
        return;
    };
    for (key, value) in entries.iter_mut() {
        if key == "results" {
            *value = plane_query::Json::Object(Vec::new());
        }
    }
}

/// Replace the envelope's row counts, keeping every other key and the order.
fn patch_counts(body: &mut plane_query::Json, hits: i64, page_count: i64) {
    let plane_query::Json::Object(entries) = body else {
        return;
    };
    for (key, value) in entries.iter_mut() {
        match key.as_str() {
            "total_count" | "total_results" => *value = plane_query::Json::Int(hits),
            "count" => *value = plane_query::Json::Int(page_count),
            _ => {}
        }
    }
}

/// `sub_issues_count`, `attachment_count` and `link_count` are
/// `(SELECT Count(U0.id) …)` subqueries **without** a `GROUP BY` in these two
/// views (`cycle/issue.py:55-105`, `module/issue.py:54-82`), so an issue with
/// none of them gets `0`, not `NULL` like `IssueViewSet.list`'s
/// `… values("count")`-with-`GROUP BY` variant (which `plane-query` models
/// after that view). The paginator is shared, so the difference is applied to
/// the decoded body.
fn zero_counts(value: &mut plane_query::Json) {
    use plane_query::Json;
    match value {
        Json::Object(entries) => {
            for (key, value) in entries.iter_mut() {
                let is_count = matches!(
                    key.as_str(),
                    "sub_issues_count" | "attachment_count" | "link_count"
                );
                if is_count {
                    if matches!(value, Json::Null) {
                        *value = Json::Int(0);
                    }
                } else {
                    zero_counts(value);
                }
            }
        }
        Json::Array(items) => {
            for item in items {
                zero_counts(item);
            }
        }
        _ => {}
    }
}
