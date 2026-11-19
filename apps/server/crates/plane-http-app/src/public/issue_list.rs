// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ProjectIssuesPublicEndpoint.get` (`plane/space/views/issue.py:109-244`,
//! route 248) — the public board's paginated issue list.
//!
//! ## What the view does, in order
//!
//! ```text
//! filters   = issue_filters(request.query_params, "GET")
//! order_by  = request.GET.get("order_by", "-created_at")
//! board     = DeployBoard.objects.filter(anchor, entity_name="project").first()
//!             → 404 {"error": "Project is not published"}
//! project   = board.entity_identifier          (NOT board.project_id)
//! slug      = board.workspace.slug
//! queryset  = Issue.issue_objects.filter(workspace__slug, project_id)
//!               .annotate(cycle_id, link_count, attachment_count, sub_issues_count)
//!               .distinct()
//!               .filter(**filters)
//! order_issue_queryset(queryset, order_by)
//! issue_queryset_grouper(queryset, group_by, sub_group_by)
//! group_by == sub_group_by → 400 {"error": "Group by and sub group by cannot have same parameters"}
//! self.paginate(…)   → OffsetPaginator / GroupedOffsetPaginator / SubGroupedOffsetPaginator
//! ```
//!
//! `issue_filters` is the only filter source: this view never calls
//! `filter_queryset`, so `?filters=` and the `IssueFilterSet` do not apply.
//! The board lookup runs **before** `paginate`, so an unknown anchor is the
//! 404 even when `per_page`/`cursor`/`group_by` are invalid.
//!
//! ## The space grouper
//!
//! `plane/space/utils/grouper.py` is a copy of the app grouper with two
//! deliberate differences that change the SQL and therefore the bytes:
//!
//! * `issue_queryset_grouper` annotates the three id arrays with `ArrayAgg`
//!   **over the through-table joins** (`COALESCE(ARRAY_AGG(DISTINCT …) FILTER
//!   (…), ARRAY[]::uuid[])`) instead of the app grouper's correlated
//!   subqueries — so those joins are present even when the list is ungrouped.
//!   Unlike the app grouper, `module_ids` has **no** `deleted_at` filter
//!   (`space/utils/grouper.py:76-80`); only grouping by the field adds the
//!   through-table predicate (`:44-48`).
//! * `issue_on_results`' field list is shorter: no `completed_at`,
//!   `sub_issues_count`, `created_at`, `updated_at`, `updated_by`,
//!   `attachment_count`, `link_count`, `is_draft` or `archived_at`, and it adds
//!   the two `vote_items` / `reaction_items` jsonb aggregates.
//!
//! The reaction aggregate reads `issue_reactions__actor__avatar_asset` — unlike
//! `IssueRetrievePublicEndpoint`, this query has no `votes__` bug.
//!
//! ## Pagination
//!
//! `BasePaginator.paginate` with `OffsetPaginator` (no `group_by`),
//! `GroupedOffsetPaginator` (one group) or `SubGroupedOffsetPaginator` (two).
//! The envelope is the same `{grouped_by, …, results}` shape for all three
//! (`plane/utils/paginator.py:733-757`).
//!
//! The grouped variants are reproduced in full: membership arrays for the m2m
//! group fields (`labels__id`, `assignees__id`, `issue_module__module_id`),
//! sub-group cells, per-group cursors (`row_number` partitioned by the group
//! columns) and the paginator's own counts. The group keys come from
//! `issue_group_values` (`space/utils/grouper.py:145-213`), which is the same
//! `plane_query::issue::group_values` port the `/api/` lists use.
//!
//! Three quirks are kept because they decide the body:
//!
//! * `total_count` is `queryset.count()` on the windowed, grouped query: the
//!   `DISTINCT`/`GROUP BY` contains the m2m group columns, so a page grouped by
//!   one m2m field counts one row per (issue, group) and a page with two m2m
//!   fields counts one per (issue, group, sub group) — see
//!   [`hits_statement`].
//! * `count` is the number of rows on the page, which is up to `per_page` rows
//!   **per group**, not in total (`paginator.py:250-277`).
//! * `total_pages` is `ceil(largest group / per_page)` from the count filter's
//!   group totals, and 0 when the page is empty (`paginator.py:283-290`).

use chrono::NaiveDate;
use deadpool_postgres::Client;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::filter::{FilterBuilder, FilterParams, LegacyFilters};
use plane_query::issue::group_values::{self, GroupValues, GroupValuesRequest};
use plane_query::issue::model::{GroupField, Relation, manager_predicate};
use plane_query::issue::order::Ordering;
use plane_query::json::Json;
use plane_query::paginator::{Cursor, OffsetWindow, PageQuery, ceil_div, request_now};
use plane_query::sql::{Frag, Join, Select, Value};
use plane_serde::Dialect;
use uuid::Uuid;

use super::board;
use super::{ok, workspace_slug};
use crate::common::client;
use crate::issues::lists::list_common::{map_query_error, user_timezone};
use crate::write::error_body;

/// `ProjectIssuesPublicEndpoint.get` (`plane/space/views/issue.py:109-244`).
pub(super) fn issues<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { issues_inner(ctx).await })
}

async fn issues_inner<'a>(ctx: &'a ViewCtx<'a>) -> Result<ViewResponse, ViewError> {
    {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let client = client(ctx)?;
        let timezone = user_timezone(ctx);
        let now = request_now();

        // `issue_filters(request.query_params, "GET")` runs before the board
        // lookup, so a malformed date is its 400 even for an unknown anchor.
        let filters = match LegacyFilters::parse(&FilterParams::Get(ctx.query()), now.date_naive())
        {
            Ok(filters) => filters,
            Err(error) => return map_query_error(error),
        };
        let ordering = Ordering::resolve(ctx.query().get_or("order_by", "-created_at"));

        // `DeployBoard.objects.filter(...).first()` -> the view's own 404.
        let Some(board) = board::published_board(client, anchor).await? else {
            return Ok(error_body(404, "Project is not published"));
        };
        let project_id = board.entity_identifier;
        let slug = workspace_slug(client, board.workspace_id).await?;

        // `group_by = request.GET.get("group_by", False)` / `sub_group_by`:
        // present-but-empty is falsey, exactly like the Python.
        let group_by_raw = ctx
            .query()
            .get("group_by")
            .filter(|value| !value.is_empty());
        let sub_group_by_raw = ctx
            .query()
            .get("sub_group_by")
            .filter(|value| !value.is_empty());

        // `issue.py:175-179`: the equality check runs before `paginate`.
        if let (Some(group), Some(sub)) = (group_by_raw, sub_group_by_raw)
            && group == sub
        {
            return Ok(error_body(
                400,
                "Group by and sub group by cannot have same parameters",
            ));
        }

        let group_by = group_by_raw.and_then(GroupField::parse);
        let sub_group_by = sub_group_by_raw.and_then(GroupField::parse);

        let query = SpaceQuery {
            project_id,
            workspace_slug: &slug,
            filters: &filters,
            timezone,
            ordering: &ordering,
            group_by,
            sub_group_by,
        };
        // `group_by_fields=issue_group_values(...)` is an argument to
        // `self.paginate`, so it is evaluated **before** `paginate` validates
        // `per_page`/`cursor`/the field names. For the issue-derived fields
        // this is the Python `AttributeError` (500), which wins over a bad
        // cursor.
        let group_fields = match group_by {
            Some(group) => Some(group_keys(client, &query, group).await?),
            None => None,
        };
        let sub_fields = match sub_group_by {
            Some(sub) => Some(group_keys(client, &query, sub).await?),
            None => None,
        };

        // `paginate` validates `per_page`, `cursor` and the two group names
        // before it evaluates the queryset (`paginator.py:681-712`).
        let page = match PageQuery::parse(ctx.query(), group_by_raw, sub_group_by_raw) {
            Ok(page) => page,
            Err(error) => return map_query_error(error),
        };
        let base = query.build();
        let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);

        match (group_by, group_fields) {
            (None, _) => ungrouped_page(client, &query, &base, &window).await,
            (Some(group), Some(group_fields)) => {
                grouped_page(
                    client,
                    &query,
                    &base,
                    &window,
                    group,
                    sub_group_by,
                    &group_fields,
                    sub_fields.as_deref(),
                )
                .await
            }
            _ => Err(ViewError::Unknown(
                "group keys were not fetched for the group field".to_owned(),
            )),
        }
    }
}

/// The inputs of one 248 request, after the board lookup.
struct SpaceQuery<'a> {
    project_id: Option<Uuid>,
    workspace_slug: &'a str,
    filters: &'a LegacyFilters,
    timezone: chrono_tz::Tz,
    ordering: &'a Ordering,
    group_by: Option<GroupField>,
    sub_group_by: Option<GroupField>,
}

impl SpaceQuery<'_> {
    /// The joins and predicate every statement of this request shares.
    fn build(&self) -> FilterBuilder {
        let mut builder = FilterBuilder::new();
        builder.require_left(Relation::States);
        builder.require_inner(Relation::Projects);
        builder.require_inner(Relation::Workspaces);
        builder.and(manager_predicate());
        match self.project_id {
            Some(project_id) => builder.and(Frag::compare(
                "issues.project_id",
                "=",
                Value::Uuid(project_id),
            )),
            None => builder.and(Frag::is_null("issues.project_id")),
        };
        builder.and(Frag::compare(
            "workspaces.slug",
            "=",
            Value::Text(self.workspace_slug.to_owned()),
        ));
        // `issue_queryset_grouper`'s through-table `deleted_at` predicate for
        // each m2m group field (`space/utils/grouper.py:44-48`).
        for group in [self.group_by, self.sub_group_by].into_iter().flatten() {
            let column = match group {
                GroupField::LabelsId => Some("issue_labels.deleted_at"),
                GroupField::AssigneesId => Some("issue_assignees.deleted_at"),
                GroupField::ModuleId => Some("module_issues.deleted_at"),
                _ => None,
            };
            if let Some(column) = column {
                builder.and(Frag::is_null(column));
            }
        }
        // `order_issue_queryset`'s annotation joins (`labels__name` needs
        // `issue_labels` -> `labels`).
        for (relation, kind) in self.ordering.joins.entries() {
            builder.require(relation, kind);
        }
        self.filters.apply(&mut builder);
        self.filters.apply_dates(&mut builder, self.timezone);
        builder
    }
}

/// The `issue_on_results` `SELECT` list, in the space grouper's order
/// (`space/utils/grouper.py:88-193`).
///
/// The three id annotations come first; grouping by one of them removes its
/// annotation and appends the raw through column in its place, after the
/// remaining annotations (`original_list.remove`/`append`).
fn projection(group: Option<GroupField>, sub: Option<GroupField>) -> Vec<(&'static str, Frag)> {
    let cycle_id = Frag::raw(
        "(SELECT U0.\"cycle_id\" FROM \"cycle_issues\" U0 \
         WHERE (U0.\"deleted_at\" IS NULL AND U0.\"deleted_at\" IS NULL \
         AND U0.\"issue_id\" = (\"issues\".\"id\")) \
         ORDER BY U0.\"created_at\" DESC LIMIT 1)",
    );
    let mut fields: Vec<(&'static str, Frag)> = vec![
        ("id", Frag::raw("issues.id")),
        ("name", Frag::raw("issues.name")),
        ("state_id", Frag::raw("issues.state_id")),
        ("sort_order", Frag::raw("issues.sort_order")),
        ("estimate_point", Frag::raw("issues.estimate_point_id")),
        ("priority", Frag::raw("issues.priority")),
        ("start_date", Frag::raw("issues.start_date")),
        ("target_date", Frag::raw("issues.target_date")),
        ("sequence_id", Frag::raw("issues.sequence_id")),
        ("project_id", Frag::raw("issues.project_id")),
        ("parent_id", Frag::raw("issues.parent_id")),
        ("cycle_id", cycle_id),
        ("created_by", Frag::raw("issues.created_by_id")),
        ("state__group", Frag::raw("states.group")),
    ];
    let mut original = ["assignee_ids", "label_ids", "module_ids"].to_vec();
    let mut substitutions: Vec<(&'static str, Frag)> = Vec::new();
    for field in [group, sub].into_iter().flatten() {
        if let Some(mapper) = field.mapper() {
            original.retain(|key| *key != mapper);
            substitutions.push((field.as_str(), field.expression()));
        }
    }
    for key in original {
        fields.push((key, array_expression(key)));
    }
    fields.extend(substitutions);
    fields.push(("vote_items", vote_items_expression()));
    fields.push(("reaction_items", reaction_items_expression()));
    fields
}

fn array_expression(key: &str) -> Frag {
    match key {
        "assignee_ids" => array_aggregate(
            "issue_assignees",
            "assignee_id",
            "issue_assignees.deleted_at IS NULL",
        ),
        "label_ids" => array_aggregate(
            "issue_labels",
            "label_id",
            "issue_labels.deleted_at IS NULL",
        ),
        // `space/utils/grouper.py:76-80`: no `deleted_at` (and no
        // `modules.archived_at`) filter, and no `modules` join.
        "module_ids" => array_aggregate("module_issues", "module_id", "TRUE"),
        _ => Frag::raw("NULL"),
    }
}

fn array_aggregate(table: &str, column: &str, filter: &str) -> Frag {
    Frag::raw(format!(
        "COALESCE(ARRAY_AGG(DISTINCT {table}.{column}) \
         FILTER (WHERE (NOT ({table}.{column} IS NULL) AND {filter})), ARRAY[]::uuid[])"
    ))
}

fn vote_items_expression() -> Frag {
    Frag::raw(
        "ARRAY_AGG(DISTINCT CASE WHEN issue_votes.deleted_at IS NULL \
         AND issue_votes.id IS NOT NULL THEN \
         JSONB_BUILD_OBJECT('vote', issue_votes.vote, 'actor_details', \
           JSONB_BUILD_OBJECT('id', issue_votes.actor_id, 'first_name', v_actor.first_name, \
             'last_name', v_actor.last_name, 'avatar', v_actor.avatar, \
             'avatar_url', CASE WHEN v_actor.avatar_asset_id IS NOT NULL \
               THEN ('/api/assets/v2/static/' || (v_actor.avatar_asset_id)::varchar || '/') \
               ELSE v_actor.avatar END, \
             'display_name', v_actor.display_name)) ELSE NULL END) \
         FILTER (WHERE issue_votes.deleted_at IS NULL AND issue_votes.id IS NOT NULL)",
    )
}

fn reaction_items_expression() -> Frag {
    Frag::raw(
        "ARRAY_AGG(DISTINCT CASE WHEN issue_reactions.deleted_at IS NULL \
         AND issue_reactions.id IS NOT NULL THEN \
         JSONB_BUILD_OBJECT('reaction', issue_reactions.reaction, 'actor_details', \
           JSONB_BUILD_OBJECT('id', issue_reactions.actor_id, 'first_name', r_actor.first_name, \
             'last_name', r_actor.last_name, 'avatar', r_actor.avatar, \
             'avatar_url', CASE WHEN r_actor.avatar_asset_id IS NOT NULL \
               THEN ('/api/assets/v2/static/' || (r_actor.avatar_asset_id)::varchar || '/') \
               ELSE r_actor.avatar END, \
             'display_name', r_actor.display_name)) ELSE NULL END) \
         FILTER (WHERE issue_reactions.deleted_at IS NULL AND issue_reactions.id IS NOT NULL)",
    )
}

/// The joins the array and jsonb aggregates need, in Django's order.
fn aggregate_joins() -> Vec<Join> {
    vec![
        Join::left(
            "issue_assignees",
            "issue_assignees",
            Frag::raw("(issues.id = issue_assignees.issue_id)"),
        ),
        Join::left(
            "issue_labels",
            "issue_labels",
            Frag::raw("(issues.id = issue_labels.issue_id)"),
        ),
        Join::left(
            "module_issues",
            "module_issues",
            Frag::raw("(issues.id = module_issues.issue_id)"),
        ),
        Join::left(
            "issue_votes",
            "issue_votes",
            Frag::raw("(issues.id = issue_votes.issue_id)"),
        ),
        Join::left(
            "users",
            "v_actor",
            Frag::raw("(issue_votes.actor_id = v_actor.id)"),
        ),
        Join::left(
            "issue_reactions",
            "issue_reactions",
            Frag::raw("(issues.id = issue_reactions.issue_id)"),
        ),
        Join::left(
            "users",
            "r_actor",
            Frag::raw("(issue_reactions.actor_id = r_actor.id)"),
        ),
    ]
}

/// The base joins plus `aggregate_joins()`, reusing an alias the base already
/// joined (Django's join reuse: `order_by=labels__name` and the `label_ids`
/// aggregate share one `issue_labels` join).
fn joins_with_aggregates(base: &FilterBuilder) -> Vec<Join> {
    let mut joins = base.joins.to_joins();
    for join in aggregate_joins() {
        if !joins.iter().any(|existing| existing.alias == join.alias) {
            joins.push(join);
        }
    }
    joins
}

fn joins_with_aggregates_and_intake(base: &FilterBuilder) -> Vec<Join> {
    let mut joins = joins_with_aggregates(base);
    if !joins
        .iter()
        .any(|existing| existing.alias == "intake_issues")
    {
        joins.push(Join::left(
            "intake_issues",
            "intake_issues",
            Frag::raw("(issues.id = intake_issues.issue_id)"),
        ));
    }
    joins
}

/// The ordered projection's SQL, with the extra sort columns [`Ordering`] needs
/// when the page is `DISTINCT`.
fn projected_fields(
    group: Option<GroupField>,
    sub: Option<GroupField>,
    ordering: &Ordering,
) -> Vec<Frag> {
    let mut fields: Vec<Frag> = projection(group, sub)
        .into_iter()
        .map(|(key, expression)| aliased(expression, key))
        .collect();
    fields.push(aliased(ordering.expression.clone(), "__order_key"));
    fields.push(aliased(Frag::raw("issues.created_at"), "__created_at"));
    fields
}

fn aliased(mut expression: Frag, key: &str) -> Frag {
    expression.push_raw(" AS \"");
    expression.push_raw(key);
    expression.push_raw("\"");
    expression
}

/// The page's `ORDER BY`: `key ASC|DESC NULLS LAST, created_at DESC`
/// (`OffsetPaginator.get_result`, `paginator.py:136-140`), over the projected
/// aliases so a `DISTINCT` select is legal.
fn page_order(ordering: &Ordering) -> Vec<Frag> {
    vec![
        Frag::raw(format!(
            "\"__order_key\" {} NULLS LAST",
            if ordering.desc { "DESC" } else { "ASC" }
        )),
        Frag::raw("\"__created_at\" DESC"),
    ]
}

/// The `GROUP BY` of a page that has aggregate annotations: the primary key,
/// every projected column of another table that is not functionally dependent
/// on it (`states.group`) and the raw through column of each m2m group field
/// (`plane/utils/paginator.py` puts `Window.get_group_by_cols()`' partition
/// columns there; the issue columns collapse into the primary key on
/// Postgres).
fn group_by_columns(group: Option<GroupField>, sub: Option<GroupField>) -> Vec<String> {
    let mut columns = vec!["issues.id".to_owned(), "states.group".to_owned()];
    for field in [group, sub].into_iter().flatten() {
        if field.mapper().is_some() {
            let column = field.expression().sql;
            if !columns.contains(&column) {
                columns.push(column);
            }
        }
    }
    columns
}

fn page_statement(base: &FilterBuilder, ordering: &Ordering, limit: i64, offset: i64) -> Select {
    let mut select = Select::new("issues");
    select.joins = joins_with_aggregates(base);
    select.predicate = Some(base.predicate.clone());
    select.distinct = true;
    select.projection = projected_fields(None, None, ordering);
    // Django 5.2's aggregate grouping: the primary key plus every projected
    // column not functionally dependent on it (`states.group`).
    select.group_by = group_by_columns(None, None);
    select.order_by = page_order(ordering);
    select.limit = Some(limit + 1);
    select.offset = Some(offset);
    select
}

/// The grouped page: a window over the joined rows, filtered on the per-group
/// row number (`GroupedOffsetPaginator.get_result`,
/// `paginator.py:250-277`; `SubGroupedOffsetPaginator.get_result`,
/// `:457-485`).
///
/// One extra row per partition is fetched so `next_page_results` needs no
/// second round trip; the caller counts the extra rows per partition.
fn grouped_page_statement(
    base: &FilterBuilder,
    ordering: &Ordering,
    group: GroupField,
    sub: Option<GroupField>,
    limit: i64,
    offset: i64,
) -> Select {
    let mut inner = Select::new("issues");
    inner.joins = joins_with_aggregates(base);
    inner.predicate = Some(base.predicate.clone());
    inner.distinct = true;
    let mut projection = projected_fields(Some(group), sub, ordering);
    let mut row_number = Frag::raw("ROW_NUMBER() OVER (PARTITION BY ");
    row_number.push(group.expression());
    if let Some(sub) = sub {
        row_number.push_raw(", ");
        row_number.push(sub.expression());
    }
    row_number.push_raw(" ORDER BY ");
    // A window specification cannot reference a select-list alias, so this one
    // repeats the expressions Django puts there (`paginator.py:255-269,461-472`).
    for (index, expression) in ordering.paginator_order().into_iter().enumerate() {
        if index > 0 {
            row_number.push_raw(", ");
        }
        row_number.push(expression);
    }
    row_number.push_raw(") AS \"row_number\"");
    projection.push(row_number);
    inner.projection = projection;
    inner.group_by = group_by_columns(Some(group), sub);
    inner.order_by = page_order(ordering);
    let inner = inner.to_frag();
    let stop = offset + limit + 1;
    let mut outer = Select::from_frag(Frag {
        sql: format!("({}) AS \"qualify\"", inner.sql),
        params: inner.params,
    });
    outer.projection = vec![Frag::raw("*")];
    // `< stop + 1` fetches the extra row that answers
    // `queryset.filter(row_number__gte=stop).exists()`.
    outer.predicate = Some(
        Frag::raw(format!("\"row_number\" > {offset}"))
            .and(Frag::raw(format!("\"row_number\" < {}", stop + 1))),
    );
    outer.order_by = page_order(ordering);
    outer
}

/// `queryset.count()` on the windowed grouped query
/// (`GroupedOffsetPaginator.get_result`, `paginator.py:278-280`).
///
/// The outer `COUNT(*)` runs over the query's selected columns, which are
/// grouped by the primary key and every m2m group column. The window annotation
/// is masked out of Django's count, so what remains is one row per
/// (issue, group[, sub group]) — see the module docs.
fn hits_statement(base: &FilterBuilder, group: GroupField, sub: Option<GroupField>) -> Select {
    let mut inner = Select::new("issues");
    inner.joins = joins_with_aggregates(base);
    inner.predicate = Some(base.predicate.clone());
    inner.distinct = true;
    inner.projection = vec![Frag::raw("issues.id"), group.expression()];
    if let Some(sub) = sub {
        inner.projection.push(sub.expression());
    }
    inner.group_by = vec!["issues.id".to_owned(), group.expression().sql];
    if let Some(sub) = sub {
        inner.group_by.push(sub.expression().sql);
    }
    let inner = inner.to_frag();
    let mut outer = Select::from_frag(Frag {
        sql: format!("({}) AS \"subquery\"", inner.sql),
        params: inner.params,
    });
    outer.projection = vec![Frag::raw("COUNT(*)")];
    outer
}

/// `queryset.count()` for the ungrouped page: the windowed query collapses to
/// one row per issue (`GROUP BY issues.id` from the array annotations).
fn total_statement(base: &FilterBuilder) -> Select {
    let mut select = Select::new("issues");
    select.joins = base.joins.to_joins();
    select.predicate = Some(base.predicate.clone());
    select.projection = vec![Frag::raw("COUNT(DISTINCT issues.id)")];
    select
}

async fn count(client: &Client, select: Select) -> Result<i64, ViewError> {
    let sql = select.into_sql();
    let row = sql
        .query(client)
        .await
        .map_err(ViewError::from)?
        .into_iter()
        .next()
        .ok_or_else(|| ViewError::Unknown("count returned no row".to_owned()))?;
    row.try_get(0).map_err(ViewError::from)
}

async fn fetch(client: &Client, select: Select) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let sql = select.into_sql();
    sql.query(client).await.map_err(ViewError::from)
}

/// The ungrouped page (`OffsetPaginator`).
async fn ungrouped_page(
    client: &Client,
    query: &SpaceQuery<'_>,
    base: &FilterBuilder,
    window: &OffsetWindow,
) -> Result<ViewResponse, ViewError> {
    let select = page_statement(base, query.ordering, window.row_limit(), window.offset);
    let rows = fetch(client, select).await?;
    let fetched = rows.len();
    let rows = &rows[..fetched.min(window.row_limit() as usize)];
    let specs = field_specs(None, None);
    let results = render_rows(rows, &specs)?;
    let total = count(client, total_statement(base)).await?;
    let body = envelope_bytes(
        None,
        None,
        total,
        window.cursors(fetched),
        results.len(),
        ceil_div(total, window.row_limit()),
        raw_array(&results),
    );
    Ok(ok(body))
}

/// The grouped page (`GroupedOffsetPaginator` / `SubGroupedOffsetPaginator`).
#[allow(clippy::too_many_arguments)]
async fn grouped_page(
    client: &Client,
    query: &SpaceQuery<'_>,
    base: &FilterBuilder,
    window: &OffsetWindow,
    group: GroupField,
    sub: Option<GroupField>,
    group_fields: &[String],
    sub_fields: Option<&[String]>,
) -> Result<ViewResponse, ViewError> {
    let select = grouped_page_statement(
        base,
        query.ordering,
        group,
        sub,
        window.row_limit(),
        window.offset,
    );
    let rows = fetch(client, select).await?;
    let specs = field_specs(Some(group), sub);
    let group_index = field_index(&specs, group.as_str());
    let sub_index = sub.and_then(|field| specs.iter().position(|spec| spec.key == field.as_str()));

    // Count the rows per partition: the first `limit` rows are the page, an
    // extra row means a later page exists for that group
    // (`queryset.filter(row_number__gte=stop).exists()`).
    let limit = window.row_limit();
    let mut seen: Vec<(String, Option<String>, i64)> = Vec::new();
    let mut has_results = false;
    let mut kept: Vec<DecodedRow> = Vec::new();
    for row in &rows {
        let decoded = decode_row(row, &specs, group_index, sub_index)?;
        let count = match seen
            .iter_mut()
            .find(|(key, sub_key, _)| *key == decoded.group_key && *sub_key == decoded.sub_key)
        {
            Some((_, _, count)) => {
                *count += 1;
                *count
            }
            None => {
                seen.push((decoded.group_key.clone(), decoded.sub_key.clone(), 1));
                1
            }
        };
        if count > limit {
            has_results = true;
            continue;
        }
        kept.push(decoded);
    }

    let hits = count(client, hits_statement(base, group, sub)).await?;
    let group_totals = group_totals(client, base, group).await?;
    let sub_totals = match sub {
        Some(sub) => sub_group_totals(client, base, group, sub).await?,
        None => Vec::new(),
    };

    let results = if kept.is_empty() {
        Vec::new()
    } else {
        shape_results(
            group,
            sub,
            &kept,
            &specs,
            group_fields,
            sub_fields.unwrap_or(&[]),
            &group_totals,
            &sub_totals,
        )?
    };
    let page_count = kept.len();

    // `max_hits`: the largest group's *raw* count (before the zero-to-one
    // adjustment), 0 when the page is empty (`paginator.py:283-290,500-506`).
    let max_group = group_totals
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(0);
    let total_pages = if page_count == 0 {
        0
    } else {
        ceil_div(max_group, limit)
    };
    // `next_page_results` is `queryset.filter(row_number__gte=stop).exists()`,
    // which the per-partition extra row above answers without a second query.
    let next = Cursor::new(limit, window.page_index + 1, false, has_results);
    let prev = Cursor::new(limit, window.page_index - 1, true, window.page_index > 0);
    let body = envelope_bytes(
        Some(group),
        sub,
        hits,
        (next, prev),
        page_count,
        total_pages,
        raw_object(&results),
    );
    Ok(ok(body))
}

/// `issue_group_values` (`space/utils/grouper.py:145-213`).
async fn group_keys(
    client: &Client,
    query: &SpaceQuery<'_>,
    field: GroupField,
) -> Result<Vec<String>, ViewError> {
    let request = GroupValuesRequest {
        field,
        workspace_slug: query.workspace_slug,
        project_id: query.project_id,
        user: None,
        // The public view passes `filters=` but no `queryset=`, so the
        // issue-derived fields are the Python `AttributeError` (500).
        issues: None,
    };
    let plan = match group_values::values(&request) {
        Ok(plan) => plan,
        // The only error `issue_group_values` raises without a queryset is the
        // Python `AttributeError` for the issue-derived fields (a 500).
        Err(error) => return Err(ViewError::Unknown(error.to_string())),
    };
    match plan {
        GroupValues::Static(values) => Ok(dedupe(values)),
        GroupValues::Query { sql, append_none } => {
            let fetched = sql.query(client).await.map_err(ViewError::from)?;
            let mut values = Vec::with_capacity(fetched.len() + 1);
            for row in &fetched {
                values.push(render_group_value(row, 0, field)?);
            }
            if append_none {
                values.push("None".to_owned());
            }
            Ok(dedupe(values))
        }
    }
}

/// `COUNT(DISTINCT issues.id) FILTER (count_filter)` per group
/// (`paginator.py:299-305,502-519`).
async fn group_totals(
    client: &Client,
    base: &FilterBuilder,
    group: GroupField,
) -> Result<Vec<(String, i64)>, ViewError> {
    let mut select = Select::new("issues");
    select.joins = joins_with_aggregates_and_intake(base);
    select.predicate = Some(base.predicate.clone());
    select.distinct = true;
    select.projection = vec![
        group.expression(),
        Frag::raw(
            "COUNT(DISTINCT issues.id) FILTER (WHERE ((intake_issues.status = 1 \
             OR intake_issues.status = -1 OR intake_issues.status = 2 \
             OR intake_issues.id IS NULL) AND issues.archived_at IS NULL \
             AND NOT issues.is_draft))",
        ),
    ];
    select.group_by = vec!["1".to_owned()];
    let sql = select.into_sql();
    let rows = sql.query(client).await.map_err(ViewError::from)?;
    let mut totals: Vec<(String, i64)> = Vec::with_capacity(rows.len());
    for row in &rows {
        let key = render_group_value(row, 0, group)?;
        let count: i64 = read(row, 1)?;
        match totals.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, existing)) => *existing += count,
            None => totals.push((key, count)),
        }
    }
    Ok(totals)
}

/// `__get_subgroup_total_queryset` (`paginator.py:508-519`): raw counts with
/// no zero-to-one adjustment.
async fn sub_group_totals(
    client: &Client,
    base: &FilterBuilder,
    group: GroupField,
    sub: GroupField,
) -> Result<Vec<(String, String, i64)>, ViewError> {
    let mut select = Select::new("issues");
    select.joins = joins_with_aggregates_and_intake(base);
    select.predicate = Some(base.predicate.clone());
    select.distinct = true;
    select.projection = vec![
        group.expression(),
        sub.expression(),
        Frag::raw(
            "COUNT(DISTINCT issues.id) FILTER (WHERE ((intake_issues.status = 1 \
             OR intake_issues.status = -1 OR intake_issues.status = 2 \
             OR intake_issues.id IS NULL) AND issues.archived_at IS NULL \
             AND NOT issues.is_draft))",
        ),
    ];
    select.group_by = vec!["1".to_owned(), "2".to_owned()];
    let sql = select.into_sql();
    let rows = sql.query(client).await.map_err(ViewError::from)?;
    let mut totals: Vec<(String, String, i64)> = Vec::with_capacity(rows.len());
    for row in &rows {
        let group_key = render_group_value(row, 0, group)?;
        let sub_key = render_group_value(row, 1, sub)?;
        let count: i64 = read(row, 2)?;
        totals.push((group_key, sub_key, count));
    }
    Ok(totals)
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Decoding and shaping
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Kind {
    Uuid,
    OptUuid,
    Text,
    OptText,
    Float,
    Int,
    OptDate,
    UuidArray,
    JsonArray,
}

struct FieldSpec {
    key: &'static str,
    kind: Kind,
    /// Whether the column is in the `SELECT` list. The membership array of an
    /// m2m group field is **not** selected (`issue_on_results` removes it), but
    /// `process_results` writes it back into each row, at the end of the dict
    /// (`paginator.py:352,579-605`).
    selected: bool,
}

fn field_specs(group: Option<GroupField>, sub: Option<GroupField>) -> Vec<FieldSpec> {
    // The raw columns the m2m group fields append (`labels__id`, …) are
    // optional UUIDs; so are the fields the fallback arm catches.
    let mut specs: Vec<FieldSpec> = projection(group, sub)
        .into_iter()
        .map(|(key, _)| FieldSpec {
            key,
            kind: match key {
                "id" | "project_id" => Kind::Uuid,
                "state_id" | "parent_id" | "cycle_id" | "created_by" | "estimate_point" => {
                    Kind::OptUuid
                }
                "name" | "priority" => Kind::Text,
                "state__group" => Kind::OptText,
                "sort_order" => Kind::Float,
                "sequence_id" => Kind::Int,
                "start_date" | "target_date" => Kind::OptDate,
                "assignee_ids" | "label_ids" | "module_ids" => Kind::UuidArray,
                "vote_items" | "reaction_items" => Kind::JsonArray,
                _ => Kind::OptUuid,
            },
            selected: true,
        })
        .collect();
    // `result[self.FIELD_MAPPER[…]]` (group first, then sub group): the key is
    // appended to the row dict by the paginator, after every selected field.
    for field in [group, sub].into_iter().flatten() {
        if let Some(mapper) = field.mapper() {
            specs.push(FieldSpec {
                key: mapper,
                kind: Kind::UuidArray,
                selected: false,
            });
        }
    }
    specs
}

fn field_index(specs: &[FieldSpec], key: &str) -> Option<usize> {
    specs.iter().position(|spec| spec.key == key)
}

enum Cell {
    Uuid(Uuid),
    OptUuid(Option<Uuid>),
    Text(String),
    OptText(Option<String>),
    Float(f64),
    Int(i64),
    OptDate(Option<NaiveDate>),
    UuidArray(Vec<Uuid>),
    JsonArray(Option<Vec<JsonText>>),
}

struct DecodedRow {
    cells: Vec<Cell>,
    id: String,
    group_key: String,
    sub_key: Option<String>,
}

fn read<T>(row: &tokio_postgres::Row, index: usize) -> Result<T, ViewError>
where
    T: for<'a> tokio_postgres::types::FromSql<'a>,
{
    row.try_get(index).map_err(ViewError::from)
}

fn decode_row(
    row: &tokio_postgres::Row,
    specs: &[FieldSpec],
    group_index: Option<usize>,
    sub_index: Option<usize>,
) -> Result<DecodedRow, ViewError> {
    let mut cells = Vec::with_capacity(specs.len());
    for (index, spec) in specs.iter().enumerate() {
        if !spec.selected {
            cells.push(Cell::UuidArray(Vec::new()));
            continue;
        }
        cells.push(match spec.kind {
            Kind::Uuid => Cell::Uuid(read(row, index)?),
            Kind::OptUuid => Cell::OptUuid(read(row, index)?),
            Kind::Text => Cell::Text(read(row, index)?),
            Kind::OptText => Cell::OptText(read(row, index)?),
            Kind::Float => Cell::Float(read(row, index)?),
            Kind::Int => Cell::Int(i64::from(read::<i32>(row, index)?)),
            Kind::OptDate => Cell::OptDate(read(row, index)?),
            Kind::UuidArray => {
                let mut ids: Vec<Uuid> = read(row, index)?;
                ids.sort();
                Cell::UuidArray(ids)
            }
            Kind::JsonArray => Cell::JsonArray(read(row, index)?),
        });
    }
    let id_index = field_index(specs, "id")
        .ok_or_else(|| ViewError::Unknown("issue list has no id column".to_owned()))?;
    let id = key_of_cell(&cells[id_index]);
    let group_key = match group_index {
        Some(index) => key_of_cell(&cells[index]),
        None => String::new(),
    };
    let sub_key = sub_index.map(|index| key_of_cell(&cells[index]));
    Ok(DecodedRow {
        cells,
        id,
        group_key,
        sub_key,
    })
}

fn key_of_cell(cell: &Cell) -> String {
    match cell {
        Cell::Uuid(value) => value.to_string(),
        Cell::OptUuid(Some(value)) => value.to_string(),
        Cell::OptUuid(None) => "None".to_owned(),
        Cell::Text(value) => value.clone(),
        Cell::OptText(Some(value)) => value.clone(),
        Cell::OptText(None) => "None".to_owned(),
        Cell::Float(value) => value.to_string(),
        Cell::Int(value) => value.to_string(),
        Cell::OptDate(Some(value)) => value.to_string(),
        Cell::OptDate(None) => "None".to_owned(),
        Cell::UuidArray(_) | Cell::JsonArray(_) => "None".to_owned(),
    }
}

fn value_bytes(value: &Json) -> Vec<u8> {
    let wrapped = Json::Array(vec![value.clone()]).to_drf();
    wrapped
        .get(1..wrapped.len().saturating_sub(1))
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

/// One cell's value, with an optional paginator override.
#[derive(Clone)]
enum Override {
    /// `list(result_group_mapping[...])` sorted (D16 compares it as a multiset).
    Array(Vec<String>),
    /// The raw group column, normalised to the group the row is filed under.
    Key(String),
}

fn push_cell(out: &mut Vec<u8>, cell: &Cell) -> Result<(), ViewError> {
    match cell {
        Cell::Uuid(value) => out.extend_from_slice(&value_bytes(&Json::uuid(*value))),
        Cell::OptUuid(value) => out.extend_from_slice(&value_bytes(&Json::opt_uuid(*value))),
        Cell::Text(value) => out.extend_from_slice(&value_bytes(&Json::string(value.clone()))),
        Cell::OptText(value) => {
            out.extend_from_slice(&value_bytes(&Json::opt_string(value.clone())))
        }
        Cell::Float(value) => out.extend_from_slice(&value_bytes(&Json::Float(*value))),
        Cell::Int(value) => out.extend_from_slice(&value_bytes(&Json::Int(*value))),
        Cell::OptDate(Some(value)) => out.extend_from_slice(&value_bytes(&Json::Date(*value))),
        Cell::OptDate(None) => out.extend_from_slice(b"null"),
        Cell::UuidArray(ids) => {
            let array = Json::Array(ids.iter().map(|id| Json::uuid(*id)).collect());
            out.extend_from_slice(&value_bytes(&array));
        }
        Cell::JsonArray(None) => out.extend_from_slice(b"null"),
        Cell::JsonArray(Some(items)) => {
            out.push(b'[');
            for (position, item) in items.iter().enumerate() {
                if position > 0 {
                    out.push(b',');
                }
                plane_serde::transcode_json(Dialect::Drf, item.as_str(), out)
                    .map_err(|error| ViewError::Unknown(error.to_string()))?;
            }
            out.push(b']');
        }
    }
    Ok(())
}

fn push_override(out: &mut Vec<u8>, over: &Override) {
    match over {
        Override::Array(values) => {
            let array = Json::Array(
                values
                    .iter()
                    .map(|value| Json::string(value.clone()))
                    .collect(),
            );
            out.extend_from_slice(&value_bytes(&array));
        }
        Override::Key(key) => {
            if key == "None" {
                out.extend_from_slice(b"null");
            } else {
                out.extend_from_slice(&value_bytes(&Json::string(key.clone())));
            }
        }
    }
}

fn render_row(
    row: &DecodedRow,
    specs: &[FieldSpec],
    overrides: &[(usize, Override)],
) -> Result<Vec<u8>, ViewError> {
    let mut out = Vec::with_capacity(1024);
    out.push(b'{');
    for (index, spec) in specs.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        out.push(b'"');
        out.extend_from_slice(spec.key.as_bytes());
        out.extend_from_slice(b"\":");
        match overrides.iter().find(|(slot, _)| *slot == index) {
            Some((_, over)) => push_override(&mut out, over),
            None => push_cell(&mut out, &row.cells[index])?,
        }
    }
    out.push(b'}');
    Ok(out)
}

fn render_rows(
    rows: &[tokio_postgres::Row],
    specs: &[FieldSpec],
) -> Result<Vec<Vec<u8>>, ViewError> {
    rows.iter()
        .map(|row| {
            let decoded = decode_row(row, specs, None, None)?;
            render_row(&decoded, specs, &[])
        })
        .collect()
}

/// The group ids of each issue, in first-appearance order, for one m2m group
/// field (`result_group_mapping` in the Python).
fn accumulate_groups(rows: &[DecodedRow], index: usize) -> Vec<(String, Vec<String>)> {
    let mut per_issue: Vec<(String, Vec<String>)> = Vec::new();
    for row in rows {
        let key = key_of_cell(&row.cells[index]);
        match per_issue
            .iter_mut()
            .find(|(existing, _)| *existing == row.id)
        {
            Some((_, keys)) => {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            None => per_issue.push((row.id.clone(), vec![key])),
        }
    }
    per_issue
}

fn fully_annotated_array(keys: &[String]) -> Vec<String> {
    if keys.iter().any(|key| key == "None") {
        Vec::new()
    } else {
        let mut keys = keys.to_vec();
        keys.sort();
        keys
    }
}

fn array_override(
    specs: &[FieldSpec],
    field: GroupField,
    group_map: &[(String, Vec<String>)],
    id: &str,
) -> Option<(usize, Override)> {
    let mapper = field.mapper()?;
    let index = field_index(specs, mapper)?;
    let keys = group_map
        .iter()
        .find(|(existing, _)| existing == id)
        .map(|(_, keys)| keys.clone())
        .unwrap_or_default();
    Some((index, Override::Array(fully_annotated_array(&keys))))
}

/// The `results` half of the envelope (`process_results`,
/// `paginator.py:334-389,549-632`).
#[allow(clippy::too_many_arguments)]
fn shape_results(
    group: GroupField,
    sub: Option<GroupField>,
    rows: &[DecodedRow],
    specs: &[FieldSpec],
    group_fields: &[String],
    _sub_fields: &[String],
    group_totals: &[(String, i64)],
    sub_totals: &[(String, String, i64)],
) -> Result<Vec<(String, Vec<u8>)>, ViewError> {
    let group_index = field_index(specs, group.as_str())
        .ok_or_else(|| ViewError::Unknown("issue list has no group column".to_owned()))?;
    let sub_index = sub.and_then(|field| field_index(specs, field.as_str()));
    let group_is_m2m = group.mapper().is_some();
    let sub_is_m2m = sub.is_some_and(|field| field.mapper().is_some());
    let multi = group_is_m2m || sub_is_m2m;

    if sub.is_none() && !multi {
        return single_group(rows, specs, group_fields, group_index, group_totals);
    }
    if sub.is_none() {
        return single_multi_group(rows, specs, group, group_index, group_totals);
    }
    let sub = sub.ok_or_else(|| ViewError::Unknown("missing sub group".to_owned()))?;
    let sub_index = sub_index
        .ok_or_else(|| ViewError::Unknown("issue list has no sub group column".to_owned()))?;
    if multi {
        multi_sub_group(
            rows,
            specs,
            group,
            sub,
            group_fields,
            group_index,
            sub_index,
            group_totals,
            sub_totals,
        )
    } else {
        plain_sub_group(
            rows,
            specs,
            group_fields,
            group_index,
            sub_index,
            group_totals,
            sub_totals,
        )
    }
}

fn group_total(group_totals: &[(String, i64)], key: &str) -> i64 {
    match group_totals.iter().find(|(existing, _)| existing == key) {
        Some((_, 0)) => 1,
        Some((_, count)) => *count,
        None => 0,
    }
}

fn single_group(
    rows: &[DecodedRow],
    specs: &[FieldSpec],
    group_fields: &[String],
    group_index: usize,
    group_totals: &[(String, i64)],
) -> Result<Vec<(String, Vec<u8>)>, ViewError> {
    // `__query_grouper` indexes the precomputed dict without checking, so a
    // row whose group is not in `issue_group_values` is a `KeyError` (500).
    for row in rows {
        let key = key_of_cell(&row.cells[group_index]);
        if !group_fields.contains(&key) {
            return Err(ViewError::Unknown(format!(
                "KeyError: '{key}' (group_by value not in issue_group_values)"
            )));
        }
    }
    let mut out = Vec::with_capacity(group_fields.len());
    for key in group_fields {
        let matching: Vec<&DecodedRow> = rows
            .iter()
            .filter(|row| key_of_cell(&row.cells[group_index]) == *key)
            .collect();
        let mut rendered = Vec::with_capacity(matching.len());
        for row in matching {
            rendered.push(render_row(row, specs, &[])?);
        }
        out.push((
            key.clone(),
            group_object(&rendered, group_total(group_totals, key)),
        ));
    }
    Ok(out)
}

fn single_multi_group(
    rows: &[DecodedRow],
    specs: &[FieldSpec],
    group: GroupField,
    group_index: usize,
    group_totals: &[(String, i64)],
) -> Result<Vec<(String, Vec<u8>)>, ViewError> {
    let per_issue = accumulate_groups(rows, group_index);
    // The Python's result dict only holds the groups the page touched, and its
    // order is the set order; sorted here (D16).
    // group key -> the page's (issue id, rendered row)
    let mut groups: Vec<GroupRows> = Vec::new();
    for row in rows {
        let keys = per_issue
            .iter()
            .find(|(existing, _)| *existing == row.id)
            .map(|(_, keys)| keys.clone())
            .unwrap_or_default();
        let mut overrides: Vec<(usize, Override)> = Vec::new();
        if let Some(entry) = array_override(specs, group, &per_issue, &row.id) {
            overrides.push(entry);
        }
        for key in &keys {
            let row_overrides = overrides.clone();
            // `__result_already_added`: the first row of an issue wins.
            let duplicate = groups
                .iter()
                .find(|(existing, _)| existing == key)
                .is_some_and(|(_, entries)| entries.iter().any(|(id, _)| *id == row.id));
            if duplicate {
                continue;
            }
            let rendered = render_row(
                row,
                specs,
                &row_overrides_with(row_overrides, group_index, key),
            )?;
            match groups.iter_mut().find(|(existing, _)| existing == key) {
                Some((_, entries)) => entries.push((row.id.clone(), rendered)),
                None => groups.push((key.clone(), vec![(row.id.clone(), rendered)])),
            }
        }
    }
    groups.sort_by(|left, right| left.0.cmp(&right.0));
    let mut out = Vec::with_capacity(groups.len());
    for (key, entries) in groups {
        let rendered: Vec<Vec<u8>> = entries.into_iter().map(|(_, rendered)| rendered).collect();
        let total = match group_totals.iter().find(|(existing, _)| *existing == key) {
            Some((_, 0)) => Json::Int(1),
            Some((_, count)) => Json::Int(*count),
            // `__query_multi_grouper` uses `total_group_dict.get(...)`, so a
            // group that is not in the totals renders `null`.
            None => Json::Null,
        };
        out.push((key, multi_group_object(&rendered, total)));
    }
    Ok(out)
}

type GroupRows = (String, Vec<(String, Vec<u8>)>);

fn row_overrides_with(
    mut overrides: Vec<(usize, Override)>,
    group_index: usize,
    key: &str,
) -> Vec<(usize, Override)> {
    overrides.push((group_index, Override::Key(key.to_owned())));
    overrides
}

fn multi_group_object(rendered: &[Vec<u8>], total: Json) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"{\"results\":");
    out.extend_from_slice(&raw_array(rendered));
    out.extend_from_slice(b",\"total_results\":");
    out.extend_from_slice(&value_bytes(&total));
    out.push(b'}');
    out
}

#[allow(clippy::too_many_arguments)]
fn multi_sub_group(
    rows: &[DecodedRow],
    specs: &[FieldSpec],
    group: GroupField,
    sub: GroupField,
    group_fields: &[String],
    group_index: usize,
    sub_index: usize,
    group_totals: &[(String, i64)],
    sub_totals: &[(String, String, i64)],
) -> Result<Vec<(String, Vec<u8>)>, ViewError> {
    let group_map = if group.mapper().is_some() {
        accumulate_groups(rows, group_index)
    } else {
        Vec::new()
    };
    let sub_map = if sub.mapper().is_some() {
        accumulate_groups(rows, sub_index)
    } else {
        Vec::new()
    };
    let mut out = Vec::with_capacity(group_fields.len());
    for key in group_fields {
        let mut cells: Vec<(&String, Vec<Vec<u8>>)> = Vec::new();
        let mut sub_keys: Vec<&String> = sub_totals
            .iter()
            .filter(|(existing, _, _)| existing == key)
            .map(|(_, sub_key, _)| sub_key)
            .collect();
        // The Python's cell order is the order Postgres returns the group rows
        // (`__get_subgroup_total_queryset` has no ORDER BY); the harness sorts
        // it, and so does this.
        sub_keys.sort();
        sub_keys.dedup();
        for sub_key in sub_keys {
            cells.push((sub_key, Vec::new()));
        }
        for row in rows {
            if key_of_cell(&row.cells[group_index]) != *key {
                continue;
            }
            let sub_key = key_of_cell(&row.cells[sub_index]);
            let Some((_, cell)) = cells.iter_mut().find(|(existing, _)| **existing == sub_key)
            else {
                continue;
            };
            let mut overrides: Vec<(usize, Override)> = Vec::new();
            if group.mapper().is_some()
                && let Some(entry) = array_override(specs, group, &group_map, &row.id)
            {
                overrides.push(entry);
            }
            if sub.mapper().is_some()
                && let Some(entry) = array_override(specs, sub, &sub_map, &row.id)
            {
                overrides.push(entry);
            }
            cell.push(render_row(row, specs, &overrides)?);
        }
        let mut rendered_cells: Vec<(String, Vec<u8>)> = Vec::with_capacity(cells.len());
        for (sub_key, rendered) in cells {
            let total = sub_totals
                .iter()
                .find(|(existing, existing_sub, _)| existing == key && existing_sub == sub_key)
                .map(|(_, _, count)| *count)
                .unwrap_or(0);
            rendered_cells.push((sub_key.clone(), group_object(&rendered, total)));
        }
        out.push((
            key.clone(),
            nested_group_object(&rendered_cells, group_total(group_totals, key)),
        ));
    }
    Ok(out)
}

fn plain_sub_group(
    rows: &[DecodedRow],
    specs: &[FieldSpec],
    group_fields: &[String],
    group_index: usize,
    sub_index: usize,
    group_totals: &[(String, i64)],
    sub_totals: &[(String, String, i64)],
) -> Result<Vec<(String, Vec<u8>)>, ViewError> {
    // `SubGroupedOffsetPaginator.__query_grouper` indexes both dictionaries
    // without checking, so an unknown group or sub-group is a `KeyError` (500).
    for row in rows {
        let key = key_of_cell(&row.cells[group_index]);
        if !group_fields.contains(&key) {
            return Err(ViewError::Unknown(format!(
                "KeyError: '{key}' (group_by value not in issue_group_values)"
            )));
        }
        let sub_key = key_of_cell(&row.cells[sub_index]);
        if !sub_totals
            .iter()
            .any(|(existing, existing_sub, _)| *existing == key && *existing_sub == sub_key)
        {
            return Err(ViewError::Unknown(format!(
                "KeyError: '{sub_key}' (sub_group_by value not in the group totals)"
            )));
        }
    }
    let mut out = Vec::with_capacity(group_fields.len());
    for key in group_fields {
        let mut sub_keys: Vec<&String> = sub_totals
            .iter()
            .filter(|(existing, _, _)| existing == key)
            .map(|(_, sub_key, _)| sub_key)
            .collect();
        sub_keys.sort();
        sub_keys.dedup();
        let mut rendered_cells: Vec<(String, Vec<u8>)> = Vec::with_capacity(sub_keys.len());
        for sub_key in sub_keys {
            let matching: Vec<&DecodedRow> = rows
                .iter()
                .filter(|row| {
                    key_of_cell(&row.cells[group_index]) == *key
                        && key_of_cell(&row.cells[sub_index]) == *sub_key
                })
                .collect();
            let mut rendered = Vec::with_capacity(matching.len());
            for row in matching {
                rendered.push(render_row(row, specs, &[])?);
            }
            let total = sub_totals
                .iter()
                .find(|(existing, existing_sub, _)| existing == key && existing_sub == sub_key)
                .map(|(_, _, count)| *count)
                .unwrap_or(0);
            rendered_cells.push((sub_key.clone(), group_object(&rendered, total)));
        }
        out.push((
            key.clone(),
            nested_group_object(&rendered_cells, group_total(group_totals, key)),
        ));
    }
    Ok(out)
}

fn group_object(rendered: &[Vec<u8>], total: i64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"{\"results\":");
    out.extend_from_slice(&raw_array(rendered));
    out.extend_from_slice(b",\"total_results\":");
    out.extend_from_slice(&value_bytes(&Json::Int(total)));
    out.push(b'}');
    out
}

fn nested_group_object(cells: &[(String, Vec<u8>)], total: i64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"{\"results\":");
    out.extend_from_slice(&raw_object(cells));
    out.extend_from_slice(b",\"total_results\":");
    out.extend_from_slice(&value_bytes(&Json::Int(total)));
    out.push(b'}');
    out
}

/// `str(value)` for one group-value row cell.
fn render_group_value(
    row: &tokio_postgres::Row,
    index: usize,
    group: GroupField,
) -> Result<String, ViewError> {
    use tokio_postgres::types::Type;
    let column = row.columns().get(index).map(tokio_postgres::Column::type_);
    if matches!(column, Some(&Type::UUID)) {
        let value: Option<Uuid> = read(row, index)?;
        return Ok(value.map_or_else(|| "None".to_owned(), |value| value.to_string()));
    }
    if matches!(column, Some(&Type::DATE)) {
        let value: Option<NaiveDate> = read(row, index)?;
        return Ok(value.map_or_else(|| "None".to_owned(), |value| value.to_string()));
    }
    let value: Option<String> = read(row, index)?;
    let _ = group;
    Ok(value.unwrap_or_else(|| "None".to_owned()))
}

// ---------------------------------------------------------------------------
// The envelope, with pre-rendered `results`
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn envelope_bytes(
    group_by: Option<GroupField>,
    sub_group_by: Option<GroupField>,
    hits: i64,
    cursors: (Cursor, Cursor),
    count: usize,
    total_pages: i64,
    results: Vec<u8>,
) -> Vec<u8> {
    let (next, prev) = cursors;
    let mut out = Vec::with_capacity(results.len() + 256);
    out.push(b'{');
    let entries: Vec<(&str, Json)> = vec![
        (
            "grouped_by",
            group_by.map_or(Json::Null, |field| Json::string(field.as_str())),
        ),
        (
            "sub_grouped_by",
            sub_group_by.map_or(Json::Null, |field| Json::string(field.as_str())),
        ),
        ("total_count", Json::Int(hits)),
        ("next_cursor", Json::string(next.to_string())),
        ("prev_cursor", Json::string(prev.to_string())),
        ("next_page_results", Json::Bool(next.has_results)),
        ("prev_page_results", Json::Bool(prev.has_results)),
        ("count", Json::Int(count as i64)),
        ("total_pages", Json::Int(total_pages)),
        ("total_results", Json::Int(hits)),
        ("extra_stats", Json::Null),
    ];
    for (index, (key, value)) in entries.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        out.push(b'"');
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(b"\":");
        out.extend_from_slice(&value_bytes(value));
    }
    out.extend_from_slice(b",\"results\":");
    out.extend_from_slice(&results);
    out.push(b'}');
    out
}

fn raw_array(rows: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rows.iter().map(Vec::len).sum::<usize>() + 2);
    out.push(b'[');
    for (position, row) in rows.iter().enumerate() {
        if position > 0 {
            out.push(b',');
        }
        out.extend_from_slice(row);
    }
    out.push(b']');
    out
}

fn raw_object(groups: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'{');
    for (position, (key, value)) in groups.iter().enumerate() {
        if position > 0 {
            out.push(b',');
        }
        out.push(b'"');
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(b"\":");
        out.extend_from_slice(value);
    }
    out.push(b'}');
    out
}
