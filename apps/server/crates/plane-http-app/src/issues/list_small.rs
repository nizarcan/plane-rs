// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Three small issue read routes that share nothing but their lookup style:
//!
//! * `GET /api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/meta/`
//!   — `IssueMetaEndpoint.get` (`plane/app/views/issue/base.py:1090-1102`);
//! * `GET /api/workspaces/{slug}/projects/{project_id}/deleted-issues/` —
//!   `DeletedIssuesListViewSet.get` (`:731-747`);
//! * `GET /api/workspaces/{slug}/projects/{project_id}/user-properties/` —
//!   `ProjectUserDisplayPropertyEndpoint.get` (`:560-564`).
//!
//! The third one is a `GET` that writes: `get_or_create` inserts a
//! `ProjectUserProperty` row the first time a user opens a project's work
//! items, and the response then serialises it. The insert reproduces Django's
//! `ProjectBaseModel.save` (which copies `project.workspace`) and
//! `BaseModel.save` (audit columns from the request user), so the database diff
//! matches.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use http::Method;
use plane_db::plane_query;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::json::Json;
use plane_serializers::generated::app_issue::ProjectUserPropertySerializer;
use uuid::Uuid;

use super::list_common::{
    client, kwarg_text, kwarg_uuid, parse_django_datetime, rendered, user_id,
};
use crate::common::render;

/// Register the three routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.base.IssueMetaEndpoint")?
        .method(Method::GET, meta)?;
    registry
        .api_view("plane.app.views.issue.base.DeletedIssuesListViewSet")?
        .method(Method::GET, deleted)?;
    registry
        .api_view("plane.app.views.issue.base.ProjectUserDisplayPropertyEndpoint")?
        .method(Method::GET, user_properties)?;
    Ok(())
}

plane_query! {
    /// `Issue.issue_objects.only("sequence_id", "project__identifier").get(...)`
    /// (`plane/app/views/issue/base.py:1092-1094`).
    ISSUE_META = "list_issue_meta",
    "SELECT issues.sequence_id, projects.identifier \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
       AND (states.group IS NULL OR states.group <> 'triage') \
       AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
       AND NOT issues.is_draft \
       AND issues.id = $1 AND issues.project_id = $2 AND workspaces.slug = $3",
    [("sequence_id", Int4), ("identifier", Varchar)]
}

plane_query! {
    /// `Issue.all_objects.filter(workspace__slug=…, project_id=…)
    /// .filter(Q(archived_at__isnull=False) | Q(deleted_at__isnull=False))
    /// .values_list("id", flat=True)` (`plane/app/views/issue/base.py:735-746`).
    ///
    /// `all_objects` is the plain manager, so soft-deleted and archived rows
    /// are exactly what this lists. `Meta.ordering = ("-created_at",)` still
    /// applies to a `values_list`.
    DELETED_ISSUES = "list_deleted_issues",
    "SELECT issues.id \
     FROM issues \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE workspaces.slug = $1 AND issues.project_id = $2 \
       AND (issues.archived_at IS NOT NULL OR issues.deleted_at IS NOT NULL) \
     ORDER BY issues.created_at DESC",
    [("id", Uuid)]
}

plane_query! {
    /// Same as [`DELETED_ISSUES`] plus the `updated_at__gt` filter the view adds
    /// (`plane/app/views/issue/base.py:734,739`).
    DELETED_ISSUES_SINCE = "list_deleted_issues_since",
    "SELECT issues.id \
     FROM issues \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE workspaces.slug = $1 AND issues.project_id = $2 \
       AND (issues.archived_at IS NOT NULL OR issues.deleted_at IS NOT NULL) \
       AND issues.updated_at > $3 \
     ORDER BY issues.created_at DESC",
    [("id", Uuid)]
}

plane_query! {
    /// `ProjectUserProperty.objects.get(user=request.user, project_id=…)`
    /// (`plane/app/views/issue/base.py:562`), with the default manager's
    /// `deleted_at IS NULL`.
    PROJECT_USER_PROPERTY = "list_project_user_property",
    "SELECT created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
     project_id, workspace_id, user_id, filters, display_filters, \
     display_properties, rich_filters, preferences, sort_order \
     FROM project_user_properties \
     WHERE user_id = $1 AND project_id = $2 AND deleted_at IS NULL \
     ORDER BY created_at DESC \
     LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("user_id", Uuid),
        ("filters", Jsonb),
        ("display_filters", Jsonb),
        ("display_properties", Jsonb),
        ("rich_filters", Jsonb),
        ("preferences", Jsonb),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `ProjectUserProperty.objects.create(user=…, project_id=…)` followed by
    /// `ProjectBaseModel.save`'s `self.workspace = self.project.workspace`
    /// (`plane/db/models/project.py:237-239`) and `BaseModel.save`'s audit
    /// columns. One statement instead of Django's project fetch plus insert.
    ///
    /// The five JSON defaults are `get_default_filters`,
    /// `get_default_display_filters`, `get_default_display_properties`,
    /// `rich_filters={}`, `get_default_preferences`
    /// (`plane/db/models/issue.py:47-88`); `sort_order=65535` is the field
    /// default.
    CREATE_PROJECT_USER_PROPERTY = "list_create_project_user_property",
    "INSERT INTO project_user_properties ( \
       created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
       project_id, workspace_id, user_id, filters, display_filters, \
       display_properties, rich_filters, preferences, sort_order) \
     SELECT $1, $1, $2, $2, NULL, $3, p.id, p.workspace_id, $2, \
       '{\"priority\": null, \"state\": null, \"state_group\": null, \
         \"assignees\": null, \"created_by\": null, \"labels\": null, \
         \"start_date\": null, \"target_date\": null, \"subscriber\": null}'::jsonb, \
       '{\"group_by\": null, \"order_by\": \"-created_at\", \"type\": null, \
         \"sub_issue\": true, \"show_empty_groups\": true, \"layout\": \"list\", \
         \"calendar_date_range\": \"\"}'::jsonb, \
       '{\"assignee\": true, \"attachment_count\": true, \"created_on\": true, \
         \"due_date\": true, \"estimate\": true, \"key\": true, \"labels\": true, \
         \"link\": true, \"priority\": true, \"start_date\": true, \
         \"state\": true, \"sub_issue_count\": true, \"updated_on\": true}'::jsonb, \
       '{}'::jsonb, \
       '{\"pages\": {\"block_display\": true}, \"navigation\": {\"default_tab\": \
         \"work_items\", \"hide_in_more_menu\": []}}'::jsonb, \
       65535 \
     FROM projects p \
     WHERE p.id = $4",
    []
}

/// `IssueMetaEndpoint.get`.
fn meta<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(ISSUE_META.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&statement, &[&issue_id, &project_id, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let sequence_id: i32 = row.get(0);
        let project_identifier: String = row.get(1);
        Ok(rendered(
            200,
            &Json::object(vec![
                ("sequence_id", Json::Int(i64::from(sequence_id))),
                ("project_identifier", Json::string(project_identifier)),
            ]),
        ))
    })
}

/// `DeletedIssuesListViewSet.get`.
fn deleted<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let client = client(ctx)?;
        let ids: Vec<Uuid> = match ctx.query().get("updated_at__gt") {
            Some(value) => {
                let since = parse_django_datetime(value)?;
                let statement = client
                    .prepare_cached(DELETED_ISSUES_SINCE.sql)
                    .await
                    .map_err(ViewError::from)?;
                client
                    .query(&statement, &[&slug, &project_id, &since])
                    .await
                    .map_err(ViewError::from)?
                    .iter()
                    .map(|row| row.get(0))
                    .collect()
            }
            None => {
                let statement = client
                    .prepare_cached(DELETED_ISSUES.sql)
                    .await
                    .map_err(ViewError::from)?;
                client
                    .query(&statement, &[&slug, &project_id])
                    .await
                    .map_err(ViewError::from)?
                    .iter()
                    .map(|row| row.get(0))
                    .collect()
            }
        };
        Ok(rendered(
            200,
            &Json::Array(ids.into_iter().map(Json::uuid).collect()),
        ))
    })
}

/// `ProjectUserDisplayPropertyEndpoint.get`.
fn user_properties<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let row = load(client, user, project_id).await?;
        let row = match row {
            Some(row) => row,
            None => create(client, user, project_id).await?,
        };
        let serializer = ProjectUserPropertySerializer {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
            filters: row
                .filters
                .as_deref()
                .map(plane_serializers::fields::PgJson),
            display_filters: row
                .display_filters
                .as_deref()
                .map(plane_serializers::fields::PgJson),
            display_properties: row
                .display_properties
                .as_deref()
                .map(plane_serializers::fields::PgJson),
            rich_filters: row
                .rich_filters
                .as_deref()
                .map(plane_serializers::fields::PgJson),
            preferences: row
                .preferences
                .as_deref()
                .map(plane_serializers::fields::PgJson),
            sort_order: row.sort_order,
            created_by: row.created_by,
            updated_by: row.updated_by,
            project: row.project_id,
            workspace: row.workspace_id,
            user: row.user_id,
        };
        Ok(ViewResponse::raw(
            200,
            plane_core::error::APPLICATION_JSON,
            render(ctx, &serializer)?,
        ))
    })
}

/// One `project_user_properties` row.
struct PropertyRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    project_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    filters: Option<String>,
    display_filters: Option<String>,
    display_properties: Option<String>,
    rich_filters: Option<String>,
    preferences: Option<String>,
    sort_order: f64,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

fn row_to_property(row: &tokio_postgres::Row) -> PropertyRow {
    use plane_db::models::JsonText;
    let text = |index: usize| -> Option<String> {
        row.get::<_, Option<JsonText>>(index)
            .map(|value| value.as_str().to_owned())
    };
    PropertyRow {
        created_at: row.get(0),
        updated_at: row.get(1),
        created_by: row.get(2),
        updated_by: row.get(3),
        deleted_at: row.get(4),
        id: row.get(5),
        project_id: row.get(6),
        workspace_id: row.get(7),
        user_id: row.get(8),
        filters: text(9),
        display_filters: text(10),
        display_properties: text(11),
        rich_filters: text(12),
        preferences: text(13),
        sort_order: row.get(14),
    }
}

async fn load(
    client: &Client,
    user: Uuid,
    project_id: Uuid,
) -> Result<Option<PropertyRow>, ViewError> {
    let statement = client
        .prepare_cached(PROJECT_USER_PROPERTY.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&user, &project_id])
        .await
        .map_err(ViewError::from)?
        .map(|row| row_to_property(&row)))
}

/// `get_or_create`'s insert branch.
async fn create(client: &Client, user: Uuid, project_id: Uuid) -> Result<PropertyRow, ViewError> {
    let now = plane_core::clock::now();
    let id = Uuid::new_v4();
    let statement = client
        .prepare_cached(CREATE_PROJECT_USER_PROPERTY.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(&statement, &[&now, &user, &id, &project_id])
        .await
        .map_err(ViewError::from)?;
    load(client, user, project_id).await?.ok_or_else(|| {
        ViewError::Unknown("the created project user property row could not be read".to_owned())
    })
}
