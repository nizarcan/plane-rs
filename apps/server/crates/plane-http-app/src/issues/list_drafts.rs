// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/workspaces/{slug}/draft-issues/` and
//! `GET /api/workspaces/{slug}/draft-issues/{pk}/` —
//! `WorkspaceDraftIssueViewSet.list` / `.retrieve`
//! (`plane/app/views/workspace/draft.py:67-230`).
//!
//! Drafts are private: both actions filter `created_by=request.user`. The list
//! applies the legacy `issue_filters` query parameters and the plain
//! `OffsetPaginator` (no `order_by` is passed, so the paginator does not
//! re-order and the queryset keeps `Meta.ordering = ("-created_at",)`); the
//! retrieve is a single row by pk.
//!
//! `cycle_id`, `label_ids`, `assignee_ids` and `module_ids` are annotations over
//! the four draft through tables (each soft-deletable, each ordered by
//! `-created_at` for its array); the arrays are `ARRAY_AGG(DISTINCT)` so they
//! come back sorted.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::filter::{FilterParams, LegacyFilters};
use plane_query::json::Json;
use plane_query::paginator::{Cursor, MAX_LIMIT, PageQuery};
use plane_query::sql::{Frag, Join, Select};
use plane_serializers::FieldValue;
use plane_serializers::generated::app_draft::DraftIssueSerializer;
use uuid::Uuid;

use super::list_common::{
    client, error_body, kwarg_text, kwarg_uuid, query_error, query_error_response, rendered,
    user_id, user_timezone,
};
use crate::common::RequestRender;

/// Register both actions.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.workspace.draft.WorkspaceDraftIssueViewSet")?
        .action("list", list)?
        .action("retrieve", retrieve)?;
    Ok(())
}

/// The four draft annotations of `get_queryset` (`draft.py:70-116`).
const DRAFT_ANNOTATIONS: &str = "\
 (SELECT draft_issue_cycles.cycle_id FROM draft_issue_cycles \
   WHERE draft_issue_cycles.deleted_at IS NULL \
     AND draft_issue_cycles.draft_issue_id = issues.id \
   ORDER BY draft_issue_cycles.created_at DESC LIMIT 1) AS cycle_id, \
 COALESCE((SELECT ARRAY_AGG(DISTINCT draft_issue_labels.label_id) \
   FROM draft_issue_labels \
   WHERE draft_issue_labels.deleted_at IS NULL \
     AND draft_issue_labels.draft_issue_id = issues.id \
   GROUP BY draft_issue_labels.draft_issue_id), ARRAY[]::uuid[]) AS label_ids, \
 COALESCE((SELECT ARRAY_AGG(DISTINCT draft_issue_assignees.assignee_id) \
   FROM draft_issue_assignees \
   INNER JOIN users ON draft_issue_assignees.assignee_id = users.id \
   INNER JOIN project_members ON users.id = project_members.member_id \
   WHERE draft_issue_assignees.deleted_at IS NULL AND project_members.is_active \
     AND project_members.deleted_at IS NULL \
     AND draft_issue_assignees.draft_issue_id = issues.id \
   GROUP BY draft_issue_assignees.draft_issue_id), ARRAY[]::uuid[]) AS assignee_ids, \
 COALESCE((SELECT ARRAY_AGG(DISTINCT draft_issue_modules.module_id) \
   FROM draft_issue_modules \
   INNER JOIN modules ON draft_issue_modules.module_id = modules.id \
   WHERE draft_issue_modules.deleted_at IS NULL AND modules.archived_at IS NULL \
     AND draft_issue_modules.draft_issue_id = issues.id \
   GROUP BY draft_issue_modules.draft_issue_id), ARRAY[]::uuid[]) AS module_ids";

const DRAFT_COLUMNS: &str = "issues.id, issues.name, issues.state_id, \
     issues.sort_order, issues.completed_at, issues.estimate_point_id, \
     issues.priority, issues.start_date, issues.target_date, \
     issues.project_id, issues.parent_id, issues.created_at, \
     issues.updated_at, issues.created_by_id, issues.updated_by_id, \
     issues.type_id, issues.description_html";

/// `WorkspaceDraftIssueViewSet.list`.
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = user_timezone(ctx);
        let now = plane_query::global::now();

        // `issue_filters(request.query_params, "GET")`.
        let filters = match LegacyFilters::parse(&FilterParams::Get(ctx.query()), now.date_naive())
        {
            Ok(filters) => filters,
            Err(error) => {
                return match query_error_response(&error) {
                    Some(response) => Ok(response),
                    None => Err(query_error(error)),
                };
            }
        };
        let page = PageQuery::parse(ctx.query(), None, None).map_err(query_error)?;

        let limit = i64::from(page.per_page.min(MAX_LIMIT));
        let offset = page.cursor.offset * limit;

        let select_builder = draft_select(slug, Some(user), &filters, timezone);
        let mut select = select_builder;
        select.projection = draft_projection();
        select.order_by = vec![Frag::raw("issues.created_at DESC")];
        select.distinct = true;
        select.limit = Some(limit + 1);
        select.offset = Some(offset);
        let rows = fetch(client, &select).await?;
        let has_results = rows.len() as i64 > limit;
        let rows: Vec<&tokio_postgres::Row> = rows.iter().take(limit as usize).collect();

        // `total_count = queryset.count()` on the annotated, distinct queryset.
        let mut count_select = draft_select(slug, Some(user), &filters, timezone);
        count_select.distinct = true;
        count_select.projection = vec![Frag::raw("count(*)")];
        let total = count(client, &count_select).await?;

        let prepared = RequestRender::from_ctx(ctx);
        let render_ctx = prepared.render_ctx();
        let total_pages = if limit == 0 {
            0
        } else {
            (total + limit - 1) / limit
        };
        let mut body = envelope_prefix(
            total,
            Cursor::new(limit, page.cursor.offset + 1, false, has_results),
            Cursor::new(limit, page.cursor.offset - 1, true, page.cursor.offset > 0),
            rows.len() as i64,
            total_pages,
        );
        for (index, row) in rows.iter().enumerate() {
            if index != 0 {
                body.push(b',');
            }
            let draft = DraftRow::from(row);
            let serializer = draft.serializer();
            let _ = timezone;
            body.extend(plane_serializers::render(&serializer, &render_ctx)?);
        }
        body.extend_from_slice(b"]}");
        Ok(ViewResponse::raw(
            200,
            plane_core::error::APPLICATION_JSON,
            body,
        ))
    })
}

/// `WorkspaceDraftIssueViewSet.retrieve`.
fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = user_timezone(ctx);

        let mut select = draft_select(slug, Some(user), &LegacyFilters::default(), timezone);
        select.projection = draft_projection();
        select.distinct = true;
        select.limit = Some(1);
        select = select.where_(
            Frag::raw("issues.id = ").append(Frag::bind(plane_query::sql::Value::Uuid(pk))),
        );
        let Some(row) = fetch_opt(client, &select).await? else {
            return Ok(error_body(404, "The required object does not exist."));
        };
        let draft = DraftRow::from(&row);
        let serializer = draft.serializer();
        let prepared = RequestRender::from_ctx(ctx);
        Ok(ViewResponse::raw(
            200,
            plane_core::error::APPLICATION_JSON,
            plane_serializers::render(&serializer, &prepared.render_ctx())?,
        ))
    })
}

fn draft_select(
    slug: &str,
    user: Option<Uuid>,
    filters: &LegacyFilters,
    timezone: chrono_tz::Tz,
) -> Select {
    // The draft table is aliased as `issues` so `isnull`/date filters built for
    // the issue columns apply unchanged.
    let mut select = Select::new("draft_issues issues").join(Join::inner(
        "workspaces",
        "workspaces",
        Frag::raw("issues.workspace_id = workspaces.id"),
    ));
    let mut predicate = Frag::raw("issues.deleted_at IS NULL").and(
        Frag::raw("workspaces.slug = ")
            .append(Frag::bind(plane_query::sql::Value::Text(slug.to_owned()))),
    );
    if let Some(user) = user {
        predicate = predicate.and(
            Frag::raw("issues.created_by_id = ")
                .append(Frag::bind(plane_query::sql::Value::Uuid(user))),
        );
    }
    let mut builder = plane_query::issue::filter::FilterBuilder::new();
    builder.and(predicate);
    apply_draft_filters(filters, &mut builder, timezone);
    select.joins.extend(builder.joins.to_joins());
    select.predicate = Some(builder.predicate);
    select
}

/// `IssueFilterSet`-free legacy filters over a `DraftIssue` queryset.
///
/// The lookup names are the same as `Issue`'s; the through tables differ
/// (`draft_issue_labels`, `draft_issue_assignees`, `draft_issue_cycles`,
/// `draft_issue_modules`), and `DraftIssue` has no mentions/subscribers/intake
/// relation, so those lookups are Django `FieldError`s (a 500 here, as there).
fn apply_draft_filters(
    filters: &LegacyFilters,
    builder: &mut plane_query::issue::filter::FilterBuilder,
    timezone: chrono_tz::Tz,
) {
    use plane_query::sql::{Frag, JoinKind, Value};

    if let Some(states) = &filters.state {
        builder.require_inner(plane_query::issue::model::Relation::States);
        builder.and(Frag::in_uuids("issues.state_id", states));
    }
    if let Some(groups) = &filters.state_group
        && filters.state_group_type.is_none()
    {
        builder.require_inner(plane_query::issue::model::Relation::States);
        builder.and(Frag::in_texts("states.group", groups));
    }
    if let Some(groups) = &filters.state_group_type {
        builder.require_inner(plane_query::issue::model::Relation::States);
        builder.and(Frag::in_texts("states.group", groups));
    }
    if let Some(points) = &filters.estimate_point {
        builder.and(Frag::in_uuids("issues.estimate_point_id", points));
    }
    if let Some(priorities) = &filters.priority {
        builder.and(Frag::in_texts("issues.priority", priorities));
    }
    if filters.parent_is_null || filters.parent_is_null_sub_issue {
        builder.and(Frag::is_null("issues.parent_id"));
    }
    if let Some(parents) = &filters.parent {
        builder.and(Frag::in_uuids("issues.parent_id", parents));
    }
    if filters.labels_join {
        builder.require(
            plane_query::issue::model::Relation::IssueLabels,
            if filters.labels.is_some() {
                JoinKind::Inner
            } else {
                JoinKind::Left
            },
        );
        if filters.labels_is_null {
            builder.and(Frag::is_null("issue_labels.label_id"));
        }
        if let Some(labels) = &filters.labels {
            builder.and(Frag::in_uuids("issue_labels.label_id", labels));
        }
        builder.and(Frag::is_null("issue_labels.deleted_at"));
    }
    if filters.assignees_join {
        builder.require(
            plane_query::issue::model::Relation::IssueAssignees,
            if filters.assignees.is_some() {
                JoinKind::Inner
            } else {
                JoinKind::Left
            },
        );
        if filters.assignees_is_null {
            builder.and(Frag::is_null("issue_assignees.assignee_id"));
        }
        if let Some(assignees) = &filters.assignees {
            builder.and(Frag::in_uuids("issue_assignees.assignee_id", assignees));
        }
        builder.and(Frag::is_null("issue_assignees.deleted_at"));
    }
    if filters.created_by_is_null {
        builder.and(Frag::is_null("issues.created_by_id"));
    }
    if let Some(created_by) = &filters.created_by {
        builder.and(Frag::in_uuids("issues.created_by_id", created_by));
    }
    if let Some(name) = &filters.name_icontains {
        builder.and(Frag::icontains("issues.name", name));
    }
    if let Some(projects) = &filters.project {
        builder.and(Frag::in_uuids("issues.project_id", projects));
    }
    if filters.cycle_join {
        builder.require(
            plane_query::issue::model::Relation::CycleIssues,
            if filters.cycle.is_some() {
                JoinKind::Inner
            } else {
                JoinKind::Left
            },
        );
        if filters.cycle_is_null {
            builder.and(Frag::is_null("cycle_issues.cycle_id"));
        }
        if let Some(cycles) = &filters.cycle {
            builder.and(Frag::in_uuids("cycle_issues.cycle_id", cycles));
        }
        builder.and(Frag::is_null("cycle_issues.deleted_at"));
    }
    if filters.module_join {
        builder.require(
            plane_query::issue::model::Relation::ModuleIssues,
            if filters.module.is_some() {
                JoinKind::Inner
            } else {
                JoinKind::Left
            },
        );
        if filters.module_is_null {
            builder.and(Frag::is_null("module_issues.module_id"));
        }
        if let Some(modules) = &filters.module {
            builder.and(Frag::in_uuids("module_issues.module_id", modules));
        }
        builder.and(Frag::is_null("module_issues.deleted_at"));
    }
    if let Some(date) = filters.start_date_exact {
        builder.and(Frag::compare("issues.start_date", "=", Value::Date(date)));
    }
    if let Some(date) = filters.target_date_exact {
        builder.and(Frag::compare("issues.target_date", "=", Value::Date(date)));
    }
    if filters.start_target_date {
        builder.and(Frag::is_not_null("issues.target_date"));
        builder.and(Frag::is_not_null("issues.start_date"));
    }
    // The date groups' `AT TIME ZONE` / `::date` columns also point at
    // `issues.*`, which is the alias.
    filters.apply_dates(builder, timezone);
}

fn draft_projection() -> Vec<Frag> {
    let mut projection: Vec<Frag> = DRAFT_COLUMNS
        .split(',')
        .map(|column| Frag::raw(column.trim().to_owned()))
        .collect();
    projection.push(Frag::raw(DRAFT_ANNOTATIONS));
    projection
}

/// One draft row, owned so the serializer can borrow from it.
struct DraftRow {
    id: Uuid,
    name: Option<String>,
    state_id: Option<Uuid>,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    estimate_point: Option<Uuid>,
    priority: String,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    project_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    type_id: Option<Uuid>,
    description_html: String,
    cycle_id: Option<Uuid>,
    label_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
    module_ids: Vec<Uuid>,
}

impl DraftRow {
    fn from(row: &tokio_postgres::Row) -> Self {
        Self {
            id: row.get(0),
            name: row.get(1),
            state_id: row.get(2),
            sort_order: row.get(3),
            completed_at: row.get(4),
            estimate_point: row.get(5),
            priority: row.get(6),
            start_date: row.get(7),
            target_date: row.get(8),
            project_id: row.get(9),
            parent_id: row.get(10),
            created_at: row.get(11),
            updated_at: row.get(12),
            created_by: row.get(13),
            updated_by: row.get(14),
            type_id: row.get(15),
            description_html: row.get(16),
            cycle_id: row.get(17),
            label_ids: row.get(18),
            assignee_ids: row.get(19),
            module_ids: row.get(20),
        }
    }

    fn serializer(&self) -> DraftIssueSerializer<'_> {
        DraftIssueSerializer {
            id: self.id,
            name: self.name.as_deref(),
            state_id: self.state_id,
            sort_order: self.sort_order,
            completed_at: self.completed_at,
            estimate_point: self.estimate_point,
            priority: &self.priority,
            start_date: self.start_date,
            target_date: self.target_date,
            project_id: self.project_id,
            parent_id: self.parent_id,
            // The four annotations are in `get_queryset`
            // (`plane/app/views/workspace/draft.py:70-116`): a correlated
            // subquery (NULL when there is no cycle) and three `COALESCE(…,
            // ARRAY[]::uuid[])` aggregates. The attributes always exist, so a
            // NULL cycle is DRF's `null`, not a missing key.
            cycle_id: FieldValue::from_attribute(
                self.cycle_id.map(plane_serializers::fields::DrfPk::Uuid),
            ),
            module_ids: FieldValue::value(self.module_ids.as_slice()),
            label_ids: FieldValue::value(self.label_ids.as_slice()),
            assignee_ids: FieldValue::value(self.assignee_ids.as_slice()),
            created_at: self.created_at,
            updated_at: self.updated_at,
            created_by: self.created_by,
            updated_by: self.updated_by,
            type_id: self.type_id,
            description_html: &self.description_html,
        }
    }
}

/// The `OffsetPaginator` envelope up to (not including) `results`.
fn envelope_prefix(
    total: i64,
    next: Cursor,
    prev: Cursor,
    count: i64,
    total_pages: i64,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(256);
    body.extend_from_slice(b"{\"grouped_by\":null,\"sub_grouped_by\":null,\"total_count\":");
    body.extend_from_slice(total.to_string().as_bytes());
    body.extend_from_slice(b",\"next_cursor\":\"");
    body.extend_from_slice(next.to_string().as_bytes());
    body.extend_from_slice(b"\",\"prev_cursor\":\"");
    body.extend_from_slice(prev.to_string().as_bytes());
    body.extend_from_slice(b"\",\"next_page_results\":");
    body.extend_from_slice(if next.has_results { b"true" } else { b"false" });
    body.extend_from_slice(b",\"prev_page_results\":");
    body.extend_from_slice(if prev.has_results { b"true" } else { b"false" });
    body.extend_from_slice(b",\"count\":");
    body.extend_from_slice(count.to_string().as_bytes());
    body.extend_from_slice(b",\"total_pages\":");
    body.extend_from_slice(total_pages.to_string().as_bytes());
    body.extend_from_slice(b",\"total_results\":");
    body.extend_from_slice(total.to_string().as_bytes());
    body.extend_from_slice(b",\"extra_stats\":null,\"results\":[");
    body
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

async fn fetch_opt(
    client: &Client,
    select: &Select,
) -> Result<Option<tokio_postgres::Row>, ViewError> {
    let sql = select.clone().into_sql();
    let statement = client.prepare(&sql.text).await.map_err(ViewError::from)?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    client
        .query_opt(&statement, &params)
        .await
        .map_err(ViewError::from)
}

async fn count(client: &Client, select: &Select) -> Result<i64, ViewError> {
    let sql = select.clone().into_sql();
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

/// `Json`/`rendered` are used by the sibling list handlers.
const _: fn(u16, &Json) -> ViewResponse = rendered;
