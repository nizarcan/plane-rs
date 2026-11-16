// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/base.py` — the user's workspace list and the
//! workspace dashboard.
//!
//! * `GET /api/users/me/workspaces/` — `UserWorkSpacesEndpoint.get`
//! * `GET /api/users/me/workspaces/{slug}/dashboard/` —
//!   `UserWorkspaceDashboardEndpoint.get`
//!
//! Both are `IsAuthenticated` only: the dashboard route does not run a
//! workspace permission class, so an authenticated user can read any
//! workspace's aggregate counts for themselves. That is upstream behaviour and
//! is ported as it is.

use chrono::{Datelike, NaiveDate};
use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::workspace::Workspace;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;
use plane_serde::values::PyDate;
use plane_serializers::FieldValue;
use plane_serializers::fields::DrfUuid;
use plane_serializers::generated::app_workspace::WorkSpaceSerializer;
// `icontains`' LIKE escaping has one implementation, in `plane-query`
// (`django/db/backends/base/operations.py:516-518`).
use plane_query::sql::prep_for_like_query as escape_like;
use plane_query::sql::{Sql, Value};
use serde::Serialize;

use super::queries;
use super::support;
use crate::common::asset_url::{self, AssetRef};

/// Register this module's handlers.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.workspace.base.UserWorkSpacesEndpoint")?
        .method(Method::GET, workspaces)?;
    registry
        .api_view("plane.app.views.workspace.base.UserWorkspaceDashboardEndpoint")?
        .method(Method::GET, dashboard)?;
    Ok(())
}

/// The `?owner=` filter's django-filter validation
/// (`django_filters.filters.ModelChoiceFilter` over the generated filterset).
///
/// `ModelChoiceFilter` validates through `User.objects.get(pk=value)`, so an
/// unparsable value raises the model `UUIDField`'s message and a well-formed
/// but missing value raises the choice message. Both are DRF
/// `ValidationError`s rendered as `{field: [message]}`.
async fn validate_owner(ctx: &ViewCtx<'_>, value: &str) -> Result<Option<uuid::Uuid>, ViewError> {
    let client = crate::common::client(ctx)?;
    let id = uuid::Uuid::parse_str(value).map_err(|_| {
        ViewError::Api(plane_core::ApiError::ValidationError(serde_json::json!({
            "owner": [format!("“{value}” is not a valid UUID.")],
        })))
    })?;
    let statement = queries::USER_EXISTS.prepare(client).await?;
    if client.query_opt(&statement, &[&id]).await?.is_none() {
        return Err(ViewError::Api(plane_core::ApiError::ValidationError(
            serde_json::json!({
                "owner": ["Select a valid choice. That choice is not one of the available choices."],
            }),
        )));
    }
    Ok(Some(id))
}

/// `UserWorkSpacesEndpoint.get` (`plane/app/views/workspace/base.py:176-212`).
///
/// The view is a `DjangoFilterBackend` + `SearchFilter` APIView with
/// `filterset_fields = ["owner"]` and `search_fields = ["name"]`; both backends
/// run before the serializer, and both are reproduced here. The serializer is
/// a `DynamicBaseSerializer`, so `?fields=` masks its output.
fn workspaces<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user_id = support::user_id(ctx)?;
        let client = crate::common::client(ctx)?;

        // DjangoFilterBackend first: `owner` is a ModelChoiceFilter.
        let owner = match ctx.query().get("owner") {
            Some(value) if !value.is_empty() => validate_owner(ctx, value).await?,
            _ => None,
        };

        let mut params: Vec<Value> = vec![Value::Uuid(user_id)];
        let mut filters = String::new();
        if let Some(owner) = owner {
            params.push(Value::Uuid(owner));
            filters.push_str(&format!(" AND w.\"owner_id\" = ${}", params.len()));
        }

        // SearchFilter: `params.replace("\x00", "").split()`, one AND
        // `name__icontains` per term.
        let search = ctx.query().get_or("search", "").replace('\0', "");
        for term in search.split_whitespace() {
            params.push(Value::Text(format!("%{}%", escape_like(term))));
            filters.push_str(&format!(
                " AND UPPER(w.\"name\"::text) LIKE UPPER(${})",
                params.len()
            ));
        }

        let sql = Sql {
            text: format!(
                "SELECT DISTINCT w.\"created_at\" AS created_at, \
             w.\"updated_at\" AS updated_at, w.\"created_by_id\" AS created_by_id, \
             w.\"updated_by_id\" AS updated_by_id, w.\"deleted_at\" AS deleted_at, \
             w.\"id\" AS id, w.\"name\" AS name, w.\"logo\" AS logo, \
             w.\"logo_asset_id\" AS logo_asset_id, w.\"owner_id\" AS owner_id, \
             w.\"slug\" AS slug, w.\"organization_size\" AS organization_size, \
             w.\"timezone\" AS timezone, w.\"background_color\" AS background_color, \
             (SELECT U0.\"role\" FROM workspace_members U0 \
              WHERE U0.\"deleted_at\" IS NULL AND U0.\"is_active\" \
                AND U0.\"member_id\" = $1 AND U0.\"workspace_id\" = w.\"id\" \
              ORDER BY U0.\"created_at\" DESC) AS role, \
             (SELECT COUNT(U0.\"id\") FROM workspace_members U0 \
              INNER JOIN users U1 ON U0.\"member_id\" = U1.\"id\" \
              WHERE U0.\"deleted_at\" IS NULL AND U0.\"is_active\" \
                AND NOT U1.\"is_bot\" AND U0.\"workspace_id\" = w.\"id\") AS total_members, \
             la.\"id\" AS logo_id, la.\"entity_type\" AS logo_entity_type, \
             law.\"slug\" AS logo_workspace_slug, la.\"project_id\" AS logo_project_id, \
             la.\"issue_id\" AS logo_issue_id \
             FROM workspaces w \
             INNER JOIN workspace_members wm ON w.\"id\" = wm.\"workspace_id\" \
             LEFT JOIN file_assets la ON la.\"id\" = w.\"logo_asset_id\" \
             LEFT JOIN workspaces law ON law.\"id\" = la.\"workspace_id\" \
             WHERE w.\"deleted_at\" IS NULL AND wm.\"is_active\" \
               AND wm.\"member_id\" = $1{filters} \
             ORDER BY w.\"created_at\" DESC"
            ),
            params,
        };
        let rows = sql.query(client).await?;

        let mut logo_urls: Vec<Option<String>> = Vec::with_capacity(rows.len());
        let mut items: Vec<WorkSpaceRow> = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let workspace = Workspace::from_row(row, 0)?;
            let role: Option<i16> = row.try_get(14)?;
            let total_members: i64 = row.try_get(15)?;
            let logo = AssetRef::from_row(row, 16)?;
            logo_urls.push(asset_url::workspace_logo_url(
                workspace.logo.as_deref(),
                &logo,
            )?);
            items.push(WorkSpaceRow {
                workspace,
                role,
                total_members,
                index,
            });
        }

        let serializers: Vec<WorkSpaceSerializer> = items
            .iter()
            .map(|item| WorkSpaceSerializer {
                id: item.workspace.id,
                // `annotate(role=role, total_members=member_count)`
                // (`plane/app/views/workspace/base.py:199-202`): both
                // attributes always exist. `role` is a correlated subquery and
                // can be SQL NULL, which DRF writes as `null`.
                total_members: FieldValue::from_attribute(Some(item.total_members)),
                logo_url: logo_urls[item.index].as_deref(),
                role: FieldValue::from_attribute(item.role.map(i64::from)),
                created_at: item.workspace.created_at,
                updated_at: item.workspace.updated_at,
                deleted_at: item.workspace.deleted_at,
                name: &item.workspace.name,
                logo: item.workspace.logo.as_deref(),
                slug: &item.workspace.slug,
                organization_size: item.workspace.organization_size.as_deref(),
                timezone: &item.workspace.timezone,
                background_color: &item.workspace.background_color,
                created_by: item.workspace.created_by_id,
                updated_by: item.workspace.updated_by_id,
                logo_asset: item.workspace.logo_asset_id,
                owner: item.workspace.owner_id,
            })
            .collect();

        // `WorkSpaceSerializer(..., fields=fields if fields else None)` — the
        // view passes `fields=` but never `expand=`, so `?expand=` is ignored
        // (`plane/app/views/workspace/base.py:206-210`).
        let fields = ctx.fields().map(|names| {
            names
                .into_iter()
                .map(plane_serializers::FieldEntry::name)
                .collect::<Vec<_>>()
        });
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let render_ctx = plane_serializers::RenderCtx::new(&timezone, fields.as_deref(), &[]);
        let body = crate::common::render_list(&serializers, &render_ctx)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// One decoded workspace row and the index of its computed `logo_url`.
struct WorkSpaceRow {
    workspace: Workspace,
    role: Option<i16>,
    total_members: i64,
    index: usize,
}

#[derive(Serialize)]
struct ActivityRow {
    created_date: PyDate,
    activity_count: i64,
}

#[derive(Serialize)]
struct CompletedIssuesRow {
    week_in_month: i32,
    completed_count: i64,
}

#[derive(Serialize)]
struct StateDistributionRow {
    state_group: Option<String>,
    state_count: i64,
}

#[derive(Serialize)]
struct OverdueIssueRow {
    id: DrfUuid,
    name: String,
    #[serde(rename = "workspace__slug")]
    workspace_slug: String,
    project_id: DrfUuid,
    target_date: Option<PyDate>,
}

#[derive(Serialize)]
struct UpcomingIssueRow {
    id: DrfUuid,
    name: String,
    #[serde(rename = "workspace__slug")]
    workspace_slug: String,
    project_id: DrfUuid,
    start_date: Option<PyDate>,
}

#[derive(Serialize)]
struct Dashboard {
    issue_activities: Vec<ActivityRow>,
    completed_issues: Vec<CompletedIssuesRow>,
    assigned_issues_count: i64,
    pending_issues_count: i64,
    completed_issues_count: i64,
    issues_due_week_count: i64,
    state_distribution: Vec<StateDistributionRow>,
    overdue_issues: Vec<OverdueIssueRow>,
    upcoming_issues: Vec<UpcomingIssueRow>,
}

/// `int(request.GET.get("month", 1))`.
///
/// A value `int()` rejects is the same uncaught `ValueError` Django answers
/// with a 500.
pub(super) fn month_parameter(ctx: &ViewCtx<'_>) -> Result<i64, ViewError> {
    match ctx.query().get("month") {
        None => Ok(1),
        Some(raw) => support::python_int(raw)
            .ok_or_else(|| ViewError::Unknown("ValueError: invalid literal for int()".to_string())),
    }
}

/// `UserWorkspaceDashboardEndpoint.get`
/// (`plane/app/views/workspace/base.py:234-320`).
fn dashboard<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user_id = support::user_id(ctx)?;
        let month = month_parameter(ctx)?;
        let client = crate::common::client(ctx)?;

        // `date.today()` for the activity window, `timezone.now().date()` for
        // the issue date comparisons. The two differ only around midnight in a
        // non-UTC process timezone, as they do in Python.
        let activity_since = support::months_ago(3);
        let utc_today: NaiveDate = plane_core::clock::now().date_naive();
        let due_week = i64::from(plane_core::clock::now().date_naive().iso_week().week());

        let statement = queries::DASHBOARD_COUNTS.prepare(client).await?;
        let counts = client
            .query_one(&statement, &[&user_id, &slug, &due_week])
            .await?;
        let assigned_issues_count: i64 = counts.try_get(0)?;
        let pending_issues_count: i64 = counts.try_get(1)?;
        let completed_issues_count: i64 = counts.try_get(2)?;
        let issues_due_week_count: i64 = counts.try_get(3)?;

        let statement = queries::DASHBOARD_ACTIVITY_GRAPH.prepare(client).await?;
        let activity_rows = client
            .query(&statement, &[&user_id, &activity_since, &slug])
            .await?;
        let issue_activities = activity_rows
            .iter()
            .map(|row| {
                Ok::<_, tokio_postgres::Error>(ActivityRow {
                    created_date: PyDate(row.try_get(0)?),
                    activity_count: row.try_get(1)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let statement = queries::DASHBOARD_COMPLETED_ISSUES.prepare(client).await?;
        let completed_rows = client.query(&statement, &[&user_id, &month, &slug]).await?;
        let completed_issues = completed_rows
            .iter()
            .map(|row| {
                Ok::<_, tokio_postgres::Error>(CompletedIssuesRow {
                    week_in_month: row.try_get(0)?,
                    completed_count: row.try_get(1)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let statement = queries::DASHBOARD_STATE_DISTRIBUTION
            .prepare(client)
            .await?;
        let distribution_rows = client.query(&statement, &[&user_id, &slug]).await?;
        let state_distribution = distribution_rows
            .iter()
            .map(|row| {
                Ok::<_, tokio_postgres::Error>(StateDistributionRow {
                    state_group: row.try_get(0)?,
                    state_count: row.try_get(1)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let statement = queries::DASHBOARD_OVERDUE_ISSUES.prepare(client).await?;
        let overdue_rows = client
            .query(&statement, &[&user_id, &utc_today, &slug])
            .await?;
        let overdue_issues = overdue_rows
            .iter()
            .map(|row| {
                Ok::<_, tokio_postgres::Error>(OverdueIssueRow {
                    id: DrfUuid(row.try_get(0)?),
                    name: row.try_get(1)?,
                    workspace_slug: row.try_get(2)?,
                    project_id: DrfUuid(row.try_get(3)?),
                    target_date: row.try_get::<_, Option<NaiveDate>>(4)?.map(PyDate),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let statement = queries::DASHBOARD_UPCOMING_ISSUES.prepare(client).await?;
        let upcoming_rows = client
            .query(&statement, &[&user_id, &utc_today, &slug])
            .await?;
        let upcoming_issues = upcoming_rows
            .iter()
            .map(|row| {
                Ok::<_, tokio_postgres::Error>(UpcomingIssueRow {
                    id: DrfUuid(row.try_get(0)?),
                    name: row.try_get(1)?,
                    workspace_slug: row.try_get(2)?,
                    project_id: DrfUuid(row.try_get(3)?),
                    start_date: row.try_get::<_, Option<NaiveDate>>(4)?.map(PyDate),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let body = plane_serde::to_vec(
            Dialect::Drf,
            &Dashboard {
                issue_activities,
                completed_issues,
                assigned_issues_count,
                pending_issues_count,
                completed_issues_count,
                issues_due_week_count,
                state_distribution,
                overdue_issues,
                upcoming_issues,
            },
        )?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_escaping_matches_django() {
        assert_eq!(escape_like("plain"), "plain");
        assert_eq!(escape_like("100%_done"), "100\\%\\_done");
        assert_eq!(escape_like("a\\b"), "a\\\\b");
    }
}
