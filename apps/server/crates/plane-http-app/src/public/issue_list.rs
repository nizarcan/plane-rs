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
//!
//! ## The space grouper
//!
//! `plane/space/utils/grouper.py` is a copy of the app grouper with two
//! deliberate differences that change the SQL and therefore the bytes:
//!
//! * `issue_queryset_grouper` annotates the three id arrays with `ArrayAgg`
//!   **over the through-table joins** (`COALESCE(ARRAY_AGG(DISTINCT …) FILTER
//!   (…), ARRAY[]::uuid[])`) instead of the app grouper's correlated
//!   subqueries — so those joins are present even when the list is ungrouped;
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
//! Ported here: the ungrouped page and a single, non-m2m `group_by`
//! (`state_id`, `state__group`, `priority`, `cycle_id`, `project_id`,
//! `created_by`, `target_date`, `start_date` — the fields that keep one row per
//! issue). The m2m group fields (`labels__id`, `assignees__id`,
//! `issue_module__module_id`) and `sub_group_by` answer an explicit
//! not-implemented error: their paginator variants (multi-group membership and
//! sub-group cells, `paginator.py:334-389,567-632`) are not ported yet. The gap
//! is recorded in the ledger; no scenario covers it.

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
use plane_query::paginator::{OffsetWindow, PageQuery, ceil_div, request_now};
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

        // `paginate` validates `per_page`, `cursor` and the two group names
        // before it evaluates the queryset (`paginator.py:681-712`).
        let page = match PageQuery::parse(ctx.query(), group_by_raw, sub_group_by_raw) {
            Ok(page) => page,
            Err(error) => return map_query_error(error),
        };
        let group_by = group_by_raw.and_then(GroupField::parse);
        let sub_group_by = sub_group_by_raw.and_then(GroupField::parse);
        if sub_group_by.is_some() {
            return Err(ViewError::Unknown(
                "public issue list: sub_group_by is not ported yet".to_owned(),
            ));
        }
        if let Some(group) = group_by {
            if group.mapper().is_some() {
                return Err(ViewError::Unknown(format!(
                    "public issue list: group_by={} (an m2m field) is not ported yet",
                    group.as_str()
                )));
            }
            if group == GroupField::StateGroup {
                return Err(ViewError::Unknown(
                    "public issue list: grouping by state__group is not ported yet".to_owned(),
                ));
            }
        }

        let filters = match LegacyFilters::parse(&FilterParams::Get(ctx.query()), now.date_naive())
        {
            Ok(filters) => filters,
            Err(error) => return map_query_error(error),
        };
        let ordering = Ordering::resolve(ctx.query().get_or("order_by", "-created_at"));

        let Some(board) = board::published_board(client, anchor).await? else {
            return Ok(error_body(404, "Project is not published"));
        };
        let project_id = board.entity_identifier;
        let slug = workspace_slug(client, board.workspace_id).await?;

        let query = SpaceQuery {
            project_id,
            workspace_slug: &slug,
            filters: &filters,
            timezone,
            ordering: &ordering,
            group_by,
        };
        let base = query.build();
        let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);

        match group_by {
            None => ungrouped_page(client, &query, &base, &window).await,
            Some(group) => grouped_page(client, &query, &base, group, &window).await,
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
        // an m2m group field; the arrays filter their own rows regardless.
        if let Some(group) = self.group_by {
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
        self.filters.apply(&mut builder);
        self.filters.apply_dates(&mut builder, self.timezone);
        builder
    }
}

/// The `issue_on_results` `SELECT` list, in the space grouper's order.
fn projection(group: Option<GroupField>) -> Vec<(&'static str, Frag)> {
    let cycle_id = Frag::raw(
        "(SELECT U0.\"cycle_id\" FROM \"cycle_issues\" U0 \
         WHERE (U0.\"deleted_at\" IS NULL AND U0.\"deleted_at\" IS NULL \
         AND U0.\"issue_id\" = (\"issues\".\"id\")) \
         ORDER BY U0.\"created_at\" DESC LIMIT 1)",
    );
    let mut fields = vec![
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
        (
            "assignee_ids",
            array_aggregate(
                "issue_assignees",
                "assignee_id",
                "issue_assignees.deleted_at IS NULL",
            ),
        ),
        (
            "label_ids",
            array_aggregate(
                "issue_labels",
                "label_id",
                "issue_labels.deleted_at IS NULL",
            ),
        ),
        (
            "module_ids",
            array_aggregate("module_issues", "module_id", "TRUE"),
        ),
    ];
    // An m2m group replaces its array with the raw through column
    // (`issue_on_results`'s `original_list.remove`/`append`). Those groupings
    // are rejected before this runs.
    let _ = group;
    fields.push((
        "vote_items",
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
        ),
    ));
    fields.push((
        "reaction_items",
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
        ),
    ));
    fields
}

fn array_aggregate(table: &str, column: &str, filter: &str) -> Frag {
    Frag::raw(format!(
        "COALESCE(ARRAY_AGG(DISTINCT {table}.{column}) \
         FILTER (WHERE (NOT ({table}.{column} IS NULL) AND {filter})), ARRAY[]::uuid[])"
    ))
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

/// The ordered projection's SQL, with the extra sort columns [`Ordering`] needs
/// when the page is `DISTINCT`.
fn projected_fields(base: &FilterBuilder, ordering: &Ordering) -> Vec<Frag> {
    let mut fields: Vec<Frag> = projection(None)
        .into_iter()
        .map(|(key, expression)| aliased(expression, key))
        .collect();
    fields.push(aliased(ordering.expression.clone(), "__order_key"));
    fields.push(aliased(Frag::raw("issues.created_at"), "__created_at"));
    let _ = base;
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

fn page_statement(base: &FilterBuilder, ordering: &Ordering, limit: i64, offset: i64) -> Select {
    let mut select = Select::new("issues");
    select.joins = base.joins.to_joins();
    select.joins.extend(aggregate_joins());
    select.predicate = Some(base.predicate.clone());
    select.distinct = true;
    select.projection = projected_fields(base, ordering);
    // Django 5.2's aggregate grouping: the primary key plus every projected
    // column not functionally dependent on it (`states.group`).
    select.group_by = vec!["issues.id".to_owned(), "states.group".to_owned()];
    select.order_by = page_order(ordering);
    select.limit = Some(limit + 1);
    select.offset = Some(offset);
    select
}

/// `queryset.count()` — the windowed count collapses to one row per issue
/// (`GROUP BY issues.id` in Django's count), so `COUNT(DISTINCT issues.id)`
/// over the same predicate is the same number.
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
    let results = render_rows(rows)?;
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

/// The one-group page (`GroupedOffsetPaginator`) for a non-m2m group field.
async fn grouped_page(
    client: &Client,
    query: &SpaceQuery<'_>,
    base: &FilterBuilder,
    group: GroupField,
    window: &OffsetWindow,
) -> Result<ViewResponse, ViewError> {
    let mut inner = Select::new("issues");
    inner.joins = base.joins.to_joins();
    inner.joins.extend(aggregate_joins());
    inner.predicate = Some(base.predicate.clone());
    inner.distinct = true;
    let mut projection = projected_fields(base, query.ordering);
    let mut row_number = Frag::raw("ROW_NUMBER() OVER (PARTITION BY ");
    row_number.push(group.expression());
    row_number.push_raw(" ORDER BY ");
    // A window specification cannot reference a select-list alias, so this one
    // repeats the expressions Django puts there (`paginator.py:255-269`).
    for (index, expression) in query.ordering.paginator_order().into_iter().enumerate() {
        if index > 0 {
            row_number.push_raw(", ");
        }
        row_number.push(expression);
    }
    row_number.push_raw(") AS \"row_number\"");
    projection.push(row_number);
    inner.projection = projection;
    inner.group_by = vec!["issues.id".to_owned(), "states.group".to_owned()];
    inner.order_by = page_order(query.ordering);
    let inner = inner.to_frag();
    let mut outer = Select::from_frag(Frag {
        sql: format!("({}) AS \"qualify\"", inner.sql),
        params: inner.params,
    });
    outer.projection = vec![Frag::raw("*")];
    outer.predicate = Some(
        Frag::raw(format!("\"row_number\" > {}", window.offset))
            .and(Frag::raw(format!("\"row_number\" < {}", window.stop()))),
    );
    outer.order_by = vec![Frag::raw("\"__created_at\" DESC NULLS LAST")];
    let rows = fetch(client, outer).await?;
    let fetched = rows.len();
    let rows = &rows[..fetched.min(window.row_limit() as usize)];

    // The group keys come from `issue_group_values` (`space/utils/grouper.py`),
    // i.e. the model managers, not from the page.
    let keys = match group_values::values(&GroupValuesRequest {
        field: group,
        workspace_slug: query.workspace_slug,
        project_id: query.project_id,
        user: None,
        issues: None,
    }) {
        Ok(GroupValues::Static(values)) => values,
        Ok(GroupValues::Query { sql, append_none }) => {
            let fetched = sql.query(client).await.map_err(ViewError::from)?;
            let mut values = Vec::with_capacity(fetched.len() + 1);
            for row in &fetched {
                values.push(render_group_value(row, 0, group)?);
            }
            if append_none {
                values.push("None".to_owned());
            }
            values
        }
        Err(error) => return map_query_error(error),
    };
    let keys = dedupe(keys);

    let totals = group_totals(client, base, group).await?;
    let mut grouped: Vec<(String, Vec<Vec<u8>>)> =
        keys.iter().map(|key| (key.clone(), Vec::new())).collect();
    for row in rows {
        let key = render_group_value(row, group_index(group), group)?;
        if let Some(slot) = grouped.iter_mut().find(|(existing, _)| *existing == key) {
            slot.1.push(render_row(row)?);
        }
    }

    let mut results: Vec<(String, Vec<u8>)> = Vec::with_capacity(grouped.len());
    let mut page_count = 0usize;
    for (key, values) in grouped {
        page_count += values.len();
        let raw = totals
            .iter()
            .find(|(existing, _)| *existing == key)
            .map(|(_, count)| *count);
        // `__get_total_dict`: an existing zero-count group reports 1; an absent
        // group reports 0 (`paginator.py:307-314,548-556`).
        let total_results = match raw {
            Some(0) => 1,
            Some(count) => count,
            None => 0,
        };
        let mut value = Vec::new();
        value.extend_from_slice(b"{\"results\":");
        value.extend_from_slice(&raw_array(&values));
        value.extend_from_slice(b",\"total_results\":");
        value.extend_from_slice(&value_bytes(&Json::Int(total_results)));
        value.push(b'}');
        results.push((key, value));
    }
    let hits = count(client, total_statement(base)).await?;
    let max_group = totals.iter().map(|(_, count)| *count).max().unwrap_or(0);
    let total_pages = if page_count == 0 {
        0
    } else {
        ceil_div(max_group, window.row_limit())
    };
    let body = envelope_bytes(
        Some(group),
        None,
        hits,
        window.cursors(fetched),
        page_count,
        total_pages,
        raw_object(&results),
    );
    Ok(ok(body))
}

/// The projection index of the group column, for the fields this module
/// accepts (see `projection`'s order).
fn group_index(group: GroupField) -> usize {
    match group {
        GroupField::StateId => 2,
        GroupField::CycleId => 11,
        GroupField::CreatedBy => 12,
        GroupField::ProjectId => 9,
        GroupField::Priority => 5,
        GroupField::TargetDate => 7,
        GroupField::StartDate => 6,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The `issue_on_results` row as DRF bytes, in the projection's key order.
fn render_rows(rows: &[tokio_postgres::Row]) -> Result<Vec<Vec<u8>>, ViewError> {
    rows.iter().map(render_row).collect()
}

fn render_row(row: &tokio_postgres::Row) -> Result<Vec<u8>, ViewError> {
    let fields = decode_fields();
    let mut out = Vec::with_capacity(1024);
    out.push(b'{');
    for (index, (key, kind)) in fields.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        out.push(b'"');
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(b"\":");
        push_value(&mut out, row, index, *kind)?;
    }
    out.push(b'}');
    Ok(out)
}

#[derive(Clone, Copy)]
enum Kind {
    Uuid,
    OptUuid,
    Text,
    Float,
    Int,
    OptDate,
    UuidArray,
    JsonArray,
}

fn decode_fields() -> Vec<(&'static str, Kind)> {
    vec![
        ("id", Kind::Uuid),
        ("name", Kind::Text),
        ("state_id", Kind::OptUuid),
        ("sort_order", Kind::Float),
        ("estimate_point", Kind::OptUuid),
        ("priority", Kind::Text),
        ("start_date", Kind::OptDate),
        ("target_date", Kind::OptDate),
        ("sequence_id", Kind::Int),
        ("project_id", Kind::Uuid),
        ("parent_id", Kind::OptUuid),
        ("cycle_id", Kind::OptUuid),
        ("created_by", Kind::OptUuid),
        ("state__group", Kind::Text),
        ("assignee_ids", Kind::UuidArray),
        ("label_ids", Kind::UuidArray),
        ("module_ids", Kind::UuidArray),
        ("vote_items", Kind::JsonArray),
        ("reaction_items", Kind::JsonArray),
    ]
}

fn push_value(
    out: &mut Vec<u8>,
    row: &tokio_postgres::Row,
    index: usize,
    kind: Kind,
) -> Result<(), ViewError> {
    match kind {
        Kind::Uuid => out.extend_from_slice(&value_bytes(&Json::uuid(read(row, index)?))),
        Kind::OptUuid => out.extend_from_slice(&value_bytes(&Json::opt_uuid(read(row, index)?))),
        Kind::Text => {
            out.extend_from_slice(&value_bytes(&Json::string(read::<String>(row, index)?)))
        }
        Kind::Float => out.extend_from_slice(&value_bytes(&Json::Float(read::<f64>(row, index)?))),
        Kind::Int => out.extend_from_slice(&value_bytes(&Json::Int(i64::from(read::<i32>(
            row, index,
        )?)))),
        Kind::OptDate => out.extend_from_slice(&value_bytes(&Json::opt_date(read(row, index)?))),
        Kind::UuidArray => {
            let mut ids: Vec<Uuid> = read(row, index)?;
            ids.sort();
            out.extend_from_slice(&Json::Array(ids.into_iter().map(Json::uuid).collect()).to_drf());
        }
        Kind::JsonArray => push_json_array(out, row, index)?,
    }
    Ok(())
}

/// `ARRAY_AGG(DISTINCT <jsonb>)`: `NULL` when the aggregate's `FILTER` matched
/// no row (Django's `None`), otherwise a JSON array in the aggregate's order.
fn push_json_array(
    out: &mut Vec<u8>,
    row: &tokio_postgres::Row,
    index: usize,
) -> Result<(), ViewError> {
    let items: Option<Vec<JsonText>> = read(row, index)?;
    let Some(items) = items else {
        out.extend_from_slice(b"null");
        return Ok(());
    };
    out.push(b'[');
    for (position, item) in items.iter().enumerate() {
        if position > 0 {
            out.push(b',');
        }
        plane_serde::transcode_json(Dialect::Drf, item.as_str(), out)
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
    }
    out.push(b']');
    Ok(())
}

/// One scalar rendered at depth 1: `JSONRenderer.render(None)` is the empty
/// body only at the document's top level (`plane-serde`'s DRF dialect), so a
/// field value is wrapped in a one-element array and the brackets dropped.
fn value_bytes(value: &Json) -> Vec<u8> {
    let wrapped = Json::Array(vec![value.clone()]).to_drf();
    wrapped
        .get(1..wrapped.len().saturating_sub(1))
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

fn read<T>(row: &tokio_postgres::Row, index: usize) -> Result<T, ViewError>
where
    T: for<'a> tokio_postgres::types::FromSql<'a>,
{
    row.try_get(index).map_err(ViewError::from)
}

/// `str(value)` for one group-value or page row cell.
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

/// `COUNT(DISTINCT issues.id) FILTER (count_filter)` per group
/// (`paginator.py:299-305,502-519`).
async fn group_totals(
    client: &Client,
    base: &FilterBuilder,
    group: GroupField,
) -> Result<Vec<(String, i64)>, ViewError> {
    let mut select = Select::new("issues");
    select.joins = base.joins.to_joins();
    // The count filter's `issue_intake` join (`issue.py:203-210`).
    select.joins.push(Join::left(
        "intake_issues",
        "intake_issues",
        Frag::raw("(issues.id = intake_issues.issue_id)"),
    ));
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
    let mut totals = Vec::with_capacity(rows.len());
    for row in &rows {
        let key = render_group_value(row, 0, group)?;
        let count: i64 = read(row, 1)?;
        totals.push((key, count));
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
// The envelope, with pre-rendered `results`
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn envelope_bytes(
    group_by: Option<GroupField>,
    sub_group_by: Option<GroupField>,
    hits: i64,
    cursors: (
        plane_query::paginator::Cursor,
        plane_query::paginator::Cursor,
    ),
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
