// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Modules on `/api/v1/` (routes 299–306; `plane/api/views/module.py`).
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | 299 `pr/modules/` | GET, POST | `ModuleListCreateAPIEndpoint` (`module.py:79-279`) |
//! | 300 `pr/modules-lite/` | GET | `ModuleListLiteAPIEndpoint` (`module.py:282-327`) |
//! | 301 `pr/modules/{pk}/` | GET, PATCH, DELETE | `ModuleDetailAPIEndpoint` (`module.py:330-584`) |
//! | 302 `pr/modules/{module_id}/module-issues/` | GET, POST | `ModuleIssueListCreateAPIEndpoint` (`module.py:587-790`) |
//! | 303 `pr/modules/{module_id}/module-issues/{issue_id}/` | DELETE | `ModuleIssueDetailAPIEndpoint` (`module.py:793-949`) |
//! | 304 `pr/modules/{pk}/archive/` | POST | `ModuleArchiveUnarchiveAPIEndpoint.post` (`module.py:1095-1115`) |
//! | 305 `pr/archived-modules/` | GET | the same view's `get` (`module.py:1067-1079`) |
//! | 306 `pr/archived-modules/{pk}/unarchive/` | DELETE | the same view's `delete` (`module.py:1129-1138`) |
//!
//! ## Not ported yet (the routes stay on Django)
//!
//! `POST pr/modules/` (create), `PATCH pr/modules/{pk}/`, `POST
//! …/module-issues/` (the duplicate-instead-of-move bulk add) and `DELETE
//! …/module-issues/{issue_id}/` are not registered. The SQL they need is
//! catalogued in [`modules_queries`] (`MODULE_INSERT`, `MODULE_UPDATE`,
//! `MODULE_MEMBER_INSERT`, `MODULE_ISSUE_ROW`, …); the handler work is the
//! two serializers' `validate`/`create`/`update`, the `ModuleMember` through
//! rows, and the `serializers.serialize("json", …)` payload of the bulk add.
//!
//! ## The manual serializer
//!
//! `ModuleSerializer` is one of the three v1 serializers the generated tables
//! cannot express (`SERIALIZERS.md`): it declares a **write-only** `members`
//! list and then `to_representation` appends `data["members"] =
//! [str(member.id) for member in instance.members.all()]` (`module.py:203-206`),
//! so `members` is always the last key, is never expanded (the append wins over
//! the base class's `expand=`), and its order is `users.created_at DESC`.
//!
//! ## Bugs ported as they are
//!
//! * **A bulk add never moves an existing row** (`module.py:740-753`): the
//!   membership test is `str(module_issue.issue_id) in issues`, where `issues`
//!   is an `Issue.objects.filter(...).values_list("id")` queryset of `UUID`
//!   objects. Comparing a string against UUIDs is always false, so an issue
//!   already in another module gets a **second** `module_issues` row (the
//!   partial unique index is on `(issue, module)`), and
//!   `updated_module_issues` is always empty.
//! * `requested_data` is `{"modules_list": str(issues)}` — the `repr` of that
//!   queryset, `<QuerySet [UUID('…')]>` (`module.py:773`).
//! * The delete's permission check accepts only a project admin, not a
//!   workspace admin (`module.py:548-556`), unlike the cycle delete.
//! * `ModuleDetail.get`/`list` filter `archived_at IS NULL`; the write lookups
//!   do not.
//! * `ModuleUpdateSerializer.update` soft-deletes every membership row and
//!   re-creates the list (`serializers/module.py:148-164`), so an explicit
//!   `members` in a PATCH rewrites `deleted_at` on the through rows.

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::paginator::OffsetWindow;
use plane_serializers::{
    DrfError, DrfSerialize, DrfUuid, DrfValue, DrfWriter, Expansion, Expansions, FilterMode,
    PgJson, RenderCtx,
};
use uuid::Uuid;

use crate::common::page;
use crate::common::render::RequestRender;
use crate::cycles::{self, CycleRelations};
use crate::issues;
use crate::modules_queries as queries;

/// `MODULE_ORDER_BY_ALLOWLIST` (`order_queryset.py:124-132`).
const MODULE_ORDER_BY_ALLOWLIST: &[&str] = &[
    "created_at",
    "updated_at",
    "name",
    "start_date",
    "target_date",
    "status",
    "sort_order",
];

fn sanitize_order_by(value: &str) -> String {
    let default = "-created_at".to_owned();
    if value.is_empty() {
        return default;
    }
    let descending = value.starts_with('-');
    let bare = if descending { &value[1..] } else { value };
    if bare.starts_with('-') || !MODULE_ORDER_BY_ALLOWLIST.contains(&bare) {
        return default;
    }
    if descending {
        format!("-{bare}")
    } else {
        bare.to_owned()
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.module.ModuleListCreateAPIEndpoint")?
        .method(http::Method::GET, list)?;
    registry
        .api_view("plane.api.views.module.ModuleListLiteAPIEndpoint")?
        .method(http::Method::GET, lite_list)?;
    registry
        .api_view("plane.api.views.module.ModuleDetailAPIEndpoint")?
        .method(http::Method::GET, detail)?
        .method(http::Method::DELETE, delete)?;
    registry
        .api_view("plane.api.views.module.ModuleIssueListCreateAPIEndpoint")?
        .method(http::Method::GET, issue_list)?;
    registry
        .api_view("plane.api.views.module.ModuleArchiveUnarchiveAPIEndpoint")?
        .method(http::Method::GET, archived_list)?
        .method(http::Method::POST, archive_post)?
        .method(http::Method::DELETE, unarchive)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// One read row: the 28 serializer columns, in `_readable_fields` order.
#[derive(Debug)]
pub(crate) struct ModuleRow {
    pub(crate) id: Uuid,
    pub(crate) total_issues: i64,
    pub(crate) cancelled_issues: i64,
    pub(crate) completed_issues: i64,
    pub(crate) started_issues: i64,
    pub(crate) unstarted_issues: i64,
    pub(crate) backlog_issues: i64,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) deleted_at: Option<DateTime<Utc>>,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) description_text: Option<JsonText>,
    pub(crate) description_html: Option<JsonText>,
    pub(crate) start_date: Option<NaiveDate>,
    pub(crate) target_date: Option<NaiveDate>,
    pub(crate) status: String,
    pub(crate) view_props: Option<JsonText>,
    pub(crate) sort_order: f64,
    pub(crate) external_source: Option<String>,
    pub(crate) external_id: Option<String>,
    pub(crate) archived_at: Option<DateTime<Utc>>,
    pub(crate) logo_props: Option<JsonText>,
    pub(crate) created_by: Option<Uuid>,
    pub(crate) updated_by: Option<Uuid>,
    pub(crate) project: Uuid,
    pub(crate) workspace: Uuid,
    pub(crate) lead: Option<Uuid>,
}

fn opt_text(row: &tokio_postgres::Row, index: usize) -> Result<Option<String>, ViewError> {
    row.try_get(index).map_err(ViewError::from)
}

fn opt_json(row: &tokio_postgres::Row, index: usize) -> Result<Option<JsonText>, ViewError> {
    row.try_get(index).map_err(ViewError::from)
}

impl ModuleRow {
    /// A bare row (`MODULE_ROW`/`MODULE_BY_PK`).
    fn from_plain_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            total_issues: 0,
            cancelled_issues: 0,
            completed_issues: 0,
            started_issues: 0,
            unstarted_issues: 0,
            backlog_issues: 0,
            created_at: row.try_get(1).map_err(ViewError::from)?,
            updated_at: row.try_get(2).map_err(ViewError::from)?,
            deleted_at: row.try_get(3).map_err(ViewError::from)?,
            name: row.try_get(4).map_err(ViewError::from)?,
            description: row.try_get(5).map_err(ViewError::from)?,
            description_text: opt_json(row, 6)?,
            description_html: opt_json(row, 7)?,
            start_date: row.try_get(8).map_err(ViewError::from)?,
            target_date: row.try_get(9).map_err(ViewError::from)?,
            status: row.try_get(10).map_err(ViewError::from)?,
            view_props: opt_json(row, 11)?,
            sort_order: row.try_get(12).map_err(ViewError::from)?,
            external_source: opt_text(row, 13)?,
            external_id: opt_text(row, 14)?,
            archived_at: row.try_get(15).map_err(ViewError::from)?,
            logo_props: opt_json(row, 16)?,
            created_by: row.try_get(17).map_err(ViewError::from)?,
            updated_by: row.try_get(18).map_err(ViewError::from)?,
            project: row.try_get(19).map_err(ViewError::from)?,
            workspace: row.try_get(20).map_err(ViewError::from)?,
            lead: row.try_get(21).map_err(ViewError::from)?,
        })
    }

    /// A read row (`MODULE_LIST`/`MODULE_DETAIL`/`MODULE_ARCHIVED_LIST`): the
    /// id, the six counts, then the 21 model columns.
    fn from_read_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            total_issues: row.try_get(1).map_err(ViewError::from)?,
            cancelled_issues: row.try_get(2).map_err(ViewError::from)?,
            completed_issues: row.try_get(3).map_err(ViewError::from)?,
            started_issues: row.try_get(4).map_err(ViewError::from)?,
            unstarted_issues: row.try_get(5).map_err(ViewError::from)?,
            backlog_issues: row.try_get(6).map_err(ViewError::from)?,
            created_at: row.try_get(7).map_err(ViewError::from)?,
            updated_at: row.try_get(8).map_err(ViewError::from)?,
            deleted_at: row.try_get(9).map_err(ViewError::from)?,
            name: row.try_get(10).map_err(ViewError::from)?,
            description: row.try_get(11).map_err(ViewError::from)?,
            description_text: opt_json(row, 12)?,
            description_html: opt_json(row, 13)?,
            start_date: row.try_get(14).map_err(ViewError::from)?,
            target_date: row.try_get(15).map_err(ViewError::from)?,
            status: row.try_get(16).map_err(ViewError::from)?,
            view_props: opt_json(row, 17)?,
            sort_order: row.try_get(18).map_err(ViewError::from)?,
            external_source: opt_text(row, 19)?,
            external_id: opt_text(row, 20)?,
            archived_at: row.try_get(21).map_err(ViewError::from)?,
            logo_props: opt_json(row, 22)?,
            created_by: row.try_get(23).map_err(ViewError::from)?,
            updated_by: row.try_get(24).map_err(ViewError::from)?,
            project: row.try_get(25).map_err(ViewError::from)?,
            workspace: row.try_get(26).map_err(ViewError::from)?,
            lead: row.try_get(27).map_err(ViewError::from)?,
        })
    }
}

/// The 28 fields `ModuleSerializer` renders, in `_readable_fields` order, plus
/// the appended `members`.
#[derive(Debug)]
struct ModuleBody<'a> {
    row: &'a ModuleRow,
    members: &'a [Uuid],
}

impl ModuleBody<'_> {
    const FIELDS: &'static [&'static str] = &[
        "id",
        "total_issues",
        "cancelled_issues",
        "completed_issues",
        "started_issues",
        "unstarted_issues",
        "backlog_issues",
        "created_at",
        "updated_at",
        "deleted_at",
        "name",
        "description",
        "description_text",
        "description_html",
        "start_date",
        "target_date",
        "status",
        "view_props",
        "sort_order",
        "external_source",
        "external_id",
        "archived_at",
        "logo_props",
        "created_by",
        "updated_by",
        "project",
        "workspace",
        "lead",
        "members",
    ];
}

impl DrfSerialize for ModuleBody<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        const MODE: FilterMode = FilterMode::RemoveFields;
        let row = self.row;
        let mut obj = w.begin_object();
        if ctx.emits("id", MODE) {
            obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
        }
        if ctx.emits("total_issues", MODE) {
            obj.field_dyn("total_issues", ctx, &row.total_issues)?;
        }
        if ctx.emits("cancelled_issues", MODE) {
            obj.field_dyn("cancelled_issues", ctx, &row.cancelled_issues)?;
        }
        if ctx.emits("completed_issues", MODE) {
            obj.field_dyn("completed_issues", ctx, &row.completed_issues)?;
        }
        if ctx.emits("started_issues", MODE) {
            obj.field_dyn("started_issues", ctx, &row.started_issues)?;
        }
        if ctx.emits("unstarted_issues", MODE) {
            obj.field_dyn("unstarted_issues", ctx, &row.unstarted_issues)?;
        }
        if ctx.emits("backlog_issues", MODE) {
            obj.field_dyn("backlog_issues", ctx, &row.backlog_issues)?;
        }
        if ctx.emits("created_at", MODE) {
            obj.field_dyn(
                "created_at",
                ctx,
                &plane_serializers::fields::drf_datetime(row.created_at, ctx.tz()),
            )?;
        }
        if ctx.emits("updated_at", MODE) {
            obj.field_dyn(
                "updated_at",
                ctx,
                &plane_serializers::fields::drf_datetime(row.updated_at, ctx.tz()),
            )?;
        }
        if ctx.emits("deleted_at", MODE) {
            obj.field_dyn(
                "deleted_at",
                ctx,
                &plane_serializers::fields::drf_datetime_opt(row.deleted_at, ctx.tz()),
            )?;
        }
        if ctx.emits("name", MODE) {
            obj.field_dyn("name", ctx, &row.name)?;
        }
        if ctx.emits("description", MODE) {
            obj.field_dyn("description", ctx, &row.description)?;
        }
        if ctx.emits("description_text", MODE) {
            obj.field_json_dyn(
                "description_text",
                ctx,
                row.description_text.as_ref().map(PgJson::from),
            )?;
        }
        if ctx.emits("description_html", MODE) {
            obj.field_json_dyn(
                "description_html",
                ctx,
                row.description_html.as_ref().map(PgJson::from),
            )?;
        }
        if ctx.emits("start_date", MODE) {
            obj.field_dyn(
                "start_date",
                ctx,
                &row.start_date.map(plane_serde::values::PyDate),
            )?;
        }
        if ctx.emits("target_date", MODE) {
            obj.field_dyn(
                "target_date",
                ctx,
                &row.target_date.map(plane_serde::values::PyDate),
            )?;
        }
        if ctx.emits("status", MODE) {
            obj.field_dyn("status", ctx, &row.status)?;
        }
        if ctx.emits("view_props", MODE) {
            obj.field_json_dyn("view_props", ctx, row.view_props.as_ref().map(PgJson::from))?;
        }
        if ctx.emits("sort_order", MODE) {
            obj.field_dyn("sort_order", ctx, &row.sort_order)?;
        }
        if ctx.emits("external_source", MODE) {
            obj.field_dyn("external_source", ctx, &row.external_source)?;
        }
        if ctx.emits("external_id", MODE) {
            obj.field_dyn("external_id", ctx, &row.external_id)?;
        }
        if ctx.emits("archived_at", MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &plane_serializers::fields::drf_datetime_opt(row.archived_at, ctx.tz()),
            )?;
        }
        if ctx.emits("logo_props", MODE) {
            obj.field_json_dyn("logo_props", ctx, row.logo_props.as_ref().map(PgJson::from))?;
        }
        if ctx.emits("created_by", MODE) {
            obj.field_dyn("created_by", ctx, &row.created_by.map(DrfUuid))?;
        }
        if ctx.emits("updated_by", MODE) {
            obj.field_dyn("updated_by", ctx, &row.updated_by.map(DrfUuid))?;
        }
        if ctx.emits("project", MODE) {
            obj.field_dyn("project", ctx, &DrfUuid(row.project))?;
        }
        if ctx.emits("workspace", MODE) {
            obj.field_dyn("workspace", ctx, &DrfUuid(row.workspace))?;
        }
        if ctx.emits("lead", MODE) {
            obj.field_dyn("lead", ctx, &row.lead.map(DrfUuid))?;
        }
        // `to_representation` appends the ids **unconditionally**
        // (`module.py:203-206`): `fields=members` is not consulted, and an
        // `expand=members` is overwritten after the base class ran.
        {
            obj.key("members")?;
            let value = DrfValue::Array(self.members.iter().copied().map(DrfValue::Uuid).collect());
            obj.value(ctx, &value)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, Self::FIELDS, MODE)?;
        obj.end();
        Ok(())
    }
}

/// One `ModuleLiteSerializer` row (`module.py:304-327`), 23 fields.
#[derive(Debug)]
struct ModuleLiteBody<'a> {
    row: &'a ModuleRow,
    members: &'a [Uuid],
}

impl DrfSerialize for ModuleLiteBody<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        const MODE: FilterMode = FilterMode::RemoveFields;
        let row = self.row;
        let mut obj = w.begin_object();
        let emits = |name: &str| ctx.emits(name, MODE);
        if emits("id") {
            obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
        }
        if emits("created_at") {
            obj.field_dyn(
                "created_at",
                ctx,
                &plane_serializers::fields::drf_datetime(row.created_at, ctx.tz()),
            )?;
        }
        if emits("updated_at") {
            obj.field_dyn(
                "updated_at",
                ctx,
                &plane_serializers::fields::drf_datetime(row.updated_at, ctx.tz()),
            )?;
        }
        if emits("deleted_at") {
            obj.field_dyn(
                "deleted_at",
                ctx,
                &plane_serializers::fields::drf_datetime_opt(row.deleted_at, ctx.tz()),
            )?;
        }
        if emits("name") {
            obj.field_dyn("name", ctx, &row.name)?;
        }
        if emits("description") {
            obj.field_dyn("description", ctx, &row.description)?;
        }
        if emits("description_text") {
            obj.field_json_dyn(
                "description_text",
                ctx,
                row.description_text.as_ref().map(PgJson::from),
            )?;
        }
        if emits("description_html") {
            obj.field_json_dyn(
                "description_html",
                ctx,
                row.description_html.as_ref().map(PgJson::from),
            )?;
        }
        if emits("start_date") {
            obj.field_dyn(
                "start_date",
                ctx,
                &row.start_date.map(plane_serde::values::PyDate),
            )?;
        }
        if emits("target_date") {
            obj.field_dyn(
                "target_date",
                ctx,
                &row.target_date.map(plane_serde::values::PyDate),
            )?;
        }
        if emits("status") {
            obj.field_dyn("status", ctx, &row.status)?;
        }
        if emits("view_props") {
            obj.field_json_dyn("view_props", ctx, row.view_props.as_ref().map(PgJson::from))?;
        }
        if emits("sort_order") {
            obj.field_dyn("sort_order", ctx, &row.sort_order)?;
        }
        if emits("external_source") {
            obj.field_dyn("external_source", ctx, &row.external_source)?;
        }
        if emits("external_id") {
            obj.field_dyn("external_id", ctx, &row.external_id)?;
        }
        if emits("archived_at") {
            obj.field_dyn(
                "archived_at",
                ctx,
                &plane_serializers::fields::drf_datetime_opt(row.archived_at, ctx.tz()),
            )?;
        }
        if emits("logo_props") {
            obj.field_json_dyn("logo_props", ctx, row.logo_props.as_ref().map(PgJson::from))?;
        }
        if emits("created_by") {
            obj.field_dyn("created_by", ctx, &row.created_by.map(DrfUuid))?;
        }
        if emits("updated_by") {
            obj.field_dyn("updated_by", ctx, &row.updated_by.map(DrfUuid))?;
        }
        if emits("project") {
            obj.field_dyn("project", ctx, &DrfUuid(row.project))?;
        }
        if emits("workspace") {
            obj.field_dyn("workspace", ctx, &DrfUuid(row.workspace))?;
        }
        if emits("lead") {
            obj.field_dyn("lead", ctx, &row.lead.map(DrfUuid))?;
        }
        // The same unconditional append as `ModuleSerializer`.
        {
            obj.key("members")?;
            let value = DrfValue::Array(self.members.iter().copied().map(DrfValue::Uuid).collect());
            obj.value(ctx, &value)?;
        }
        ctx.expansions().write_injected(
            &mut obj,
            ctx,
            &[
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "name",
                "description",
                "description_text",
                "description_html",
                "start_date",
                "target_date",
                "status",
                "view_props",
                "sort_order",
                "external_source",
                "external_id",
                "archived_at",
                "logo_props",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "lead",
                "members",
            ],
            MODE,
        )?;
        obj.end();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

async fn member_ids(client: &Client, module_id: Uuid) -> Result<Vec<Uuid>, ViewError> {
    let prepared = queries::MODULE_MEMBER_IDS
        .prepare(client)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&prepared, &[&module_id])
        .await
        .map_err(cycles::db_error)?;
    rows.iter()
        .map(|row| row.try_get::<_, Uuid>(0).map_err(ViewError::from))
        .collect()
}

async fn members_of(client: &Client, modules: &[ModuleRow]) -> Result<Vec<Vec<Uuid>>, ViewError> {
    let mut out = Vec::with_capacity(modules.len());
    for module in modules {
        out.push(member_ids(client, module.id).await?);
    }
    Ok(out)
}

/// The relations the requested `expand=` names need for a module page.
async fn load_module_relations(
    ctx: &ViewCtx<'_>,
    modules: &[ModuleRow],
    requested: &[&str],
) -> Result<CycleRelations, ViewError> {
    if requested.is_empty() {
        return Ok(CycleRelations::default());
    }
    let client = cycles::client(ctx)?;
    let mut user_ids = Vec::new();
    let mut project_ids = Vec::new();
    let mut workspace_ids = Vec::new();
    for name in requested {
        match *name {
            "created_by" => user_ids.extend(modules.iter().filter_map(|row| row.created_by)),
            "updated_by" => user_ids.extend(modules.iter().filter_map(|row| row.updated_by)),
            "project" => project_ids.extend(modules.iter().map(|row| row.project)),
            "workspace" => workspace_ids.extend(modules.iter().map(|row| row.workspace)),
            _ => {}
        }
    }
    user_ids.sort_unstable();
    user_ids.dedup();
    project_ids.sort_unstable();
    project_ids.dedup();
    workspace_ids.sort_unstable();
    workspace_ids.dedup();
    Ok(CycleRelations {
        users: issues::load_users(client, &user_ids).await?,
        projects: issues::load_projects(client, &project_ids).await?,
        workspaces: issues::load_workspaces(client, &workspace_ids).await?,
    })
}

/// The expansion values of one module page.
async fn render_modules(
    ctx: &ViewCtx<'_>,
    modules: &[ModuleRow],
    lite: bool,
) -> Result<Vec<u8>, ViewError> {
    let client = cycles::client(ctx)?;
    let members = members_of(client, modules).await?;
    let requested = ctx.expand().unwrap_or_default();
    let relations = load_module_relations(ctx, modules, &requested).await?;
    let prepared = RequestRender::from_ctx(ctx);
    let mut body = Vec::with_capacity(modules.len() * 700 + 2);
    body.push(b'[');
    for (index, row) in modules.iter().enumerate() {
        if index != 0 {
            body.push(b',');
        }
        let values = module_expansions(row, &relations, &requested);
        let expansions = Expansions::new(&requested, &values);
        let render_ctx = prepared.render_ctx().with_expansions(expansions);
        if lite {
            let value = ModuleLiteBody {
                row,
                members: &members[index],
            };
            body.extend(plane_serializers::render(&value, &render_ctx).map_err(ViewError::from)?);
        } else {
            let value = ModuleBody {
                row,
                members: &members[index],
            };
            body.extend(plane_serializers::render(&value, &render_ctx).map_err(ViewError::from)?);
        }
    }
    body.push(b']');
    Ok(body)
}

/// Render one module (the detail route), without the page array.
async fn render_module(
    ctx: &ViewCtx<'_>,
    module: &ModuleRow,
    lite: bool,
) -> Result<Vec<u8>, ViewError> {
    let client = cycles::client(ctx)?;
    let members = member_ids(client, module.id).await?;
    let requested = ctx.expand().unwrap_or_default();
    let relations = load_module_relations(ctx, std::slice::from_ref(module), &requested).await?;
    let values = module_expansions(module, &relations, &requested);
    let expansions = Expansions::new(&requested, &values);
    let prepared = RequestRender::from_ctx(ctx);
    let render_ctx = prepared.render_ctx().with_expansions(expansions);
    if lite {
        let value = ModuleLiteBody {
            row: module,
            members: &members,
        };
        plane_serializers::render(&value, &render_ctx).map_err(ViewError::from)
    } else {
        let value = ModuleBody {
            row: module,
            members: &members,
        };
        plane_serializers::render(&value, &render_ctx).map_err(ViewError::from)
    }
}

/// The mapper names of a module: `lead` is **not** one (it falls back to
/// `lead_id`), `created_by`/`updated_by` are users, `project`/`workspace` their
/// lite serializers and `members` is overwritten by `to_representation`.
fn module_expansions<'a>(
    row: &'a ModuleRow,
    relations: &'a CycleRelations,
    requested: &[&'a str],
) -> Vec<Expansion<'a>> {
    let user_value = |id: Uuid| -> plane_serializers::DrfValue<'a> {
        relations
            .users
            .get(&id)
            .map_or(plane_serializers::DrfValue::Null, issues::user_value)
    };
    let mut values = Vec::with_capacity(requested.len());
    for name in requested {
        if !ModuleBody::FIELDS.contains(name) {
            continue;
        }
        let value = match *name {
            "created_by" => row
                .created_by
                .map_or(plane_serializers::DrfValue::Null, user_value),
            "updated_by" => row
                .updated_by
                .map_or(plane_serializers::DrfValue::Null, user_value),
            "project" => relations
                .projects
                .get(&row.project)
                .map_or(plane_serializers::DrfValue::Null, cycles::project_value),
            "workspace" => relations
                .workspaces
                .get(&row.workspace)
                .map_or(plane_serializers::DrfValue::Null, cycles::workspace_value),
            // Not in the 14-name mapper: `getattr(instance, "lead_id", None)`.
            "lead" => row.lead.map_or(
                plane_serializers::DrfValue::Null,
                plane_serializers::DrfValue::Uuid,
            ),
            _ => plane_serializers::DrfValue::Null,
        };
        values.push(Expansion { name, value });
    }
    values
}

// ---------------------------------------------------------------------------
// 299 GET — list
// ---------------------------------------------------------------------------

async fn list_page(
    ctx: &ViewCtx<'_>,
    statement: &plane_db::Query,
    filter: &str,
    binds: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<ViewResponse, ViewError> {
    let client = cycles::client(ctx)?;
    let page = page::default_query(ctx)?;
    let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
    let limit = window.row_limit() + 1;
    let offset = window.offset;
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = binds.to_vec();
    params.push(&limit);
    params.push(&offset);
    let sql = format!(
        "{} LIMIT ${} OFFSET ${}",
        statement.sql,
        params.len() - 1,
        params.len()
    );
    let rows = client
        .query(&sql, &params)
        .await
        .map_err(cycles::db_error)?;
    let fetched = rows.len();
    let mut decoded = Vec::with_capacity(fetched.min(window.row_limit() as usize));
    for row in rows.iter().take(window.row_limit() as usize) {
        decoded.push(ModuleRow::from_read_row(row)?);
    }
    let body = render_modules(ctx, &decoded, false).await?;
    let total_sql = format!("SELECT COUNT(*) FROM (SELECT DISTINCT modules.id {filter}) subquery");
    let total_row = client
        .query_one(&total_sql, binds)
        .await
        .map_err(cycles::db_error)?;
    let total: i64 = total_row.try_get(0).map_err(ViewError::from)?;
    let (next, prev) = window.cursors(fetched);
    Ok(ViewResponse::raw(
        200,
        APPLICATION_JSON,
        page::page_with_bytes(total, &window, &next, &prev, decoded.len(), &body),
    ))
}

const LIST_FROM: &str = "FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $2 \
       AND workspaces.slug = $1 AND modules.archived_at IS NULL";

/// `ModuleListCreateAPIEndpoint.get` (`module.py:267-279`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = cycles::params(ctx)?;
        list_page(
            ctx,
            &queries::MODULE_LIST,
            LIST_FROM,
            &[&params.slug, &params.project_id],
        )
        .await
    })
}

/// `ModuleArchiveUnarchiveAPIEndpoint.get` (`module.py:1067-1079`).
pub fn archived_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = cycles::params(ctx)?;
        list_page(
            ctx,
            &queries::MODULE_ARCHIVED_LIST,
            "FROM modules \
             INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
             WHERE modules.deleted_at IS NULL AND modules.project_id = $2 \
               AND workspaces.slug = $1 AND modules.archived_at IS NOT NULL",
            &[&params.slug, &params.project_id],
        )
        .await
    })
}

// ---------------------------------------------------------------------------
// 300 GET — lite list
// ---------------------------------------------------------------------------

/// `ModuleListLiteAPIEndpoint.get` (`module.py:304-327`).
pub fn lite_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = cycles::params(ctx)?;
        let order = sanitize_order_by(ctx.query().get_or("order_by", "-created_at"));
        let (descending, bare) = match order.strip_prefix('-') {
            Some(bare) => (true, bare),
            None => (false, order.as_str()),
        };
        let direction = if descending { "DESC" } else { "ASC" };
        let sql = format!(
            "SELECT modules.id, modules.created_at, modules.updated_at, \
                 modules.deleted_at, modules.name, modules.description, \
                 modules.description_text, modules.description_html, \
                 modules.start_date, modules.target_date, modules.status, \
                 modules.view_props, modules.sort_order, \
                 modules.external_source, modules.external_id, \
                 modules.archived_at, modules.logo_props, \
                 modules.created_by_id, modules.updated_by_id, \
                 modules.project_id, modules.workspace_id, modules.lead_id \
             FROM modules \
             INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
             WHERE modules.deleted_at IS NULL AND modules.project_id = $1 \
               AND workspaces.slug = $2 AND modules.archived_at IS NULL \
             ORDER BY modules.{bare} {direction} LIMIT $3 OFFSET $4"
        );
        let client = cycles::client(ctx)?;
        let page = page::default_query(ctx)?;
        let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
        let limit = window.row_limit() + 1;
        let offset = window.offset;
        let rows = client
            .query(&sql, &[&params.project_id, &params.slug, &limit, &offset])
            .await
            .map_err(cycles::db_error)?;
        let fetched = rows.len();
        let mut decoded = Vec::with_capacity(fetched.min(window.row_limit() as usize));
        for row in rows.iter().take(window.row_limit() as usize) {
            decoded.push(ModuleRow::from_plain_row(row)?);
        }
        let body = render_modules(ctx, &decoded, true).await?;
        let total_row = client
            .query_one(
                "SELECT COUNT(*) FROM modules \
                 INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
                 WHERE modules.deleted_at IS NULL AND modules.project_id = $1 \
                   AND workspaces.slug = $2 AND modules.archived_at IS NULL",
                &[&params.project_id, &params.slug],
            )
            .await
            .map_err(cycles::db_error)?;
        let total: i64 = total_row.try_get(0).map_err(ViewError::from)?;
        let (next, prev) = window.cursors(fetched);
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            page::page_with_bytes(total, &window, &next, &prev, decoded.len(), &body),
        ))
    })
}

// ---------------------------------------------------------------------------
// 301 GET — detail
// ---------------------------------------------------------------------------

/// `ModuleDetailAPIEndpoint.get` (`module.py:519-526`).
pub fn detail<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = cycles::params(ctx)?;
        let pk = cycles::uuid_kwarg(ctx, "pk")?;
        let client = cycles::client(ctx)?;
        let prepared = queries::MODULE_DETAIL
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&prepared, &[&params.slug, &params.project_id, &pk])
            .await
            .map_err(cycles::db_error)?;
        let Some(row) = rows.first() else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let decoded = ModuleRow::from_read_row(row)?;
        let body = render_module(ctx, &decoded, false).await?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// 302 GET — module work items
// ---------------------------------------------------------------------------

/// `ModuleIssueListCreateAPIEndpoint.get` (`module.py:645-696`).
pub fn issue_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = cycles::uuid_kwarg(ctx, "project_id")?;
        let module_id = cycles::uuid_kwarg(ctx, "module_id")?;
        cycles::issue_scope_page(
            ctx,
            &slug,
            project_id,
            cycles::IssueScope::Module(module_id),
            false,
        )
        .await
    })
}

// ---------------------------------------------------------------------------
// 304 POST / 306 DELETE — archive and unarchive
// ---------------------------------------------------------------------------

/// `ModuleArchiveUnarchiveAPIEndpoint.post` (`module.py:1095-1115`).
pub fn archive_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = cycles::params(ctx)?;
        let pk = cycles::uuid_kwarg(ctx, "pk")?;
        let client = cycles::client(ctx)?;
        let prepared = queries::MODULE_ARCHIVE_LOOKUP
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&prepared, &[&pk, &params.project_id, &params.slug])
            .await
            .map_err(cycles::db_error)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let status: String = row.try_get(0).map_err(ViewError::from)?;
        if status != "completed" && status != "cancelled" {
            return Ok(cycles::error_body(
                400,
                "Only completed or cancelled modules can be archived",
            ));
        }
        let archived_at = plane_http_app::write::now();
        let updated_at = plane_http_app::write::now();
        let statement = queries::MODULE_SET_ARCHIVED
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[&archived_at, &updated_at, &Some(params.user_id), &pk],
            )
            .await
            .map_err(cycles::db_error)?;
        let statement = queries::MODULE_FAVORITE_SOFT_DELETE_WS
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &plane_http_app::write::now(),
                    &pk,
                    &params.project_id,
                    &params.slug,
                ],
            )
            .await
            .map_err(cycles::db_error)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `ModuleArchiveUnarchiveAPIEndpoint.delete` (`module.py:1129-1138`).
pub fn unarchive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = cycles::params(ctx)?;
        let pk = cycles::uuid_kwarg(ctx, "pk")?;
        let client = cycles::client(ctx)?;
        let prepared = queries::MODULE_ARCHIVE_LOOKUP
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        if client
            .query_opt(&prepared, &[&pk, &params.project_id, &params.slug])
            .await
            .map_err(cycles::db_error)?
            .is_none()
        {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let statement = queries::MODULE_CLEAR_ARCHIVED
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[&plane_http_app::write::now(), &Some(params.user_id), &pk],
            )
            .await
            .map_err(cycles::db_error)?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// 301 DELETE — soft delete
// ---------------------------------------------------------------------------

/// `ModuleDetailAPIEndpoint.delete` (`module.py:541-584`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = cycles::params(ctx)?;
        let pk = cycles::uuid_kwarg(ctx, "pk")?;
        let client = cycles::client(ctx)?;
        let prepared = queries::MODULE_ROW
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&prepared, &[&params.slug, &params.project_id, &pk])
            .await
            .map_err(cycles::db_error)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let module = ModuleRow::from_plain_row(&row)?;
        let created_by = module.created_by;
        if created_by != Some(params.user_id) {
            let prepared = queries::MODULE_PROJECT_ADMIN
                .prepare(client)
                .await
                .map_err(ViewError::from)?;
            if client
                .query_opt(
                    &prepared,
                    &[&params.slug, &params.user_id, &params.project_id],
                )
                .await
                .map_err(cycles::db_error)?
                .is_none()
            {
                return Ok(cycles::error_body(
                    403,
                    "Only admin or creator can delete the module",
                ));
            }
        }

        let prepared = queries::MODULE_ISSUE_IDS
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        let issues: Vec<Uuid> = client
            .query(&prepared, &[&pk])
            .await
            .map_err(cycles::db_error)?
            .iter()
            .map(|row| row.try_get::<_, Uuid>(0).map_err(ViewError::from))
            .collect::<Result<_, _>>()?;

        use plane_serde::pyjson::{PyDict, PyStr, Value as Py};
        let mut requested_data = PyDict::with_capacity(3);
        requested_data.insert(PyStr::new("module_id"), Py::from(pk.to_string()));
        requested_data.insert(PyStr::new("module_name"), Py::from(module.name.clone()));
        requested_data.insert(
            PyStr::new("issues"),
            Py::List(
                issues
                    .iter()
                    .map(|issue| Py::from(issue.to_string()))
                    .collect(),
            ),
        );
        let mut current = PyDict::with_capacity(1);
        current.insert(PyStr::new("module_name"), Py::from(module.name.clone()));
        let payload = Py::Dict(requested_data);
        let current = Py::Dict(current);
        let base = plane_jobs::celery::tasks::IssueActivityBase::new(
            "module.activity.deleted",
            plane_jobs::celery::args::TaskArg::json_text(crate::issues::dumps(&payload)?),
            params.user_id.to_string(),
            plane_jobs::celery::args::TaskArg::Null,
            plane_jobs::celery::args::TaskArg::str(params.project_id.to_string()),
            plane_jobs::celery::args::TaskArg::json_text(crate::issues::dumps(&current)?),
            Utc::now().timestamp(),
        );
        let call = plane_jobs::celery::tasks::IssueActivityCall::with_origin(
            base,
            cycles::base_host(ctx)?,
        );
        let message = plane_jobs::celery::tasks::issue_activity(call);
        let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
        plane_http_app::common::jobs::dispatch_task(
            ctx,
            task,
            &message,
            plane_jobs::queue::Job::new(task),
        )
        .await?;

        // `module.delete()` — H03 plus the cascade task.
        let statement = queries::MODULE_SOFT_DELETE
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &plane_http_app::write::now(),
                    &plane_http_app::write::now(),
                    &Some(params.user_id),
                    &pk,
                ],
            )
            .await
            .map_err(cycles::db_error)?;
        plane_http_app::write::publish_soft_delete_cascade(ctx, "db", "module", pk).await?;

        let statement = queries::MODULE_ISSUES_SOFT_DELETE
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[&plane_http_app::write::now(), &pk, &params.project_id],
            )
            .await
            .map_err(cycles::db_error)?;
        let statement = queries::MODULE_FAVORITE_SOFT_DELETE_WS
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &plane_http_app::write::now(),
                    &pk,
                    &params.project_id,
                    &params.slug,
                ],
            )
            .await
            .map_err(cycles::db_error)?;
        Ok(ViewResponse::empty(204))
    })
}
