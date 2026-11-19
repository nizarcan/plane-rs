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
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | `ws/projects/` | `POST` | `ProjectListCreateAPIEndpoint.post` (`:228-343`) |
//! | `ws/projects/{pk}/` | `PATCH` | `ProjectDetailAPIEndpoint.patch` (`:550-614`) |
//! | `ws/projects/{pk}/` | `DELETE` | `ProjectDetailAPIEndpoint.delete` (`:627-650`) |
//!
//! The create runs H08/H01, one `ProjectMember` (role 20) for the caller and
//! another for a different `project_lead`, H32's `ProjectUserProperty` for each,
//! the six `DEFAULT_STATES`, and publishes `model_activity` **after** the
//! commit with `robust=True`. It never writes `project_identifiers` (the v1
//! serializer differs from the app's, `api/serializers/project.py:143-163`), so
//! a duplicate identifier only answers 409 when the fixture has a
//! `project_identifiers` row; a duplicate name hits the partial unique index
//! and postgres's DETAIL says "already exists", so that is a 409 name body.
//!
//! The PATCH mirrors `ProjectUpdateSerializer.update`: a `default_state` in the
//! body is the ported Django bug (`id=<State instance>` → a core
//! `ValidationError` → 400 "Please provide valid detail"), a foreign `estimate`
//! is the view's 409 identifier body, `intake_view` is injected into the body
//! and creates the default `Intake` when missing, and `current_instance` is
//! `json.dumps(ProjectSerializer(project).data, cls=DjangoJSONEncoder)`.
//!
//! The delete soft-deletes the project's `user_favorites` rows (H04), the
//! project itself (H03 + cascade task) and publishes `webhook_activity`.
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
        .method(http::Method::GET, list)?
        .method(http::Method::POST, create)?;
    registry
        .api_view("plane.api.views.project.ProjectListLiteAPIEndpoint")?
        .method(http::Method::GET, list_lite)?;
    registry
        .api_view("plane.api.views.project.ProjectDetailAPIEndpoint")?
        .method(http::Method::GET, detail_get)?
        .method(http::Method::PATCH, patch)?
        .method(http::Method::DELETE, delete)?;
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
    is_member: FieldValue<bool>,
    total_members: FieldValue<i64>,
    total_cycles: FieldValue<i64>,
    total_modules: FieldValue<i64>,
    member_role: FieldValue<i64>,
    is_deployed: FieldValue<bool>,
    sort_order: FieldValue<f64>,
    cover_image_url: Option<String>,
}

fn project_row(row: &tokio_postgres::Row) -> Result<ProjectRow, ViewError> {
    Ok(ProjectRow {
        project: Project::from_row(row, 0)?,
        is_member: FieldValue::Value(row.get(36)),
        total_members: FieldValue::from_attribute(row.get(37)),
        total_cycles: FieldValue::from_attribute(row.get(38)),
        total_modules: FieldValue::from_attribute(row.get(39)),
        member_role: FieldValue::from_attribute(row.get::<_, Option<i16>>(40).map(i64::from)),
        is_deployed: FieldValue::Value(row.get(41)),
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
            is_member: self.is_member,
            sort_order: self.sort_order,
            member_role: self.member_role,
            is_deployed: self.is_deployed,
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

// ---------------------------------------------------------------------------
// Writes: 307 POST, 309 PATCH/DELETE
// ---------------------------------------------------------------------------

/// `ProjectCreateSerializer`/`ProjectUpdateSerializer` after the generated
/// field pass.
#[derive(Default)]
struct ProjectWrite {
    name: Option<String>,
    description: Option<String>,
    project_lead: Option<Uuid>,
    default_assignee: Option<Uuid>,
    identifier: Option<String>,
    icon_prop: Option<plane_db::JsonText>,
    emoji: Option<String>,
    cover_image: Option<String>,
    module_view: Option<bool>,
    cycle_view: Option<bool>,
    issue_views_view: Option<bool>,
    page_view: Option<bool>,
    intake_view: Option<bool>,
    guest_view_all_features: Option<bool>,
    archive_in: Option<i32>,
    close_in: Option<i32>,
    timezone: Option<String>,
    external_source: Option<String>,
    external_id: Option<String>,
    is_issue_type_enabled: Option<bool>,
    is_time_tracking_enabled: Option<bool>,
    /// `default_state`/`estimate` exist on the update serializer only.
    default_state: Option<Uuid>,
    estimate: Option<Uuid>,
}

/// `ProjectCreateSerializer`/`ProjectUpdateSerializer.to_internal_value` in
/// `Meta.fields` order (`api/serializers/project.py:71-95`, `:174-181`).
///
/// The generated table cannot be used here because `ProjectDetail.patch`
/// injects `intake_view` into the body (`api/views/project.py:569-574`), which
/// [`Serializer::bool_field_value`] is the only way to express.
#[allow(clippy::field_reassign_with_default)]
async fn read_project_body(
    serializer: &mut plane_http_app::write::Serializer<'_>,
    client: &Client,
    injected_intake: Option<plane_serde::pyjson::Value>,
    update: bool,
) -> Result<ProjectWrite, ViewError> {
    use plane_http_app::write::{CharOptions, Detail};

    let mut write = ProjectWrite::default();
    write.name = serializer.char_field(
        "name",
        CharOptions {
            allow_null: false,
            allow_blank: false,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    if write.name.is_none() && serializer.missing_required("name") {
        serializer.push_error("name", Detail::required());
    }
    write.description = serializer.char_field(
        "description",
        CharOptions {
            allow_null: false,
            allow_blank: true,
            trim_whitespace: true,
            max_length: None,
        },
    );
    write.project_lead = serializer
        .primary_key_field(client, "project_lead", true, &queries::PROJECT_USER_BY_ID)
        .await?;
    write.default_assignee = serializer
        .primary_key_field(
            client,
            "default_assignee",
            true,
            &queries::PROJECT_USER_BY_ID,
        )
        .await?;
    write.identifier = serializer.char_field(
        "identifier",
        CharOptions {
            allow_null: false,
            allow_blank: false,
            trim_whitespace: true,
            max_length: Some(12),
        },
    );
    if write.identifier.is_none() && serializer.missing_required("identifier") {
        serializer.push_error("identifier", Detail::required());
    }
    write.icon_prop = serializer.json_field("icon_prop", true);
    write.emoji = serializer.char_field(
        "emoji",
        CharOptions {
            allow_null: true,
            allow_blank: true,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    write.cover_image = serializer.char_field(
        "cover_image",
        CharOptions {
            allow_null: true,
            allow_blank: true,
            trim_whitespace: true,
            max_length: None,
        },
    );
    write.module_view = serializer.bool_field("module_view");
    write.cycle_view = serializer.bool_field("cycle_view");
    write.issue_views_view = serializer.bool_field("issue_views_view");
    write.page_view = serializer.bool_field("page_view");
    // `intake_view`: the view's injected value on PATCH, the body on create.
    write.intake_view = match injected_intake {
        Some(value) => serializer.bool_field_value(
            "intake_view",
            plane_http_app::write::fields::Raw::Json(&value),
        ),
        None => serializer.bool_field("intake_view"),
    };
    write.guest_view_all_features = serializer.bool_field("guest_view_all_features");
    write.archive_in = serializer
        .integer_field(
            "archive_in",
            false,
            plane_http_app::write::IntegerOptions {
                min_value: Some(0),
                max_value: Some(12),
            },
        )
        .and_then(|value| i32::try_from(value).ok());
    write.close_in = serializer
        .integer_field(
            "close_in",
            false,
            plane_http_app::write::IntegerOptions {
                min_value: Some(0),
                max_value: Some(12),
            },
        )
        .and_then(|value| i32::try_from(value).ok());
    write.timezone = serializer.choice_field("timezone", false, false, COMMON_TIMEZONES);
    write.external_source = serializer.char_field(
        "external_source",
        CharOptions::nullable().with_max_length(255),
    );
    write.external_id =
        serializer.char_field("external_id", CharOptions::nullable().with_max_length(255));
    write.is_issue_type_enabled = serializer.bool_field("is_issue_type_enabled");
    write.is_time_tracking_enabled = serializer.bool_field("is_time_tracking_enabled");
    if update {
        write.default_state = serializer
            .primary_key_field(client, "default_state", true, &queries::PROJECT_STATE_BY_ID)
            .await?;
        write.estimate = serializer
            .primary_key_field(client, "estimate", true, &queries::PROJECT_ESTIMATE_BY_ID)
            .await?;
    }
    Ok(write)
}

/// `Project.timezone`'s choices: `pytz.common_timezones`
/// (`plane/db/models/project.py:117-119`).
const COMMON_TIMEZONES: &[&str] = &[
    "Africa/Abidjan",
    "Africa/Accra",
    "Africa/Addis_Ababa",
    "Africa/Algiers",
    "Africa/Asmara",
    "Africa/Bamako",
    "Africa/Bangui",
    "Africa/Banjul",
    "Africa/Bissau",
    "Africa/Blantyre",
    "Africa/Brazzaville",
    "Africa/Bujumbura",
    "Africa/Cairo",
    "Africa/Casablanca",
    "Africa/Ceuta",
    "Africa/Conakry",
    "Africa/Dakar",
    "Africa/Dar_es_Salaam",
    "Africa/Djibouti",
    "Africa/Douala",
    "Africa/El_Aaiun",
    "Africa/Freetown",
    "Africa/Gaborone",
    "Africa/Harare",
    "Africa/Johannesburg",
    "Africa/Juba",
    "Africa/Kampala",
    "Africa/Khartoum",
    "Africa/Kigali",
    "Africa/Kinshasa",
    "Africa/Lagos",
    "Africa/Libreville",
    "Africa/Lome",
    "Africa/Luanda",
    "Africa/Lubumbashi",
    "Africa/Lusaka",
    "Africa/Malabo",
    "Africa/Maputo",
    "Africa/Maseru",
    "Africa/Mbabane",
    "Africa/Mogadishu",
    "Africa/Monrovia",
    "Africa/Nairobi",
    "Africa/Ndjamena",
    "Africa/Niamey",
    "Africa/Nouakchott",
    "Africa/Ouagadougou",
    "Africa/Porto-Novo",
    "Africa/Sao_Tome",
    "Africa/Tripoli",
    "Africa/Tunis",
    "Africa/Windhoek",
    "America/Adak",
    "America/Anchorage",
    "America/Anguilla",
    "America/Antigua",
    "America/Araguaina",
    "America/Argentina/Buenos_Aires",
    "America/Argentina/Catamarca",
    "America/Argentina/Cordoba",
    "America/Argentina/Jujuy",
    "America/Argentina/La_Rioja",
    "America/Argentina/Mendoza",
    "America/Argentina/Rio_Gallegos",
    "America/Argentina/Salta",
    "America/Argentina/San_Juan",
    "America/Argentina/San_Luis",
    "America/Argentina/Tucuman",
    "America/Argentina/Ushuaia",
    "America/Aruba",
    "America/Asuncion",
    "America/Atikokan",
    "America/Bahia",
    "America/Bahia_Banderas",
    "America/Barbados",
    "America/Belem",
    "America/Belize",
    "America/Blanc-Sablon",
    "America/Boa_Vista",
    "America/Bogota",
    "America/Boise",
    "America/Cambridge_Bay",
    "America/Campo_Grande",
    "America/Cancun",
    "America/Caracas",
    "America/Cayenne",
    "America/Cayman",
    "America/Chicago",
    "America/Chihuahua",
    "America/Ciudad_Juarez",
    "America/Costa_Rica",
    "America/Creston",
    "America/Cuiaba",
    "America/Curacao",
    "America/Danmarkshavn",
    "America/Dawson",
    "America/Dawson_Creek",
    "America/Denver",
    "America/Detroit",
    "America/Dominica",
    "America/Edmonton",
    "America/Eirunepe",
    "America/El_Salvador",
    "America/Fort_Nelson",
    "America/Fortaleza",
    "America/Glace_Bay",
    "America/Goose_Bay",
    "America/Grand_Turk",
    "America/Grenada",
    "America/Guadeloupe",
    "America/Guatemala",
    "America/Guayaquil",
    "America/Guyana",
    "America/Halifax",
    "America/Havana",
    "America/Hermosillo",
    "America/Indiana/Indianapolis",
    "America/Indiana/Knox",
    "America/Indiana/Marengo",
    "America/Indiana/Petersburg",
    "America/Indiana/Tell_City",
    "America/Indiana/Vevay",
    "America/Indiana/Vincennes",
    "America/Indiana/Winamac",
    "America/Inuvik",
    "America/Iqaluit",
    "America/Jamaica",
    "America/Juneau",
    "America/Kentucky/Louisville",
    "America/Kentucky/Monticello",
    "America/Kralendijk",
    "America/La_Paz",
    "America/Lima",
    "America/Los_Angeles",
    "America/Lower_Princes",
    "America/Maceio",
    "America/Managua",
    "America/Manaus",
    "America/Marigot",
    "America/Martinique",
    "America/Matamoros",
    "America/Mazatlan",
    "America/Menominee",
    "America/Merida",
    "America/Metlakatla",
    "America/Mexico_City",
    "America/Miquelon",
    "America/Moncton",
    "America/Monterrey",
    "America/Montevideo",
    "America/Montserrat",
    "America/Nassau",
    "America/New_York",
    "America/Nome",
    "America/Noronha",
    "America/North_Dakota/Beulah",
    "America/North_Dakota/Center",
    "America/North_Dakota/New_Salem",
    "America/Nuuk",
    "America/Ojinaga",
    "America/Panama",
    "America/Paramaribo",
    "America/Phoenix",
    "America/Port-au-Prince",
    "America/Port_of_Spain",
    "America/Porto_Velho",
    "America/Puerto_Rico",
    "America/Punta_Arenas",
    "America/Rankin_Inlet",
    "America/Recife",
    "America/Regina",
    "America/Resolute",
    "America/Rio_Branco",
    "America/Santarem",
    "America/Santiago",
    "America/Santo_Domingo",
    "America/Sao_Paulo",
    "America/Scoresbysund",
    "America/Sitka",
    "America/St_Barthelemy",
    "America/St_Johns",
    "America/St_Kitts",
    "America/St_Lucia",
    "America/St_Thomas",
    "America/St_Vincent",
    "America/Swift_Current",
    "America/Tegucigalpa",
    "America/Thule",
    "America/Tijuana",
    "America/Toronto",
    "America/Tortola",
    "America/Vancouver",
    "America/Whitehorse",
    "America/Winnipeg",
    "America/Yakutat",
    "Antarctica/Casey",
    "Antarctica/Davis",
    "Antarctica/DumontDUrville",
    "Antarctica/Macquarie",
    "Antarctica/Mawson",
    "Antarctica/McMurdo",
    "Antarctica/Palmer",
    "Antarctica/Rothera",
    "Antarctica/Syowa",
    "Antarctica/Troll",
    "Antarctica/Vostok",
    "Arctic/Longyearbyen",
    "Asia/Aden",
    "Asia/Almaty",
    "Asia/Amman",
    "Asia/Anadyr",
    "Asia/Aqtau",
    "Asia/Aqtobe",
    "Asia/Ashgabat",
    "Asia/Atyrau",
    "Asia/Baghdad",
    "Asia/Bahrain",
    "Asia/Baku",
    "Asia/Bangkok",
    "Asia/Barnaul",
    "Asia/Beirut",
    "Asia/Bishkek",
    "Asia/Brunei",
    "Asia/Chita",
    "Asia/Choibalsan",
    "Asia/Colombo",
    "Asia/Damascus",
    "Asia/Dhaka",
    "Asia/Dili",
    "Asia/Dubai",
    "Asia/Dushanbe",
    "Asia/Famagusta",
    "Asia/Gaza",
    "Asia/Hebron",
    "Asia/Ho_Chi_Minh",
    "Asia/Hong_Kong",
    "Asia/Hovd",
    "Asia/Irkutsk",
    "Asia/Jakarta",
    "Asia/Jayapura",
    "Asia/Jerusalem",
    "Asia/Kabul",
    "Asia/Kamchatka",
    "Asia/Karachi",
    "Asia/Kathmandu",
    "Asia/Khandyga",
    "Asia/Kolkata",
    "Asia/Krasnoyarsk",
    "Asia/Kuala_Lumpur",
    "Asia/Kuching",
    "Asia/Kuwait",
    "Asia/Macau",
    "Asia/Magadan",
    "Asia/Makassar",
    "Asia/Manila",
    "Asia/Muscat",
    "Asia/Nicosia",
    "Asia/Novokuznetsk",
    "Asia/Novosibirsk",
    "Asia/Omsk",
    "Asia/Oral",
    "Asia/Phnom_Penh",
    "Asia/Pontianak",
    "Asia/Pyongyang",
    "Asia/Qatar",
    "Asia/Qostanay",
    "Asia/Qyzylorda",
    "Asia/Riyadh",
    "Asia/Sakhalin",
    "Asia/Samarkand",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Singapore",
    "Asia/Srednekolymsk",
    "Asia/Taipei",
    "Asia/Tashkent",
    "Asia/Tbilisi",
    "Asia/Tehran",
    "Asia/Thimphu",
    "Asia/Tokyo",
    "Asia/Tomsk",
    "Asia/Ulaanbaatar",
    "Asia/Urumqi",
    "Asia/Ust-Nera",
    "Asia/Vientiane",
    "Asia/Vladivostok",
    "Asia/Yakutsk",
    "Asia/Yangon",
    "Asia/Yekaterinburg",
    "Asia/Yerevan",
    "Atlantic/Azores",
    "Atlantic/Bermuda",
    "Atlantic/Canary",
    "Atlantic/Cape_Verde",
    "Atlantic/Faroe",
    "Atlantic/Madeira",
    "Atlantic/Reykjavik",
    "Atlantic/South_Georgia",
    "Atlantic/St_Helena",
    "Atlantic/Stanley",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Broken_Hill",
    "Australia/Darwin",
    "Australia/Eucla",
    "Australia/Hobart",
    "Australia/Lindeman",
    "Australia/Lord_Howe",
    "Australia/Melbourne",
    "Australia/Perth",
    "Australia/Sydney",
    "Canada/Atlantic",
    "Canada/Central",
    "Canada/Eastern",
    "Canada/Mountain",
    "Canada/Newfoundland",
    "Canada/Pacific",
    "Europe/Amsterdam",
    "Europe/Andorra",
    "Europe/Astrakhan",
    "Europe/Athens",
    "Europe/Belgrade",
    "Europe/Berlin",
    "Europe/Bratislava",
    "Europe/Brussels",
    "Europe/Bucharest",
    "Europe/Budapest",
    "Europe/Busingen",
    "Europe/Chisinau",
    "Europe/Copenhagen",
    "Europe/Dublin",
    "Europe/Gibraltar",
    "Europe/Guernsey",
    "Europe/Helsinki",
    "Europe/Isle_of_Man",
    "Europe/Istanbul",
    "Europe/Jersey",
    "Europe/Kaliningrad",
    "Europe/Kirov",
    "Europe/Kyiv",
    "Europe/Lisbon",
    "Europe/Ljubljana",
    "Europe/London",
    "Europe/Luxembourg",
    "Europe/Madrid",
    "Europe/Malta",
    "Europe/Mariehamn",
    "Europe/Minsk",
    "Europe/Monaco",
    "Europe/Moscow",
    "Europe/Oslo",
    "Europe/Paris",
    "Europe/Podgorica",
    "Europe/Prague",
    "Europe/Riga",
    "Europe/Rome",
    "Europe/Samara",
    "Europe/San_Marino",
    "Europe/Sarajevo",
    "Europe/Saratov",
    "Europe/Simferopol",
    "Europe/Skopje",
    "Europe/Sofia",
    "Europe/Stockholm",
    "Europe/Tallinn",
    "Europe/Tirane",
    "Europe/Ulyanovsk",
    "Europe/Vaduz",
    "Europe/Vatican",
    "Europe/Vienna",
    "Europe/Vilnius",
    "Europe/Volgograd",
    "Europe/Warsaw",
    "Europe/Zagreb",
    "Europe/Zurich",
    "GMT",
    "Indian/Antananarivo",
    "Indian/Chagos",
    "Indian/Christmas",
    "Indian/Cocos",
    "Indian/Comoro",
    "Indian/Kerguelen",
    "Indian/Mahe",
    "Indian/Maldives",
    "Indian/Mauritius",
    "Indian/Mayotte",
    "Indian/Reunion",
    "Pacific/Apia",
    "Pacific/Auckland",
    "Pacific/Bougainville",
    "Pacific/Chatham",
    "Pacific/Chuuk",
    "Pacific/Easter",
    "Pacific/Efate",
    "Pacific/Fakaofo",
    "Pacific/Fiji",
    "Pacific/Funafuti",
    "Pacific/Galapagos",
    "Pacific/Gambier",
    "Pacific/Guadalcanal",
    "Pacific/Guam",
    "Pacific/Honolulu",
    "Pacific/Kanton",
    "Pacific/Kiritimati",
    "Pacific/Kosrae",
    "Pacific/Kwajalein",
    "Pacific/Majuro",
    "Pacific/Marquesas",
    "Pacific/Midway",
    "Pacific/Nauru",
    "Pacific/Niue",
    "Pacific/Norfolk",
    "Pacific/Noumea",
    "Pacific/Pago_Pago",
    "Pacific/Palau",
    "Pacific/Pitcairn",
    "Pacific/Pohnpei",
    "Pacific/Port_Moresby",
    "Pacific/Rarotonga",
    "Pacific/Saipan",
    "Pacific/Tahiti",
    "Pacific/Tarawa",
    "Pacific/Tongatapu",
    "Pacific/Wake",
    "Pacific/Wallis",
    "US/Alaska",
    "US/Arizona",
    "US/Central",
    "US/Eastern",
    "US/Hawaii",
    "US/Mountain",
    "US/Pacific",
    "UTC",
];

/// `FORBIDDEN_IDENTIFIER_CHARS_PATTERN` (`db/models/project.py:145`):
/// `re.match(r"^.*[&+,:;$^}{*=?@#|'<>.()%!-].*$", value)`.
const FORBIDDEN_CHARS: &str = "&+,:;$^}{*}{=?@#|'<>.()%!-";

fn has_forbidden_chars(value: &str) -> bool {
    value
        .chars()
        .any(|character| FORBIDDEN_CHARS.contains(character))
}

/// `ProjectCreateSerializer.validate` (`api/serializers/project.py:107-141`).
async fn validate_project_write(
    client: &Client,
    workspace_id: Uuid,
    write: &ProjectWrite,
) -> Result<Option<ViewResponse>, ViewError> {
    if let Some(name) = write.name.as_deref()
        && has_forbidden_chars(name)
    {
        return Ok(Some(crate::cycles::non_field_errors(
            "Project name cannot contain special characters.",
        )));
    }
    if let Some(identifier) = write.identifier.as_deref()
        && has_forbidden_chars(identifier)
    {
        return Ok(Some(crate::cycles::non_field_errors(
            "Project identifier cannot contain special characters.",
        )));
    }
    if let Some(lead) = write.project_lead
        && !workspace_member_active(client, workspace_id, lead).await?
    {
        return Ok(Some(plane_http_app::write::field_string_error(
            "project_lead",
            "The provided user is not a member of this workspace.",
        )));
    }
    if let Some(assignee) = write.default_assignee
        && !workspace_member_active(client, workspace_id, assignee).await?
    {
        return Ok(Some(crate::cycles::non_field_errors(
            "Default assignee should be a user in the workspace",
        )));
    }
    Ok(None)
}

/// `WorkspaceMember.objects.filter(workspace_id=…, member=…, is_active=True)
/// .exists()` (`serializers/project.py:118-130`), soft-delete aware.
async fn workspace_member_active(
    client: &Client,
    workspace_id: Uuid,
    member: Uuid,
) -> Result<bool, ViewError> {
    Ok(client
        .query_opt(
            "SELECT 1 FROM workspace_members \
             WHERE workspace_members.deleted_at IS NULL \
               AND workspace_members.workspace_id = $1 \
               AND workspace_members.member_id = $2 \
               AND workspace_members.is_active LIMIT 1",
            &[&workspace_id, &member],
        )
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// The identifiers `ProjectCreateSerializer.create` accepts: `strip().upper()`
/// then `ProjectIdentifier` uniqueness (`serializers/project.py:143-163`).
async fn taken_identifier(
    client: &Client,
    identifier: &str,
    workspace_id: Uuid,
) -> Result<bool, ViewError> {
    Ok(client
        .query_opt(
            "SELECT 1 FROM project_identifiers \
             WHERE project_identifiers.deleted_at IS NULL \
               AND project_identifiers.name = $1 \
               AND project_identifiers.workspace_id = $2 LIMIT 1",
            &[&identifier, &workspace_id],
        )
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// `409 {"identifier": "The project identifier is already taken"}`
/// (`api/views/project.py:329-333`, reached by every `ValidationError` the
/// serializer raises from `save()`).
fn identifier_taken() -> ViewResponse {
    ViewResponse::raw(
        409,
        APPLICATION_JSON,
        plane_query::json::Json::object(vec![(
            "identifier",
            plane_query::json::Json::string("The project identifier is already taken"),
        )])
        .to_drf(),
    )
}

/// `409 {"name": "The project name is already taken"}` (`:310-315`,
/// `:602-607`) — the postgres DETAIL says "already exists." for both the name
/// and the identifier index, so both map here.
fn name_taken() -> ViewResponse {
    ViewResponse::raw(
        409,
        APPLICATION_JSON,
        plane_query::json::Json::object(vec![(
            "name",
            plane_query::json::Json::string("The project name is already taken"),
        )])
        .to_drf(),
    )
}

/// A database failure with its message (and DETAIL), so the create/update can
/// tell "already exists" apart.
fn db_message(error: tokio_postgres::Error) -> ViewError {
    ViewError::Unknown(match error.as_db_error() {
        Some(db) => format!("{} {}", db.message(), db.detail().unwrap_or_default()),
        None => error.to_string(),
    })
}

/// The `except Exception` branch (`:334-343`).
fn unexpected_error() -> ViewResponse {
    crate::cycles::error_body(500, "An unexpected error occurred")
}

/// `ProjectListCreateAPIEndpoint.post` (`api/views/project.py:228-343`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`:235`).
        let workspace = match plane_http_app::common::lookup::workspace_by_slug(client, &slug).await
        {
            Ok(workspace) => workspace,
            Err(plane_http_app::common::lookup::Error::NotFound) => {
                return Ok(crate::cycles::error_body(404, "Workspace does not exist"));
            }
            Err(error) => return Err(ViewError::Unknown(error.to_string())),
        };

        // `data={**request.data}` (`:237`): a non-mapping body is a `TypeError`
        // inside the view, which the family handler renders as a 500.
        ensure_project_mapping(ctx.data()?)?;
        let mut serializer = plane_http_app::write::Serializer::new_expanded(ctx, false)?;
        let write = read_project_body(&mut serializer, client, None, false).await?;
        if let Err(errors) = serializer.finish() {
            return Ok(errors.into_response());
        };
        if let Some(response) = validate_project_write(client, workspace.id, &write).await? {
            return Ok(response);
        }

        // `create()` (`:143-163`).
        let identifier = write
            .identifier
            .clone()
            .unwrap_or_default()
            .trim()
            .to_uppercase();
        if identifier.is_empty() {
            return Ok(identifier_taken());
        }
        if taken_identifier(client, &identifier, workspace.id).await? {
            return Ok(identifier_taken());
        }
        // The random icon (`:152-160`); `logo_props` is read-only, so every v1
        // create regenerates it.
        let logo_props = random_logo_props();

        let name = write
            .name
            .clone()
            .ok_or_else(|| ViewError::Unknown("validated name vanished".to_owned()))?;
        let prepared = plane_db::hooks::project::before_insert(
            &identifier,
            write.timezone.as_deref(),
            &workspace.timezone,
        );
        let now = plane_http_app::write::now();
        let project = Project {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            name,
            description: write.description.clone().unwrap_or_default(),
            description_text: None,
            description_html: None,
            network: 2,
            workspace_id: workspace.id,
            identifier: prepared.identifier,
            default_assignee_id: write.default_assignee,
            project_lead_id: write.project_lead,
            emoji: write.emoji.clone(),
            icon_prop: write.icon_prop.clone(),
            module_view: write.module_view.unwrap_or(false),
            cycle_view: write.cycle_view.unwrap_or(false),
            issue_views_view: write.issue_views_view.unwrap_or(false),
            page_view: write.page_view.unwrap_or(true),
            intake_view: write.intake_view.unwrap_or(false),
            is_time_tracking_enabled: write.is_time_tracking_enabled.unwrap_or(false),
            is_issue_type_enabled: write.is_issue_type_enabled.unwrap_or(false),
            guest_view_all_features: write.guest_view_all_features.unwrap_or(false),
            cover_image: write.cover_image.clone(),
            cover_image_asset_id: None,
            estimate_id: None,
            archive_in: write.archive_in.unwrap_or(0),
            close_in: write.close_in.unwrap_or(0),
            logo_props,
            default_state_id: None,
            archived_at: None,
            timezone: prepared.timezone,
            external_source: write.external_source.clone(),
            external_id: write.external_id.clone(),
        };

        // `with transaction.atomic():` (`:240-305`).
        client
            .batch_execute("BEGIN")
            .await
            .map_err(ViewError::from)?;
        let outcome = async {
            client
                .execute(
                    plane_http_app::projects::queries::PROJECT_INSERT.sql,
                    &project_insert_args(&project),
                )
                .await
                .map_err(db_message)?;
            // `ProjectMember.objects.create(project, request.user, role=20)`
            // and the same for a different lead (`:244-259`).
            insert_project_member(client, &project, user, 20, user, now).await?;
            if let Some(lead) = project.project_lead_id
                && lead != user
            {
                insert_project_member(client, &project, lead, 20, user, now).await?;
            }
            // `State.objects.bulk_create(DEFAULT_STATES)` (`:261-275`).
            insert_default_states(client, &project, user, now).await
        }
        .await;
        match outcome {
            Ok(()) => client
                .batch_execute("COMMIT")
                .await
                .map_err(ViewError::from)?,
            Err(error) => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Ok(match error {
                    // `except IntegrityError` (`:310-326`): postgres's DETAIL
                    // for both the name and the identifier partial unique index
                    // ends in "already exists.", so both answer the name body.
                    ViewError::Unknown(message) if message.contains("already exists") => {
                        name_taken()
                    }
                    ViewError::Unknown(_) => unexpected_error(),
                    other => return Err(other),
                });
            }
        }

        // `self.get_queryset().filter(pk=…).first()` (`:277`) then
        // `ProjectSerializer(project)` (`:307`).
        let row = client
            .query_opt(
                queries::PROJECT_DETAIL_ONE.sql,
                &[&slug, &user, &project.id],
            )
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'id'".to_owned(),
            ));
        };
        let mut rows = vec![project_row(&row)?];
        rows[0].sort_order = FieldValue::Absent;
        resolve_cover_urls(client, &mut rows).await?;

        // `transaction.on_commit(_dispatch_model_activity, robust=True)`
        // (`:294-305`): the callback runs after the commit, and `robust=True`
        // logs a broker failure instead of surfacing it, so the response stays
        // 201 (`django/db/transaction.py:54-77`).
        if publish_project_activity(ctx, &slug, project.id, None, user)
            .await
            .is_err()
        {
            tracing::warn!(
                project_id = %project.id,
                "model_activity dispatch failed after the project create committed"
            );
        }

        let prepared = RequestRender::from_ctx(ctx);
        let body = render_project_plain(&rows[0], &prepared)?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `{**request.data}`: a JSON body must be an object; a form body is a
/// `QueryDict`, which is a mapping (`api/views/project.py:237`).
fn ensure_project_mapping(data: &plane_drf::parsers::Data) -> Result<(), ViewError> {
    match data {
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Dict(_))
        | plane_drf::parsers::Data::Form(_) => Ok(()),
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Null) => Err(
            ViewError::Unknown("TypeError: 'NoneType' object is not a mapping".to_owned()),
        ),
        plane_drf::parsers::Data::Json(other) => Err(ViewError::Unknown(format!(
            "TypeError: '{}' object is not a mapping",
            python_type_name(other)
        ))),
    }
}

fn python_type_name(value: &plane_serde::pyjson::Value) -> &'static str {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Null => "NoneType",
        Py::Bool(_) => "bool",
        Py::Int(_) => "int",
        Py::Float(_) => "float",
        Py::Str(_) => "str",
        Py::List(_) => "list",
        Py::Dict(_) => "dict",
    }
}

/// `random.choice(self.PROJECT_ICON_DEFAULT_ICONS/COLORS)`
/// (`serializers/project.py:152-160`).
fn random_logo_props() -> plane_db::JsonText {
    const ICONS: [&str; 28] = [
        "home",
        "apps",
        "settings",
        "star",
        "favorite",
        "done",
        "check_circle",
        "add_task",
        "create_new_folder",
        "dataset",
        "terminal",
        "key",
        "rocket",
        "public",
        "quiz",
        "mood",
        "gavel",
        "eco",
        "diamond",
        "forest",
        "bolt",
        "sync",
        "cached",
        "library_add",
        "view_timeline",
        "view_kanban",
        "empty_dashboard",
        "cycle",
    ];
    const COLORS: [&str; 8] = [
        "#95999f", "#6d7b8a", "#5e6ad2", "#02b5ed", "#02b55c", "#f2be02", "#e57a00", "#f38e82",
    ];
    // `random.choice` over a 28-entry and an 8-entry sequence: the shared
    // `rand` stream is not Python's Mersenne Twister, and the harness compares
    // this column by shape only (`scenarios/normalize.yaml`, scoped to
    // `POST /api/v1/workspaces/*/projects/`).
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or_default();
    let icon = ICONS[nanos % ICONS.len()];
    let color = COLORS[(nanos / ICONS.len()) % COLORS.len()];
    let mut value = plane_serde::pyjson::PyDict::with_capacity(2);
    value.insert(
        plane_serde::pyjson::PyStr::new("in_use"),
        plane_serde::pyjson::Value::from("icon"),
    );
    let mut icon_obj = plane_serde::pyjson::PyDict::with_capacity(2);
    icon_obj.insert(
        plane_serde::pyjson::PyStr::new("name"),
        plane_serde::pyjson::Value::from(icon),
    );
    icon_obj.insert(
        plane_serde::pyjson::PyStr::new("color"),
        plane_serde::pyjson::Value::from(color),
    );
    value.insert(
        plane_serde::pyjson::PyStr::new("icon"),
        plane_serde::pyjson::Value::Dict(icon_obj),
    );
    plane_db::JsonText::new(
        std::str::from_utf8(
            &plane_serde::to_vec(
                plane_serde::Dialect::Drf,
                &plane_serde::pyjson::Value::Dict(value),
            )
            .unwrap_or_default(),
        )
        .unwrap_or("{}"),
    )
}

fn project_insert_args(project: &Project) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
    vec![
        &project.created_at,
        &project.updated_at,
        &project.created_by_id,
        &project.updated_by_id,
        &project.deleted_at,
        &project.id,
        &project.name,
        &project.description,
        &project.description_text,
        &project.description_html,
        &project.network,
        &project.workspace_id,
        &project.identifier,
        &project.default_assignee_id,
        &project.project_lead_id,
        &project.emoji,
        &project.icon_prop,
        &project.module_view,
        &project.cycle_view,
        &project.issue_views_view,
        &project.page_view,
        &project.intake_view,
        &project.is_time_tracking_enabled,
        &project.is_issue_type_enabled,
        &project.guest_view_all_features,
        &project.cover_image,
        &project.cover_image_asset_id,
        &project.estimate_id,
        &project.archive_in,
        &project.close_in,
        &project.logo_props,
        &project.default_state_id,
        &project.archived_at,
        &project.timezone,
        &project.external_source,
        &project.external_id,
    ]
}

/// `ProjectMember.objects.create(...)` plus H32's `ProjectUserProperty`
/// (`plane/db/models/project.py:276-293`).
async fn insert_project_member(
    client: &Client,
    project: &Project,
    member: Uuid,
    role: i16,
    user: Uuid,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    use plane_http_app::projects::queries;
    // The model defaults (`get_default_props`, `get_default_preferences`) and
    // the same texts the app surface binds.
    const MEMBER_PROPS: &str = r#"{"filters": {"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}, "display_filters": {"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}}"#;
    const MEMBER_PREFERENCES: &str = r#"{"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}}"#;
    const PUP_FILTERS: &str = r#"{"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}"#;
    const PUP_DISPLAY_FILTERS: &str = r#"{"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}"#;
    const PUP_DISPLAY_PROPERTIES: &str = r#"{"assignee": true, "attachment_count": true, "created_on": true, "due_date": true, "estimate": true, "key": true, "labels": true, "link": true, "priority": true, "start_date": true, "state": true, "sub_issue_count": true, "updated_on": true}"#;

    let property = plane_db::hooks::project_member::before_insert(
        client,
        project.workspace_id,
        project.id,
        member,
    )
    .await
    .map_err(ViewError::from)?;
    client
        .execute(
            queries::PROJECT_USER_PROPERTY_MEMBER_INSERT.sql,
            &[
                &now,
                &now,
                &Some(user),
                &Uuid::new_v4(),
                &property.project_id,
                &property.workspace_id,
                &property.user_id,
                &plane_db::JsonText::new(PUP_FILTERS),
                &plane_db::JsonText::new(PUP_DISPLAY_FILTERS),
                &plane_db::JsonText::new(PUP_DISPLAY_PROPERTIES),
                &plane_db::JsonText::new(MEMBER_PREFERENCES),
                &property.sort_order,
            ],
        )
        .await
        .map_err(db_message)?;
    client
        .execute(
            queries::PROJECT_MEMBER_INSERT.sql,
            &[
                &now,
                &now,
                &Some(user),
                &None::<Uuid>,
                &Uuid::new_v4(),
                &project.id,
                &project.workspace_id,
                &member,
                &role,
                &plane_db::JsonText::new(MEMBER_PROPS),
                &plane_db::JsonText::new(MEMBER_PROPS),
                &plane_db::JsonText::new(MEMBER_PREFERENCES),
                &65535.0_f64,
            ],
        )
        .await
        .map_err(db_message)?;
    Ok(())
}

/// `State.objects.bulk_create(DEFAULT_STATES)` (`api/views/project.py:261-275`).
async fn insert_default_states(
    client: &Client,
    project: &Project,
    user: Uuid,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    let mut sql = String::from(
        "INSERT INTO states (created_at, updated_at, created_by_id, updated_by_id, \
         deleted_at, id, project_id, workspace_id, name, description, color, slug, \
         sequence, \"group\", is_triage, \"default\") VALUES ",
    );
    let ids: Vec<Uuid> = (0..queries::DEFAULT_STATES.len())
        .map(|_| Uuid::new_v4())
        .collect();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
        Vec::with_capacity(queries::DEFAULT_STATES.len() * 11);
    for (index, state) in queries::DEFAULT_STATES.iter().enumerate() {
        if index != 0 {
            sql.push_str(", ");
        }
        let base = index * 11;
        sql.push_str(&format!(
            "(${}, ${}, ${}, NULL, NULL, ${}, ${}, ${}, ${}, '', ${}, '', ${}, ${}, FALSE, ${})",
            base + 1,
            base + 2,
            base + 3,
            base + 4,
            base + 5,
            base + 6,
            base + 7,
            base + 8,
            base + 9,
            base + 10,
            base + 11
        ));
        params.push(&now);
        params.push(&now);
        params.push(&user);
        params.push(&ids[index]);
        params.push(&project.id);
        params.push(&project.workspace_id);
        params.push(&state.name);
        params.push(&state.color);
        params.push(&state.sequence);
        params.push(&state.group);
        params.push(&state.default);
    }
    let statement = client.prepare(&sql).await.map_err(ViewError::from)?;
    client
        .execute(&statement, &params)
        .await
        .map_err(db_message)?;
    Ok(())
}

/// `model_activity.delay(model_name="project", …)`
/// (`api/views/project.py:294-305`, `:589-597`).
async fn publish_project_activity(
    ctx: &ViewCtx<'_>,
    slug: &str,
    project_id: Uuid,
    current_instance: Option<String>,
    user: Uuid,
) -> Result<(), ViewError> {
    let origin = plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let task = plane_jobs::celery::task_name::MODEL_ACTIVITY;
    let message =
        plane_jobs::celery::tasks::model_activity(plane_jobs::celery::tasks::ModelActivityCall {
            model_name: "project".to_owned(),
            model_id: project_id.to_string(),
            requested_data: plane_http_app::write::request_data_arg(ctx)?,
            current_instance,
            actor_id: user,
            slug: slug.to_owned(),
            origin,
        });
    plane_http_app::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::from_message(&message),
    )
    .await
    .map(|_| ())
}

/// `ProjectSerializer(project).data` with no `fields=`/`expand=`, rendered for
/// the create/patch responses and used as `current_instance`'s source.
fn render_project_plain(
    row: &ProjectRow,
    prepared: &RequestRender<'_>,
) -> Result<Vec<u8>, ViewError> {
    let serializer = row.serializer();
    plane_serializers::render(&serializer, &prepared.render_ctx()).map_err(ViewError::from)
}

/// `json.dumps(ProjectSerializer(project).data, cls=DjangoJSONEncoder)`
/// (`api/views/project.py:559`).
async fn project_current_instance(client: &Client, project: &Project) -> Result<String, ViewError> {
    let mut row = ProjectRow {
        project: project.clone(),
        is_member: FieldValue::Absent,
        total_members: FieldValue::Absent,
        total_cycles: FieldValue::Absent,
        total_modules: FieldValue::Absent,
        member_role: FieldValue::Absent,
        is_deployed: FieldValue::Absent,
        sort_order: FieldValue::Absent,
        cover_image_url: None,
    };
    let mut rows = vec![row];
    resolve_cover_urls(client, &mut rows).await?;
    row = rows.remove(0);
    let prepared = RenderCtx::utc();
    let body = plane_serializers::render(&row.serializer(), &prepared).map_err(ViewError::from)?;
    let value = plane_serde::pyjson::loads(&body, "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, &value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `ProjectDetailAPIEndpoint.patch` (`api/views/project.py:550-614`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(patch_inner(ctx))
}

async fn patch_inner<'a>(ctx: &'a ViewCtx<'a>) -> Result<ViewResponse, ViewError> {
    {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`:557`).
        let workspace = match plane_http_app::common::lookup::workspace_by_slug(client, &slug).await
        {
            Ok(workspace) => workspace,
            Err(plane_http_app::common::lookup::Error::NotFound) => {
                return Ok(crate::cycles::error_body(404, "Project does not exist"));
            }
            Err(error) => return Err(ViewError::Unknown(error.to_string())),
        };
        // `Project.objects.get(pk=pk)` (`:558`) — no workspace scope.
        let Some(row) = client
            .query_opt(queries::PROJECT_BY_PK_ANY.sql, &[&pk])
            .await
            .map_err(ViewError::from)?
        else {
            return Ok(crate::cycles::error_body(404, "Project does not exist"));
        };
        let project = match Project::from_row(&row, 0) {
            Ok(project) => project,
            Err(error) => return Err(ViewError::Unknown(error.to_string())),
        };
        // `current_instance = json.dumps(ProjectSerializer(project).data, …)`
        // (`:559`).
        let current_instance = project_current_instance(client, &project).await?;

        // `intake_view = request.data.get("intake_view", project.intake_view)`
        // (`:561`).
        let data = ctx.data()?;
        ensure_project_mapping(data)?;
        let default_intake = plane_serde::pyjson::Value::Bool(project.intake_view);
        let injected_intake = match data {
            plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Dict(object)) => {
                match object.get("intake_view") {
                    Some(value) => value.clone(),
                    None => default_intake,
                }
            }
            other => match other.get("intake_view") {
                Some(text) => plane_serde::pyjson::Value::from(text),
                None => default_intake,
            },
        };

        if project.archived_at.is_some() {
            return Ok(crate::cycles::error_body(
                400,
                "Archived project cannot be updated",
            ));
        }

        // `ProjectUpdateSerializer(project, data={**request.data,
        // "intake_view": intake_view}, …)` (`:569-574`).
        let mut serializer = plane_http_app::write::Serializer::new_expanded(ctx, true)?;
        let write =
            read_project_body(&mut serializer, client, Some(injected_intake.clone()), true).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };
        if let Some(response) = validate_project_write(client, workspace.id, &write).await? {
            return Ok(response);
        }
        // `ProjectUpdateSerializer.update` (`:183-207`).
        if let Some(name) = write.name.as_deref()
            && has_forbidden_chars(name)
        {
            return Ok(crate::cycles::non_field_errors(
                "Project name cannot contain special characters.",
            ));
        }
        if let Some(identifier) = write.identifier.as_deref()
            && has_forbidden_chars(identifier)
        {
            return Ok(crate::cycles::non_field_errors(
                "Project identifier cannot contain special characters.",
            ));
        }
        // `ProjectUpdateSerializer.update` (`:194-199`):
        // `State.objects.filter(project=instance, id=<State instance>)` passes a
        // model instance to a UUID lookup, which is a **core**
        // `ValidationError` — the family handler's 400 "Please provide valid
        // detail" (`plane/api/views/base.py:85-89`), not the view's 409. Ported
        // as-is.
        if write.default_state.is_some() {
            return Ok(crate::cycles::error_body(
                400,
                "Please provide valid detail",
            ));
        }
        // The estimate branch uses `.id` (`:201-206`), so a foreign estimate is
        // the view's `except ValidationError` → 409.
        if let Some(estimate) = write.estimate
            && !estimate_in_project(client, &project.id, estimate).await?
        {
            return Ok(identifier_taken());
        }

        // Assign the validated fields, then `save()` (H09 + H01).
        let mut updated = project.clone();
        if let Some(name) = write.name.clone() {
            updated.name = name;
        }
        if let Some(description) = write.description.clone() {
            updated.description = description;
        }
        if values.contains("project_lead") {
            updated.project_lead_id = write.project_lead;
        }
        if values.contains("default_assignee") {
            updated.default_assignee_id = write.default_assignee;
        }
        if let Some(identifier) = write.identifier.clone() {
            updated.identifier = identifier;
        }
        if values.contains("icon_prop") {
            updated.icon_prop = write.icon_prop.clone();
        }
        if values.contains("emoji") {
            updated.emoji = write.emoji.clone();
        }
        if values.contains("cover_image") {
            updated.cover_image = write.cover_image.clone();
        }
        if let Some(value) = write.module_view {
            updated.module_view = value;
        }
        if let Some(value) = write.cycle_view {
            updated.cycle_view = value;
        }
        if let Some(value) = write.issue_views_view {
            updated.issue_views_view = value;
        }
        if let Some(value) = write.page_view {
            updated.page_view = value;
        }
        // The injected value is always in `validated_data`.
        if let Some(value) = values.bool("intake_view") {
            updated.intake_view = value;
        }
        if let Some(value) = write.guest_view_all_features {
            updated.guest_view_all_features = value;
        }
        if let Some(value) = write.archive_in {
            updated.archive_in = value;
        }
        if let Some(value) = write.close_in {
            updated.close_in = value;
        }
        if let Some(value) = write.timezone.clone() {
            updated.timezone = value;
        }
        if values.contains("external_source") {
            updated.external_source = write.external_source.clone();
        }
        if values.contains("external_id") {
            updated.external_id = write.external_id.clone();
        }
        if let Some(value) = write.is_issue_type_enabled {
            updated.is_issue_type_enabled = value;
        }
        if let Some(value) = write.is_time_tracking_enabled {
            updated.is_time_tracking_enabled = value;
        }
        if values.contains("default_state") {
            updated.default_state_id = write.default_state;
        }
        if values.contains("estimate") {
            updated.estimate_id = write.estimate;
        }
        let timezone_changed = write.timezone.is_some() && updated.timezone != project.timezone;
        let now = plane_http_app::write::now();
        updated.identifier = plane_db::hooks::project::before_update(&updated.identifier);
        updated.updated_at = now;
        updated.updated_by_id = Some(user);

        // H09a runs in the same transaction when the persisted zone changed.
        if timezone_changed {
            let old_zone =
                plane_db::hooks::project::parse_timezone(&project.timezone).ok_or_else(|| {
                    ViewError::Unknown(format!("UnknownTimeZoneError: {}", project.timezone))
                })?;
            let new_zone =
                plane_db::hooks::project::parse_timezone(&updated.timezone).ok_or_else(|| {
                    ViewError::Unknown(format!("UnknownTimeZoneError: {}", updated.timezone))
                })?;
            client
                .batch_execute("BEGIN")
                .await
                .map_err(ViewError::from)?;
            let outcome = async {
                if let Some(response) = update_project_row(client, &updated).await? {
                    return Ok(Err(response));
                }
                plane_db::hooks::project::retime_intake_snoozes_with(
                    client, project.id, old_zone, new_zone,
                )
                .await
                .map_err(ViewError::from)?;
                Ok(Ok(()))
            }
            .await;
            match outcome {
                Ok(Ok(())) => client
                    .batch_execute("COMMIT")
                    .await
                    .map_err(ViewError::from)?,
                Ok(Err(response)) => {
                    let _ = client.batch_execute("ROLLBACK").await;
                    return Ok(response);
                }
                Err(error) => {
                    let _ = client.batch_execute("ROLLBACK").await;
                    return Err(error);
                }
            }
        } else if let Some(response) = update_project_row(client, &updated).await? {
            return Ok(response);
        }

        // `if serializer.data["intake_view"]:` (`:578-585`).
        if python_truthy(&injected_intake) {
            let exists = client
                .query_opt(
                    plane_http_app::projects::queries::DEFAULT_INTAKE_EXISTS.sql,
                    &[&project.id],
                )
                .await
                .map_err(ViewError::from)?
                .is_some();
            if !exists {
                client
                    .execute(
                        plane_http_app::projects::queries::INTAKE_INSERT.sql,
                        &[
                            &now,
                            &now,
                            &Some(user),
                            &None::<Uuid>,
                            &Uuid::new_v4(),
                            &project.id,
                            &workspace.id,
                            &format!("{} Intake", updated.name),
                        ],
                    )
                    .await
                    .map_err(crate::cycles::integrity_or_unknown)?;
            }
        }

        // `project = self.get_queryset().filter(pk=…).first()` (`:587`);
        // a project outside the URL workspace leaves `None` and `project.id`
        // is an `AttributeError`.
        let Some(row) = client
            .query_opt(queries::PROJECT_DETAIL_ONE.sql, &[&slug, &user, &pk])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'id'".to_owned(),
            ));
        };
        let mut rows = vec![project_row(&row)?];
        rows[0].sort_order = FieldValue::Absent;
        resolve_cover_urls(client, &mut rows).await?;

        publish_project_activity(ctx, &slug, pk, Some(current_instance), user).await?;
        let prepared = RequestRender::from_ctx(ctx);
        let body = render_project_plain(&rows[0], &prepared)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    }
}

fn python_truthy(value: &plane_serde::pyjson::Value) -> bool {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Null => false,
        Py::Bool(flag) => *flag,
        Py::Int(number) => number.as_i64() != Some(0),
        Py::Float(number) => *number != 0.0,
        Py::Str(text) => text.as_str().is_some_and(|text| !text.is_empty()),
        Py::List(items) => !items.is_empty(),
        Py::Dict(map) => map.iter().next().is_some(),
    }
}

/// `project.save()`. `ProjectDetail.patch` catches only an `IntegrityError`
/// whose message says "already exists": anything else returns `None` from the
/// view, which Django turns into its HTML 500 page. The family's JSON 500 is
/// the closest faithful answer; `scenarios/write/v1-admin.yaml` never sends a
/// body that reaches it.
async fn update_project_row(
    client: &Client,
    project: &Project,
) -> Result<Option<ViewResponse>, ViewError> {
    match client
        .execute(
            plane_http_app::projects::queries::PROJECT_UPDATE.sql,
            &project_update_args(project),
        )
        .await
    {
        Ok(_) => Ok(None),
        Err(error) => match error.as_db_error() {
            Some(db) if db.code().code().starts_with("23") => {
                let message = format!("{} {}", db.message(), db.detail().unwrap_or_default());
                Ok(Some(if message.contains("already exists") {
                    name_taken()
                } else {
                    unexpected_error()
                }))
            }
            _ => Err(ViewError::from(error)),
        },
    }
}

/// `PROJECT_UPDATE` (`projects_update`) numbers the columns `name` = $6 …
/// `external_id` = $35 and the row id = $36, so the id goes last.
fn project_update_args(project: &Project) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
    vec![
        &project.created_at,
        &project.updated_at,
        &project.created_by_id,
        &project.updated_by_id,
        &project.deleted_at,
        &project.name,
        &project.description,
        &project.description_text,
        &project.description_html,
        &project.network,
        &project.workspace_id,
        &project.identifier,
        &project.default_assignee_id,
        &project.project_lead_id,
        &project.emoji,
        &project.icon_prop,
        &project.module_view,
        &project.cycle_view,
        &project.issue_views_view,
        &project.page_view,
        &project.intake_view,
        &project.is_time_tracking_enabled,
        &project.is_issue_type_enabled,
        &project.guest_view_all_features,
        &project.cover_image,
        &project.cover_image_asset_id,
        &project.estimate_id,
        &project.archive_in,
        &project.close_in,
        &project.logo_props,
        &project.default_state_id,
        &project.archived_at,
        &project.timezone,
        &project.external_source,
        &project.external_id,
        &project.id,
    ]
}

async fn estimate_in_project(
    client: &Client,
    project_id: &Uuid,
    estimate: Uuid,
) -> Result<bool, ViewError> {
    Ok(client
        .query_opt(
            "SELECT 1 FROM estimates WHERE estimates.deleted_at IS NULL \
               AND estimates.project_id = $1 AND estimates.id = $2 LIMIT 1",
            &[project_id, &estimate],
        )
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// `ProjectDetailAPIEndpoint.delete` (`api/views/project.py:627-650`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        // `Project.objects.get(pk=pk, workspace__slug=slug)` (`:633`).
        let project = project_in_workspace(client, &slug, pk).await?;

        // `UserFavorite.objects.filter(...).delete()` (`:635`) — H04.
        let now = plane_http_app::write::now();
        client
            .execute(
                queries::PROJECT_FAVORITES_BY_PROJECT.sql,
                &[&now, &project.id],
            )
            .await
            .map_err(ViewError::from)?;

        // `project.delete()` (`:636`) — H03 + H09 + H01, then the cascade task.
        let identifier = plane_db::hooks::project::before_update(&project.identifier);
        client
            .execute(
                plane_http_app::projects::queries::PROJECT_SOFT_DELETE.sql,
                &[&now, &Some(user), &identifier, &project.id],
            )
            .await
            .map_err(ViewError::from)?;
        plane_http_app::write::publish_soft_delete_cascade(ctx, "db", "project", project.id)
            .await?;

        // `webhook_activity.delay(...)` (`:637-649`).
        let origin = plane_auth::base_host(
            ctx.settings,
            plane_auth::LoginFlags {
                is_app: true,
                ..Default::default()
            },
        )
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let task = plane_jobs::celery::task_name::WEBHOOK_ACTIVITY;
        let message = plane_jobs::celery::tasks::webhook_activity(
            plane_jobs::celery::tasks::WebhookActivityCall {
                event: "project".to_owned(),
                verb: "deleted".to_owned(),
                field: None,
                old_value: plane_jobs::celery::args::TaskArg::Null,
                new_value: plane_jobs::celery::args::TaskArg::Null,
                actor_id: user,
                slug,
                current_site: origin,
                event_id: plane_jobs::celery::args::TaskArg::uuid(project.id),
                old_identifier: None,
                new_identifier: None,
            },
        );
        plane_http_app::common::jobs::dispatch_task(
            ctx,
            task,
            &message,
            plane_jobs::queue::Job::from_message(&message),
        )
        .await?;
        Ok(ViewResponse::empty(204))
    })
}
