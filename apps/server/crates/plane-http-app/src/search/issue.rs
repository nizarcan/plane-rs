// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.search.issue.IssueSearchEndpoint`
//! (`GET /api/workspaces/{slug}/projects/{project_id}/search-issues/`).
//!
//! Python: `plane/app/views/search/issue.py:19-162`. The response is
//! `Response(issues.values(...)[:100])`, i.e. DRF's `JSONEncoder` over a values
//! queryset — see [`crate::search::values`].
//!
//! ## Predicate order (and its errors)
//!
//! The view builds one queryset and then narrows it in a fixed order
//! (`:111-145`):
//!
//! 1. the membership/guest predicate;
//! 2. `project_id` when `workspace_search == "false"` (the default);
//! 3. `search_issues(query)` — `name`/`project__identifier` `icontains` plus an
//!    equality per numeric run in the query;
//! 4. `parent=true&issue_id=…`: re-lookup the issue and exclude it, its parent
//!    and its children;
//! 5. `issue_relation=true&issue_id=…`: exclude every issue related to it;
//! 6. `sub_issue=true&issue_id=…`: root issues only;
//! 7. `cycle=true`: exclude issues that are in a live cycle;
//! 8. `module=<uuid>`: exclude issues in that module;
//! 9. `target_date=none`: issues without a target date.
//!
//! The three `issue_id` filters each run their own
//! `Issue.issue_objects.filter(pk=…)` lookup (`:43,53,72`), so the round-trip
//! count matches.
//!
//! ## Upstream bugs ported as they are
//!
//! * `filter_root_issues_only` reads `issue.parent` **outside** the `if issue:`
//!   guard (`issue.py:72-77`), so an `issue_id` that does not resolve to a
//!   non-draft, non-archived issue in a live project is an `AttributeError`
//!   (500), not an empty list.
//! * `module=<invalid uuid>` reaches `issue_module__module=` with a string,
//!   which is a `ValidationError` → 400 (`COVERAGE.md` records the generated
//!   scenario's `module=true` step as that 400).

use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::sql::{Frag, Value};
use plane_serializers::RenderCtx;
use uuid::Uuid;

use crate::common::client;
use crate::search::base::{SequenceStyle, search_predicate};
use crate::search::queries;
use crate::search::values::{Cell, RowValue};

pub fn register(
    registry: &mut plane_drf::registry::Registry,
) -> Result<(), plane_drf::registry::RegistryError> {
    registry
        .api_view("plane.app.views.search.issue.IssueSearchEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

/// `IssueSearchEndpoint.get` (`plane/app/views/search/issue.py:100-162`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = crate::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )
        .map_err(ViewError::from)?;
        let user = ctx.user.id().ok_or(ViewError::Api(
            plane_core::error::ApiError::NotAuthenticated,
        ))?;
        let client = client(ctx)?;

        let query = ctx.query().get("search").filter(|value| !value.is_empty());
        let workspace_search = ctx.query().get_or("workspace_search", "false");
        let parent = ctx.query().get_or("parent", "false");
        let issue_relation = ctx.query().get_or("issue_relation", "false");
        let cycle = ctx.query().get_or("cycle", "false");
        let module = ctx.query().get("module").filter(|value| !value.is_empty());
        let sub_issue = ctx.query().get_or("sub_issue", "false");
        let target_date = ctx.query().get_or("target_date", "");
        let issue_id = ctx
            .query()
            .get("issue_id")
            .filter(|value| !value.is_empty());

        let mut sql = Frag::raw(
            "SELECT DISTINCT i.name, i.id, i.start_date, i.sequence_id, pr.name, \
             pr.identifier, i.project_id, ws.slug, s.name, s.\"group\", s.color, \
             i.created_at \
             FROM issues i \
             LEFT OUTER JOIN states s ON i.state_id = s.id \
             INNER JOIN projects pr ON i.project_id = pr.id \
             INNER JOIN project_members pm ON pr.id = pm.project_id \
             INNER JOIN workspaces ws ON i.workspace_id = ws.id \
             WHERE i.deleted_at IS NULL \
               AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) \
               AND NOT (i.archived_at IS NOT NULL) \
               AND NOT (pr.archived_at IS NOT NULL) \
               AND NOT (i.is_draft)",
        );

        // `:111-121`.
        sql.push_raw(" AND (pm.role > 5 OR pr.guest_view_all_features OR i.created_by_id = ");
        sql.push(Frag::bind(Value::Uuid(user)));
        sql.push_raw(") AND ws.slug = ");
        sql.push(Frag::bind(Value::Text(slug.to_owned())));
        sql.push_raw(" AND pm.member_id = ");
        sql.push(Frag::bind(Value::Uuid(user)));
        sql.push_raw(" AND pm.is_active AND pm.deleted_at IS NULL AND pr.archived_at IS NULL");

        // `:123-124`.
        if workspace_search == "false" {
            sql.push_raw(" AND i.project_id = ");
            sql.push(Frag::bind(Value::Uuid(project_id)));
        }

        // `:126-127`, `plane/utils/issue_search.py:14-24`.
        if let Some(query) = query {
            sql.push_raw(" AND ");
            sql.push(search_predicate(
                query,
                "i.name",
                Some("pr.identifier"),
                Some(("i.sequence_id", SequenceStyle::LengthGated)),
            ));
        }

        // `:129-136`. All three predicates are inside the `if issue:` guard,
        // so an `issue_id` that does not resolve applies none of them.
        if parent == "true" && issue_id.is_some() {
            let issue_id = issue_id_value(issue_id)?;
            if let Some(parent_id) = issue_parent(client, issue_id).await? {
                sql.push_raw(" AND NOT (i.id = ");
                sql.push(Frag::bind(Value::Uuid(issue_id)));
                sql.push_raw(")");
                // `~Q(pk=issue.parent_id)` with a NULL parent is
                // `NOT (id IS NULL)` — always true, so it is omitted.
                if let Some(parent_id) = parent_id {
                    sql.push_raw(" AND NOT (i.id = ");
                    sql.push(Frag::bind(Value::Uuid(parent_id)));
                    sql.push_raw(")");
                }
                // `~Q(parent_id=issue_id)`: Django's negation on a nullable
                // column adds the `IS NOT NULL` guard, so a row with no parent
                // is kept (`NOT (parent_id = x AND parent_id IS NOT NULL)`).
                sql.push_raw(" AND NOT (i.parent_id = ");
                sql.push(Frag::bind(Value::Uuid(issue_id)));
                sql.push_raw(" AND i.parent_id IS NOT NULL)");
            }
        }

        // `:132-133`.
        if issue_relation == "true" && issue_id.is_some() {
            let issue_id = issue_id_value(issue_id)?;
            let issue = issue_parent(client, issue_id).await?;
            // The relation query runs whether or not the issue exists
            // (`Q(issue=None)` is `IS NULL`, which matches nothing).
            let mut related = related_issue_ids(client, issue_id).await?;
            if issue.is_some() {
                related.push(issue_id);
                related.sort_unstable();
                related.dedup();
                sql.push_raw(" AND NOT (i.id IN (");
                let mut first = true;
                for id in related {
                    if !first {
                        sql.push_raw(", ");
                    }
                    first = false;
                    sql.push(Frag::bind(Value::Uuid(id)));
                }
                sql.push_raw("))");
            }
        }

        // `:135-136`.
        if sub_issue == "true" && issue_id.is_some() {
            let issue_id = issue_id_value(issue_id)?;
            let issue = issue_parent(client, issue_id).await?;
            // `if issue:` guards the filter, but `if issue.parent:` is outside
            // the guard (`issue.py:72-77`): a miss is an `AttributeError` 500.
            let Some(parent_id) = issue else {
                return Err(ViewError::Unknown(
                    "AttributeError: 'NoneType' object has no attribute 'parent'".to_owned(),
                ));
            };
            sql.push_raw(" AND NOT (i.id = ");
            sql.push(Frag::bind(Value::Uuid(issue_id)));
            sql.push_raw(") AND i.parent_id IS NULL");
            if let Some(parent_id) = parent_id {
                sql.push_raw(" AND NOT (i.id = ");
                sql.push(Frag::bind(Value::Uuid(parent_id)));
                sql.push_raw(")");
            }
        }

        // `:138-139`.
        if cycle == "true" {
            sql.push_raw(
                " AND NOT EXISTS (SELECT 1 FROM cycle_issues ci \
                   WHERE ci.issue_id = i.id AND ci.deleted_at IS NULL)",
            );
        }

        // `:141-142`.
        if let Some(module) = module {
            let module = Uuid::parse_str(module).map_err(|_| ViewError::Validation)?;
            sql.push_raw(" AND NOT EXISTS (SELECT 1 FROM module_issues mi WHERE mi.module_id = ");
            sql.push(Frag::bind(Value::Uuid(module)));
            sql.push_raw(" AND mi.issue_id = i.id AND mi.deleted_at IS NULL)");
        }

        // `:144-145`.
        if target_date == "none" {
            sql.push_raw(" AND i.target_date IS NULL");
        }

        sql.push_raw(" ORDER BY i.created_at DESC LIMIT 100");
        let rows = run(client, sql).await?;
        let mut values = Vec::with_capacity(rows.len());
        for row in &rows {
            values.push(RowValue::new(vec![
                ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
                ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
                ("start_date", opt_date(row, 2)?),
                (
                    "sequence_id",
                    Cell::Int(i64::from(
                        row.try_get::<_, i32>(3).map_err(ViewError::from)?,
                    )),
                ),
                (
                    "project__name",
                    Cell::Text(row.try_get(4).map_err(ViewError::from)?),
                ),
                (
                    "project__identifier",
                    Cell::Text(row.try_get(5).map_err(ViewError::from)?),
                ),
                (
                    "project_id",
                    Cell::Uuid(row.try_get(6).map_err(ViewError::from)?),
                ),
                (
                    "workspace__slug",
                    Cell::Text(row.try_get(7).map_err(ViewError::from)?),
                ),
                ("state__name", opt_text(row, 8)?),
                ("state__group", opt_text(row, 9)?),
                ("state__color", opt_text(row, 10)?),
            ]));
        }

        // `.values()` rows go through the DRF JSON encoder, which renders a
        // datetime in its own tzinfo (UTC from the connection) — no timezone
        // activation applies.
        let body = plane_serializers::render_list(&values, &RenderCtx::utc())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `issue_id` as the URL/query value Django would bind: a string parsed as a
/// UUID (`ValidationError` → 400 when it is not one).
fn issue_id_value(issue_id: Option<&str>) -> Result<Uuid, ViewError> {
    let raw = issue_id.ok_or(ViewError::Key)?;
    Uuid::parse_str(raw).map_err(|_| ViewError::Validation)
}

/// `Issue.issue_objects.filter(pk=issue_id).first()`: `Some(parent_id)` (which
/// is `None` when the issue has no parent) when the row exists.
async fn issue_parent(client: &Client, issue_id: Uuid) -> Result<Option<Option<Uuid>>, ViewError> {
    let statement = client
        .prepare_cached(queries::ISSUE_PARENT_BY_PK.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&issue_id])
        .await
        .map_err(ViewError::from)?;
    Ok(row.map(|row| row.get::<_, Option<Uuid>>(1)))
}

/// `IssueRelation.objects.filter(Q(related_issue=issue) | Q(issue=issue))
/// .values_list("issue_id", "related_issue_id").distinct()`, flattened
/// (`issue.py:54-60`).
async fn related_issue_ids(client: &Client, issue_id: Uuid) -> Result<Vec<Uuid>, ViewError> {
    let statement = client
        .prepare_cached(queries::RELATED_ISSUES.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&issue_id])
        .await
        .map_err(ViewError::from)?;
    let mut ids = Vec::with_capacity(rows.len() * 2);
    for row in &rows {
        ids.push(row.try_get::<_, Uuid>(0).map_err(ViewError::from)?);
        ids.push(row.try_get::<_, Uuid>(1).map_err(ViewError::from)?);
    }
    Ok(ids)
}

fn opt_text(row: &tokio_postgres::Row, index: usize) -> Result<Cell, ViewError> {
    Ok(
        match row
            .try_get::<_, Option<String>>(index)
            .map_err(ViewError::from)?
        {
            Some(value) => Cell::Text(value),
            None => Cell::Null,
        },
    )
}

fn opt_date(row: &tokio_postgres::Row, index: usize) -> Result<Cell, ViewError> {
    Ok(
        match row
            .try_get::<_, Option<chrono::NaiveDate>>(index)
            .map_err(ViewError::from)?
        {
            Some(value) => Cell::Date(value),
            None => Cell::Null,
        },
    )
}

/// Run one composed statement (dynamic SQL, so unnamed).
async fn run(client: &Client, frag: Frag) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    frag.into_sql().query(client).await.map_err(ViewError::from)
}
