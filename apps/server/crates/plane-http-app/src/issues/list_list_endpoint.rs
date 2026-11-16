// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/workspaces/{slug}/projects/{project_id}/issues/list/` —
//! `IssueListEndpoint` (`plane/app/views/issue/base.py:65-200`).
//!
//! This endpoint is not paginated: it renders every matching issue's
//! `issue_on_results`-style projection (`values(...)`, then
//! `user_timezone_converter`), or the full `IssueSerializer` when `fields=` or
//! `expand=` is present.
//!
//! It differs from `IssueViewSet.list` in four ways, all of them ported here:
//!
//! * `?issues=` is required (`400 {"error": "Issues are required"}`);
//! * the scope is `pk__in` the requested ids plus
//!   `state__deleted_at__isnull=True`, not the project's whole list;
//! * the guest restriction is a single `ProjectMember` query that joins the
//!   project and asks for `guest_view_all_features=False` (`:102-111`);
//! * it uses `order_issue_queryset`'s own ordering, not a paginator's
//!   (`NULLS LAST` is a paginator rule).
//!
//! `recent_visited_task.delay(...)` is enqueued after the group annotations and
//! before the rows are read (`:165`), so it is published before the query —
//! Django's `.values()` (which evaluates the queryset) comes later.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use plane_core::error::APPLICATION_JSON;
use plane_db::plane_query;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_query::issue::query::IssueScope;
use plane_query::json::Json;
use plane_query::sql::{Frag, Select};
use plane_serializers::FieldValue;
use plane_serializers::generated::app_issue::IssueSerializer;
use uuid::Uuid;

use crate::common::RequestRender;
use crate::common::jobs;
use crate::issues::lists::list_common::{
    self, ListRequest, client, error_body, kwarg_text, kwarg_uuid, prevalidate_list_filters,
    query_error, query_error_response, rendered, user_id, user_timezone,
};
use crate::issues::row;

plane_query! {
    /// `ProjectMember.objects.filter(workspace__slug=…, project_id=…,
    /// member=…, role=5, is_active=True, project__guest_view_all_features=False)
    /// .exists()` — `IssueListEndpoint.get` (`plane/app/views/issue/base.py:102-111`).
    ///
    /// The related `projects` and `workspaces` joins do not consult their
    /// soft-delete managers (Django joins never do).
    RESTRICTED_GUEST_IN_PROJECT = "list_restricted_guest_in_project",
    "SELECT 1 FROM project_members \
     INNER JOIN projects ON project_members.project_id = projects.id \
     INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
     WHERE workspaces.slug = $1 AND project_members.project_id = $2 \
       AND project_members.member_id = $3 AND project_members.role = 5 \
       AND project_members.is_active AND project_members.deleted_at IS NULL \
       AND projects.guest_view_all_features = false \
     LIMIT 1",
    [("exists", Int4)]
}

/// `IssueListEndpoint.filter_backends = (ComplexFilterBackend,)`
/// (`plane/app/views/issue/base.py:82`).
fn list_options() -> plane_query::ListOptions {
    plane_query::ListOptions {
        filter_backends: plane_query::FilterBackends::ComplexOnly,
        ..plane_query::ListOptions::default()
    }
}

/// Register `IssueListEndpoint.get`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.base.IssueListEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

/// `IssueListEndpoint.get`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `issue_ids = request.GET.get("issues", False)`;
        // `if not issue_ids: return 400` (`:67-71`).
        let Some(raw_ids) = ctx.query().get("issues").filter(|value| !value.is_empty()) else {
            return Ok(error_body(400, "Issues are required"));
        };
        let issue_ids: Vec<Uuid> = raw_ids
            .split(',')
            .filter(|value| !value.is_empty())
            .map(|value| Uuid::parse_str(value).map_err(|_| ViewError::Validation))
            .collect::<Result<_, _>>()?;

        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = user_timezone(ctx);
        let now = plane_query::global::now();

        // `ProjectMember.objects.filter(..., project__guest_view_all_features=False)`
        // (`:102-111`), a single statement.
        let restricted_guest = {
            let statement = client
                .prepare_cached(RESTRICTED_GUEST_IN_PROJECT.sql)
                .await
                .map_err(ViewError::from)?;
            client
                .query_opt(&statement, &[&slug, &project_id, &user])
                .await
                .map_err(ViewError::from)?
                .is_some()
        };

        // `self.filter_queryset(...)` then `issue_filters(...)`; both are pure
        // and both run before `recent_visited_task.delay`.
        if let Err(error) = prevalidate_list_filters(ctx.query(), timezone, now) {
            return match query_error_response(&error) {
                Some(response) => Ok(response),
                None => Err(query_error(error)),
            };
        }

        // `recent_visited_task.delay(slug, project_id, entity_name="project",
        // entity_identifier=project_id, user_id)` (`:165`).
        jobs::recent_visited_project(
            ctx,
            "project",
            TaskArg::uuid(project_id),
            Some(TaskArg::uuid(project_id)),
            TaskArg::uuid(user),
        )
        .await?;

        // The two predicates outside `issue_filters`: the requested ids
        // (`:75`) and `state__deleted_at__isnull=True` (`:114`).
        let mut extra = Frag::is_null("states.deleted_at");
        if issue_ids.is_empty() {
            // `pk__in=[]` never matches.
            extra = extra.and(Frag::raw("FALSE"));
        } else {
            extra = extra.and(Frag::in_uuids("issues.id", &issue_ids));
        }

        let scope = IssueScope {
            workspace_slug: slug.to_owned(),
            project_id: Some(project_id),
            distinct: true,
            created_by: if restricted_guest { Some(user) } else { None },
        };

        // `if self.fields or self.expand: IssueSerializer(...)` (`:167-168`).
        if ctx.fields().is_some() || ctx.expand().is_some() {
            return serialized(ctx, client, scope, timezone, now, extra).await;
        }

        let request = ListRequest {
            query: ctx.query(),
            scope,
            timezone,
            now,
            extra,
            options: list_options(),
        };
        // `run_issue_list` is the paginated shape; the values projection and
        // the ordering differ, so the pieces are composed here instead.
        let query = match list_common::build_issue_query(&request) {
            Ok(query) => query,
            Err(error) => return map_error(error),
        };

        let fields = row::value_fields();
        let mut select = query.filtered_select();
        select.projection = row::projection();
        if select.distinct {
            query.ordering.apply_own_order(&mut select, &fields);
        } else {
            select.order_by = query.ordering.own_order();
        }
        if query.ordering.group_by_issue {
            // `Min(...)` puts the query in `GROUP BY issues.id`; every selected
            // column belongs to `issues`, so the primary key collapses the list
            // (Django's `collapse_group_by`).
            select.group_by = vec!["issues.id".to_owned()];
        }

        let rows = fetch(client, &select).await?;
        let mut body: Vec<Json> = Vec::with_capacity(rows.len());
        for row in &rows {
            body.push(row::shift_timezone(row::decode_row(row)?, timezone));
        }

        Ok(rendered(200, &Json::Array(body)))
    })
}

/// The `fields=` / `expand=` shape: `IssueSerializer(issue_queryset)`.
///
/// The serializer's 25 declared fields are exactly the projection's first 25
/// columns, so the values are rendered straight from the decoded row; `expand`
/// names are resolved by the renderer and a name this route does not resolve is
/// a 500 (Django raises `AttributeError` there, see the module report).
async fn serialized<'a>(
    ctx: &'a ViewCtx<'a>,
    client: &deadpool_postgres::Client,
    scope: IssueScope,
    timezone: Tz,
    now: DateTime<Utc>,
    extra: Frag,
) -> Result<ViewResponse, ViewError> {
    let request = ListRequest {
        query: ctx.query(),
        scope,
        timezone,
        now,
        extra,
        options: list_options(),
    };
    let query = list_common::build_issue_query(&request)
        .map_err(|error| ViewError::Unknown(error.message()))?;

    let fields = row::value_fields();
    let mut select = query.filtered_select();
    // The full 26-column projection: `decode_row` reads `LIST_FIELDS` by
    // position, so the statement and the decoder must agree even though the
    // serializer only consumes the model columns.
    select.projection = row::projection();
    if select.distinct {
        query.ordering.apply_own_order(&mut select, &fields);
    } else {
        select.order_by = query.ordering.own_order();
    }
    if query.ordering.group_by_issue {
        select.group_by = vec!["issues.id".to_owned()];
    }

    let rows = fetch(client, &select).await?;
    let prepared = RequestRender::from_ctx(ctx);
    let render_ctx = prepared.render_ctx();
    let mut body = Vec::with_capacity(rows.len() * 512 + 2);
    body.push(b'[');
    for (index, row) in rows.iter().enumerate() {
        let value = row::decode_row(row)?;
        let Json::Object(entries) = &value else {
            return Err(ViewError::Unknown("unexpected projection shape".to_owned()));
        };
        let get = |key: &str| {
            entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        };
        let name = as_string(get("name"))?;
        let priority = as_string(get("priority"))?;
        let module_ids = as_uuid_list(get("module_ids"))?;
        let label_ids = as_uuid_list(get("label_ids"))?;
        let assignee_ids = as_uuid_list(get("assignee_ids"))?;
        let cycle_id = as_opt_uuid(get("cycle_id"))?;
        let serializer = IssueSerializer {
            id: required_uuid(get("id"))?,
            name: &name,
            state_id: as_opt_uuid(get("state_id"))?,
            sort_order: as_float(get("sort_order"))?,
            completed_at: as_opt_datetime(get("completed_at"))?,
            estimate_point: as_opt_uuid(get("estimate_point"))?,
            priority: &priority,
            start_date: as_opt_date(get("start_date"))?,
            target_date: as_opt_date(get("target_date"))?,
            sequence_id: as_int(get("sequence_id"))?,
            project_id: required_uuid(get("project_id"))?,
            parent_id: as_opt_uuid(get("parent_id"))?,
            // Every annotation below is applied unconditionally
            // (`plane/app/views/issue/base.py:129-155`) and every projection
            // key exists in `.values()` (`:174-201`), so the attribute is
            // never missing. A SQL NULL is DRF's `null`
            // (`rest_framework/serializers.py:541-546`).
            cycle_id: FieldValue::from_attribute(
                cycle_id.map(plane_serializers::fields::DrfPk::Uuid),
            ),
            module_ids: FieldValue::value(module_ids.as_slice()),
            label_ids: FieldValue::value(label_ids.as_slice()),
            assignee_ids: FieldValue::value(assignee_ids.as_slice()),
            sub_issues_count: FieldValue::from_attribute(as_opt_int(get("sub_issues_count"))?),
            created_at: required_datetime(get("created_at"))?,
            updated_at: required_datetime(get("updated_at"))?,
            created_by: as_opt_uuid(get("created_by"))?,
            updated_by: as_opt_uuid(get("updated_by"))?,
            attachment_count: FieldValue::from_attribute(as_opt_int(get("attachment_count"))?),
            link_count: FieldValue::from_attribute(as_opt_int(get("link_count"))?),
            is_draft: as_bool(get("is_draft"))?,
            archived_at: as_opt_date(get("archived_at"))?,
        };
        if index != 0 {
            body.push(b',');
        }
        body.extend(plane_serializers::render(&serializer, &render_ctx)?);
    }
    body.push(b']');
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

fn map_error(error: plane_query::QueryError) -> Result<ViewResponse, ViewError> {
    match query_error_response(&error) {
        Some(response) => Ok(response),
        None => Err(query_error(error)),
    }
}

fn shape_error() -> ViewError {
    ViewError::Unknown("unexpected value in the issue projection".to_owned())
}

fn as_string(value: Option<Json>) -> Result<String, ViewError> {
    match value {
        Some(Json::Str(value)) => Ok(value),
        _ => Err(shape_error()),
    }
}

fn as_float(value: Option<Json>) -> Result<f64, ViewError> {
    match value {
        Some(Json::Float(value)) => Ok(value),
        _ => Err(shape_error()),
    }
}

fn as_int(value: Option<Json>) -> Result<i64, ViewError> {
    match value {
        Some(Json::Int(value)) => Ok(value),
        _ => Err(shape_error()),
    }
}

fn as_opt_int(value: Option<Json>) -> Result<Option<i64>, ViewError> {
    match value {
        Some(Json::Int(value)) => Ok(Some(value)),
        Some(Json::Null) | None => Ok(None),
        _ => Err(shape_error()),
    }
}

fn as_bool(value: Option<Json>) -> Result<bool, ViewError> {
    match value {
        Some(Json::Bool(value)) => Ok(value),
        _ => Err(shape_error()),
    }
}

fn required_uuid(value: Option<Json>) -> Result<Uuid, ViewError> {
    as_opt_uuid(value)?.ok_or_else(shape_error)
}

fn as_opt_uuid(value: Option<Json>) -> Result<Option<Uuid>, ViewError> {
    match value {
        Some(Json::Str(value)) => Uuid::parse_str(&value).map(Some).map_err(|_| shape_error()),
        Some(Json::Null) | None => Ok(None),
        _ => Err(shape_error()),
    }
}

fn as_uuid_list(value: Option<Json>) -> Result<Vec<Uuid>, ViewError> {
    match value {
        Some(Json::Array(items)) => items
            .into_iter()
            .map(|item| match item {
                Json::Str(value) => Uuid::parse_str(&value).map_err(|_| shape_error()),
                _ => Err(shape_error()),
            })
            .collect(),
        _ => Err(shape_error()),
    }
}

fn required_datetime(value: Option<Json>) -> Result<DateTime<Utc>, ViewError> {
    as_opt_datetime(value)?.ok_or_else(shape_error)
}

fn as_opt_datetime(value: Option<Json>) -> Result<Option<DateTime<Utc>>, ViewError> {
    match value {
        Some(Json::DateTime(value)) => Ok(Some(value)),
        Some(Json::Null) | None => Ok(None),
        _ => Err(shape_error()),
    }
}

fn as_opt_date(value: Option<Json>) -> Result<Option<NaiveDate>, ViewError> {
    match value {
        Some(Json::Date(value)) => Ok(Some(value)),
        Some(Json::Null) | None => Ok(None),
        _ => Err(shape_error()),
    }
}

async fn fetch(
    client: &deadpool_postgres::Client,
    select: &Select,
) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let sql = select.clone().into_sql();
    sql.query(client).await.map_err(ViewError::from)
}
