// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.search.base` (`plane/app/views/search/base.py`).
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/search/` | `GlobalSearchEndpoint.get` | `base.py:46-326` |
//! | `GET /api/workspaces/{slug}/entity-search/` | `SearchEndpoint.get` | `base.py:329-782` |
//!
//! Neither view serialises with a DRF serializer: they return `Response` objects
//! built from `.values(...)` querysets, so the body is DRF's `JSONEncoder`
//! rendering of the dicts — see [`crate::search::values`].
//!
//! ## Queries
//!
//! The SQL is built with [`Frag`] and sent as an **unnamed** statement: the
//! `search`/`query` needle and the `entities`/`query_type` selection decide
//! whether a `LIKE` term and a project filter exist at all, so the statement
//! text depends on the request (`DECISIONS.md` D1's amendment). Every statement
//! reproduces the queryset Django builds, including the joins a
//! `filter(fk__field=…)` adds and the predicates a related model's default
//! manager does *not* add (Django never applies it to a join).
//!
//! The `icontains` lookup is `UPPER(col::text) LIKE UPPER(%needle%)` with
//! `django/db/backends/utils.py` `prep_for_like_query` escaping (`\`, `%`, `_`)
//! — `plane_query::sql::Frag::icontains` does not escape, so [`icontains`]
//! builds the term here.
//!
//! ## Errors
//!
//! Both views evaluate their querysets while rendering the response (inside
//! `get`), so a database error is the family handler's 400/500:
//!
//! * an invalid `project_id`/`module`/`issue_id` UUID is a Django
//!   `ValidationError` → 400 `{"error": "Please provide valid detail"}`
//!   (`plane/app/views/base.py:216-220`);
//! * `int(count)` failing or a negative `count` (Python's negative slice
//!   raises `ValueError`) is a 500.
//!
//! `GlobalSearchEndpoint` applies its project filter **lazily**: only entities
//! that read `project_id` coerce it, and only when `workspace_search` is
//! `"false"`, so `?project_id=garbage&entities=workspace` is a 200.

use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::sql::{Frag, Sql, Value};
use plane_serializers::RenderCtx;
use uuid::Uuid;

use crate::common::client;
use crate::search::values::{Cell, Results, RowValue, TypedResults};

pub fn register(
    registry: &mut plane_drf::registry::Registry,
) -> Result<(), plane_drf::registry::RegistryError> {
    registry
        .api_view("plane.app.views.search.base.GlobalSearchEndpoint")?
        .method(http::Method::GET, global_get)?;
    registry
        .api_view("plane.app.views.search.base.SearchEndpoint")?
        .method(http::Method::GET, entity_get)?;
    Ok(())
}

/// The entities `GlobalSearchEndpoint.get` knows, in `MODELS_MAPPER` order
/// (`base.py:301-310`). An `entities=` value outside this list is ignored.
const ENTITIES: [&str; 8] = [
    "workspace",
    "project",
    "issue",
    "cycle",
    "module",
    "issue_view",
    "page",
    "intake",
];

/// `django/db/backends/utils.py` `prep_for_like_query`.
fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

/// `column__icontains`: `UPPER(column::text) LIKE UPPER(%needle%)`.
pub(crate) fn icontains(column: &str, needle: &str) -> Frag {
    let mut frag = Frag::raw(format!("UPPER({column}::text) LIKE UPPER("));
    frag.push(Frag::bind(Value::Text(format!(
        "%{}%",
        escape_like(needle)
    ))));
    frag.push_raw(")");
    frag
}

/// Run one composed statement. Always an unnamed statement (dynamic SQL).
async fn run(client: &Client, frag: Frag) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let Sql { text, params } = frag.into_sql();
    let statement = client.prepare(&text).await.map_err(ViewError::from)?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    client
        .query(&statement, &params)
        .await
        .map_err(ViewError::from)
}

/// `Q(sequence_id=…)` terms for the numeric runs in `query`
/// (`re.findall(r"\b\d+\b", query)`; `base.py:88-92`, `:429-433`).
fn sequence_terms(query: &str) -> Vec<i64> {
    let characters: Vec<char> = query.chars().collect();
    let mut terms = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        if plane_core::py::decimal_value(characters[index]).is_none() {
            index += 1;
            continue;
        }
        let start = index;
        while index < characters.len() && plane_core::py::decimal_value(characters[index]).is_some()
        {
            index += 1;
        }
        // Python's `\b` needs a non-word character (or an end) on both sides.
        let before_ok = start == 0 || !is_word(characters[start - 1]);
        let after_ok = index == characters.len() || !is_word(characters[index]);
        if !(before_ok && after_ok) {
            continue;
        }
        let run: String = characters[start..index].iter().collect();
        // `re` matched; `int()` then converts the run. A run too large for i64
        // makes the IntegerField lookup fail in Postgres (a 500).
        if let Ok(value) = plane_core::py::int(&run) {
            terms.push(value);
        }
    }
    terms
}

/// Python `str.isalnum()`-or-`_`, the `\w` character class `\b` is defined on.
fn is_word(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

/// How a caller treats `sequence_id` in the needle predicate.
#[derive(Clone, Copy)]
pub(crate) enum SequenceStyle {
    /// `filter_issues`/`filter_intakes`: every numeric run is an equality
    /// (`base.py:88-92`, `:257-263`).
    Equality,
    /// `search_issues` (`plane/utils/issue_search.py:18-23`): numeric runs only
    /// when `len(query) <= 20`, otherwise `sequence_id__icontains`.
    LengthGated,
}

/// The `search`/`query` needle predicate shared by the issue-like entities.
pub(crate) fn search_predicate(
    query: &str,
    name: &str,
    identifier: Option<&str>,
    sequence: Option<(&str, SequenceStyle)>,
) -> Frag {
    let mut predicate = icontains(name, query);
    if let Some(identifier) = identifier {
        predicate = predicate.or(icontains(identifier, query));
    }
    if let Some((sequence, style)) = sequence {
        match style {
            SequenceStyle::Equality => {
                for term in sequence_terms(query) {
                    // `sequence_id` is an `integer` column, so a term outside
                    // its range can never match; Django binds it, Postgres
                    // compares an int8/numeric and finds nothing.
                    if let Ok(term) = i32::try_from(term) {
                        predicate = predicate.or(Frag::compare(sequence, "=", Value::Int(term)));
                    }
                }
            }
            SequenceStyle::LengthGated => {
                if query.chars().count() <= 20 {
                    for term in sequence_terms(query) {
                        if let Ok(term) = i32::try_from(term) {
                            predicate =
                                predicate.or(Frag::compare(sequence, "=", Value::Int(term)));
                        }
                    }
                } else {
                    predicate = predicate.or(icontains(sequence, query));
                }
            }
        }
    }
    predicate.paren()
}

/// Parse a `project_id` query parameter into the value `filter(project_id=…)`
/// binds; a bad value is Django's `ValidationError` (400).
fn project_uuid(raw: &str) -> Result<Uuid, ViewError> {
    Uuid::parse_str(raw).map_err(|_| ViewError::Validation)
}

// ---------------------------------------------------------------------------
// GlobalSearchEndpoint
// ---------------------------------------------------------------------------

/// `GlobalSearchEndpoint.get` (`base.py:295-326`).
pub fn global_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx.user.id().ok_or(ViewError::Api(
            plane_core::error::ApiError::NotAuthenticated,
        ))?;

        // `request.query_params.get("search", False)` — `False` (falsy) when
        // absent, so an empty value is the same as no search.
        let query = ctx.query().get("search").filter(|value| !value.is_empty());
        let entities_param = ctx.query().get("entities");
        let workspace_search = ctx.query().get_or("workspace_search", "false");
        // `request.query_params.get("project_id", False)`.
        let project_id = ctx
            .query()
            .get("project_id")
            .filter(|value| !value.is_empty());
        // `if workspace_search == "false" and project_id` (`base.py:109`).
        let scoped_project = if workspace_search == "false" {
            project_id
        } else {
            None
        };

        let requested: Vec<&str> = match entities_param {
            Some(raw) if !raw.is_empty() => raw
                .split(',')
                .map(str::trim)
                .filter(|entity| !entity.is_empty())
                .filter(|entity| ENTITIES.contains(entity))
                .collect(),
            _ => ENTITIES.to_vec(),
        };

        let client = client(ctx)?;
        let mut results: Vec<(String, Vec<RowValue>)> = Vec::new();
        for entity in requested {
            // A duplicate `entities=` value re-runs the query and overwrites the
            // same key, which keeps its first position; recomputing would cost
            // a statement for an identical body, so the second occurrence is
            // skipped.
            if results.iter().any(|(name, _)| name == entity) {
                continue;
            }
            let rows = match entity {
                "workspace" => global_workspaces(client, user, query).await?,
                "project" => global_projects(client, user, slug, query).await?,
                "issue" => {
                    let project = scoped_project.map(project_uuid).transpose()?;
                    global_issues(client, user, slug, query, project).await?
                }
                "cycle" => {
                    let project = scoped_project.map(project_uuid).transpose()?;
                    global_cycles(client, user, slug, query, project).await?
                }
                "module" => {
                    let project = scoped_project.map(project_uuid).transpose()?;
                    global_modules(client, user, slug, query, project).await?
                }
                "issue_view" => {
                    let project = scoped_project.map(project_uuid).transpose()?;
                    global_views(client, user, slug, query, project).await?
                }
                "page" => {
                    let project = scoped_project.map(project_uuid).transpose()?;
                    global_pages(client, user, slug, query, project).await?
                }
                "intake" => {
                    let project = scoped_project.map(project_uuid).transpose()?;
                    global_intakes(client, user, slug, query, project).await?
                }
                _ => continue,
            };
            results.push((entity.to_owned(), rows));
        }

        let body = plane_serializers::render(&Results(results), &RenderCtx::full(&chrono_tz::UTC))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `filter_workspaces` (`base.py:51-62`). The `workspace_member` join adds no
/// `is_active`/`deleted_at` predicate: the view does not filter them, and
/// Django's join never applies the related manager.
async fn global_workspaces(
    client: &Client,
    user: Uuid,
    query: Option<&str>,
) -> Result<Vec<RowValue>, ViewError> {
    let mut sql = Frag::raw(
        "SELECT DISTINCT w.name, w.id, w.slug, w.created_at \
         FROM workspaces w \
         INNER JOIN workspace_members wm ON w.id = wm.workspace_id \
         WHERE w.deleted_at IS NULL AND wm.member_id = ",
    );
    sql.push(Frag::bind(Value::Uuid(user)));
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(icontains("w.name", query));
    }
    sql.push_raw(" ORDER BY w.created_at DESC");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            ("slug", Cell::Text(row.try_get(2).map_err(ViewError::from)?)),
        ]));
    }
    Ok(values)
}

/// `filter_projects` (`base.py:64-81`).
async fn global_projects(
    client: &Client,
    user: Uuid,
    slug: &str,
    query: Option<&str>,
) -> Result<Vec<RowValue>, ViewError> {
    let mut sql = Frag::raw(
        "SELECT DISTINCT p.name, p.id, p.identifier, ws.slug, p.created_at \
         FROM projects p \
         INNER JOIN project_members pm ON p.id = pm.project_id \
         INNER JOIN workspaces ws ON p.workspace_id = ws.id \
         WHERE p.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(
            icontains("p.name", query)
                .or(icontains("p.identifier", query))
                .paren(),
        );
    }
    sql.push_raw(" AND p.archived_at IS NULL AND pm.is_active AND pm.member_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" ORDER BY p.created_at DESC");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "identifier",
                Cell::Text(row.try_get(2).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(3).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `filter_issues` (`base.py:83-119`).
async fn global_issues(
    client: &Client,
    user: Uuid,
    slug: &str,
    query: Option<&str>,
    project: Option<Uuid>,
) -> Result<Vec<RowValue>, ViewError> {
    let mut sql = Frag::raw(
        "SELECT DISTINCT i.name, i.id, i.sequence_id, pr.identifier, i.project_id, \
         ws.slug, i.created_at \
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
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(search_predicate(
            query,
            "i.name",
            Some("pr.identifier"),
            Some(("i.sequence_id", SequenceStyle::Equality)),
        ));
    }
    sql.push_raw(" AND (pm.role > 5 OR pr.guest_view_all_features OR i.created_by_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(
        ") AND pr.archived_at IS NULL AND pm.deleted_at IS NULL AND pm.is_active \
         AND pm.member_id = ",
    );
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    if let Some(project) = project {
        sql.push_raw(" AND i.project_id = ");
        sql.push(Frag::bind(Value::Uuid(project)));
    }
    sql.push_raw(" ORDER BY i.created_at DESC LIMIT 100");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "sequence_id",
                Cell::Int(i64::from(
                    row.try_get::<_, i32>(2).map_err(ViewError::from)?,
                )),
            ),
            (
                "project__identifier",
                Cell::Text(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "project_id",
                Cell::Uuid(row.try_get(4).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(5).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `filter_cycles` (`base.py:121-143`).
async fn global_cycles(
    client: &Client,
    user: Uuid,
    slug: &str,
    query: Option<&str>,
    project: Option<Uuid>,
) -> Result<Vec<RowValue>, ViewError> {
    let mut sql = Frag::raw(
        "SELECT DISTINCT c.name, c.id, c.project_id, pr.identifier, ws.slug, c.created_at \
         FROM cycles c \
         INNER JOIN projects pr ON c.project_id = pr.id \
         INNER JOIN project_members pm ON pr.id = pm.project_id \
         INNER JOIN workspaces ws ON c.workspace_id = ws.id \
         WHERE c.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(icontains("c.name", query));
    }
    sql.push_raw(" AND pr.archived_at IS NULL AND pm.is_active AND pm.member_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    if let Some(project) = project {
        sql.push_raw(" AND c.project_id = ");
        sql.push(Frag::bind(Value::Uuid(project)));
    }
    sql.push_raw(" ORDER BY c.created_at DESC");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "project_id",
                Cell::Uuid(row.try_get(2).map_err(ViewError::from)?),
            ),
            (
                "project__identifier",
                Cell::Text(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(4).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `filter_modules` (`base.py:145-167`).
async fn global_modules(
    client: &Client,
    user: Uuid,
    slug: &str,
    query: Option<&str>,
    project: Option<Uuid>,
) -> Result<Vec<RowValue>, ViewError> {
    let mut sql = Frag::raw(
        "SELECT DISTINCT m.name, m.id, m.project_id, pr.identifier, ws.slug, m.created_at \
         FROM modules m \
         INNER JOIN projects pr ON m.project_id = pr.id \
         INNER JOIN project_members pm ON pr.id = pm.project_id \
         INNER JOIN workspaces ws ON m.workspace_id = ws.id \
         WHERE m.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(icontains("m.name", query));
    }
    sql.push_raw(" AND pr.archived_at IS NULL AND pm.is_active AND pm.member_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    if let Some(project) = project {
        sql.push_raw(" AND m.project_id = ");
        sql.push(Frag::bind(Value::Uuid(project)));
    }
    sql.push_raw(" ORDER BY m.created_at DESC");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "project_id",
                Cell::Uuid(row.try_get(2).map_err(ViewError::from)?),
            ),
            (
                "project__identifier",
                Cell::Text(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(4).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `filter_views` (`base.py:222-251`).
async fn global_views(
    client: &Client,
    user: Uuid,
    slug: &str,
    query: Option<&str>,
    project: Option<Uuid>,
) -> Result<Vec<RowValue>, ViewError> {
    let mut sql = Frag::raw(
        "SELECT DISTINCT iv.name, iv.id, iv.project_id, pr.identifier, ws.slug, iv.created_at \
         FROM issue_views iv \
         INNER JOIN projects pr ON iv.project_id = pr.id \
         INNER JOIN project_members pm ON pr.id = pm.project_id \
         INNER JOIN workspaces ws ON iv.workspace_id = ws.id \
         WHERE iv.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(icontains("iv.name", query));
    }
    sql.push_raw(" AND (pm.role > 5 OR pr.guest_view_all_features OR iv.owned_by_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(") AND (iv.owned_by_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(
        " OR iv.access = 1) AND pr.archived_at IS NULL AND pm.deleted_at IS NULL \
         AND pm.is_active AND pm.member_id = ",
    );
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    if let Some(project) = project {
        sql.push_raw(" AND iv.project_id = ");
        sql.push(Frag::bind(Value::Uuid(project)));
    }
    sql.push_raw(" ORDER BY iv.created_at DESC");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "project_id",
                Cell::Uuid(row.try_get(2).map_err(ViewError::from)?),
            ),
            (
                "project__identifier",
                Cell::Text(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(4).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `filter_pages` (`base.py:169-220`).
///
/// The two `ArrayAgg(...)` annotations aggregate over the **through table**
/// (`project_pages.project_id` for `projects__id`, `projects.identifier` for
/// `projects__identifier`), filtered by `~Q(projects__id=True)` — Django's
/// `UUIDField.to_python(True)` is `uuid.UUID(int=1)`
/// (`django/db/models/fields/__init__.py:2762-2773`). The project-scoped branch
/// adds a correlated subquery annotation and filters on it.
async fn global_pages(
    client: &Client,
    user: Uuid,
    slug: &str,
    query: Option<&str>,
    project: Option<Uuid>,
) -> Result<Vec<RowValue>, ViewError> {
    let mut sql = Frag::raw(
        "SELECT DISTINCT p.name, p.id, \
           COALESCE(ARRAY_AGG(DISTINCT pp.project_id) \
             FILTER (WHERE NOT (pp.project_id = '00000000-0000-0000-0000-000000000001')), \
             '{}'::uuid[]) AS project_ids, \
           COALESCE(ARRAY_AGG(DISTINCT pr.identifier) \
             FILTER (WHERE NOT (pp.project_id = '00000000-0000-0000-0000-000000000001')), \
             '{}'::varchar[]) AS project_identifiers, \
           ws.slug, p.created_at \
         FROM pages p \
         INNER JOIN project_pages pp ON p.id = pp.page_id \
         INNER JOIN projects pr ON pp.project_id = pr.id \
         INNER JOIN project_members pm ON pr.id = pm.project_id \
         INNER JOIN workspaces ws ON p.workspace_id = ws.id \
         WHERE p.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(icontains("p.name", query));
    }
    sql.push_raw(" AND (pm.role > 5 OR pr.guest_view_all_features OR p.owned_by_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(") AND (p.owned_by_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(
        " OR p.access = 0) AND pr.archived_at IS NULL AND pm.deleted_at IS NULL \
         AND pm.is_active AND pm.member_id = ",
    );
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    if let Some(project) = project {
        // `pages.annotate(project_id=Subquery(...)).filter(project_id=…)`
        // (`base.py:209-214`); the annotation is not in `.values()`, so it only
        // appears in the predicate.
        sql.push_raw(
            " AND (SELECT u0.project_id FROM project_pages u0 \
                   WHERE u0.deleted_at IS NULL AND u0.page_id = p.id AND u0.project_id = ",
        );
        sql.push(Frag::bind(Value::Uuid(project)));
        sql.push_raw(" ORDER BY u0.created_at DESC LIMIT 1) = ");
        sql.push(Frag::bind(Value::Uuid(project)));
    }
    sql.push_raw(" GROUP BY p.id, ws.slug ORDER BY p.created_at DESC");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "project_ids",
                Cell::UuidList(
                    row.try_get::<_, Option<Vec<Uuid>>>(2)
                        .map_err(ViewError::from)?
                        .unwrap_or_default(),
                ),
            ),
            (
                "project_identifiers",
                Cell::TextList(
                    row.try_get::<_, Option<Vec<String>>>(3)
                        .map_err(ViewError::from)?
                        .unwrap_or_default(),
                ),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(4).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `filter_intakes` (`base.py:253-293`). `Issue.objects`, not
/// `Issue.issue_objects`: no triage/archived/draft exclusion.
async fn global_intakes(
    client: &Client,
    user: Uuid,
    slug: &str,
    query: Option<&str>,
    project: Option<Uuid>,
) -> Result<Vec<RowValue>, ViewError> {
    let mut sql = Frag::raw(
        "SELECT DISTINCT i.name, i.id, i.sequence_id, pr.identifier, i.project_id, \
         ws.slug, i.created_at \
         FROM issues i \
         INNER JOIN projects pr ON i.project_id = pr.id \
         INNER JOIN project_members pm ON pr.id = pm.project_id \
         INNER JOIN workspaces ws ON i.workspace_id = ws.id \
         INNER JOIN intake_issues ii ON i.id = ii.issue_id \
         WHERE i.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(search_predicate(
            query,
            "i.name",
            Some("pr.identifier"),
            Some(("i.sequence_id", SequenceStyle::Equality)),
        ));
    }
    sql.push_raw(" AND (pm.role > 5 OR pr.guest_view_all_features OR i.created_by_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(
        ") AND pr.archived_at IS NULL AND pm.deleted_at IS NULL AND pm.is_active \
         AND pm.member_id = ",
    );
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" AND (ii.status = 0 OR ii.status = -2)");
    if let Some(project) = project {
        sql.push_raw(" AND i.project_id = ");
        sql.push(Frag::bind(Value::Uuid(project)));
    }
    sql.push_raw(" ORDER BY i.created_at DESC LIMIT 100");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "sequence_id",
                Cell::Int(i64::from(
                    row.try_get::<_, i32>(2).map_err(ViewError::from)?,
                )),
            ),
            (
                "project__identifier",
                Cell::Text(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "project_id",
                Cell::Uuid(row.try_get(4).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(5).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

// ---------------------------------------------------------------------------
// SearchEndpoint
// ---------------------------------------------------------------------------

/// `SearchEndpoint.get` (`base.py:332-782`).
pub fn entity_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx.user.id().ok_or(ViewError::Api(
            plane_core::error::ApiError::NotAuthenticated,
        ))?;

        let query = ctx.query().get("query").filter(|value| !value.is_empty());
        let query_types: Vec<&str> = ctx
            .query()
            .get_or("query_type", "user_mention")
            .split(',')
            .map(str::trim)
            .collect();
        // `count = int(request.query_params.get("count", 5))` — a failure is an
        // uncaught `ValueError` (500), not a 400.
        let count: i64 = match ctx.query().get("count") {
            Some(raw) => plane_core::py::int(raw)
                .map_err(|error| ViewError::Unknown(format!("ValueError: {error}")))?,
            None => 5,
        };
        let project_id = ctx
            .query()
            .get("project_id")
            .filter(|value| !value.is_empty());
        let client = client(ctx)?;

        // `if project_id and not ProjectMember.objects.filter(...).exists()`.
        let project = match project_id {
            Some(raw) => {
                let project = project_uuid(raw)?;
                if !project_member_exists(client, slug, project, user).await? {
                    // `{query_type: [] for query_type in query_types}`, in order.
                    let empty: Vec<(String, Vec<RowValue>)> = query_types
                        .iter()
                        .map(|query_type| ((*query_type).to_owned(), Vec::new()))
                        .collect();
                    let body = plane_serializers::render(
                        &TypedResults(empty),
                        &RenderCtx::full(&chrono_tz::UTC),
                    )?;
                    return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
                }
                Some(project)
            }
            None => None,
        };

        let mut response: Vec<(String, Vec<RowValue>)> = Vec::new();
        for query_type in query_types {
            // A duplicate `query_type` overwrites the same key and keeps its
            // first position; the second evaluation cannot change the value.
            if response.iter().any(|(name, _)| name == query_type) {
                continue;
            }
            let rows = match (query_type, project) {
                ("user_mention", Some(project)) => {
                    user_mention_project(client, slug, project, query, count).await?
                }
                ("user_mention", None) => {
                    user_mention_workspace(client, slug, query, count).await?
                }
                ("project", _) => project_search(client, user, slug, query, count).await?,
                ("issue", Some(project)) => {
                    issue_search_entity(client, user, slug, Some(project), query, count).await?
                }
                ("issue", None) => {
                    issue_search_entity(client, user, slug, None, query, count).await?
                }
                ("cycle", Some(project)) => {
                    cycle_search(client, user, slug, Some(project), query, count).await?
                }
                ("cycle", None) => cycle_search(client, user, slug, None, query, count).await?,
                ("module", Some(project)) => {
                    module_search(client, user, slug, Some(project), query, count).await?
                }
                ("module", None) => module_search(client, user, slug, None, query, count).await?,
                ("page", Some(project)) => {
                    page_search(client, user, slug, Some(project), query, count).await?
                }
                ("page", None) => page_search(client, user, slug, None, query, count).await?,
                _ => continue,
            };
            response.push((query_type.to_owned(), rows));
        }

        let body =
            plane_serializers::render(&TypedResults(response), &RenderCtx::full(&chrono_tz::UTC))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `ProjectMember.objects.filter(member=…, project_id=…, workspace__slug=…,
/// is_active=True).exists()` (`base.py:343-350`).
async fn project_member_exists(
    client: &Client,
    slug: &str,
    project: Uuid,
    user: Uuid,
) -> Result<bool, ViewError> {
    let rows = run(
        client,
        Frag::raw(
            "SELECT 1 FROM project_members pm \
             INNER JOIN workspaces ws ON pm.workspace_id = ws.id \
             WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = ",
        )
        .append(Frag::bind(Value::Uuid(user)))
        .append(Frag::raw(" AND pm.project_id = "))
        .append(Frag::bind(Value::Uuid(project)))
        .append(Frag::raw(" AND ws.slug = "))
        .append(Frag::bind(Value::Text(slug.to_owned())))
        .append(Frag::raw(" LIMIT 1")),
    )
    .await?;
    Ok(!rows.is_empty())
}

/// The `CASE WHEN … END` avatar URL of the `user_mention` queries
/// (`base.py:375-392`, `:594-611`).
const AVATAR_URL: &str = "CASE WHEN u.avatar_asset_id IS NOT NULL \
     THEN '/api/assets/v2/static/' || CAST(u.avatar_asset_id AS varchar) || '/' \
     WHEN u.avatar_asset_id IS NULL THEN u.avatar ELSE NULL END";

/// `query_type=user_mention` with a `project_id` (`base.py:355-402`).
async fn user_mention_project(
    client: &Client,
    slug: &str,
    project: Uuid,
    query: Option<&str>,
    count: i64,
) -> Result<Vec<RowValue>, ViewError> {
    check_count(count)?;
    let mut sql = Frag::raw(format!(
        "SELECT DISTINCT {AVATAR_URL} AS avatar_url, u.display_name, pm.member_id, pm.created_at \
         FROM project_members pm \
         LEFT OUTER JOIN users u ON pm.member_id = u.id \
         INNER JOIN workspaces ws ON pm.workspace_id = ws.id \
         WHERE pm.deleted_at IS NULL"
    ));
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(
            icontains("u.first_name", query)
                .or(icontains("u.last_name", query))
                .or(icontains("u.display_name", query))
                .paren(),
        );
    }
    sql.push_raw(" AND pm.is_active AND NOT u.is_bot AND pm.project_id = ");
    sql.push(Frag::bind(Value::Uuid(project)));
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" ORDER BY pm.created_at DESC LIMIT ");
    sql.push(Frag::bind(Value::Int(count_i32(count)?)));
    // Postgres types a bare `LIMIT $n` as `bigint`; `Value::Int` is an int4
    // parameter, so the cast is what makes tokio-postgres accept it.
    sql.push_raw("::int4");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("member__avatar_url", avatar_cell(row, 0)?),
            (
                "member__display_name",
                Cell::Text(row.try_get(1).map_err(ViewError::from)?),
            ),
            (
                "member__id",
                Cell::Uuid(row.try_get(2).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `query_type=user_mention` without a `project_id` (`base.py:575-615`).
async fn user_mention_workspace(
    client: &Client,
    slug: &str,
    query: Option<&str>,
    count: i64,
) -> Result<Vec<RowValue>, ViewError> {
    check_count(count)?;
    let mut sql = Frag::raw(format!(
        "SELECT {AVATAR_URL} AS avatar_url, u.display_name, wm.member_id \
         FROM workspace_members wm \
         INNER JOIN users u ON wm.member_id = u.id \
         INNER JOIN workspaces ws ON wm.workspace_id = ws.id \
         WHERE wm.deleted_at IS NULL"
    ));
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(
            icontains("u.first_name", query)
                .or(icontains("u.last_name", query))
                .or(icontains("u.display_name", query))
                .paren(),
        );
    }
    sql.push_raw(" AND wm.is_active AND NOT u.is_bot AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" ORDER BY wm.created_at DESC LIMIT ");
    sql.push(Frag::bind(Value::Int(count_i32(count)?)));
    // Postgres types a bare `LIMIT $n` as `bigint`; `Value::Int` is an int4
    // parameter, so the cast is what makes tokio-postgres accept it.
    sql.push_raw("::int4");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("member__avatar_url", avatar_cell(row, 0)?),
            (
                "member__display_name",
                Cell::Text(row.try_get(1).map_err(ViewError::from)?),
            ),
            (
                "member__id",
                Cell::Uuid(row.try_get(2).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// The `CASE` above is `text NOT NULL`? No: the `ELSE NULL` and a NULL
/// `member__avatar` make the column nullable, so a NULL is JSON `null`.
fn avatar_cell(row: &tokio_postgres::Row, index: usize) -> Result<Cell, ViewError> {
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

/// `query_type=project` (`base.py:404-421`, `:617-634`).
async fn project_search(
    client: &Client,
    user: Uuid,
    slug: &str,
    query: Option<&str>,
    count: i64,
) -> Result<Vec<RowValue>, ViewError> {
    check_count(count)?;
    let mut sql = Frag::raw(
        "SELECT DISTINCT p.name, p.id, p.identifier, p.logo_props, ws.slug, p.created_at \
         FROM projects p \
         LEFT OUTER JOIN project_members pm ON p.id = pm.project_id \
         INNER JOIN workspaces ws ON p.workspace_id = ws.id \
         WHERE p.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(
            icontains("p.name", query)
                .or(icontains("p.identifier", query))
                .paren(),
        );
    }
    sql.push_raw(" AND (pm.member_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(" OR p.network = 2) AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" ORDER BY p.created_at DESC LIMIT ");
    sql.push(Frag::bind(Value::Int(count_i32(count)?)));
    // Postgres types a bare `LIMIT $n` as `bigint`; `Value::Int` is an int4
    // parameter, so the cast is what makes tokio-postgres accept it.
    sql.push_raw("::int4");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "identifier",
                Cell::Text(row.try_get(2).map_err(ViewError::from)?),
            ),
            (
                "logo_props",
                Cell::Json(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(4).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `query_type=issue` (`base.py:423-461`, `:636-673`).
async fn issue_search_entity(
    client: &Client,
    user: Uuid,
    slug: &str,
    project: Option<Uuid>,
    query: Option<&str>,
    count: i64,
) -> Result<Vec<RowValue>, ViewError> {
    check_count(count)?;
    let mut sql = Frag::raw(
        "SELECT DISTINCT i.name, i.id, i.sequence_id, pr.identifier, i.project_id, \
         i.priority, i.state_id, i.type_id, i.created_at \
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
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(search_predicate(
            query,
            "i.name",
            Some("pr.identifier"),
            Some(("i.sequence_id", SequenceStyle::Equality)),
        ));
    }
    sql.push_raw(" AND (pm.role > 5 OR pr.guest_view_all_features OR i.created_by_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(") AND pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    if let Some(project) = project {
        sql.push_raw(" AND i.project_id = ");
        sql.push(Frag::bind(Value::Uuid(project)));
    }
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" ORDER BY i.created_at DESC LIMIT ");
    sql.push(Frag::bind(Value::Int(count_i32(count)?)));
    // Postgres types a bare `LIMIT $n` as `bigint`; `Value::Int` is an int4
    // parameter, so the cast is what makes tokio-postgres accept it.
    sql.push_raw("::int4");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "sequence_id",
                Cell::Int(i64::from(
                    row.try_get::<_, i32>(2).map_err(ViewError::from)?,
                )),
            ),
            (
                "project__identifier",
                Cell::Text(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "project_id",
                Cell::Uuid(row.try_get(4).map_err(ViewError::from)?),
            ),
            (
                "priority",
                Cell::Text(row.try_get(5).map_err(ViewError::from)?),
            ),
            ("state_id", opt_uuid(row, 6)?),
            ("type_id", opt_uuid(row, 7)?),
        ]));
    }
    Ok(values)
}

/// `query_type=cycle` (`base.py:463-509`, `:675-720`).
///
/// `timezone.now()` is evaluated once per `When` in Python; every branch gets
/// the same value here (`timezone.now()` is called while the annotation is
/// built, microseconds apart).
async fn cycle_search(
    client: &Client,
    user: Uuid,
    slug: &str,
    project: Option<Uuid>,
    query: Option<&str>,
    count: i64,
) -> Result<Vec<RowValue>, ViewError> {
    check_count(count)?;
    let now = plane_query::global::now();
    let mut sql = Frag::raw(
        "SELECT DISTINCT c.name, c.id, c.project_id, pr.identifier, \
         CASE WHEN (c.start_date <= ",
    );
    sql.push(Frag::bind(Value::Timestamp(now)));
    sql.push_raw(" AND c.end_date >= ");
    sql.push(Frag::bind(Value::Timestamp(now)));
    sql.push_raw(") THEN 'CURRENT' WHEN c.start_date > ");
    sql.push(Frag::bind(Value::Timestamp(now)));
    sql.push_raw(" THEN 'UPCOMING' WHEN c.end_date < ");
    sql.push(Frag::bind(Value::Timestamp(now)));
    sql.push_raw(
        " THEN 'COMPLETED' WHEN (c.start_date IS NULL AND c.end_date IS NULL) \
         THEN 'DRAFT' ELSE 'DRAFT' END AS status, ws.slug, c.created_at \
         FROM cycles c \
         INNER JOIN projects pr ON c.project_id = pr.id \
         INNER JOIN project_members pm ON pr.id = pm.project_id \
         INNER JOIN workspaces ws ON c.workspace_id = ws.id \
         WHERE c.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(icontains("c.name", query));
    }
    sql.push_raw(" AND pm.is_active AND pm.member_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    if let Some(project) = project {
        sql.push_raw(" AND c.project_id = ");
        sql.push(Frag::bind(Value::Uuid(project)));
    }
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" ORDER BY c.created_at DESC LIMIT ");
    sql.push(Frag::bind(Value::Int(count_i32(count)?)));
    // Postgres types a bare `LIMIT $n` as `bigint`; `Value::Int` is an int4
    // parameter, so the cast is what makes tokio-postgres accept it.
    sql.push_raw("::int4");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "project_id",
                Cell::Uuid(row.try_get(2).map_err(ViewError::from)?),
            ),
            (
                "project__identifier",
                Cell::Text(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "status",
                Cell::Text(row.try_get(4).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(5).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `query_type=module` (`base.py:511-538`, `:722-748`).
async fn module_search(
    client: &Client,
    user: Uuid,
    slug: &str,
    project: Option<Uuid>,
    query: Option<&str>,
    count: i64,
) -> Result<Vec<RowValue>, ViewError> {
    check_count(count)?;
    let mut sql = Frag::raw(
        "SELECT DISTINCT m.name, m.id, m.project_id, pr.identifier, m.status, \
         ws.slug, m.created_at \
         FROM modules m \
         INNER JOIN projects pr ON m.project_id = pr.id \
         INNER JOIN project_members pm ON pr.id = pm.project_id \
         INNER JOIN workspaces ws ON m.workspace_id = ws.id \
         WHERE m.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(icontains("m.name", query));
    }
    sql.push_raw(" AND pm.is_active AND pm.member_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    if let Some(project) = project {
        sql.push_raw(" AND m.project_id = ");
        sql.push(Frag::bind(Value::Uuid(project)));
    }
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" ORDER BY m.created_at DESC LIMIT ");
    sql.push(Frag::bind(Value::Int(count_i32(count)?)));
    // Postgres types a bare `LIMIT $n` as `bigint`; `Value::Int` is an int4
    // parameter, so the cast is what makes tokio-postgres accept it.
    sql.push_raw("::int4");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "project_id",
                Cell::Uuid(row.try_get(2).map_err(ViewError::from)?),
            ),
            (
                "project__identifier",
                Cell::Text(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "status",
                Cell::Text(row.try_get(4).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(5).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

/// `query_type=page` (`base.py:540-571`, `:750-781`).
///
/// The project-scoped branch filters `projects__id=project_id` and has no
/// `is_global`; the workspace branch adds `is_global=True` (`access=0` in both,
/// which excludes private pages).
async fn page_search(
    client: &Client,
    user: Uuid,
    slug: &str,
    project: Option<Uuid>,
    query: Option<&str>,
    count: i64,
) -> Result<Vec<RowValue>, ViewError> {
    check_count(count)?;
    let mut sql = Frag::raw(
        "SELECT DISTINCT p.name, p.id, p.logo_props, pp.project_id, ws.slug, p.created_at \
         FROM pages p \
         INNER JOIN project_pages pp ON p.id = pp.page_id \
         INNER JOIN projects pr ON pp.project_id = pr.id \
         INNER JOIN project_members pm ON pr.id = pm.project_id \
         INNER JOIN workspaces ws ON p.workspace_id = ws.id \
         WHERE p.deleted_at IS NULL",
    );
    if let Some(query) = query {
        sql.push_raw(" AND ");
        sql.push(icontains("p.name", query));
    }
    sql.push_raw(" AND (pm.role > 5 OR pr.guest_view_all_features OR p.owned_by_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(") AND p.access = 0");
    match project {
        Some(project) => {
            sql.push_raw(" AND pp.project_id = ");
            sql.push(Frag::bind(Value::Uuid(project)));
        }
        None => sql.push_raw(" AND p.is_global"),
    }
    sql.push_raw(" AND pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = ");
    sql.push(Frag::bind(Value::Uuid(user)));
    sql.push_raw(" AND ws.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" ORDER BY p.created_at DESC LIMIT ");
    sql.push(Frag::bind(Value::Int(count_i32(count)?)));
    // Postgres types a bare `LIMIT $n` as `bigint`; `Value::Int` is an int4
    // parameter, so the cast is what makes tokio-postgres accept it.
    sql.push_raw("::int4");
    let rows = run(client, sql).await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        values.push(RowValue::new(vec![
            ("name", Cell::Text(row.try_get(0).map_err(ViewError::from)?)),
            ("id", Cell::Uuid(row.try_get(1).map_err(ViewError::from)?)),
            (
                "logo_props",
                Cell::Json(row.try_get(2).map_err(ViewError::from)?),
            ),
            (
                "projects__id",
                Cell::Uuid(row.try_get(3).map_err(ViewError::from)?),
            ),
            (
                "workspace__slug",
                Cell::Text(row.try_get(4).map_err(ViewError::from)?),
            ),
        ]));
    }
    Ok(values)
}

fn opt_uuid(row: &tokio_postgres::Row, index: usize) -> Result<Cell, ViewError> {
    Ok(
        match row
            .try_get::<_, Option<Uuid>>(index)
            .map_err(ViewError::from)?
        {
            Some(value) => Cell::Uuid(value),
            None => Cell::Null,
        },
    )
}

/// Python's negative slice (`queryset[:-1]`) raises
/// `ValueError: Negative indexing is not supported.` (`django/db/models/
/// query.py:410-417`), which no view catches: a 500.
fn check_count(count: i64) -> Result<(), ViewError> {
    if count < 0 {
        return Err(ViewError::Unknown(
            "ValueError: Negative indexing is not supported.".to_owned(),
        ));
    }
    Ok(())
}

/// `LIMIT count`: a value above `i32::MAX` is no limit at all for any real
/// table, and Postgres' `LIMIT` takes an int8 (`Value::Int` is an i32
/// parameter), so clamp it.
fn count_i32(count: i64) -> Result<i32, ViewError> {
    i32::try_from(count).map_err(|_| ViewError::Unknown("count out of range".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_runs_follow_python_word_boundaries() {
        assert_eq!(sequence_terms("ALP-26"), vec![26]);
        assert!(sequence_terms("a26").is_empty());
        assert!(sequence_terms("26a").is_empty());
        // `2` has no word boundary before it (`v` is a word character).
        assert_eq!(sequence_terms("v2.5"), vec![5]);
        assert!(sequence_terms("_7_").is_empty());
        assert!(sequence_terms("no digits").is_empty());
        assert_eq!(sequence_terms("ALP-1 ALP-2"), vec![1, 2]);
    }

    #[test]
    fn like_needles_are_escaped() {
        assert_eq!(escape_like("a%b_c\\d"), "a\\%b\\_c\\\\d");
    }

    #[test]
    fn a_negative_count_is_a_header_five_hundred() {
        assert!(matches!(check_count(-1), Err(ViewError::Unknown(_))));
        assert!(check_count(0).is_ok());
    }
}
