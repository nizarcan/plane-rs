// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.project.base.ProjectViewSet` — the three read actions.
//!
//! Python: `apps/api/plane/app/views/project/base.py:48-256`.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/projects/` | `list` | `:147-224` |
//! | `GET /api/workspaces/{slug}/projects/details/` | `list_detail` | `:102-144` |
//! | `GET /api/workspaces/{slug}/projects/{pk}/` | `retrieve` | `:226-256` |
//!
//! ## `list` — the `.values()` projection
//!
//! `list` does not use a serializer: it returns a `ValuesQuerySet`
//! (`project/base.py:176-198`), which `Response` hands to DRF's `JSONEncoder`;
//! `JSONEncoder.default` turns a `QuerySet` into a tuple and encodes each
//! `dict` with its keys in `.values()` order. Three consequences are behaviour:
//!
//! * the field set is the *projection*, not `ProjectListSerializer`;
//! * datetimes are encoded by `JSONEncoder` (`isoformat()`, `Z` for UTC) with
//!   **no** timezone conversion — `?fields=`/`?expand=` do nothing;
//! * `inbox_view` is the `F("intake_view")` annotation.
//!
//! ## `list_detail` — `ProjectListSerializer`
//!
//! `list_detail` builds its own queryset from `get_queryset()` (which adds the
//! four annotations and the members prefetch), filters by the caller's
//! workspace role, and either paginates (`per_page` **and** `cursor` are both
//! present — `project/base.py:131`) or returns every project ordered by
//! `("sort_order", "name")`. `?fields=` reaches the serializer; `?expand=` does
//! not (the view passes `fields=` only).
//!
//! ## `retrieve`
//!
//! `retrieve` looks the project up through `get_queryset()` with
//! `archived_at__isnull=True`, then checks the caller against the prefetched
//! `members_list`:
//!
//! * not a member and `network == 0` (secret) → `403 {"error": "You do not have
//!   permission"}`;
//! * not a member otherwise → `409 {"error": "You are not a member of this
//!   project"}`;
//! * a member → `recent_visited_task.delay(...)` and the project.
//!
//! A project that does not exist, is soft-deleted or is archived is
//! `404 {"error": "Project does not exist"}` — the view's own body, not DRF's.
//! The single-object response ignores `?fields=`/`?expand=` too: the view
//! constructs `ProjectListSerializer(project)` with no arguments.

use std::collections::{HashMap, HashSet};

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::project::Project;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_query::sql::{Sql, Value};
use plane_serializers::{
    DrfValue, DrfWriter, FieldEntry, PgJson, RenderCtx, drf_datetime, drf_datetime_opt,
};
use uuid::Uuid;

use crate::common::{client, jobs, lookup, render_list};
use crate::projects::lite;
use crate::projects::queries;
use crate::projects::serializer::ProjectList;

/// `ROLE.GUEST` (`plane/db/models/project.py:23-26`).
const ROLE_GUEST: i16 = 5;
/// `ROLE.MEMBER`.
const ROLE_MEMBER: i16 = 15;
/// `ProjectNetwork.SECRET`.
const NETWORK_SECRET: i16 = 0;

/// `PROJECT_ORDER_BY_ALLOWLIST` (`plane/utils/order_queryset.py:57-63`).
const PROJECT_ORDER_BY_ALLOWLIST: &[&str] =
    &["created_at", "updated_at", "name", "network", "sort_order"];

/// One `ProjectListSerializer` row: the model row plus its annotations and the
/// page-level side queries.
struct ProjectRow {
    project: Project,
    is_favorite: bool,
    member_role: Option<i16>,
    anchor: Option<String>,
    sort_order: Option<f64>,
    members: Vec<Uuid>,
    next_work_item_sequence: i64,
    cover_image_url: Option<String>,
}

/// The caller's workspace role, as `list`/`list_detail`'s two `if`s see it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Guest,
    Member,
    Other,
}

/// `ProjectViewSet.list` (`plane/app/views/project/base.py:147-224`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let member_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let sql = match role_of(ctx, slug, member_id).await? {
            Role::Guest => queries::PROJECT_LIST_GUEST.sql,
            Role::Member => queries::PROJECT_LIST_MEMBER.sql,
            Role::Other => queries::PROJECT_LIST_ADMIN.sql,
        };
        let statement = client.prepare_cached(sql).await?;
        let rows = client.query(&statement, &[&slug, &member_id]).await?;

        // The projection, in `.values(...)` order (`project/base.py:176-198`),
        // encoded by DRF's `JSONEncoder`.
        let mut writer = DrfWriter::with_capacity(rows.len() * 512);
        writer.write_byte(b'[');
        for (index, row) in rows.iter().enumerate() {
            if index != 0 {
                writer.write_byte(b',');
            }
            let logo_props: plane_db::JsonText = row.get(4);
            let mut object = writer.begin_object();
            object.field("id", &plane_serializers::DrfUuid(row.get(0)))?;
            object.field("name", &row.get::<_, String>(1))?;
            object.field("identifier", &row.get::<_, String>(2))?;
            object.field("sort_order", &row.get::<_, Option<f64>>(3))?;
            object.field_json("logo_props", Some(PgJson::new(logo_props.as_str())))?;
            object.field("member_role", &row.get::<_, Option<i16>>(5))?;
            object.field("intake_count", &row.get::<_, i64>(6))?;
            object.field(
                "archived_at",
                &drf_datetime_opt(row.get(7), &chrono_tz::UTC),
            )?;
            object.field("workspace", &plane_serializers::DrfUuid(row.get(8)))?;
            object.field("cycle_view", &row.get::<_, bool>(9))?;
            object.field("issue_views_view", &row.get::<_, bool>(10))?;
            object.field("module_view", &row.get::<_, bool>(11))?;
            object.field("page_view", &row.get::<_, bool>(12))?;
            object.field("inbox_view", &row.get::<_, bool>(13))?;
            object.field("guest_view_all_features", &row.get::<_, bool>(14))?;
            object.field(
                "project_lead",
                &row.get::<_, Option<Uuid>>(15)
                    .map(plane_serializers::DrfUuid),
            )?;
            object.field("network", &row.get::<_, i16>(16))?;
            object.field("created_at", &drf_datetime(row.get(17), &chrono_tz::UTC))?;
            object.field("updated_at", &drf_datetime(row.get(18), &chrono_tz::UTC))?;
            object.field(
                "created_by",
                &row.get::<_, Option<Uuid>>(19)
                    .map(plane_serializers::DrfUuid),
            )?;
            object.field(
                "updated_by",
                &row.get::<_, Option<Uuid>>(20)
                    .map(plane_serializers::DrfUuid),
            )?;
            object.end();
        }
        writer.write_byte(b']');
        Ok(ViewResponse::raw(200, APPLICATION_JSON, writer.into_vec()))
    })
}

/// `ProjectViewSet.list_detail` (`plane/app/views/project/base.py:102-144`).
pub fn list_detail<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let member_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let role = role_of(ctx, slug, member_id).await?;
        let (join, filter) = role_filter(role);

        // `if request.GET.get("per_page", False) and request.GET.get("cursor", False)`
        // (`project/base.py:131`): both parameters are required.
        let paginated =
            ctx.query().get("per_page").is_some() && ctx.query().get("cursor").is_some();

        if !paginated {
            let sql = Sql {
                text: queries::detail_sql(join, filter, DETAIL_DEFAULT_ORDER).ok_or_else(|| {
                    ViewError::Unknown("detail statement has no WHERE".to_string())
                })?,
                params: vec![Value::Text(slug.to_owned()), Value::Uuid(member_id)],
            };
            let rows = sql.query(client).await?;
            let mut projects = Vec::with_capacity(rows.len());
            for row in &rows {
                projects.push(row_project(row)?);
            }
            enrich(ctx, &mut projects).await?;

            // `ProjectListSerializer(projects, many=True, fields=fields or None)`:
            // `fields=` is honoured, `expand=` is not passed.
            let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
            let fields: Option<Vec<FieldEntry<'_>>> = ctx
                .fields()
                .map(|names| names.into_iter().map(FieldEntry::name).collect());
            let render_ctx = RenderCtx::new(&timezone, fields.as_deref(), &[]);
            let serializers: Vec<ProjectList<'_>> =
                projects.iter().map(project_serializer).collect();
            let body = render_list(&serializers, &render_ctx)?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        }

        // The paginated branch: `BasePaginator.paginate`
        // (`plane/utils/paginator.py:661-775`) around `OffsetPaginator`
        // (`:105-198`). It answers the cursor envelope, not a bare array, and
        // `on_results` builds the serializer without `fields=`.
        let page = plane_query::PageQuery::parse(ctx.query(), None, None).map_err(query_error)?;
        let order = order_by_clause(ctx);
        let limit = i64::from(page.per_page);
        let offset = page.cursor.offset * limit;

        // `queryset.count()` then `page_results.count()`, in that order.
        let total_sql = Sql {
            text: count_sql(join, filter, "")?,
            params: vec![Value::Text(slug.to_owned()), Value::Uuid(member_id)],
        };
        let hits: i64 = total_sql
            .query(client)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| ViewError::Unknown("count returned no row".to_owned()))?
            .get(0);
        let page_count_sql = Sql {
            text: count_sql(join, filter, &format!("{order} LIMIT $3 OFFSET $4"))?,
            params: vec![
                Value::Text(slug.to_owned()),
                Value::Uuid(member_id),
                Value::Int8(limit + 1),
                Value::Int8(offset),
            ],
        };
        let page_count: i64 = page_count_sql
            .query(client)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| ViewError::Unknown("count returned no row".to_owned()))?
            .get(0);

        let sql = Sql {
            text: queries::detail_sql(join, filter, &format!("{order} LIMIT $3 OFFSET $4"))
                .ok_or_else(|| ViewError::Unknown("detail statement has no WHERE".to_string()))?,
            params: vec![
                Value::Text(slug.to_owned()),
                Value::Uuid(member_id),
                Value::Int8(limit),
                Value::Int8(offset),
            ],
        };
        let page_rows = sql.query(client).await?;
        let mut projects = Vec::with_capacity(page_rows.len());
        for row in &page_rows {
            projects.push(row_project(row)?);
        }

        // `if cursor.value != limit and cursor.is_prev: results = results[-(limit + 1):]`
        // then `results = results[:limit]` (`paginator.py:161-180`).
        if page.cursor.value != f64::from(page.per_page) && page.cursor.is_prev {
            let keep = (limit + 1) as usize;
            if projects.len() > keep {
                projects.drain(..projects.len() - keep);
            }
        }
        projects.truncate(limit as usize);
        let count = projects.len() as i64;
        enrich(ctx, &mut projects).await?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let render_ctx = RenderCtx::full(&timezone);
        let serializers: Vec<ProjectList<'_>> = projects.iter().map(project_serializer).collect();
        let results = DrfValue::Array(serializers.iter().map(DrfValue::nested).collect());

        // `Response({...})` (`plane/utils/paginator.py:742-759`), in its key
        // order. `next_cursor`/`prev_cursor` are `str(Cursor(limit, page ± 1,
        // is_prev))`, whose `value` is the page size.
        let page_index = page.cursor.offset;
        let next_cursor = format!("{limit}:{}:0", page_index + 1);
        let prev_cursor = format!("{limit}:{}:1", page_index - 1);
        let total_pages = if hits == 0 {
            0
        } else {
            (hits + limit - 1) / limit
        };

        let mut writer = DrfWriter::with_capacity(320 + count as usize * 512);
        let mut object = writer.begin_object();
        object.field("grouped_by", &Option::<&str>::None)?;
        object.field("sub_grouped_by", &Option::<&str>::None)?;
        object.field("total_count", &hits)?;
        object.field("next_cursor", &next_cursor)?;
        object.field("prev_cursor", &prev_cursor)?;
        object.field("next_page_results", &(page_count > limit))?;
        object.field("prev_page_results", &(page_index > 0))?;
        object.field("count", &count)?;
        object.field("total_pages", &total_pages)?;
        object.field("total_results", &hits)?;
        object.field("extra_stats", &Option::<&str>::None)?;
        object.field_value_dyn("results", &render_ctx, &results)?;
        object.end();
        Ok(ViewResponse::raw(200, APPLICATION_JSON, writer.into_vec()))
    })
}

/// `ProjectViewSet.retrieve` (`plane/app/views/project/base.py:226-256`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = ctx.kwarg("pk").ok_or(ViewError::Key)?.to_string();
        let project_id = lookup::uuid_kwarg("pk", &pk)?;
        let member_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

        let client = client(ctx)?;
        let row = client
            .query_opt(
                queries::PROJECT_DETAIL_ONE.sql,
                &[&slug, &member_id, &project_id],
            )
            .await?;
        let Some(row) = row else {
            return error_body(404, "Project does not exist");
        };
        let mut rows = vec![row_project(&row)?];
        enrich(ctx, &mut rows).await?;
        let project = &rows[0];

        // `member_ids = [str(m.member_id) for m in project.members_list]` and
        // `if str(request.user.id) not in member_ids` (`project/base.py:233-245`).
        if !project.members.contains(&member_id) {
            return if project.project.network == NETWORK_SECRET {
                error_body(403, "You do not have permission")
            } else {
                error_body(409, "You are not a member of this project")
            };
        }

        // `recent_visited_task.delay(slug=…, project_id=pk, entity_name="project",
        // entity_identifier=pk, user_id=request.user.id)`
        // (`project/base.py:247-253`) — the `ProjectKeyed` keyword order. `pk` is
        // the `<uuid:pk>` URL converter's `uuid.UUID`, which is what the observed
        // `kwargsrepr` shows (`UUID('…')`, not a string); only `slug` is a `str`.
        jobs::recent_visited_project(
            ctx,
            "project",
            TaskArg::uuid(project_id),
            Some(TaskArg::uuid(project_id)),
            TaskArg::uuid(member_id),
        )
        .await?;

        // `ProjectListSerializer(project)`: no `fields=`, no `expand=`.
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let serializer = project_serializer(project);
        let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// A one-key `{"error": …}` body, in Django's key order.
fn error_body(status: u16, message: &str) -> Result<ViewResponse, ViewError> {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        error: &'a str,
    }
    let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &Body { error: message })?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, rendered))
}

/// The two `.exists()` role probes `list`/`list_detail` run, pipelined on the
/// one connection.
///
/// The guest probe runs first and the member probe second; a user has a single
/// workspace membership row, so at most one can match.
async fn role_of(ctx: &ViewCtx<'_>, slug: &str, member_id: Uuid) -> Result<Role, ViewError> {
    let client = client(ctx)?;
    // Two `.exists()` calls, in the order the Python runs them: the guest probe
    // first, the member probe second.
    let guest = client
        .query_opt(
            queries::WORKSPACE_MEMBER_HAS_ROLE.sql,
            &[&member_id, &ROLE_GUEST, &slug],
        )
        .await?;
    let member = client
        .query_opt(
            queries::WORKSPACE_MEMBER_HAS_ROLE.sql,
            &[&member_id, &ROLE_MEMBER, &slug],
        )
        .await?;
    Ok(if guest.is_some() {
        Role::Guest
    } else if member.is_some() {
        Role::Member
    } else {
        Role::Other
    })
}

/// The extra join and `WHERE` fragment `list_detail` adds for a role
/// (`plane/app/views/project/base.py:106-129`).
fn role_filter(role: Role) -> (&'static str, &'static str) {
    match role {
        Role::Guest => (
            "INNER JOIN project_members pmf ON (p.id = pmf.project_id)",
            "AND pmf.is_active AND pmf.member_id = $2",
        ),
        Role::Member => (
            "LEFT OUTER JOIN project_members pmf ON (p.id = pmf.project_id)",
            "AND ((pmf.is_active AND pmf.member_id = $2) OR p.network = 2)",
        ),
        Role::Other => ("", ""),
    }
}

/// The non-paginated branch's ordering (`project/base.py:105`).
const DETAIL_DEFAULT_ORDER: &str = "ORDER BY sort_order ASC, p.name ASC";

/// `SELECT COUNT(*) FROM (<detail>) subquery` — `queryset.count()`
/// (`plane/utils/paginator.py:169`), with an optional trailing
/// `ORDER BY … LIMIT … OFFSET …` for the page count.
fn count_sql(join: &str, filter: &str, tail: &str) -> Result<String, ViewError> {
    let inner = queries::detail_sql(join, filter, tail)
        .ok_or_else(|| ViewError::Unknown("detail statement has no WHERE".to_string()))?;
    Ok(format!("SELECT COUNT(*) FROM ({inner}) subquery"))
}

/// `sanitize_order_by(…)` + `OffsetPaginator`'s ordering:
/// `F(key).asc|desc(nulls_last=True), "-created_at"`
/// (`plane/utils/order_queryset.py:135-156`, `plane/utils/paginator.py:141-146`).
fn order_by_clause(ctx: &ViewCtx<'_>) -> String {
    let raw = ctx.query().get("order_by").unwrap_or("");
    let sanitized = sanitize_order_by(raw);
    let (bare, descending) = match sanitized.strip_prefix('-') {
        Some(rest) => (rest, true),
        None => (sanitized.as_str(), false),
    };
    // The sort_order annotation is the subquery's alias; the other allowlisted
    // names are project columns.
    let expression = if bare == "sort_order" {
        "sort_order".to_string()
    } else {
        format!("p.{bare}")
    };
    let direction = if descending { "DESC" } else { "ASC" };
    format!("ORDER BY {expression} {direction} NULLS LAST, p.created_at DESC")
}

/// `sanitize_order_by(value, PROJECT_ORDER_BY_ALLOWLIST, "-created_at")`.
fn sanitize_order_by(value: &str) -> String {
    let default = "-created_at".to_string();
    if value.is_empty() {
        return default;
    }
    let (is_desc, bare) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    if bare.starts_with('-') || !PROJECT_ORDER_BY_ALLOWLIST.contains(&bare) {
        return default;
    }
    if is_desc {
        format!("-{bare}")
    } else {
        bare.to_string()
    }
}

/// `plane_query::QueryError` is the paginator's error type; DRF would render its
/// `ApiError` as the `ParseError`/`ValidationError` body.
fn query_error(error: plane_query::QueryError) -> ViewError {
    match error {
        plane_query::QueryError::Api(api) => ViewError::Api(ApiError::ParseError(api.message)),
        other => ViewError::Unknown(other.to_string()),
    }
}

/// Decode one detail row: the model's 36 columns, then the four annotations.
fn row_project(row: &tokio_postgres::Row) -> Result<ProjectRow, ViewError> {
    Ok(ProjectRow {
        project: Project::from_row(row, 0)?,
        is_favorite: row.get(36),
        member_role: row.get(37),
        anchor: row.get(38),
        sort_order: row.get(39),
        members: Vec::new(),
        next_work_item_sequence: 1,
        cover_image_url: None,
    })
}

/// The page-level side queries `ProjectListSerializer` triggers: the members
/// prefetch, `next_work_item_sequence` and `cover_image_url`.
///
/// Django issues one query for the members, then one `MAX(sequence)` **per
/// project** and one `file_assets` fetch per project with a cover asset. This
/// uses one grouped query for the sequences and one for the assets, so the page
/// costs two round trips instead of `1 + 2n`.
async fn enrich(ctx: &ViewCtx<'_>, rows: &mut [ProjectRow]) -> Result<(), ViewError> {
    if rows.is_empty() {
        return Ok(());
    }
    let client = client(ctx)?;
    let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
    let ids: Vec<Uuid> = rows.iter().map(|row| row.project.id).collect();

    let members = load_members(client, slug, &ids).await?;
    let sequences = load_sequences(client, &ids).await?;
    let assets = load_cover_assets(client, rows).await?;

    for row in rows.iter_mut() {
        row.members = members.get(&row.project.id).cloned().unwrap_or_default();
        // `(max_sequence + 1) if max_sequence else 1`.
        row.next_work_item_sequence = match sequences.get(&row.project.id) {
            Some(max) if *max != 0 => max + 1,
            _ => 1,
        };
        row.cover_image_url = assets.get(&row.project.id).cloned().flatten();
    }
    Ok(())
}

/// `Project.objects.filter(project_id=…, is_active=True)` per project, as one
/// query, keeping `ProjectMember.Meta.ordering` and the `is_bot` filter of
/// `ProjectListSerializer.get_members`.
async fn load_members(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<Uuid>>, ViewError> {
    let rows = client
        .query(
            queries::PROJECT_MEMBERS_PREFETCH.sql,
            &[&slug, &project_ids],
        )
        .await?;
    let mut members: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for row in &rows {
        let project_id: Uuid = row.get(0);
        let member_id: Option<Uuid> = row.get(1);
        let is_bot: Option<bool> = row.get(2);
        // `member.member.is_bot` on a NULL relation is an `AttributeError` in
        // Python, i.e. a 500 from the family handler.
        let Some(member_id) = member_id else {
            return Err(ViewError::Unknown(
                "a project member row has no member".to_string(),
            ));
        };
        if is_bot.unwrap_or(false) {
            continue;
        }
        members.entry(project_id).or_default().push(member_id);
    }
    Ok(members)
}

/// `IssueSequence.objects.filter(project_id=…).aggregate(Max("sequence"))` for
/// the whole page in one statement.
async fn load_sequences(
    client: &deadpool_postgres::Client,
    project_ids: &[Uuid],
) -> Result<HashMap<Uuid, i64>, ViewError> {
    let statement = queries::ISSUE_SEQUENCE_MAX.prepare(client).await?;
    let rows = client.query(&statement, &[&project_ids]).await?;
    let mut sequences = HashMap::with_capacity(rows.len());
    for row in &rows {
        sequences.insert(row.get(0), row.get(1));
    }
    Ok(sequences)
}

/// `Project.cover_image_url` for the page: one asset query, then the
/// three-branch property (`project.py:128-139`).
async fn load_cover_assets(
    client: &deadpool_postgres::Client,
    rows: &[ProjectRow],
) -> Result<HashMap<Uuid, Option<String>>, ViewError> {
    let asset_ids: HashSet<Uuid> = rows
        .iter()
        .filter_map(|row| row.project.cover_image_asset_id)
        .collect();
    let urls = lite::load_asset_urls(client, &asset_ids).await?;
    let mut out = HashMap::with_capacity(rows.len());
    for row in rows {
        let text = row.project.cover_image.clone().unwrap_or_default();
        let url = lite::asset_or_text(row.project.cover_image_asset_id, text, &urls)?;
        out.insert(row.project.id, url);
    }
    Ok(out)
}

/// `ProjectListSerializer(row.project)`.
fn project_serializer(row: &ProjectRow) -> ProjectList<'_> {
    let project = &row.project;
    ProjectList {
        id: project.id,
        is_favorite: row.is_favorite,
        sort_order: row.sort_order,
        member_role: row.member_role.map(i64::from),
        anchor: row.anchor.as_deref(),
        members: DrfValue::array(row.members.iter().map(|id| DrfValue::Uuid(*id)).collect()),
        cover_image_url: row.cover_image_url.as_deref(),
        inbox_view: project.intake_view,
        next_work_item_sequence: DrfValue::Int(row.next_work_item_sequence),
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
