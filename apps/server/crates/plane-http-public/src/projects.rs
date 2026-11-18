// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Projects on `/api/v1/` (routes 307–311).
//!
//! Python: `plane/api/views/project.py:81-821`.
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | `ws/projects/` | `GET` | `ProjectListCreateAPIEndpoint.get` (`:171-208`) |
//! | `ws/projects-lite/` | `GET` | `ProjectListLiteAPIEndpoint.get` (`:398-431`) |
//! | `ws/projects/{pk}/` | `GET` | `ProjectDetailAPIEndpoint.get` (`:520-527`) |
//! | `ws/projects/{project_id}/archive/` | `POST`/`DELETE` | `ProjectArchiveUnarchiveAPIEndpoint` (`:672-705`) |
//! | `ws/projects/{project_id}/summary/` | `GET` | `ProjectSummaryAPIEndpoint.get` (`:724-746`) |
//!
//! `ProjectListCreateAPIEndpoint.post`, `ProjectDetailAPIEndpoint.patch` and
//! `.delete` are **not** registered yet: their bodies carry the model-activity
//! and webhook Celery publishes plus the `ProjectCreateSerializer` side effects
//! (default states, `ProjectMember` rows, `ProjectUserProperty` rows) and are
//! the next step for this set.
//!
//! ## The list
//!
//! The view composes an annotated queryset whose `ProjectSerializer` fields do
//! not exist on the model (`plane/api/serializers/project.py:218-225`), then
//! orders it with `sanitize_order_by(request.GET.get("order_by", "sort_order"),
//! PROJECT_ORDER_BY_ALLOWLIST, default="sort_order")` (`:194-200`). Unlike the
//! issue list, `BasePaginator.paginate` is called without `order_by=`, so the
//! queryset's own order survives (`plane/utils/paginator.py:658-670`).
//!
//! `ProjectSummary`'s `counts` keys follow Python `set` iteration order; per
//! DECISIONS.md D16 this port writes them in ascending order and the read
//! scenario compares `counts` as an object (`scenarios/normalize.yaml`).
//!
//! ## Bugs ported as they are
//!
//! * `?include_archived` is string equality on `("true", "1")` after
//!   `.lower()` (`project.py:416`).
//! * `ProjectSummary` returns `{"error": "Project not found"}` rather than a
//!   family 404 (`project.py:730-731`).

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::project::Project;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::paginator::{Cursor, OffsetWindow};
use plane_serializers::generated::api_project::{ProjectLiteSerializer, ProjectSerializer};
use plane_serializers::generated::api_user::UserLiteSerializer;
use plane_serializers::generated::api_workspace::WorkspaceLiteSerializer;
use plane_serializers::{DrfValue, Expansion, Expansions, FieldValue, PgJson, RenderCtx};
use uuid::Uuid;

use crate::common::{page, render::RequestRender};
use crate::projects_queries as queries;

/// `PROJECT_ORDER_BY_ALLOWLIST` (`plane/utils/order_queryset.py:57-63`).
const PROJECT_ORDER_BY_ALLOWLIST: &[&str] = &["created_at", "updated_at", "name", "sort_order"];

/// `ALLOWED_PROJECT_SUMMARY_FIELDS` (`plane/api/views/project.py:708-717`).
const SUMMARY_FIELDS: &[&str] = &[
    "members", "states", "labels", "cycles", "modules", "issues", "intakes", "pages",
];

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.project.ProjectListCreateAPIEndpoint")?
        .method(http::Method::GET, list)?;
    registry
        .api_view("plane.api.views.project.ProjectListLiteAPIEndpoint")?
        .method(http::Method::GET, list_lite)?;
    registry
        .api_view("plane.api.views.project.ProjectDetailAPIEndpoint")?
        .method(http::Method::GET, detail_get)?;
    registry
        .api_view("plane.api.views.project.ProjectArchiveUnarchiveAPIEndpoint")?
        .method(http::Method::POST, archive)?
        .method(http::Method::DELETE, unarchive)?;
    registry
        .api_view("plane.api.views.project.ProjectSummaryAPIEndpoint")?
        .method(http::Method::GET, summary)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Paging, shared with the other list routes of this set
// ---------------------------------------------------------------------------

/// Run one page of a list statement: append `ORDER BY`/`LIMIT`/`OFFSET`
/// values after the caller's parameters, decode the rows, then run the count.
///
/// `order_by` is a SQL fragment chosen from an allowlist, never request text.
/// The page fetch asks for `limit + 1` rows because `OffsetPaginator` decides
/// `next_page_results` from the extra row (`plane/utils/paginator.py:145-160`).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fetch_ordered_page<T>(
    ctx: &ViewCtx<'_>,
    client: &Client,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    total_sql: &str,
    total_params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    order_by: &str,
    default_per_page: u32,
    decode: impl Fn(&tokio_postgres::Row) -> Result<T, ViewError>,
) -> Result<(Vec<T>, i64, OffsetWindow, Cursor, Cursor), ViewError> {
    let page = page::query(ctx, default_per_page, 1000)?;
    let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
    let limit = window.row_limit() + 1;
    let offset = window.offset;
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params.to_vec();
    params.push(&limit);
    params.push(&offset);
    let statement = format!(
        "{sql} ORDER BY {order_by} LIMIT ${} OFFSET ${}",
        params.len() - 1,
        params.len()
    );
    let rows = client
        .query(&statement, &params)
        .await
        .map_err(ViewError::from)?;
    let fetched = rows.len();
    let mut items = Vec::with_capacity(fetched.min(window.row_limit() as usize));
    for row in rows.iter().take(window.row_limit() as usize) {
        items.push(decode(row)?);
    }
    let (next, prev) = window.cursors(fetched);
    let total: i64 = client
        .query_one(total_sql, total_params)
        .await
        .map_err(ViewError::from)?
        .get(0);
    Ok((items, total, window, next, prev))
}

/// [`fetch_ordered_page`] for a statement that already ends with its own
/// `ORDER BY` (`LIMIT`/`OFFSET` are still appended).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fetch_page<T>(
    ctx: &ViewCtx<'_>,
    client: &Client,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    total_sql: &str,
    total_params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    default_per_page: u32,
    decode: impl Fn(&tokio_postgres::Row) -> Result<T, ViewError>,
) -> Result<(Vec<T>, i64, OffsetWindow, Cursor, Cursor), ViewError> {
    let page = page::query(ctx, default_per_page, 1000)?;
    let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
    let limit = window.row_limit() + 1;
    let offset = window.offset;
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params.to_vec();
    params.push(&limit);
    params.push(&offset);
    let statement = format!("{sql} LIMIT ${} OFFSET ${}", params.len() - 1, params.len());
    let rows = client
        .query(&statement, &params)
        .await
        .map_err(ViewError::from)?;
    let fetched = rows.len();
    let mut items = Vec::with_capacity(fetched.min(window.row_limit() as usize));
    for row in rows.iter().take(window.row_limit() as usize) {
        items.push(decode(row)?);
    }
    let (next, prev) = window.cursors(fetched);
    let total: i64 = client
        .query_one(total_sql, total_params)
        .await
        .map_err(ViewError::from)?
        .get(0);
    Ok((items, total, window, next, prev))
}

/// The paginator envelope (`plane/utils/paginator.py:733-755`) around an
/// already-rendered `results` array.
pub(crate) fn page_envelope(
    total: i64,
    window: &OffsetWindow,
    next: &Cursor,
    prev: &Cursor,
    count: usize,
    results: Vec<u8>,
) -> ViewResponse {
    let total_pages = plane_query::paginator::ceil_div(total, window.limit);
    let head = format!(
        "{{\"grouped_by\":null,\"sub_grouped_by\":null,\"total_count\":{total},\"next_cursor\":\"{next}\",\"prev_cursor\":\"{prev}\",\"next_page_results\":{},\"prev_page_results\":{},\"count\":{count},\"total_pages\":{total_pages},\"total_results\":{total},\"extra_stats\":null,\"results\":",
        next.has_results, prev.has_results,
    );
    let mut body = head.into_bytes();
    body.extend_from_slice(&results);
    body.push(b'}');
    ViewResponse::raw(200, APPLICATION_JSON, body)
}

/// A one-key `{"error": …}` body, in Django's key order.
pub(crate) fn error_body(status: u16, message: &str) -> ViewResponse {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        error: &'a str,
    }
    match plane_serde::to_vec(plane_serde::Dialect::Drf, &Body { error: message }) {
        Ok(rendered) => ViewResponse::raw(status, APPLICATION_JSON, rendered),
        Err(_) => ViewResponse::raw(status, APPLICATION_JSON, br#"{"error":"error"}"#.to_vec()),
    }
}

/// `sanitize_order_by(value, PROJECT_ORDER_BY_ALLOWLIST, default)`, as the SQL
/// fragment the page statement appends.
fn order_by_fragment(ctx: &ViewCtx<'_>, default: &str) -> String {
    let raw = ctx.query().get("order_by").unwrap_or("");
    let sanitized = sanitize_order_by(raw, default);
    let (bare, descending) = match sanitized.strip_prefix('-') {
        Some(rest) => (rest, true),
        None => (sanitized.as_str(), false),
    };
    let expression = if bare == "sort_order" {
        // The annotation's alias, not a `projects` column.
        "sort_order".to_string()
    } else {
        format!("p.{bare}")
    };
    format!("{expression} {}", if descending { "DESC" } else { "ASC" })
}

/// `plane/utils/order_queryset.py:135-156`.
fn sanitize_order_by(value: &str, default: &str) -> String {
    if value.is_empty() {
        return default.to_string();
    }
    let (is_desc, bare) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    if bare.starts_with('-') || !PROJECT_ORDER_BY_ALLOWLIST.contains(&bare) {
        return default.to_string();
    }
    if is_desc {
        format!("-{bare}")
    } else {
        bare.to_string()
    }
}

// ---------------------------------------------------------------------------
// The annotated project row
// ---------------------------------------------------------------------------

/// One `get_queryset()` row plus its annotations.
struct ProjectRow {
    project: Project,
    is_member: bool,
    total_members: FieldValue<i64>,
    total_cycles: FieldValue<i64>,
    total_modules: FieldValue<i64>,
    member_role: Option<i16>,
    is_deployed: bool,
    sort_order: FieldValue<f64>,
    cover_image_url: Option<String>,
}

fn project_row(row: &tokio_postgres::Row) -> Result<ProjectRow, ViewError> {
    Ok(ProjectRow {
        project: Project::from_row(row, 0)?,
        is_member: row.get(36),
        total_members: FieldValue::from_attribute(row.get(37)),
        total_cycles: FieldValue::from_attribute(row.get(38)),
        total_modules: FieldValue::from_attribute(row.get(39)),
        member_role: row.get(40),
        is_deployed: row.get(41),
        sort_order: FieldValue::from_attribute(row.get(42)),
        cover_image_url: None,
    })
}

impl ProjectRow {
    fn serializer(&self) -> ProjectSerializer<'_> {
        let project = &self.project;
        ProjectSerializer {
            id: project.id,
            total_members: self.total_members,
            total_cycles: self.total_cycles,
            total_modules: self.total_modules,
            is_member: FieldValue::Value(self.is_member),
            sort_order: self.sort_order,
            member_role: FieldValue::from_attribute(self.member_role.map(i64::from)),
            is_deployed: FieldValue::Value(self.is_deployed),
            cover_image_url: self.cover_image_url.as_deref(),
            created_at: project.created_at,
            updated_at: project.updated_at,
            deleted_at: project.deleted_at,
            name: &project.name,
            description: &project.description,
            description_text: project.description_text.as_ref().map(PgJson::from),
            description_html: project.description_html.as_ref().map(PgJson::from),
            network: i64::from(project.network),
            identifier: &project.identifier,
            emoji: project.emoji.as_deref(),
            icon_prop: project.icon_prop.as_ref().map(PgJson::from),
            module_view: project.module_view,
            cycle_view: project.cycle_view,
            issue_views_view: project.issue_views_view,
            page_view: project.page_view,
            intake_view: project.intake_view,
            is_time_tracking_enabled: project.is_time_tracking_enabled,
            is_issue_type_enabled: project.is_issue_type_enabled,
            guest_view_all_features: project.guest_view_all_features,
            cover_image: project.cover_image.as_deref(),
            archive_in: i64::from(project.archive_in),
            close_in: i64::from(project.close_in),
            logo_props: Some(PgJson::from(&project.logo_props)),
            archived_at: project.archived_at,
            timezone: &project.timezone,
            external_source: project.external_source.as_deref(),
            external_id: project.external_id.as_deref(),
            created_by: project.created_by_id,
            updated_by: project.updated_by_id,
            workspace: project.workspace_id,
            default_assignee: project.default_assignee_id,
            project_lead: project.project_lead_id,
            cover_image_asset: project.cover_image_asset_id,
            estimate: project.estimate_id,
            default_state: project.default_state_id,
        }
    }
}

/// `Project.cover_image_url` (`plane/db/models/project.py:128-139`) for a
/// page: one batched asset query, then the three-branch property.
async fn resolve_cover_urls(client: &Client, rows: &mut [ProjectRow]) -> Result<(), ViewError> {
    let ids: std::collections::HashSet<Uuid> = rows
        .iter()
        .filter_map(|row| row.project.cover_image_asset_id)
        .collect();
    let urls = plane_http_app::projects::lite::load_asset_urls(client, &ids).await?;
    for row in rows {
        row.cover_image_url = plane_http_app::projects::lite::asset_or_text(
            row.project.cover_image_asset_id,
            row.project.cover_image.clone().unwrap_or_default(),
            &urls,
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `?expand=` for the project serializer
// ---------------------------------------------------------------------------

/// The nested objects a `ProjectSerializer` expansion can resolve.
///
/// `plane/api/serializers/base.py:96-115` maps `workspace`, `project_lead`,
/// `default_assignee`, `created_by` and `updated_by` for this serializer; every
/// other name in the request is either not a field (ignored) or a field
/// without a relation, which `:139` turns into `getattr(instance, "<name>_id",
/// None)` — normally `null`.
#[derive(Default)]
struct ExpansionData {
    workspace: Option<LiteWorkspace>,
    users: std::collections::HashMap<Uuid, LiteUser>,
}

struct LiteWorkspace {
    name: String,
    slug: String,
    id: Uuid,
}

impl LiteWorkspace {
    fn serializer(&self) -> WorkspaceLiteSerializer<'_> {
        WorkspaceLiteSerializer {
            name: &self.name,
            slug: &self.slug,
            id: self.id,
        }
    }
}

struct LiteUser {
    id: Uuid,
    first_name: String,
    last_name: String,
    email: Option<String>,
    avatar: String,
    avatar_url: Option<String>,
    display_name: String,
}

impl LiteUser {
    fn serializer(&self) -> UserLiteSerializer<'_> {
        UserLiteSerializer {
            id: self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            email: self.email.as_deref(),
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            display_name: &self.display_name,
        }
    }
}

/// The expansion names a `ProjectSerializer` field can hold.
const PROJECT_EXPANSION_RELATIONS: &[&str] = &[
    "workspace",
    "project_lead",
    "default_assignee",
    "created_by",
    "updated_by",
];

/// Load only what the request asked to expand.
async fn load_expansions(
    ctx: &ViewCtx<'_>,
    client: &Client,
    rows: &[ProjectRow],
    requested: &[&str],
) -> Result<ExpansionData, ViewError> {
    let mut data = ExpansionData::default();
    if requested.contains(&"workspace") {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let row = client
            .query_opt(queries::WORKSPACE_LITE.sql, &[&slug])
            .await
            .map_err(ViewError::from)?;
        // The forward descriptor would raise `Workspace.DoesNotExist` (a 500
        // through the family handler) if the row were missing; the route's
        // scoping makes that unreachable.
        data.workspace = row.map(|row| LiteWorkspace {
            name: row.get(0),
            slug: row.get(1),
            id: row.get(2),
        });
    }
    let mut ids: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    for row in rows {
        for name in PROJECT_EXPANSION_RELATIONS {
            if !requested.contains(name) {
                continue;
            }
            let id = match *name {
                "project_lead" => row.project.project_lead_id,
                "default_assignee" => row.project.default_assignee_id,
                "created_by" => row.project.created_by_id,
                "updated_by" => row.project.updated_by_id,
                _ => None,
            };
            ids.extend(id);
        }
    }
    if !ids.is_empty() {
        let ids: Vec<Uuid> = ids.into_iter().collect();
        let rows = client
            .query(queries::USERS_LITE_BY_IDS.sql, &[&ids])
            .await
            .map_err(ViewError::from)?;
        let mut users = Vec::with_capacity(rows.len());
        let mut asset_ids = std::collections::HashSet::new();
        for row in &rows {
            asset_ids.extend(row.get::<_, Option<Uuid>>(5));
            users.push((
                row.get::<_, Uuid>(0),
                row.get::<_, String>(1),
                row.get::<_, String>(2),
                row.get::<_, Option<String>>(3),
                row.get::<_, String>(4),
                row.get::<_, Option<Uuid>>(5),
                row.get::<_, String>(6),
            ));
        }
        let urls = plane_http_app::projects::lite::load_asset_urls(client, &asset_ids).await?;
        for (id, first_name, last_name, email, avatar, avatar_asset_id, display_name) in users {
            let avatar_url = plane_http_app::projects::lite::asset_or_text(
                avatar_asset_id,
                avatar.clone(),
                &urls,
            )?;
            data.users.insert(
                id,
                LiteUser {
                    id,
                    first_name,
                    last_name,
                    email,
                    avatar,
                    avatar_url,
                    display_name,
                },
            );
        }
    }
    Ok(data)
}

/// Render one row with `fields=` and this row's `expand=` values.
fn render_project(
    row: &ProjectRow,
    data: &ExpansionData,
    prepared: &RequestRender<'_>,
    requested: &[&str],
) -> Result<Vec<u8>, ViewError> {
    let workspace = data.workspace.as_ref().map(LiteWorkspace::serializer);
    let mut lead = None;
    let mut assignee = None;
    let mut created_by = None;
    let mut updated_by = None;
    for (slot, id) in [
        (&mut lead, row.project.project_lead_id),
        (&mut assignee, row.project.default_assignee_id),
        (&mut created_by, row.project.created_by_id),
        (&mut updated_by, row.project.updated_by_id),
    ] {
        if let Some(user) = id.and_then(|id| data.users.get(&id)) {
            *slot = Some(user.serializer());
        }
    }

    let mut resolved: Vec<Expansion<'_>> = Vec::new();
    if let Some(workspace) = &workspace {
        resolved.push(Expansion {
            name: "workspace",
            value: DrfValue::nested(workspace),
        });
    }
    for (name, user) in [
        ("project_lead", &lead),
        ("default_assignee", &assignee),
        ("created_by", &created_by),
        ("updated_by", &updated_by),
    ] {
        if let Some(user) = user {
            resolved.push(Expansion {
                name,
                value: DrfValue::nested(user),
            });
        }
    }
    let values = crate::common::render::expansions(requested, &resolved, |_| None);
    let render_ctx = prepared
        .render_ctx()
        .with_expansions(Expansions::new(requested, &values));
    plane_serializers::render(&row.serializer(), &render_ctx).map_err(ViewError::from)
}

/// Render a whole page, one expansion context per row.
fn render_projects(
    rows: &[ProjectRow],
    data: &ExpansionData,
    prepared: &RequestRender<'_>,
    requested: &[&str],
) -> Result<Vec<u8>, ViewError> {
    let serializers: Vec<ProjectSerializer<'_>> = rows.iter().map(ProjectRow::serializer).collect();
    if requested.is_empty() {
        return plane_http_app::common::render_list(&serializers, &prepared.render_ctx());
    }
    let mut body = Vec::with_capacity(rows.len() * 512 + 2);
    body.push(b'[');
    for (index, row) in rows.iter().enumerate() {
        if index != 0 {
            body.push(b',');
        }
        body.extend_from_slice(&render_project(row, data, prepared, requested)?);
    }
    body.push(b']');
    Ok(body)
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// `ProjectListCreateAPIEndpoint.get` (`plane/api/views/project.py:171-208`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let member = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;
        let order = order_by_fragment(ctx, "sort_order");

        let (mut rows, total, window, next, prev) = fetch_ordered_page(
            ctx,
            client,
            queries::PROJECT_LIST_BASE.sql,
            &[&slug, &member],
            queries::PROJECT_LIST_COUNT.sql,
            &[&slug, &member],
            &order,
            1000,
            project_row,
        )
        .await?;
        resolve_cover_urls(client, &mut rows).await?;

        let requested = ctx.expand().unwrap_or_default();
        let data = load_expansions(ctx, client, &rows, &requested).await?;
        let prepared = RequestRender::from_ctx(ctx);
        let results = render_projects(&rows, &data, &prepared, &requested)?;
        Ok(page_envelope(
            total,
            &window,
            &next,
            &prev,
            rows.len(),
            results,
        ))
    })
}

/// `ProjectDetailAPIEndpoint.get` (`plane/api/views/project.py:520-527`).
pub fn detail_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let member = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        let row = client
            .query_opt(queries::PROJECT_DETAIL_ONE.sql, &[&slug, &member, &pk])
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            // `get_queryset().get(...)` → `Project.DoesNotExist`
            // (`plane/api/views/project.py:525`).
            return Err(ViewError::ObjectDoesNotExist);
        };
        let mut rows = vec![project_row(&row)?];
        // `ProjectDetailAPIEndpoint.get_queryset` does not annotate
        // `sort_order` (`plane/api/views/project.py:444-502`), so DRF skips
        // the declared read-only field and the key is absent.
        rows[0].sort_order = FieldValue::Absent;
        resolve_cover_urls(client, &mut rows).await?;

        let requested = ctx.expand().unwrap_or_default();
        let data = load_expansions(ctx, client, &rows, &requested).await?;
        let prepared = RequestRender::from_ctx(ctx);
        let body = render_project(&rows[0], &data, &prepared, &requested)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// One [`ProjectLiteSerializer`] row.
struct LiteRow {
    id: Uuid,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    icon_prop: Option<plane_db::JsonText>,
    emoji: Option<String>,
    description: String,
    cover_image_asset_id: Option<Uuid>,
    archived_at: Option<DateTime<Utc>>,
}

fn lite_row(row: &tokio_postgres::Row) -> Result<LiteRow, ViewError> {
    Ok(LiteRow {
        id: row.get(0),
        identifier: row.get(1),
        name: row.get(2),
        cover_image: row.get(3),
        icon_prop: row.get(4),
        emoji: row.get(5),
        description: row.get(6),
        cover_image_asset_id: row.get(7),
        archived_at: row.get(8),
    })
}

/// `ProjectListLiteAPIEndpoint.get` (`plane/api/views/project.py:398-431`).
pub fn list_lite<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let member = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        // `if not Workspace.objects.filter(slug=slug).exists()`
        // (`project.py:405-409`) — a 404 with the view's own body.
        let workspace_present = workspace_exists(client, slug).await?;
        if !workspace_present {
            return Ok(error_body(404, "Provided workspace does not exist"));
        }

        let include_archived = ctx
            .query()
            .get("include_archived")
            .unwrap_or("false")
            .to_lowercase();
        let include_archived = include_archived == "true" || include_archived == "1";
        let (sql, count_sql) = if include_archived {
            (
                queries::PROJECT_LITE_PAGE.sql,
                queries::PROJECT_LITE_COUNT.sql,
            )
        } else {
            (
                queries::PROJECT_LITE_PAGE_LIVE.sql,
                queries::PROJECT_LITE_COUNT_LIVE.sql,
            )
        };
        let order = order_by_fragment(ctx, "-created_at");

        let (rows, total, window, next, prev) = fetch_ordered_page(
            ctx,
            client,
            sql,
            &[&slug, &member],
            count_sql,
            &[&slug, &member],
            &order,
            1000,
            lite_row,
        )
        .await?;

        let ids: std::collections::HashSet<Uuid> = rows
            .iter()
            .filter_map(|row| row.cover_image_asset_id)
            .collect();
        let urls = plane_http_app::projects::lite::load_asset_urls(client, &ids).await?;
        let mut cover_urls = Vec::with_capacity(rows.len());
        for row in &rows {
            cover_urls.push(plane_http_app::projects::lite::asset_or_text(
                row.cover_image_asset_id,
                row.cover_image.clone().unwrap_or_default(),
                &urls,
            )?);
        }
        let mut serializers = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            serializers.push(ProjectLiteSerializer {
                id: row.id,
                identifier: &row.identifier,
                name: &row.name,
                cover_image: row.cover_image.as_deref(),
                icon_prop: row.icon_prop.as_ref().map(PgJson::from),
                emoji: row.emoji.as_deref(),
                description: &row.description,
                cover_image_url: cover_urls[index].as_deref(),
                archived_at: row.archived_at,
            });
        }

        // `ProjectLiteSerializer(many=True)` with no `fields=`/`expand=`.
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let results =
            plane_http_app::common::render_list(&serializers, &RenderCtx::full(&timezone))?;
        Ok(page_envelope(
            total,
            &window,
            &next,
            &prev,
            serializers.len(),
            results,
        ))
    })
}

async fn workspace_exists(client: &Client, slug: &str) -> Result<bool, ViewError> {
    Ok(client
        .query_one(crate::members_queries::WORKSPACE_EXISTS.sql, &[&slug])
        .await
        .map_err(ViewError::from)?
        .get(0))
}

/// `ProjectSummaryAPIEndpoint.get` (`plane/api/views/project.py:724-746`).
pub fn summary<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = plane_http_app::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let client = plane_http_app::common::client(ctx)?;

        let project = client
            .query_opt(queries::SUMMARY_PROJECT.sql, &[&project_id, &slug])
            .await
            .map_err(ViewError::from)?;
        let Some(project) = project else {
            return Ok(error_body(404, "Project not found"));
        };
        let id: Uuid = project.get(0);
        let name: String = project.get(1);
        let identifier: String = project.get(2);

        // `fields = request.GET.get("fields", "").split(",")`, then a set
        // intersection with the allowed names; empty means every allowed name.
        let mut requested: Vec<&str> = ctx
            .query()
            .get("fields")
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|value| SUMMARY_FIELDS.contains(value))
            .collect();
        if requested.is_empty() {
            requested = SUMMARY_FIELDS.to_vec();
        }
        // Django iterates a `set` here (D16); Rust writes ascending order and
        // the scenario compares `counts` as an object.
        requested.sort_unstable();
        requested.dedup();

        let counts = client
            .query_one(queries::SUMMARY_COUNTS.sql, &[&project_id])
            .await
            .map_err(ViewError::from)?;
        let mut counts_body = String::from("{");
        for (index, field) in requested.iter().enumerate() {
            let position = SUMMARY_FIELDS
                .iter()
                .position(|allowed| allowed == field)
                .ok_or_else(|| ViewError::Unknown("summary field".to_string()))?;
            let value: i64 = counts.get(position);
            if index != 0 {
                counts_body.push(',');
            }
            counts_body.push_str(&format!("\"{field}\":{value}"));
        }
        counts_body.push('}');

        let body = format!(
            "{{\"id\":\"{id}\",\"name\":{},\"identifier\":{},\"counts\":{counts_body}}}",
            json_string(&name)?,
            json_string(&identifier)?,
        );
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body.into_bytes()))
    })
}

/// One JSON string literal, through the DRF dialect (no escaping of non-ASCII).
fn json_string(value: &str) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Drf, &value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `Project.objects.get(pk=project_id, workspace__slug=slug)` for the archive
/// pair, without the annotated queryset (the archive handlers never read the
/// row back).
async fn project_in_workspace(
    client: &Client,
    slug: &str,
    project_id: Uuid,
) -> Result<Project, ViewError> {
    let row = client
        .query_opt(
            queries::PROJECT_BY_ID_IN_WORKSPACE.sql,
            &[&project_id, &slug],
        )
        .await
        .map_err(ViewError::from)?;
    match row {
        Some(row) => Project::from_row(&row, 0).map_err(ViewError::from),
        None => Err(ViewError::ObjectDoesNotExist),
    }
}

/// `ProjectArchiveUnarchiveAPIEndpoint.post`
/// (`plane/api/views/project.py:672-682`).
pub fn archive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = plane_http_app::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let actor = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        let project = project_in_workspace(client, slug, project_id).await?;
        let now = plane_http_app::write::now();
        client
            .execute(queries::PROJECT_ARCHIVE.sql, &[&now, &actor, &project.id])
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                queries::FAVORITES_SOFT_DELETE.sql,
                &[&now, &slug, &project_id],
            )
            .await
            .map_err(ViewError::from)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `ProjectArchiveUnarchiveAPIEndpoint.delete`
/// (`plane/api/views/project.py:696-705`).
pub fn unarchive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = plane_http_app::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?;
        let actor = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        let project = project_in_workspace(client, slug, project_id).await?;
        let now = plane_http_app::write::now();
        client
            .execute(queries::PROJECT_UNARCHIVE.sql, &[&now, &actor, &project.id])
            .await
            .map_err(ViewError::from)?;
        Ok(ViewResponse::empty(204))
    })
}
