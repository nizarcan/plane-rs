// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/user.py`: the workspace user pages.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/user-stats/{user_id}/` | `WorkspaceUserProfileStatsEndpoint.get` | `user.py:438-562` |
//! | `GET /api/workspaces/{slug}/user-activity/{user_id}/` | `WorkspaceUserActivityEndpoint.get` | `user.py:409-435` |
//! | `GET /api/workspaces/{slug}/user-profile/{user_id}/` | `WorkspaceUserProfileEndpoint.get` | `user.py:299-406` |
//! | `GET /api/workspaces/{slug}/user-issues/{user_id}/` | `WorkspaceUserProfileIssuesEndpoint.get` | `user.py:115-268` |
//! | `GET /api/workspaces/{slug}/user-properties/` | `WorkspaceUserPropertiesEndpoint.get` | `user.py:288-296` |
//!
//! ## `_visible_issues_q`
//!
//! Three of these endpoints scope issues with `_visible_issues_q`
//! (`user.py:68-82`):
//!
//! ```text
//! project__project_projectmember__member = user
//! AND project__project_projectmember__is_active
//! AND project__project_projectmember__deleted_at IS NULL
//! AND (project__project_projectmember__role > 5
//!      OR project__guest_view_all_features
//!      OR created_by = user)
//! ```
//!
//! The issue-based statements keep the join (`vpm`) because it can multiply
//! rows exactly as Django's does; the `EXISTS` form is used only where the join
//! is provably single-valued (`(project, member)` is unique among live rows).

use chrono_tz::Tz;
use http::Method;
use plane_core::ApiError;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::workspace::WorkspaceUserProperties;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::QueryError;
use plane_query::issue::filter::{
    DateOperand, DateSuffix, FilterBuilder, FilterParams, LegacyFilters,
};
use plane_query::issue::model::{GroupField, Relation, manager_predicate};
use plane_query::issue::order::Ordering;
use plane_query::issue::query::{GroupPlan, IssueQuery, IssueScope};
use plane_query::issue::{group_values, rich};
use plane_query::paginator::{Cursor, PageQuery, issue_list_page};
use plane_query::sql::{Frag, Join, Select, Value, prep_for_like_query};
use plane_serializers::fields::{DrfUuid, PgJson};
use plane_serializers::generated::app_issue::IssueActivitySerializer;
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use plane_serializers::value::DrfValue;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, RenderCtx};
use uuid::Uuid;

use crate::common::client;
use crate::workspaces::queries;
use crate::workspaces::rows::{self, UserRow};

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.workspace.user.WorkspaceUserProfileStatsEndpoint")?
        .method(Method::GET, user_stats)?;
    registry
        .api_view("plane.app.views.workspace.user.WorkspaceUserActivityEndpoint")?
        .method(Method::GET, user_activity)?;
    registry
        .api_view("plane.app.views.workspace.user.WorkspaceUserProfileEndpoint")?
        .method(Method::GET, user_profile)?;
    registry
        .api_view("plane.app.views.workspace.user.WorkspaceUserProfileIssuesEndpoint")?
        .method(Method::GET, user_issues)?;
    registry
        .api_view("plane.app.views.workspace.user.WorkspaceUserPropertiesEndpoint")?
        .method(Method::GET, user_properties)?;
    Ok(())
}

/// The `user_id` URL kwarg as a UUID (all five routes take one).
fn target_user(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    let value = ctx.kwarg("user_id").unwrap_or_default();
    Uuid::parse_str(value).map_err(|_| ViewError::Validation)
}

/// `request.user.id` for the visibility rules.
fn caller(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".to_string()))
}

/// `_visible_issues_q(user)` over a query whose `issues`, `projects` and
/// `project_members AS vpm` tables are joined.
fn visible_issues(user: Uuid) -> Frag {
    let mut member = Frag::raw(
        "EXISTS (SELECT 1 FROM project_members vpm \
         WHERE vpm.project_id = issues.project_id AND vpm.member_id = ",
    );
    member.push(Frag::bind(Value::Uuid(user)));
    member.push_raw(" AND vpm.is_active AND vpm.deleted_at IS NULL AND (vpm.role > 5");
    member.push_raw(" OR projects.guest_view_all_features OR issues.created_by_id = ");
    member.push(Frag::bind(Value::Uuid(user)));
    member.push_raw("))");
    member
}

/// The manager's joins and predicate plus the legacy filters and the slug.
fn issue_base(slug: &str, filters: &LegacyFilters, timezone: Tz) -> FilterBuilder {
    let mut builder = FilterBuilder::new();
    builder.require_left(Relation::States);
    builder.require_inner(Relation::Projects);
    builder.require_inner(Relation::Workspaces);
    builder.and(manager_predicate());
    builder.and(Frag::compare(
        "workspaces.slug",
        "=",
        Value::Text(slug.to_owned()),
    ));
    filters.apply(&mut builder);
    filters.apply_dates(&mut builder, timezone);
    builder
}

/// A `SELECT` over `issues` with the visibility join.
fn issue_select(builder: FilterBuilder, user: Uuid) -> Select {
    let mut select = Select::new("issues");
    select.joins = builder.joins.to_joins();
    select.predicate = Some(builder.predicate.and(visible_issues(user)));
    select
}

/// Map a `QueryError` to the family body it produces.
fn query_error(error: QueryError) -> ViewError {
    match error {
        QueryError::Api(api) => match api.errors {
            Some(errors) => ViewError::Api(ApiError::ValidationError(errors.to_serde())),
            None => ViewError::Api(ApiError::ParseError(api.message)),
        },
        other => ViewError::Unknown(other.to_string()),
    }
}

/// Prepare and run a composed statement.
async fn run(
    client: &deadpool_postgres::Client,
    select: Select,
) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let sql = select.into_sql();
    Ok(sql.query(client).await?)
}

/// Prepare and run a composed statement that returns one row.
async fn run_one(
    client: &deadpool_postgres::Client,
    select: Select,
) -> Result<tokio_postgres::Row, ViewError> {
    let sql = select.into_sql();
    let row = sql
        .query(client)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ViewError::Unknown("query returned no row".to_owned()))?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// user-properties
// ---------------------------------------------------------------------------

/// `WorkspaceUserPropertiesEndpoint.get` (`user.py:288-296`).
fn user_properties<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = caller(ctx)?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(queries::WORKSPACE_ID_BY_SLUG.sql)
            .await?;
        let Some(row) = client.query_opt(&statement, &[&slug]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace: Uuid = row.try_get(0)?;

        let statement = client
            .prepare_cached(queries::USER_PROPERTIES_FOR_WORKSPACE.sql)
            .await?;
        let rows = client.query(&statement, &[&user, &workspace]).await?;
        let (model, created) = match rows.len() {
            0 => (create_user_properties(client, workspace, user).await?, true),
            1 => (WorkspaceUserProperties::from_row(&rows[0], 0)?, false),
            _ => return Err(rows::multiple_objects("WorkspaceUserProperties")),
        };

        // A row Django just created carries the *Python* dict's key order: the
        // value is rendered from the in-memory instance, not from `jsonb`
        // (which sorts keys by length, then bytes). A row read back from the
        // database renders the `jsonb` order, so its stored text is used.
        let (filters, display_filters, display_properties, rich_filters) = if created {
            (
                DEFAULT_FILTERS.to_owned(),
                DEFAULT_DISPLAY_FILTERS.to_owned(),
                DEFAULT_DISPLAY_PROPERTIES.to_owned(),
                "{}".to_owned(),
            )
        } else {
            (
                model.filters.into_string(),
                model.display_filters.into_string(),
                model.display_properties.into_string(),
                model.rich_filters.into_string(),
            )
        };

        let serializer = UserPropertiesView {
            id: model.id,
            created_at: model.created_at,
            updated_at: model.updated_at,
            deleted_at: model.deleted_at,
            filters: &filters,
            display_filters: &display_filters,
            display_properties: &display_properties,
            rich_filters: &rich_filters,
            navigation_project_limit: i64::from(model.navigation_project_limit),
            navigation_control_preference: &model.navigation_control_preference,
            created_by: model.created_by_id,
            updated_by: model.updated_by_id,
            workspace: model.workspace_id,
            user: model.user_id,
        };
        let body = rows::render_plain(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkspaceUserPropertiesSerializer`
/// (`plane/app/serializers/workspace.py:164-170`) over text that may come from
/// either the database (`jsonb` order) or the model's defaults (Python order).
#[derive(Debug)]
struct UserPropertiesView<'a> {
    id: Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    filters: &'a str,
    display_filters: &'a str,
    display_properties: &'a str,
    rich_filters: &'a str,
    navigation_project_limit: i64,
    navigation_control_preference: &'a str,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    workspace: Uuid,
    user: Uuid,
}

impl DrfSerialize for UserPropertiesView<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field_dyn("id", ctx, &DrfUuid(self.id))?;
        object.field_dyn(
            "created_at",
            ctx,
            &plane_serializers::fields::drf_datetime(self.created_at, ctx.tz()),
        )?;
        object.field_dyn(
            "updated_at",
            ctx,
            &plane_serializers::fields::drf_datetime(self.updated_at, ctx.tz()),
        )?;
        object.field_dyn(
            "deleted_at",
            ctx,
            &plane_serializers::fields::drf_datetime_opt(self.deleted_at, ctx.tz()),
        )?;
        object.field_json("filters", Some(PgJson::new(self.filters)))?;
        object.field_json("display_filters", Some(PgJson::new(self.display_filters)))?;
        object.field_json(
            "display_properties",
            Some(PgJson::new(self.display_properties)),
        )?;
        object.field_json("rich_filters", Some(PgJson::new(self.rich_filters)))?;
        object.field("navigation_project_limit", &self.navigation_project_limit)?;
        object.field(
            "navigation_control_preference",
            &self.navigation_control_preference,
        )?;
        object.field_dyn("created_by", ctx, &self.created_by.map(DrfUuid))?;
        object.field_dyn("updated_by", ctx, &self.updated_by.map(DrfUuid))?;
        object.field_dyn("workspace", ctx, &DrfUuid(self.workspace))?;
        object.field_dyn("user", ctx, &DrfUuid(self.user))?;
        object.end();
        Ok(())
    }
}

/// `get_default_filters()` (`plane/db/models/workspace.py:62-73`), in the
/// Python dict's order.
const DEFAULT_FILTERS: &str = "{\"priority\": null, \"state\": null, \"state_group\": null, \
 \"assignees\": null, \"created_by\": null, \"labels\": null, \"start_date\": null, \
 \"target_date\": null, \"subscriber\": null}";

/// `get_default_display_filters()` (`plane/db/models/workspace.py:76-87`).
const DEFAULT_DISPLAY_FILTERS: &str = "{\"display_filters\": {\"group_by\": null, \
 \"order_by\": \"-created_at\", \"type\": null, \"sub_issue\": true, \
 \"show_empty_groups\": true, \"layout\": \"list\", \"calendar_date_range\": \"\"}}";

/// `get_default_display_properties()` (`plane/db/models/workspace.py:90-107`).
const DEFAULT_DISPLAY_PROPERTIES: &str = "{\"display_properties\": {\"assignee\": true, \
 \"attachment_count\": true, \"created_on\": true, \"due_date\": true, \"estimate\": true, \
 \"key\": true, \"labels\": true, \"link\": true, \"priority\": true, \"start_date\": true, \
 \"state\": true, \"sub_issue_count\": true, \"updated_on\": true}}";

/// `get_or_create`'s `create(**params)` with the model's column defaults
/// (`plane/db/models/workspace.py:352-390`). `Model.save()` runs for a
/// `create()`, so the audit columns carry the request user
/// (`plane/db/models/base.py:21-40`).
async fn create_user_properties(
    client: &deadpool_postgres::Client,
    workspace: Uuid,
    user: Uuid,
) -> Result<WorkspaceUserProperties, ViewError> {
    let now = plane_core::clock::now();
    let id = Uuid::new_v4();
    // `jsonb` parameters need a JSON value, not text: `postgres-types` accepts
    // `serde_json::Value` for `json`/`jsonb` and rejects `&str` (`WrongType`).
    let filters: serde_json::Value = serde_json::from_str(DEFAULT_FILTERS)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let display_filters: serde_json::Value = serde_json::from_str(DEFAULT_DISPLAY_FILTERS)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let display_properties: serde_json::Value = serde_json::from_str(DEFAULT_DISPLAY_PROPERTIES)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let statement = client
        .prepare(
            "INSERT INTO workspace_user_properties \
             (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, workspace_id, \
              user_id, filters, display_filters, display_properties, rich_filters, \
              navigation_project_limit, navigation_control_preference) \
             VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, $9, '{}'::jsonb, 10, \
              'ACCORDION') \
             ON CONFLICT DO NOTHING \
             RETURNING created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
              workspace_id, user_id, filters, display_filters, display_properties, \
              rich_filters, navigation_project_limit, navigation_control_preference",
        )
        .await?;
    let rows = client
        .query(
            &statement,
            &[
                &now,
                &now,
                &user,
                &id,
                &workspace,
                &user,
                &filters,
                &display_filters,
                &display_properties,
            ],
        )
        .await?;
    match rows.len() {
        1 => Ok(WorkspaceUserProperties::from_row(&rows[0], 0)?),
        // A concurrent create won the unique index; re-read.
        0 => {
            let statement = client
                .prepare_cached(queries::USER_PROPERTIES_FOR_WORKSPACE.sql)
                .await?;
            let rows = client.query(&statement, &[&user, &workspace]).await?;
            match rows.len() {
                1 => Ok(WorkspaceUserProperties::from_row(&rows[0], 0)?),
                _ => Err(rows::multiple_objects("WorkspaceUserProperties")),
            }
        }
        _ => Err(rows::multiple_objects("WorkspaceUserProperties")),
    }
}

// ---------------------------------------------------------------------------
// user-profile
// ---------------------------------------------------------------------------

/// One `values("id", "logo_props", …)` row of the project aggregates.
#[derive(Debug)]
struct ProjectStats<'a> {
    id: Uuid,
    logo_props: &'a JsonText,
    created_issues: i64,
    assigned_issues: i64,
    completed_issues: i64,
    pending_issues: i64,
}

impl DrfSerialize for ProjectStats<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field_dyn("id", ctx, &DrfUuid(self.id))?;
        object.field_json("logo_props", Some(PgJson::from(self.logo_props)))?;
        object.field("created_issues", &self.created_issues)?;
        object.field("assigned_issues", &self.assigned_issues)?;
        object.field("completed_issues", &self.completed_issues)?;
        object.field("pending_issues", &self.pending_issues)?;
        object.end();
        Ok(())
    }
}

/// `user_data` (`user.py:394-403`): a plain dict, so `date_joined` is rendered
/// by DRF's `JSONEncoder` (`isoformat()`, `+00:00` → `Z`), not by a
/// `DateTimeField`, and the request timezone does not apply.
#[derive(Debug)]
struct UserData<'a> {
    email: Option<&'a str>,
    first_name: &'a str,
    last_name: &'a str,
    avatar_url: Option<String>,
    cover_image_url: Option<String>,
    date_joined: chrono::DateTime<chrono::Utc>,
    user_timezone: &'a str,
    display_name: &'a str,
}

impl DrfSerialize for UserData<'_> {
    fn drf_write(&self, w: &mut DrfWriter, _ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field("email", &self.email)?;
        object.field("first_name", &self.first_name)?;
        object.field("last_name", &self.last_name)?;
        object.field("avatar_url", &self.avatar_url)?;
        object.field("cover_image_url", &self.cover_image_url)?;
        object.field(
            "date_joined",
            &plane_serde::values::DrfDateTime(self.date_joined),
        )?;
        object.field("user_timezone", &self.user_timezone)?;
        object.field("display_name", &self.display_name)?;
        object.end();
        Ok(())
    }
}

#[derive(Debug)]
struct ProfileBody<'a> {
    projects: &'a [ProjectStats<'a>],
    user: UserData<'a>,
}

impl DrfSerialize for ProfileBody<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field_nested_list("project_data", ctx, self.projects)?;
        object.field_nested("user_data", ctx, Some(&self.user))?;
        object.end();
        Ok(())
    }
}

/// `WorkspaceUserProfileEndpoint.get` (`user.py:299-406`).
fn user_profile<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = caller(ctx)?;
        let target = target_user(ctx)?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(queries::USER_PROFILE_REQUESTER.sql)
            .await?;
        let rows = client.query(&statement, &[&slug, &user]).await?;
        let requester_role: i16 = match rows.len() {
            0 => return Err(ViewError::ObjectDoesNotExist),
            1 => rows[0].try_get(0)?,
            _ => return Err(rows::multiple_objects("WorkspaceMember")),
        };

        let statement = client
            .prepare_cached(queries::USER_PROFILE_TARGET.sql)
            .await?;
        let rows = client.query(&statement, &[&slug, &target]).await?;
        let row = match rows.len() {
            0 => return Err(ViewError::ObjectDoesNotExist),
            1 => &rows[0],
            _ => return Err(rows::multiple_objects("WorkspaceMember")),
        };
        let email: Option<String> = row.try_get(17)?;
        let first_name: String = row.try_get(18)?;
        let last_name: String = row.try_get(19)?;
        let display_name: String = row.try_get(20)?;
        let user_timezone: String = row.try_get(21)?;
        let avatar: String = row.try_get(22)?;
        let avatar_asset_id: Option<Uuid> = row.try_get(23)?;
        let cover_image: Option<String> = row.try_get(24)?;
        let cover_image_asset_id: Option<Uuid> = row.try_get(25)?;
        let date_joined: chrono::DateTime<chrono::Utc> = row.try_get(26)?;
        let avatar_entity_type: Option<String> = row.try_get(27)?;
        let cover_entity_type: Option<String> = row.try_get(28)?;

        let avatar_url =
            rows::user_avatar_url(avatar_asset_id, avatar_entity_type.as_deref(), &avatar);
        let cover_image_url =
            match rows::static_asset_url(cover_entity_type.as_deref(), cover_image_asset_id) {
                Some(url) => Some(url),
                None => cover_image.clone().filter(|value| !value.is_empty()),
            };

        // `if requesting_workspace_member.role >= 15` (`user.py:312`).
        let mut projects = Vec::new();
        let mut logo_props: Vec<JsonText> = Vec::new();
        if requester_role >= 15 {
            let statement = client
                .prepare_cached(queries::USER_PROFILE_PROJECTS.sql)
                .await?;
            // $1 slug, $2 the caller (visibility), $3 the profile's user.
            let rows = client.query(&statement, &[&slug, &user, &target]).await?;
            logo_props.reserve(rows.len());
            for row in &rows {
                logo_props.push(row.try_get(1)?);
            }
            for (index, row) in rows.iter().enumerate() {
                projects.push(ProjectStats {
                    id: row.try_get(0)?,
                    logo_props: &logo_props[index],
                    created_issues: row.try_get(2)?,
                    assigned_issues: row.try_get(3)?,
                    completed_issues: row.try_get(4)?,
                    pending_issues: row.try_get(5)?,
                });
            }
        }

        let body = ProfileBody {
            projects: &projects,
            user: UserData {
                email: email.as_deref(),
                first_name: &first_name,
                last_name: &last_name,
                avatar_url,
                cover_image_url,
                date_joined,
                user_timezone: &user_timezone,
                display_name: &display_name,
            },
        };
        let timezone = rows::timezone(ctx);
        let rendered = plane_serializers::render(&body, &rows::plain_ctx(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, rendered))
    })
}

// ---------------------------------------------------------------------------
// user-stats
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct StateRow {
    state_group: Option<String>,
    state_count: i64,
}

#[derive(Debug)]
struct PriorityRow {
    priority: String,
    priority_count: i64,
    priority_order: i32,
}

/// One `(cycle__name, cycle__id, cycle__project_id)` row.
#[derive(Debug)]
struct CycleRow {
    name: String,
    id: Uuid,
    project_id: Uuid,
}

#[derive(Debug)]
struct StatsBody {
    state_distribution: Vec<StateRow>,
    priority_distribution: Vec<PriorityRow>,
    created_issues: i64,
    assigned_issues: i64,
    completed_issues: i64,
    pending_issues: i64,
    subscribed_issues: i64,
    present_cycles: Vec<CycleRow>,
    upcoming_cycles: Vec<CycleRow>,
}

impl DrfSerialize for StatsBody {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field_nested_list("state_distribution", ctx, &self.state_distribution)?;
        object.field_nested_list("priority_distribution", ctx, &self.priority_distribution)?;
        object.field("created_issues", &self.created_issues)?;
        object.field("assigned_issues", &self.assigned_issues)?;
        object.field("completed_issues", &self.completed_issues)?;
        object.field("pending_issues", &self.pending_issues)?;
        object.field("subscribed_issues", &self.subscribed_issues)?;
        object.field_nested_list("present_cycles", ctx, &self.present_cycles)?;
        object.field_nested_list("upcoming_cycles", ctx, &self.upcoming_cycles)?;
        object.end();
        Ok(())
    }
}

impl DrfSerialize for StateRow {
    fn drf_write(&self, w: &mut DrfWriter, _ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field("state_group", &self.state_group)?;
        object.field("state_count", &self.state_count)?;
        object.end();
        Ok(())
    }
}

impl DrfSerialize for PriorityRow {
    fn drf_write(&self, w: &mut DrfWriter, _ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field("priority", &self.priority)?;
        object.field("priority_count", &self.priority_count)?;
        object.field("priority_order", &self.priority_order)?;
        object.end();
        Ok(())
    }
}

impl DrfSerialize for CycleRow {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field("cycle__name", &self.name)?;
        object.field_dyn("cycle__id", ctx, &DrfUuid(self.id))?;
        object.field_dyn("cycle__project_id", ctx, &DrfUuid(self.project_id))?;
        object.end();
        Ok(())
    }
}

/// `WorkspaceUserProfileStatsEndpoint.get` (`user.py:438-562`).
fn user_stats<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = caller(ctx)?;
        let target = target_user(ctx)?;
        let client = client(ctx)?;
        let timezone = rows::timezone(ctx);
        let filters = LegacyFilters::parse(
            &FilterParams::Get(ctx.query()),
            plane_core::clock::now().date_naive(),
        )
        .map_err(query_error)?;

        // state_distribution (`user.py:444-455`).
        let mut builder = issue_base(slug, &filters, timezone);
        builder.require_inner(Relation::IssueAssignees);
        builder.and(Frag::compare(
            "issue_assignees.assignee_id",
            "=",
            Value::Uuid(target),
        ));
        builder.and(Frag::is_null("issue_assignees.deleted_at"));
        let mut select = issue_select(builder, user);
        select.projection = vec![
            Frag::raw("states.\"group\" AS state_group"),
            Frag::raw("COUNT(states.\"group\") AS state_count"),
        ];
        select.group_by = vec!["1".to_owned()];
        select.order_by = vec![Frag::raw("1 ASC")];
        let rows = run(client, select).await?;
        let mut state_distribution = Vec::with_capacity(rows.len());
        for row in &rows {
            state_distribution.push(StateRow {
                state_group: row.try_get(0)?,
                state_count: row.try_get(1)?,
            });
        }

        // priority_distribution (`user.py:459-477`). Django's
        // `HAVING COUNT(priority) >= 1` is `priority IS NOT NULL` for a group on
        // `priority` itself, so the predicate is a `WHERE`.
        let mut builder = issue_base(slug, &filters, timezone);
        builder.require_inner(Relation::IssueAssignees);
        builder.and(Frag::compare(
            "issue_assignees.assignee_id",
            "=",
            Value::Uuid(target),
        ));
        builder.and(Frag::is_null("issue_assignees.deleted_at"));
        builder.and(Frag::is_not_null("issues.priority"));
        let mut select = issue_select(builder, user);
        select.projection = vec![
            Frag::raw("issues.priority AS priority"),
            Frag::raw("COUNT(issues.priority) AS priority_count"),
            Frag::raw(
                "CASE WHEN issues.priority = 'urgent' THEN 0 \
                 WHEN issues.priority = 'high' THEN 1 \
                 WHEN issues.priority = 'medium' THEN 2 \
                 WHEN issues.priority = 'low' THEN 3 \
                 WHEN issues.priority = 'none' THEN 4 ELSE 5 END AS priority_order",
            ),
        ];
        select.group_by = vec!["1".to_owned(), "3".to_owned()];
        select.order_by = vec![Frag::raw("3 ASC")];
        let rows = run(client, select).await?;
        let mut priority_distribution = Vec::with_capacity(rows.len());
        for row in &rows {
            priority_distribution.push(PriorityRow {
                priority: row.try_get(0)?,
                priority_count: row.try_get(1)?,
                priority_order: row.try_get(2)?,
            });
        }

        // The four issue counts (`user.py:479-519`).
        let created_issues = count_issues(client, slug, user, &filters, timezone, |builder| {
            builder.and(Frag::compare(
                "issues.created_by_id",
                "=",
                Value::Uuid(target),
            ));
        })
        .await?;
        let assigned_issues = count_issues(client, slug, user, &filters, timezone, |builder| {
            builder.require_inner(Relation::IssueAssignees);
            builder.and(Frag::compare(
                "issue_assignees.assignee_id",
                "=",
                Value::Uuid(target),
            ));
            builder.and(Frag::is_null("issue_assignees.deleted_at"));
        })
        .await?;
        let pending_issues = count_issues(client, slug, user, &filters, timezone, |builder| {
            builder.and(
                Frag::in_texts(
                    "states.\"group\"",
                    &["completed".to_owned(), "cancelled".to_owned()],
                )
                .and(Frag::is_not_null("states.\"group\""))
                .negate(),
            );
            builder.require_inner(Relation::IssueAssignees);
            builder.and(Frag::compare(
                "issue_assignees.assignee_id",
                "=",
                Value::Uuid(target),
            ));
            builder.and(Frag::is_null("issue_assignees.deleted_at"));
        })
        .await?;
        let completed_issues = count_issues(client, slug, user, &filters, timezone, |builder| {
            builder.require_inner(Relation::IssueAssignees);
            builder.require_inner(Relation::States);
            builder.and(Frag::compare(
                "issue_assignees.assignee_id",
                "=",
                Value::Uuid(target),
            ));
            builder.and(Frag::is_null("issue_assignees.deleted_at"));
            builder.and(Frag::compare(
                "states.\"group\"",
                "=",
                Value::Text("completed".to_owned()),
            ));
        })
        .await?;

        let subscribed_issues =
            subscribed_count(client, slug, user, target, &filters, timezone).await?;
        let present_cycles = cycle_rows(client, slug, user, target, CycleKind::Present).await?;
        let upcoming_cycles = cycle_rows(client, slug, user, target, CycleKind::Upcoming).await?;

        let body = StatsBody {
            state_distribution,
            priority_distribution,
            created_issues,
            assigned_issues,
            completed_issues,
            pending_issues,
            subscribed_issues,
            present_cycles,
            upcoming_cycles,
        };
        let rendered = plane_serializers::render(&body, &rows::plain_ctx(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, rendered))
    })
}

/// One `.count()` over the issue base with extra conditions.
async fn count_issues<F>(
    client: &deadpool_postgres::Client,
    slug: &str,
    user: Uuid,
    filters: &LegacyFilters,
    timezone: Tz,
    extra: F,
) -> Result<i64, ViewError>
where
    F: FnOnce(&mut FilterBuilder),
{
    let mut builder = issue_base(slug, filters, timezone);
    extra(&mut builder);
    let select = issue_select(builder, user);
    let row = run_one(client, {
        let mut counted = select;
        counted.projection = vec![Frag::raw("count(*)")];
        counted.order_by.clear();
        counted.limit = None;
        counted.offset = None;
        counted
    })
    .await?;
    Ok(row.try_get(0)?)
}

/// `subscribed_issues_count` (`user.py:521-530`).
///
/// The Python applies `issue_filters` to an `IssueSubscriber` queryset, so a
/// filter whose column the model lacks is a `FieldError`, i.e. a 500. Only
/// `created_at`, `updated_at`, `created_by`, `project` and `subscriber` resolve
/// there; the rest keeps Django's failure.
async fn subscribed_count(
    client: &deadpool_postgres::Client,
    slug: &str,
    user: Uuid,
    target: Uuid,
    filters: &LegacyFilters,
    timezone: Tz,
) -> Result<i64, ViewError> {
    if !subscriber_filters_supported(filters) {
        return Err(ViewError::Unknown(
            "FieldError: cannot resolve keyword into field on IssueSubscriber".to_string(),
        ));
    }
    let mut select = Select::new("issue_subscribers");
    select.joins = vec![
        Join::inner(
            "issues",
            "issues",
            Frag::raw("issue_subscribers.issue_id = issues.id"),
        ),
        Join::inner(
            "projects",
            "projects",
            Frag::raw("issues.project_id = projects.id"),
        ),
        Join::inner(
            "projects",
            "sp",
            Frag::raw("issue_subscribers.project_id = sp.id"),
        ),
        Join::inner(
            "workspaces",
            "workspaces",
            Frag::raw("issue_subscribers.workspace_id = workspaces.id"),
        ),
    ];
    let mut predicate = Frag::is_null("issue_subscribers.deleted_at")
        .and(visible_issues(user))
        .and(Frag::is_null("sp.archived_at"))
        .and(Frag::compare(
            "issue_subscribers.subscriber_id",
            "=",
            Value::Uuid(target),
        ))
        .and(Frag::compare(
            "workspaces.slug",
            "=",
            Value::Text(slug.to_owned()),
        ));
    if let Some(created_by) = &filters.created_by {
        predicate = predicate.and(Frag::in_uuids(
            "issue_subscribers.created_by_id",
            created_by,
        ));
    }
    if filters.created_by_is_null {
        predicate = predicate.and(Frag::is_null("issue_subscribers.created_by_id"));
    }
    if let Some(projects) = &filters.project {
        predicate = predicate.and(Frag::in_uuids("issue_subscribers.project_id", projects));
    }
    if let Some(subscribers) = &filters.subscriber {
        predicate = predicate.and(Frag::in_uuids(
            "issue_subscribers.subscriber_id",
            subscribers,
        ));
    }
    predicate = predicate.and(subscriber_dates(filters, timezone));
    select.predicate = Some(predicate);
    select.projection = vec![Frag::raw("count(*)")];
    let row = run_one(client, select).await?;
    Ok(row.try_get(0)?)
}

/// Whether every set `issue_filters` lookup resolves on `IssueSubscriber`.
fn subscriber_filters_supported(filters: &LegacyFilters) -> bool {
    filters.state.is_none()
        && filters.state_group.is_none()
        && filters.estimate_point.is_none()
        && filters.priority.is_none()
        && !filters.parent_is_null
        && filters.parent.is_none()
        && !filters.labels_is_null
        && filters.labels.is_none()
        && filters.mentions.is_none()
        && filters.name_icontains.is_none()
        && filters.completed_at.is_empty()
        && filters.start_date.is_empty()
        && filters.target_date.is_empty()
        && filters.start_date_exact.is_none()
        && filters.target_date_exact.is_none()
        && filters.state_group_type.is_none()
        && !filters.cycle_is_null
        && filters.cycle.is_none()
        && !filters.module_is_null
        && filters.module.is_none()
        && filters.intake_status.is_none()
        && !filters.parent_is_null_sub_issue
        && !filters.start_target_date
}

/// The `created_at`/`updated_at` date lookups of `issue_filters`, against
/// `issue_subscribers` instead of `issues` (`plane_query::issue::filter`
/// `apply_dates`).
fn subscriber_dates(filters: &LegacyFilters, timezone: Tz) -> Frag {
    let mut predicate = Frag::empty();
    for (column, date) in [
        ("issue_subscribers.created_at", &filters.created_at),
        ("issue_subscribers.updated_at", &filters.updated_at),
    ] {
        if date.is_empty() {
            continue;
        }
        // `apply_dates` builds the `__date` transform once and uses it for
        // both the range operands and `__contains`.
        let mut cast = Frag::raw(format!("({column} AT TIME ZONE "));
        cast.push(Frag::bind(Value::Text(timezone.name().to_owned())));
        cast.push_raw(")::date");
        for (suffix, operand) in &date.entries {
            match operand {
                DateOperand::Date(value) => {
                    let mut frag = cast.clone();
                    frag.push_raw(match suffix {
                        DateSuffix::Gte => " >= ",
                        DateSuffix::Lte => " <= ",
                        DateSuffix::Contains => " LIKE ",
                    });
                    frag.push(Frag::bind(Value::Date(*value)));
                    predicate = predicate.and(frag);
                }
                DateOperand::Contains(text) => {
                    let mut frag = cast.clone();
                    frag.push_raw("::text LIKE ");
                    frag.push(Frag::bind(Value::Text(format!(
                        "%{}%",
                        prep_for_like_query(text)
                    ))));
                    predicate = predicate.and(frag);
                }
            }
        }
    }
    predicate
}

#[derive(Clone, Copy)]
enum CycleKind {
    Present,
    Upcoming,
}

/// `present_cycles` / `upcoming_cycles` (`user.py:532-548`). No `issue_filters`
/// apply here.
async fn cycle_rows(
    client: &deadpool_postgres::Client,
    slug: &str,
    user: Uuid,
    target: Uuid,
    kind: CycleKind,
) -> Result<Vec<CycleRow>, ViewError> {
    let now = plane_core::clock::now();
    let mut select = Select::new("cycle_issues");
    select.joins = vec![
        Join::inner(
            "issues",
            "issues",
            Frag::raw("cycle_issues.issue_id = issues.id"),
        ),
        Join::inner(
            "projects",
            "projects",
            Frag::raw("issues.project_id = projects.id"),
        ),
        Join::inner(
            "cycles",
            "cycles",
            Frag::raw("cycle_issues.cycle_id = cycles.id"),
        ),
        Join::inner(
            "issue_assignees",
            "issue_assignees",
            Frag::raw("issues.id = issue_assignees.issue_id"),
        ),
        Join::inner(
            "workspaces",
            "workspaces",
            Frag::raw("cycle_issues.workspace_id = workspaces.id"),
        ),
    ];
    let mut predicate = Frag::is_null("cycle_issues.deleted_at")
        .and(visible_issues(user))
        .and(Frag::raw("cycles.project_id = (issues.project_id)"))
        .and(Frag::compare(
            "issue_assignees.assignee_id",
            "=",
            Value::Uuid(target),
        ))
        .and(Frag::compare(
            "workspaces.slug",
            "=",
            Value::Text(slug.to_owned()),
        ));
    predicate = match kind {
        CycleKind::Present => predicate
            .and(Frag::compare(
                "cycles.start_date",
                "<",
                Value::Timestamp(now),
            ))
            .and(Frag::compare("cycles.end_date", ">", Value::Timestamp(now))),
        CycleKind::Upcoming => predicate.and(Frag::compare(
            "cycles.start_date",
            ">",
            Value::Timestamp(now),
        )),
    };
    select.predicate = Some(predicate);
    select.projection = vec![
        Frag::raw("cycles.name"),
        Frag::raw("cycle_issues.cycle_id"),
        Frag::raw("cycles.project_id"),
    ];
    select.order_by = vec![Frag::raw("cycle_issues.created_at DESC")];
    let rows = run(client, select).await?;
    let mut cycles = Vec::with_capacity(rows.len());
    for row in &rows {
        cycles.push(CycleRow {
            name: row.try_get(0)?,
            id: row.try_get(1)?,
            project_id: row.try_get(2)?,
        });
    }
    Ok(cycles)
}

// ---------------------------------------------------------------------------
// user-activity
// ---------------------------------------------------------------------------

/// One joined activity row plus the nested serializer inputs.
#[derive(Debug)]
struct ActivityRecord {
    activity: plane_db::models::issue::IssueActivity,
    actor: UserRow,
    issue: plane_db::models::issue::Issue,
    project: plane_db::models::project::Project,
    project_cover_image_url: Option<String>,
    workspace: plane_db::models::workspace::Workspace,
    workspace_logo_url: Option<String>,
}

impl ActivityRecord {
    /// Column layout: `issue_activities` (20), the actor (10), the issue (28),
    /// the project (36), the project's cover asset type, the workspace (14) and
    /// its logo asset type.
    fn from_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        let project = plane_db::models::project::Project::from_row(row, 58)?;
        let project_cover_entity_type: Option<String> = row.try_get(94)?;
        let workspace = plane_db::models::workspace::Workspace::from_row(row, 95)?;
        let workspace_logo_entity_type: Option<String> = row.try_get(109)?;
        Ok(Self {
            activity: plane_db::models::issue::IssueActivity::from_row(row, 0)?,
            actor: UserRow::from_row(row, 20)?,
            issue: plane_db::models::issue::Issue::from_row(row, 30)?,
            project_cover_image_url: rows::static_asset_url(
                project_cover_entity_type.as_deref(),
                project.cover_image_asset_id,
            ),
            workspace_logo_url: rows::logo_url(
                workspace.logo_asset_id,
                workspace_logo_entity_type.as_deref(),
                workspace.logo.as_deref(),
            ),
            project,
            workspace,
        })
    }
}

/// The `IssueActivitySerializer` inputs for one row.
#[derive(Debug)]
struct ActivityView<'a> {
    record: &'a ActivityRecord,
    actor: UserLiteSerializer<'a>,
    issue: plane_serializers::generated::app_issue::IssueFlatSerializer<'a>,
    project: ProjectLiteSerializer<'a>,
    workspace: WorkspaceLiteSerializer<'a>,
    attachments: Vec<&'a str>,
}

impl DrfSerialize for ActivityView<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let activity = &self.record.activity;
        IssueActivitySerializer {
            id: activity.id,
            actor_detail: Some(&self.actor),
            issue_detail: Some(&self.issue),
            project_detail: Some(&self.project),
            workspace_detail: Some(&self.workspace),
            // `get_source_data` (`plane/app/serializers/issue.py:350-357`) reads
            // `issue.source_data`, a prefetch attribute only the issue-activity
            // endpoint sets; nothing sets it here, so `hasattr` is false.
            source_data: DrfValue::Null,
            created_at: activity.created_at,
            updated_at: activity.updated_at,
            deleted_at: activity.deleted_at,
            verb: &activity.verb,
            field: activity.field.as_deref(),
            old_value: activity.old_value.as_deref(),
            new_value: activity.new_value.as_deref(),
            comment: &activity.comment,
            attachments: &self.attachments,
            old_identifier: activity.old_identifier,
            new_identifier: activity.new_identifier,
            epoch: activity.epoch,
            created_by: activity.created_by_id,
            updated_by: activity.updated_by_id,
            project: activity.project_id,
            workspace: activity.workspace_id,
            issue: activity.issue_id,
            issue_comment: activity.issue_comment_id,
            actor: activity.actor_id,
        }
        .drf_write(w, ctx)
    }
}

/// `WorkspaceUserActivityEndpoint.get` (`user.py:412-435`).
fn user_activity<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = caller(ctx)?;
        let target = target_user(ctx)?;
        let client = client(ctx)?;
        let timezone = rows::timezone(ctx);

        let projects: Vec<Uuid> = ctx
            .query()
            .getlist("project")
            .into_iter()
            .map(Uuid::parse_str)
            .collect::<Result<_, _>>()
            .map_err(|_| ViewError::Validation)?;

        // `sanitize_order_by(request.GET.get("order_by", "-created_at"),
        // ACTIVITY_ORDER_BY_ALLOWLIST, "-created_at")`.
        let order_by = sanitize_order_by(ctx.query().get_or("order_by", "-created_at"));
        let (descending, column) = match order_by {
            "created_at" => (false, "issue_activities.created_at"),
            "-updated_at" => (true, "issue_activities.updated_at"),
            "updated_at" => (false, "issue_activities.updated_at"),
            _ => (true, "issue_activities.created_at"),
        };
        let order = format!(
            "{column} {} NULLS LAST",
            if descending { "DESC" } else { "ASC" }
        );

        let page = PageQuery::parse(ctx.query(), None, None).map_err(query_error)?;
        let limit = i64::from(page.per_page);
        let page_index = page.cursor.offset;
        let offset = page_index * limit;

        let base = activity_select(slug, user, target, &projects);

        let mut select = base.clone();
        select.order_by = activity_order(&order, column);
        select.limit = Some(limit + 1);
        select.offset = Some(offset);
        let rows = run(client, select).await?;
        let has_results = rows.len() as i64 > limit;
        let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

        // `queryset.count()`.
        let total_row = run_one(client, {
            let mut counted = base;
            counted.projection = vec![Frag::raw("count(*)")];
            counted.order_by.clear();
            counted
        })
        .await?;
        let total: i64 = total_row.try_get(0)?;

        let mut records = Vec::with_capacity(rows.len());
        for row in &rows {
            records.push(ActivityRecord::from_row(row)?);
        }
        let actors: Vec<_> = records.iter().map(|record| record.actor.lite()).collect();
        let bodies: Vec<ActivityView<'_>> = records
            .iter()
            .zip(actors.iter())
            .map(|(record, actor)| {
                let issue = &record.issue;
                let project = &record.project;
                let workspace = &record.workspace;
                ActivityView {
                    record,
                    actor: UserLiteSerializer {
                        id: actor.id,
                        first_name: actor.first_name,
                        last_name: actor.last_name,
                        avatar: actor.avatar,
                        avatar_url: actor.avatar_url,
                        is_bot: actor.is_bot,
                        display_name: actor.display_name,
                    },
                    issue: plane_serializers::generated::app_issue::IssueFlatSerializer {
                        id: issue.id,
                        name: &issue.name,
                        description_json: Some(PgJson::from(&issue.description_json)),
                        description_html: &issue.description_html,
                        priority: &issue.priority,
                        start_date: issue.start_date,
                        target_date: issue.target_date,
                        sequence_id: i64::from(issue.sequence_id),
                        sort_order: issue.sort_order,
                        is_draft: issue.is_draft,
                    },
                    project: ProjectLiteSerializer {
                        id: project.id,
                        identifier: &project.identifier,
                        name: &project.name,
                        cover_image: project.cover_image.as_deref(),
                        cover_image_url: record.project_cover_image_url.as_deref(),
                        logo_props: Some(PgJson::from(&project.logo_props)),
                        description: &project.description,
                    },
                    workspace: WorkspaceLiteSerializer {
                        name: &workspace.name,
                        slug: &workspace.slug,
                        id: workspace.id,
                        logo_url: record.workspace_logo_url.as_deref(),
                    },
                    attachments: record
                        .activity
                        .attachments
                        .iter()
                        .map(String::as_str)
                        .collect(),
                }
            })
            .collect();

        let envelope = ActivityEnvelope {
            total,
            has_results,
            limit,
            page_index,
            results: &bodies,
        };
        let rendered = plane_serializers::render(&envelope, &rows::plain_ctx(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, rendered))
    })
}

/// `plane/utils/order_queryset.py:sanitize_order_by` for the activity list.
fn sanitize_order_by(value: &str) -> &str {
    if value.is_empty() {
        return "-created_at";
    }
    let (is_desc, bare) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    if bare.starts_with('-') || !matches!(bare, "created_at" | "updated_at") {
        return "-created_at";
    }
    match (bare, is_desc) {
        ("created_at", true) => "-created_at",
        ("created_at", false) => "created_at",
        (_, true) => "-updated_at",
        (_, false) => "updated_at",
    }
}

/// `order_by(F(key).desc(nulls_last=True), "-created_at")`
/// (`plane/utils/paginator.py:131-135`).
///
/// Django deduplicates the ordering terms by expression, so `-created_at` and
/// the `-created_at` tiebreak collapse into one term while an ascending
/// `created_at` keeps both (`ORDER BY created_at ASC NULLS LAST, created_at
/// DESC`).
fn activity_order(order: &str, column: &str) -> Vec<Frag> {
    if column == "issue_activities.created_at" && order.ends_with("DESC NULLS LAST") {
        vec![Frag::raw(order.to_owned())]
    } else {
        vec![
            Frag::raw(order.to_owned()),
            Frag::raw("issue_activities.created_at DESC"),
        ]
    }
}

/// The shared `SELECT … FROM issue_activities …` of the activity paginator,
/// with every joined row the serializer reads.
fn activity_select(slug: &str, user: Uuid, target: Uuid, projects: &[Uuid]) -> Select {
    let mut select = Select::new("issue_activities");
    select.joins = vec![
        Join::inner(
            "issues",
            "issues",
            Frag::raw("issue_activities.issue_id = issues.id"),
        ),
        Join::inner(
            "projects",
            "projects",
            Frag::raw("issues.project_id = projects.id"),
        ),
        Join::inner(
            "users",
            "received",
            Frag::raw("received.id = issue_activities.actor_id"),
        ),
        Join::inner(
            "projects",
            "issue_projects",
            Frag::raw("issue_projects.id = issue_activities.project_id"),
        ),
        Join::inner(
            "workspaces",
            "workspaces",
            Frag::raw("workspaces.id = issue_activities.workspace_id"),
        ),
        Join::left(
            "file_assets",
            "actor_asset",
            Frag::raw("actor_asset.id = received.avatar_asset_id"),
        ),
        Join::left(
            "file_assets",
            "cover_asset",
            Frag::raw("cover_asset.id = issue_projects.cover_image_asset_id"),
        ),
        Join::left(
            "file_assets",
            "logo_asset",
            Frag::raw("logo_asset.id = workspaces.logo_asset_id"),
        ),
    ];
    let mut predicate = Frag::is_null("issue_activities.deleted_at")
        .and(
            Frag::in_texts(
                "issue_activities.field",
                &[
                    "comment".to_owned(),
                    "vote".to_owned(),
                    "reaction".to_owned(),
                    "draft".to_owned(),
                ],
            )
            .and(Frag::is_not_null("issue_activities.field"))
            .negate(),
        )
        .and(visible_issues(user))
        .and(Frag::is_null("issue_projects.archived_at"))
        .and(Frag::compare(
            "issue_activities.actor_id",
            "=",
            Value::Uuid(target),
        ))
        .and(Frag::compare(
            "workspaces.slug",
            "=",
            Value::Text(slug.to_owned()),
        ));
    if !projects.is_empty() {
        predicate = predicate.and(Frag::in_uuids("issue_activities.project_id", projects));
    }
    let mut projection: Vec<Frag> = Vec::new();
    for column in [
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
        "issue_id",
        "verb",
        "field",
        "old_value",
        "new_value",
        "comment",
        "attachments",
        "issue_comment_id",
        "actor_id",
        "old_identifier",
        "new_identifier",
        "epoch",
    ] {
        projection.push(Frag::raw(format!("issue_activities.{column}")));
    }
    for column in [
        "id",
        "email",
        "first_name",
        "last_name",
        "avatar",
        "avatar_asset_id",
        "is_bot",
        "display_name",
        "last_login_medium",
    ] {
        projection.push(Frag::raw(format!("received.{column}")));
    }
    projection.push(Frag::raw("actor_asset.entity_type"));
    for column in [
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "project_id",
        "workspace_id",
        "parent_id",
        "state_id",
        "point",
        "estimate_point_id",
        "name",
        "description_json",
        "description_html",
        "description_stripped",
        "description_binary",
        "priority",
        "start_date",
        "target_date",
        "sequence_id",
        "sort_order",
        "completed_at",
        "archived_at",
        "is_draft",
        "external_source",
        "external_id",
        "type_id",
    ] {
        projection.push(Frag::raw(format!("issues.{column}")));
    }
    for column in [
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "name",
        "description",
        "description_text",
        "description_html",
        "network",
        "workspace_id",
        "identifier",
        "default_assignee_id",
        "project_lead_id",
        "emoji",
        "icon_prop",
        "module_view",
        "cycle_view",
        "issue_views_view",
        "page_view",
        "intake_view",
        "is_time_tracking_enabled",
        "is_issue_type_enabled",
        "guest_view_all_features",
        "cover_image",
        "cover_image_asset_id",
        "estimate_id",
        "archive_in",
        "close_in",
        "logo_props",
        "default_state_id",
        "archived_at",
        "timezone",
        "external_source",
        "external_id",
    ] {
        projection.push(Frag::raw(format!("issue_projects.{column}")));
    }
    projection.push(Frag::raw("cover_asset.entity_type"));
    for column in [
        "created_at",
        "updated_at",
        "created_by_id",
        "updated_by_id",
        "deleted_at",
        "id",
        "name",
        "logo",
        "logo_asset_id",
        "owner_id",
        "slug",
        "organization_size",
        "timezone",
        "background_color",
    ] {
        projection.push(Frag::raw(format!("workspaces.{column}")));
    }
    projection.push(Frag::raw("logo_asset.entity_type"));
    select.projection = projection;
    select.predicate = Some(predicate);
    select
}

/// The `BasePaginator.paginate` envelope (`plane/utils/paginator.py:735-758`).
#[derive(Debug)]
struct ActivityEnvelope<'a> {
    total: i64,
    has_results: bool,
    limit: i64,
    page_index: i64,
    results: &'a [ActivityView<'a>],
}

impl DrfSerialize for ActivityEnvelope<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let next = Cursor::new(self.limit, self.page_index + 1, false, self.has_results);
        let prev = Cursor::new(self.limit, self.page_index - 1, true, self.page_index > 0);
        let total_pages = if self.limit > 0 {
            (self.total + self.limit - 1) / self.limit
        } else {
            0
        };
        let mut object = w.begin_object();
        object.null_field("grouped_by")?;
        object.null_field("sub_grouped_by")?;
        object.field("total_count", &self.total)?;
        object.field("next_cursor", &next.to_string())?;
        object.field("prev_cursor", &prev.to_string())?;
        object.field("next_page_results", &next.has_results)?;
        object.field("prev_page_results", &prev.has_results)?;
        object.field("count", &(self.results.len() as i64))?;
        object.field("total_pages", &total_pages)?;
        object.field("total_results", &self.total)?;
        object.null_field("extra_stats")?;
        object.field_nested_list("results", ctx, self.results)?;
        object.end();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// user-issues
// ---------------------------------------------------------------------------

/// `WorkspaceUserProfileIssuesEndpoint.get` (`user.py:152-268`).
///
/// `plane_query` composes the page; the two scope predicates this endpoint adds
/// over `IssueViewSet.list` — `id__in` (issues the user is related to) and
/// `_visible_issues_q` — travel in the `rich` filter slot, which is the only
/// public seam into `IssueQuery`'s predicate.
fn user_issues<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = caller(ctx)?;
        let target = target_user(ctx)?;
        let client = client(ctx)?;
        let timezone = rows::timezone(ctx);
        let now = plane_core::clock::now();

        let query = ctx.query();
        let group_by_param = query.get("group_by").filter(|value| !value.is_empty());
        let sub_group_by_param = query.get("sub_group_by").filter(|value| !value.is_empty());
        let page =
            PageQuery::parse(query, group_by_param, sub_group_by_param).map_err(query_error)?;
        let legacy = LegacyFilters::parse(&FilterParams::Get(query), now.date_naive())
            .map_err(query_error)?;
        let rich = rich::parse_filterset(query, timezone).map_err(query_error)?;
        let rich = match rich::parse_complex_query(query, timezone).map_err(query_error)? {
            Some(complex) => Some(complex),
            None => rich,
        };
        let ordering = Ordering::resolve(query.get_or("order_by", "-created_at"));
        let group_by = group_by_param.and_then(GroupField::parse);
        let sub_group_by = sub_group_by_param.and_then(GroupField::parse);
        if group_by.is_some() && group_by == sub_group_by {
            return rows::error_body(400, "Group by and sub group by cannot have same parameters");
        }

        // `id__in=Issue.issue_objects.filter(Q(assignees__in=[user_id]) |
        // Q(created_by_id=user_id) | Q(issue_subscribers__subscriber_id=user_id),
        // workspace__slug=slug).values_list("id", flat=True)` (`user.py:158-161`).
        let mut scope = Frag::raw("issues.id IN (");
        scope.push(Frag::raw(
            "SELECT i2.id FROM issues i2 \
             LEFT OUTER JOIN states s2 ON i2.state_id = s2.id \
             INNER JOIN projects p2 ON i2.project_id = p2.id \
             LEFT OUTER JOIN issue_assignees ia2 ON i2.id = ia2.issue_id \
             LEFT OUTER JOIN issue_subscribers is2 ON i2.id = is2.issue_id \
             INNER JOIN workspaces w2 ON i2.workspace_id = w2.id \
             WHERE i2.deleted_at IS NULL \
               AND NOT (s2.\"group\" = 'triage' AND s2.\"group\" IS NOT NULL) \
               AND i2.archived_at IS NULL AND p2.archived_at IS NULL \
               AND NOT i2.is_draft \
               AND (ia2.assignee_id = ",
        ));
        scope.push(Frag::bind(Value::Uuid(target)));
        scope.push_raw(" OR i2.created_by_id = ");
        scope.push(Frag::bind(Value::Uuid(target)));
        scope.push_raw(" OR is2.subscriber_id = ");
        scope.push(Frag::bind(Value::Uuid(target)));
        scope.push_raw(") AND w2.slug = ");
        scope.push(Frag::bind(Value::Text(slug.to_owned())));
        scope.push_raw(")");
        scope = scope.and(visible_issues(user));

        let rich = match rich {
            Some(rich) => Some(rich::RichFilters {
                joins: rich.joins,
                predicate: rich.predicate.and(scope),
            }),
            None => Some(rich::RichFilters {
                joins: Vec::new(),
                predicate: scope,
            }),
        };

        let issue_query = IssueQuery {
            scope: IssueScope {
                workspace_slug: slug.to_owned(),
                project_id: None,
                // `get_queryset` here does not call `.distinct()`
                // (`user.py:156-163`).
                distinct: false,
                created_by: None,
            },
            filters: legacy,
            rich,
            ordering,
            group_by,
            sub_group_by,
            timezone,
        };

        let plan = match group_by {
            Some(field) => {
                let group_fields = fetch_group_values(client, &issue_query, field, user).await?;
                let (sub_group_by, sub_group_fields) = match sub_group_by {
                    Some(sub) => (
                        Some(sub),
                        fetch_group_values(client, &issue_query, sub, user).await?,
                    ),
                    None => (None, Vec::new()),
                };
                Some(GroupPlan {
                    group_by: field,
                    group_fields,
                    sub_group_by,
                    sub_group_fields,
                })
            }
            None => None,
        };

        let mut body = issue_list_page(client, &issue_query, plan.as_ref(), &page)
            .await
            .map_err(query_error)?;
        coalesce_empty_counts(&mut body);
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body.to_drf()))
    })
}

/// This endpoint's `apply_annotations` (`user.py:128-148`) annotates the
/// aggregate querysets directly instead of wrapping them in `Subquery` as
/// `IssueViewSet` does (`plane/app/views/issue/base.py:235-256`). Without the
/// `GROUP BY` those subqueries return `0` for an empty relation where the
/// `Subquery` form returns `NULL`, so `plane_query`'s projection (which models
/// the viewset form) reports `null` and this endpoint reports `0`.
///
/// `COUNT(...)` is never null when its subquery produced a row, so `null` here
/// means "no related row" and the faithful value is `0`.
fn coalesce_empty_counts(value: &mut plane_query::Json) {
    use plane_query::Json;
    match value {
        Json::Object(entries) => {
            for (key, entry) in entries.iter_mut() {
                if matches!(
                    key.as_str(),
                    "link_count" | "attachment_count" | "sub_issues_count"
                ) && matches!(entry, Json::Null)
                {
                    *entry = Json::Int(0);
                } else {
                    coalesce_empty_counts(entry);
                }
            }
        }
        Json::Array(items) => {
            for item in items.iter_mut() {
                coalesce_empty_counts(item);
            }
        }
        _ => {}
    }
}

/// `issue_group_values(field, slug, filters, queryset=<filtered>, user=<caller>)`
/// (`plane/utils/grouper.py:144-219`) for a workspace-wide list: the group keys
/// come from the caller's projects, not from a `project_id`.
async fn fetch_group_values(
    client: &deadpool_postgres::Client,
    query: &IssueQuery,
    field: GroupField,
    user: Uuid,
) -> Result<Vec<String>, ViewError> {
    let filtered = query.filtered_base();
    let plan = group_values::values(&group_values::GroupValuesRequest {
        field,
        workspace_slug: &query.scope.workspace_slug,
        project_id: None,
        user: Some(user),
        issues: Some(&filtered),
    })
    .map_err(query_error)?;
    match plan {
        group_values::GroupValues::Static(values) => Ok(values),
        group_values::GroupValues::Query { sql, append_none } => {
            let rows = sql.query(client).await?;
            let mut values = Vec::with_capacity(rows.len() + 1);
            for row in &rows {
                values.push(
                    plane_query::issue::query::render_group_value(row, field)
                        .map_err(query_error)?,
                );
            }
            if append_none {
                values.push("None".to_owned());
            }
            // `__get_field_dict` builds a dict from this list, so duplicates
            // collapse; first appearance wins.
            let mut seen = std::collections::HashSet::new();
            values.retain(|value| seen.insert(value.clone()));
            Ok(values)
        }
    }
}
