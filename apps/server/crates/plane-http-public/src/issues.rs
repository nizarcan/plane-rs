// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Work items on `/api/v1/`: the list, the identifier lookup and the detail
//! read (routes 316/328, 317/329, 318/330).
//!
//! Python: `plane/api/views/issue.py:248-945`. The three views share one
//! queryset shape (`Issue.issue_objects` plus the join filters the manager's
//! `EXISTS` predicates already express), one manual serializer
//! (`plane/api/serializers/issue.py:46-353`) and one 404 rule for a restricted
//! guest (`visible_issue_q`, `issue.py:215-245`).
//!
//! ## The serializer
//!
//! `IssueSerializer` is the one v1 serializer the generated tables cannot
//! express. It is a plain `ModelSerializer` over `Issue` with
//! `exclude = ["description_json", "description_stripped"]`, two declared
//! write-only list fields (`assignees`, `labels`) and a declared `type_id`
//! aliasing `type`; `to_representation` overwrites the two list fields with
//! either the through-row ids or, under `expand=`, nested serializers. The
//! output order is DRF's `_readable_fields` order, which this module writes
//! literally (`ISSUE_FIELDS`). 29 fields:
//!
//! `id, assignees, labels, type_id, created_at, updated_at, deleted_at, point,
//! name, description_html, description_binary, priority, start_date,
//! target_date, sequence_id, sort_order, completed_at, archived_at, is_draft,
//! external_source, external_id, created_by, updated_by, project, workspace,
//! parent, state, estimate_point, type`
//!
//! `fields=` removes fields; `expand=` only replaces a field that survived.
//! For `assignees`/`labels` the expansion path is the serializer's own
//! (a `UserLiteSerializer` list and a full `LabelSerializer` list), not the
//! base class's mapper: the base class's expansion runs first and the
//! serializer overwrites it, which is observable only in ordering and in the
//! soft-deleted-member rule.
//!
//! ## Annotations that are inert on this surface
//!
//! `IssueListCreateAPIEndpoint.get` annotates `cycle_id`, `link_count` and
//! `attachment_count` (`issue.py:440-462`) and every view annotates
//! `sub_issues_count`; none of the four is a model field or a serializer field,
//! so none reaches the body. They are not computed here: a scenario cannot see
//! the difference and skipping them removes three correlated subqueries per
//! row.
//!
//! ## Ordering
//!
//! `sanitize_order_by(...ISSUE_ORDER_BY_ALLOWLIST, "-created_at")`
//! (`order_queryset.py:18-33,135-156`) then five special cases
//! (`issue.py:470-506`): `priority` and `state__group` sort through a `CASE`
//! index (`state__name` sorts on the same `CASE`, a Django quirk: the branch
//! tests `state__group` for both names), `labels__name` /
//! `assignees__first_name` sort on a `MAX()` over the through rows, and
//! everything else is a plain column. A tie has no secondary key in Python and
//! none here.
//!
//! ## Bugs ported as they are
//!
//! * `state__name` sorts by `state__group`'s ranking, not by the state's name
//!   (`issue.py:481-494`).
//! * `?external_id=&external_source=` on the list returns a **single object**
//!   from `Issue.objects` (soft delete only: drafts, triage and archived rows
//!   included), not a page (`:406-421`).
//! * `?limit=` on the list is ignored; the page size is `?per_page=`
//!   (`:386-513`).
//! * `PUT` on the detail is dead: no v1 URL binds it (`urls/work_item.py`).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::issue::Issue;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_http_app::write::{
    CharOptions, Detail, IntegerOptions, PrimaryKeyListOptions, Serializer, Validated,
    ValidationErrors,
};
use plane_query::paginator::{OffsetWindow, ceil_div};
use plane_query::sql::{Frag, Value};
use plane_serializers::generated::api_issue::LabelSerializer;
use plane_serializers::generated::api_user::UserLiteSerializer;
use plane_serializers::{
    DrfError, DrfSerialize, DrfUuid, DrfValue, DrfWriter, Expansion, FilterMode, RenderCtx,
};
use uuid::Uuid;

use crate::common::page;
use crate::common::render::RequestRender;
use crate::issues_queries as queries;

/// `IssueSerializer.Meta`'s `_readable_fields` order (`issue.py:46-92`, checked
/// against a live Django process).
pub const ISSUE_FIELDS: &[&str] = &[
    "id",
    "type_id",
    "created_at",
    "updated_at",
    "deleted_at",
    "point",
    "name",
    "description_html",
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
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "parent",
    "state",
    "estimate_point",
    "type",
    // `assignees`/`labels` are write-only declared fields, so
    // `_readable_fields` skips them and `to_representation`'s assignment
    // appends them after every model field.
    "assignees",
    "labels",
];

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.issue.WorkspaceIssueAPIEndpoint")?
        .method(http::Method::GET, workspace_get)?;
    registry
        .api_view("plane.api.views.issue.IssueListCreateAPIEndpoint")?
        .method(http::Method::GET, list_get)?
        .method(http::Method::POST, create)?;
    registry
        .api_view("plane.api.views.issue.IssueDetailAPIEndpoint")?
        .method(http::Method::GET, detail_get)?
        .method(http::Method::PATCH, patch)?
        .method(http::Method::DELETE, delete)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Data the serializer renders
// ---------------------------------------------------------------------------

/// `UserLiteSerializer` data (`plane/api/serializers/user.py:13-40`): the v1
/// user shape has `email` and no `is_bot`.
#[derive(Clone, Debug)]
pub struct UserLiteOwned {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub avatar: String,
    pub avatar_url: Option<String>,
    pub display_name: String,
}

impl UserLiteOwned {
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

/// `LabelSerializer` data (`plane/api/serializers/issue.py:401-430`).
#[derive(Clone, Debug)]
pub struct LabelOwned {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub name: String,
    pub description: String,
    pub color: String,
    pub sort_order: f64,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub workspace: Uuid,
    pub project: Option<Uuid>,
    pub parent: Option<Uuid>,
}

impl LabelOwned {
    fn serializer(&self) -> LabelSerializer<'_> {
        LabelSerializer {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            name: &self.name,
            description: &self.description,
            color: &self.color,
            sort_order: self.sort_order,
            external_source: self.external_source.as_deref(),
            external_id: self.external_id.as_deref(),
            created_by: self.created_by,
            updated_by: self.updated_by,
            workspace: self.workspace,
            project: self.project,
            parent: self.parent,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceOwned {
    pub name: String,
    pub slug: String,
    pub id: Uuid,
}

#[derive(Clone, Debug)]
pub struct ProjectOwned {
    pub id: Uuid,
    pub identifier: String,
    pub name: String,
    pub cover_image: Option<String>,
    pub icon_prop: Option<plane_db::JsonText>,
    pub emoji: Option<String>,
    pub description: String,
    pub cover_image_url: Option<String>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct StateOwned {
    pub id: Uuid,
    pub name: String,
    pub color: String,
    pub group: String,
}

#[derive(Clone, Debug)]
pub struct EstimatePointOwned {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub key: i64,
    pub description: String,
    pub value: String,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub project: Uuid,
    pub workspace: Uuid,
    pub estimate: Uuid,
}

/// `(id, sequence_id, project_id)` of the parent, for `IssueLiteSerializer`.
#[derive(Clone, Debug)]
pub struct ParentOwned {
    pub id: Uuid,
    pub sequence_id: i64,
    pub project_id: Uuid,
}

/// One issue with everything its serializer can emit.
pub(crate) struct IssueBundle {
    pub issue: Issue,
    pub assignee_ids: Vec<Uuid>,
    pub label_ids: Vec<Uuid>,
    pub assignees: Vec<UserLiteOwned>,
    pub labels: Vec<LabelOwned>,
    pub workspace: Option<WorkspaceOwned>,
    pub project: Option<ProjectOwned>,
    pub state: Option<StateOwned>,
    pub parent: Option<ParentOwned>,
    pub created_by: Option<UserLiteOwned>,
    pub updated_by: Option<UserLiteOwned>,
    pub estimate_point: Option<EstimatePointOwned>,
}

impl IssueBundle {
    fn serializer(&self) -> IssueSerializer<'_> {
        IssueSerializer { bundle: self }
    }
}

/// `plane.api.serializers.issue.IssueSerializer` (`issue.py:46-353`).
pub struct IssueSerializer<'a> {
    bundle: &'a IssueBundle,
}

impl std::fmt::Debug for IssueSerializer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssueSerializer")
            .field("id", &self.bundle.issue.id)
            .finish()
    }
}

impl DrfSerialize for IssueSerializer<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        const MODE: FilterMode = FilterMode::RemoveFields;
        let issue = &self.bundle.issue;
        let mut obj = w.begin_object();

        if ctx.emits("id", MODE) {
            obj.field_dyn("id", ctx, &DrfUuid(issue.id))?;
        }
        if ctx.emits("type_id", MODE) {
            obj.field_dyn("type_id", ctx, &issue.type_id.map(DrfUuid))?;
        }
        if ctx.emits("created_at", MODE) {
            obj.field_dyn(
                "created_at",
                ctx,
                &plane_serializers::drf_datetime(issue.created_at, ctx.tz()),
            )?;
        }
        if ctx.emits("updated_at", MODE) {
            obj.field_dyn(
                "updated_at",
                ctx,
                &plane_serializers::drf_datetime(issue.updated_at, ctx.tz()),
            )?;
        }
        if ctx.emits("deleted_at", MODE) {
            obj.field_dyn(
                "deleted_at",
                ctx,
                &plane_serializers::drf_datetime_opt(issue.deleted_at, ctx.tz()),
            )?;
        }
        if ctx.emits("point", MODE) {
            obj.field_dyn("point", ctx, &issue.point)?;
        }
        if ctx.emits("name", MODE) {
            obj.field_dyn("name", ctx, &issue.name)?;
        }
        if ctx.emits("description_html", MODE) {
            obj.field_dyn("description_html", ctx, &issue.description_html)?;
        }
        if ctx.emits("description_binary", MODE) {
            // `ModelField.to_representation` on a non-null `BinaryField` hands
            // bytes to DRF's JSON encoder, which raises (a 500). No write path
            // produces one: the field is read-only and defaults to null.
            if issue.description_binary.is_some() {
                return Err(DrfError::MissingExpansion("description_binary".to_owned()));
            }
            obj.null_field("description_binary")?;
        }
        if ctx.emits("priority", MODE) {
            obj.field_dyn("priority", ctx, &issue.priority)?;
        }
        if ctx.emits("start_date", MODE) {
            obj.field_dyn(
                "start_date",
                ctx,
                &issue.start_date.map(plane_serializers::PyDate),
            )?;
        }
        if ctx.emits("target_date", MODE) {
            obj.field_dyn(
                "target_date",
                ctx,
                &issue.target_date.map(plane_serializers::PyDate),
            )?;
        }
        if ctx.emits("sequence_id", MODE) {
            obj.field_dyn("sequence_id", ctx, &issue.sequence_id)?;
        }
        if ctx.emits("sort_order", MODE) {
            obj.field_dyn("sort_order", ctx, &issue.sort_order)?;
        }
        if ctx.emits("completed_at", MODE) {
            obj.field_dyn(
                "completed_at",
                ctx,
                &plane_serializers::drf_datetime_opt(issue.completed_at, ctx.tz()),
            )?;
        }
        if ctx.emits("archived_at", MODE) {
            obj.field_dyn(
                "archived_at",
                ctx,
                &issue.archived_at.map(plane_serializers::PyDate),
            )?;
        }
        if ctx.emits("is_draft", MODE) {
            obj.field_dyn("is_draft", ctx, &issue.is_draft)?;
        }
        if ctx.emits("external_source", MODE) {
            obj.field_dyn("external_source", ctx, &issue.external_source)?;
        }
        if ctx.emits("external_id", MODE) {
            obj.field_dyn("external_id", ctx, &issue.external_id)?;
        }
        if ctx.emits("created_by", MODE) {
            obj.field_dyn("created_by", ctx, &issue.created_by_id.map(DrfUuid))?;
        }
        if ctx.emits("updated_by", MODE) {
            obj.field_dyn("updated_by", ctx, &issue.updated_by_id.map(DrfUuid))?;
        }
        if ctx.emits("project", MODE) {
            obj.field_dyn("project", ctx, &DrfUuid(issue.project_id))?;
        }
        if ctx.emits("workspace", MODE) {
            obj.field_dyn("workspace", ctx, &DrfUuid(issue.workspace_id))?;
        }
        if ctx.emits("parent", MODE) {
            obj.field_dyn("parent", ctx, &issue.parent_id.map(DrfUuid))?;
        }
        if ctx.emits("state", MODE) {
            obj.field_dyn("state", ctx, &issue.state_id.map(DrfUuid))?;
        }
        if ctx.emits("estimate_point", MODE) {
            obj.field_dyn("estimate_point", ctx, &issue.estimate_point_id.map(DrfUuid))?;
        }
        if ctx.emits("type", MODE) {
            obj.field_dyn("type", ctx, &issue.type_id.map(DrfUuid))?;
        }
        // `to_representation` overwrites both list fields; `write_injected`
        // still requires a resolution for every emitted base field.
        if ctx.emits("assignees", MODE) {
            if ctx.expansions().is_requested("assignees") {
                let serializers: Vec<UserLiteSerializer<'_>> = self
                    .bundle
                    .assignees
                    .iter()
                    .map(UserLiteOwned::serializer)
                    .collect();
                obj.field_nested_list("assignees", ctx, &serializers)?;
            } else {
                let ids: Vec<DrfUuid> = self
                    .bundle
                    .assignee_ids
                    .iter()
                    .copied()
                    .map(DrfUuid)
                    .collect();
                obj.field_dyn("assignees", ctx, &ids)?;
            }
        }
        if ctx.emits("labels", MODE) {
            if ctx.expansions().is_requested("labels") {
                let serializers: Vec<LabelSerializer<'_>> = self
                    .bundle
                    .labels
                    .iter()
                    .map(LabelOwned::serializer)
                    .collect();
                obj.field_nested_list("labels", ctx, &serializers)?;
            } else {
                let ids: Vec<DrfUuid> =
                    self.bundle.label_ids.iter().copied().map(DrfUuid).collect();
                obj.field_dyn("labels", ctx, &ids)?;
            }
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, ISSUE_FIELDS, MODE)?;
        obj.end();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Row loading
// ---------------------------------------------------------------------------

/// Columns of one issue, alias `issues`.
pub(crate) fn issue_columns() -> String {
    Issue::COLUMNS.replace("{alias}", "issues")
}

/// `IssueManager.get_queryset` (`plane/db/models/issue.py:92-101`).
pub(crate) fn issue_objects() -> String {
    Issue::ISSUE_OBJECTS.replace("{alias}", "issues")
}

fn client<'a>(ctx: &'a ViewCtx<'a>) -> Result<&'a deadpool_postgres::Client, ViewError> {
    plane_http_app::common::client(ctx)
}

pub(crate) fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user.id().ok_or(ViewError::Api(
        plane_core::error::ApiError::NotAuthenticated,
    ))
}

fn kwarg_uuid(ctx: &ViewCtx<'_>, name: &str) -> Result<Uuid, ViewError> {
    let value = ctx.kwarg(name).ok_or(ViewError::Key)?;
    Uuid::parse_str(value).map_err(|_| ViewError::Validation)
}

/// `view.fields` applied to one name: absent `fields=` emits everything.
fn emits(ctx: &ViewCtx<'_>, name: &str) -> bool {
    ctx.fields().is_none_or(|fields| fields.contains(&name))
}

/// `ISSUE_ORDER_BY_ALLOWLIST` (`plane/utils/order_queryset.py:18-33`).
const ISSUE_ORDER_BY_ALLOWLIST: &[&str] = &[
    "created_at",
    "updated_at",
    "sequence_id",
    "sort_order",
    "target_date",
    "start_date",
    "completed_at",
    "archived_at",
    "priority",
    "state__name",
    "state__group",
    "assignees__first_name",
    "labels__name",
    "issue_module__module__name",
];

/// `sanitize_order_by(value, ISSUE_ORDER_BY_ALLOWLIST, "-created_at")`
/// (`order_queryset.py:135-156`).
pub(crate) fn sanitize_order_by(value: &str) -> String {
    if value.is_empty() {
        return "-created_at".to_owned();
    }
    let descending = value.starts_with('-');
    let bare = if descending { &value[1..] } else { value };
    if bare.starts_with('-') || !ISSUE_ORDER_BY_ALLOWLIST.contains(&bare) {
        return "-created_at".to_owned();
    }
    if descending {
        format!("-{bare}")
    } else {
        bare.to_owned()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OrderKind {
    Column,
    Priority,
    State,
    LabelName,
    AssigneeName,
    ModuleName,
}

fn order_kind(bare: &str) -> OrderKind {
    match bare {
        "priority" => OrderKind::Priority,
        // `state__name` takes the same branch as `state__group`
        // (`issue.py:481-494`) and sorts by the group ranking.
        "state__name" | "state__group" => OrderKind::State,
        "labels__name" => OrderKind::LabelName,
        "assignees__first_name" => OrderKind::AssigneeName,
        "issue_module__module__name" => OrderKind::ModuleName,
        _ => OrderKind::Column,
    }
}

/// The `ORDER BY` term for a sanitized `order_by` value.
pub(crate) fn order_sql(order: &str) -> String {
    let (descending, bare) = match order.strip_prefix('-') {
        Some(bare) => (true, bare),
        None => (false, order),
    };
    let direction = if descending { "DESC" } else { "ASC" };
    match order_kind(bare) {
        OrderKind::Column => format!("issues.{bare} {direction}"),
        OrderKind::Priority => {
            let mut values = ["urgent", "high", "medium", "low", "none"];
            if descending {
                values.reverse();
            }
            let mut sql = String::from("CASE issues.priority");
            for (index, value) in values.iter().enumerate() {
                sql.push_str(&format!(" WHEN '{value}' THEN {index}"));
            }
            sql.push_str(" ELSE 5 END ASC");
            sql
        }
        OrderKind::State => {
            let mut values = ["backlog", "unstarted", "started", "completed", "cancelled"];
            if descending {
                values.reverse();
            }
            let mut sql = String::from(
                "CASE (SELECT states.\"group\" FROM states WHERE states.id = issues.state_id)",
            );
            for (index, value) in values.iter().enumerate() {
                sql.push_str(&format!(" WHEN '{value}' THEN {index}"));
            }
            sql.push_str(" ELSE 5 END ASC");
            sql
        }
        // `annotate(max_values=Max(...)).order_by("max_values")`
        // (`issue.py:496-504`): the join is not restricted to live through
        // rows, exactly like Django's related-field join.
        OrderKind::LabelName => format!(
            "(SELECT MAX(labels.name) FROM issue_labels \
             INNER JOIN labels ON labels.id = issue_labels.label_id \
             WHERE issue_labels.issue_id = issues.id) {direction}"
        ),
        OrderKind::AssigneeName => format!(
            "(SELECT MAX(users.first_name) FROM issue_assignees \
             INNER JOIN users ON users.id = issue_assignees.assignee_id \
             WHERE issue_assignees.issue_id = issues.id) {direction}"
        ),
        OrderKind::ModuleName => format!("modules.name {direction}"),
    }
}

/// Whether the sanitized order needs the module join (`issue.py:505-506`).
pub(crate) fn needs_module_join(order: &str) -> bool {
    let bare = order.strip_prefix('-').unwrap_or(order);
    order_kind(bare) == OrderKind::ModuleName
}

/// Build the shared `SELECT` of the three issue views.
///
/// The manager's predicates use `EXISTS`, so the only joins needed are the ones
/// the URL filters name (`workspaces` always, `projects` for the identifier
/// lookup) plus the ordering's `issue_modules`/`modules`.
fn issue_select(slug: &str, filters: Frag) -> Frag {
    let mut sql = Frag::raw(format!(
        "SELECT {} FROM issues \
         INNER JOIN workspaces ON issues.workspace_id = workspaces.id ",
        issue_columns()
    ));
    sql.push_raw("WHERE ");
    sql.push_raw(&issue_objects());
    sql.push_raw(" AND workspaces.slug = ");
    sql.push(Frag::bind(Value::Text(slug.to_owned())));
    sql.push_raw(" AND ");
    sql.push(filters);
    sql
}

fn decode_issue(row: &tokio_postgres::Row) -> Result<IssueBundle, ViewError> {
    Ok(IssueBundle {
        issue: Issue::from_row(row, 0)?,
        assignee_ids: Vec::new(),
        label_ids: Vec::new(),
        assignees: Vec::new(),
        labels: Vec::new(),
        workspace: None,
        project: None,
        state: None,
        parent: None,
        created_by: None,
        updated_by: None,
        estimate_point: None,
    })
}

/// Run the statement and attach every relation the serializer can emit.
pub(crate) async fn load_bundles(
    ctx: &ViewCtx<'_>,
    client: &Client,
    sql: Frag,
) -> Result<Vec<IssueBundle>, ViewError> {
    let rows = sql
        .into_sql()
        .query(client)
        .await
        .map_err(ViewError::from)?;
    let mut bundles: Vec<IssueBundle> = rows.iter().map(decode_issue).collect::<Result<_, _>>()?;
    if bundles.is_empty() {
        return Ok(bundles);
    }
    let ids: Vec<Uuid> = bundles.iter().map(|bundle| bundle.issue.id).collect();

    // `to_representation`'s two list fields (`issue.py:323-353`): read-only
    // when `fields=` removed them.
    let want_assignees = emits(ctx, "assignees");
    let want_labels = emits(ctx, "labels");
    if want_assignees {
        load_assignee_ids(client, &ids, &mut bundles).await?;
    }
    if want_labels {
        load_label_ids(client, &ids, &mut bundles).await?;
    }

    let expand = ctx.expand().unwrap_or_default();
    let requested = |name: &str| emits(ctx, name) && expand.contains(&name);
    if requested("assignees") && want_assignees {
        load_assignee_objects(client, &ids, &mut bundles).await?;
    }
    if requested("labels") && want_labels {
        load_label_objects(client, &ids, &mut bundles).await?;
    }

    // The base `expand=` mapper (`plane/api/serializers/base.py:98-136`).
    if requested("workspace") {
        let wanted = unique(bundles.iter().map(|bundle| bundle.issue.workspace_id));
        let rows = load_workspaces(client, &wanted).await?;
        for bundle in &mut bundles {
            bundle.workspace = rows.get(&bundle.issue.workspace_id).cloned();
        }
    }
    if requested("project") {
        let wanted = unique(bundles.iter().map(|bundle| bundle.issue.project_id));
        let rows = load_projects(client, &wanted).await?;
        for bundle in &mut bundles {
            bundle.project = rows.get(&bundle.issue.project_id).cloned();
        }
    }
    if requested("state") {
        let wanted = unique(bundles.iter().filter_map(|bundle| bundle.issue.state_id));
        let rows = load_states(client, &wanted).await?;
        for bundle in &mut bundles {
            bundle.state = bundle.issue.state_id.and_then(|id| rows.get(&id).cloned());
        }
    }
    if requested("parent") {
        let wanted = unique(bundles.iter().filter_map(|bundle| bundle.issue.parent_id));
        let rows = load_parents(client, &wanted).await?;
        for bundle in &mut bundles {
            bundle.parent = bundle.issue.parent_id.and_then(|id| rows.get(&id).cloned());
        }
    }
    if requested("created_by") {
        let wanted = unique(
            bundles
                .iter()
                .filter_map(|bundle| bundle.issue.created_by_id),
        );
        let rows = load_users(client, &wanted).await?;
        for bundle in &mut bundles {
            bundle.created_by = bundle
                .issue
                .created_by_id
                .and_then(|id| rows.get(&id).cloned());
        }
    }
    if requested("updated_by") {
        let wanted = unique(
            bundles
                .iter()
                .filter_map(|bundle| bundle.issue.updated_by_id),
        );
        let rows = load_users(client, &wanted).await?;
        for bundle in &mut bundles {
            bundle.updated_by = bundle
                .issue
                .updated_by_id
                .and_then(|id| rows.get(&id).cloned());
        }
    }
    if requested("estimate_point") {
        let wanted = unique(
            bundles
                .iter()
                .filter_map(|bundle| bundle.issue.estimate_point_id),
        );
        let rows = load_estimate_points(client, &wanted).await?;
        for bundle in &mut bundles {
            bundle.estimate_point = bundle
                .issue
                .estimate_point_id
                .and_then(|id| rows.get(&id).cloned());
        }
    }
    Ok(bundles)
}

fn unique(values: impl Iterator<Item = Uuid>) -> Vec<Uuid> {
    let mut out: Vec<Uuid> = values.collect();
    out.sort_unstable();
    out.dedup();
    out
}

async fn load_assignee_ids(
    client: &Client,
    ids: &[Uuid],
    bundles: &mut [IssueBundle],
) -> Result<(), ViewError> {
    // `IssueAssignee.Meta.ordering = ("-created_at",)` and the through model's
    // soft-deletion manager (`plane/db/models/issue.py:345-366`).
    let rows = client
        .query(
            "SELECT issue_id, assignee_id FROM issue_assignees \
             WHERE issue_id = ANY($1) AND deleted_at IS NULL \
             ORDER BY issue_id, created_at DESC",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    for row in &rows {
        let issue_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        let assignee_id: Uuid = row.try_get(1).map_err(ViewError::from)?;
        if let Some(bundle) = bundles
            .iter_mut()
            .find(|bundle| bundle.issue.id == issue_id)
        {
            bundle.assignee_ids.push(assignee_id);
        }
    }
    Ok(())
}

async fn load_label_ids(
    client: &Client,
    ids: &[Uuid],
    bundles: &mut [IssueBundle],
) -> Result<(), ViewError> {
    let rows = client
        .query(
            "SELECT issue_id, label_id FROM issue_labels \
             WHERE issue_id = ANY($1) AND deleted_at IS NULL \
             ORDER BY issue_id, created_at DESC",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    for row in &rows {
        let issue_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        let label_id: Uuid = row.try_get(1).map_err(ViewError::from)?;
        if let Some(bundle) = bundles
            .iter_mut()
            .find(|bundle| bundle.issue.id == issue_id)
        {
            bundle.label_ids.push(label_id);
        }
    }
    Ok(())
}

async fn load_assignee_objects(
    client: &Client,
    ids: &[Uuid],
    bundles: &mut [IssueBundle],
) -> Result<(), ViewError> {
    // `User.objects.filter(pk__in=…).order_by("-created_at")`
    // (`issue.py:329-334`, `User.Meta.ordering`).
    let rows = client
        .query(
            "SELECT issue_assignees.issue_id, users.id, users.first_name, \
                    users.last_name, users.email, users.avatar, users.display_name, \
                    assets.id, assets.entity_type, assets.project_id, \
                    assets.issue_id, assets_ws.slug \
             FROM issue_assignees \
             INNER JOIN users ON users.id = issue_assignees.assignee_id \
             LEFT JOIN file_assets assets ON assets.id = users.avatar_asset_id \
             LEFT JOIN workspaces assets_ws ON assets_ws.id = assets.workspace_id \
             WHERE issue_assignees.issue_id = ANY($1) \
               AND issue_assignees.deleted_at IS NULL \
             ORDER BY issue_assignees.issue_id, users.created_at DESC",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    for row in &rows {
        let issue_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        let user = UserLiteOwned::from_row(row, 1)?.ok_or_else(shape_error)?;
        if let Some(bundle) = bundles
            .iter_mut()
            .find(|bundle| bundle.issue.id == issue_id)
        {
            bundle.assignees.push(user);
        }
    }
    Ok(())
}

async fn load_label_objects(
    client: &Client,
    ids: &[Uuid],
    bundles: &mut [IssueBundle],
) -> Result<(), ViewError> {
    let rows = client
        .query(
            "SELECT issue_labels.issue_id, labels.id, labels.created_at, \
                    labels.updated_at, labels.deleted_at, labels.name, \
                    labels.description, labels.color, labels.sort_order, \
                    labels.external_source, labels.external_id, \
                    labels.created_by_id, labels.updated_by_id, \
                    labels.workspace_id, labels.project_id, labels.parent_id \
             FROM issue_labels \
             INNER JOIN labels ON labels.id = issue_labels.label_id \
             WHERE issue_labels.issue_id = ANY($1) \
               AND issue_labels.deleted_at IS NULL AND labels.deleted_at IS NULL \
             ORDER BY issue_labels.issue_id, labels.created_at DESC",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    for row in &rows {
        let issue_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        let label = LabelOwned::from_row(row, 1)?;
        if let Some(bundle) = bundles
            .iter_mut()
            .find(|bundle| bundle.issue.id == issue_id)
        {
            bundle.labels.push(label);
        }
    }
    Ok(())
}

impl UserLiteOwned {
    /// Decode the 11 columns of [`load_users`]; `None` when the id is NULL.
    fn from_row(row: &tokio_postgres::Row, start: usize) -> Result<Option<Self>, ViewError> {
        let Some(id) = row
            .try_get::<_, Option<Uuid>>(start)
            .map_err(ViewError::from)?
        else {
            return Ok(None);
        };
        let avatar: String = row.try_get(start + 4).map_err(ViewError::from)?;
        let entity_type: Option<String> = row.try_get(start + 7).map_err(ViewError::from)?;
        let asset_id: Option<Uuid> = row.try_get(start + 6).map_err(ViewError::from)?;
        let project_id: Option<Uuid> = row.try_get(start + 8).map_err(ViewError::from)?;
        let issue_id: Option<Uuid> = row.try_get(start + 9).map_err(ViewError::from)?;
        let workspace_slug: Option<String> = row.try_get(start + 10).map_err(ViewError::from)?;
        let avatar_url = match (asset_id, entity_type.as_deref()) {
            (Some(asset_id), Some(entity_type)) => asset_url(
                entity_type,
                project_id,
                issue_id,
                workspace_slug.as_deref(),
                asset_id,
            ),
            _ if !avatar.is_empty() => Some(avatar.clone()),
            _ => None,
        };
        Ok(Some(Self {
            id,
            first_name: row.try_get(start + 1).map_err(ViewError::from)?,
            last_name: row.try_get(start + 2).map_err(ViewError::from)?,
            email: row.try_get(start + 3).map_err(ViewError::from)?,
            avatar,
            avatar_url,
            display_name: row.try_get(start + 5).map_err(ViewError::from)?,
        }))
    }
}

/// `FileAsset.asset_url` (`plane/db/models/asset.py:84-104`).
fn asset_url(
    entity_type: &str,
    project_id: Option<Uuid>,
    issue_id: Option<Uuid>,
    workspace_slug: Option<&str>,
    asset_id: Uuid,
) -> Option<String> {
    match entity_type {
        "WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER" => {
            Some(format!("/api/assets/v2/static/{asset_id}/"))
        }
        "ISSUE_ATTACHMENT" => Some(format!(
            "/api/assets/v2/workspaces/{}/projects/{}/issues/{}/attachments/{}/",
            workspace_slug.unwrap_or_default(),
            project_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "None".to_owned()),
            issue_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "None".to_owned()),
            asset_id
        )),
        "ISSUE_DESCRIPTION"
        | "COMMENT_DESCRIPTION"
        | "PAGE_DESCRIPTION"
        | "DRAFT_ISSUE_DESCRIPTION" => Some(format!(
            "/api/assets/v2/workspaces/{}/projects/{}/{}",
            workspace_slug.unwrap_or_default(),
            project_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "None".to_owned()),
            asset_id
        )),
        _ => None,
    }
}

impl LabelOwned {
    fn from_row(row: &tokio_postgres::Row, start: usize) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(start).map_err(ViewError::from)?,
            created_at: row.try_get(start + 1).map_err(ViewError::from)?,
            updated_at: row.try_get(start + 2).map_err(ViewError::from)?,
            deleted_at: row.try_get(start + 3).map_err(ViewError::from)?,
            name: row.try_get(start + 4).map_err(ViewError::from)?,
            description: row.try_get(start + 5).map_err(ViewError::from)?,
            color: row.try_get(start + 6).map_err(ViewError::from)?,
            sort_order: row.try_get(start + 7).map_err(ViewError::from)?,
            external_source: row.try_get(start + 8).map_err(ViewError::from)?,
            external_id: row.try_get(start + 9).map_err(ViewError::from)?,
            created_by: row.try_get(start + 10).map_err(ViewError::from)?,
            updated_by: row.try_get(start + 11).map_err(ViewError::from)?,
            workspace: row.try_get(start + 12).map_err(ViewError::from)?,
            project: row.try_get(start + 13).map_err(ViewError::from)?,
            parent: row.try_get(start + 14).map_err(ViewError::from)?,
        })
    }
}

pub(crate) async fn load_users(
    client: &Client,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, UserLiteOwned>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = client
        .query(
            "SELECT users.id, users.first_name, users.last_name, users.email, \
                    users.avatar, users.display_name, assets.id, assets.entity_type, \
                    assets.project_id, assets.issue_id, assets_ws.slug \
             FROM users \
             LEFT JOIN file_assets assets ON assets.id = users.avatar_asset_id \
             LEFT JOIN workspaces assets_ws ON assets_ws.id = assets.workspace_id \
             WHERE users.id = ANY($1)",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    let mut out = HashMap::new();
    for row in &rows {
        if let Some(user) = UserLiteOwned::from_row(row, 0)? {
            out.insert(user.id, user);
        }
    }
    Ok(out)
}

pub(crate) async fn load_workspaces(
    client: &Client,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, WorkspaceOwned>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = client
        .query(
            "SELECT id, name, slug FROM workspaces WHERE id = ANY($1) AND deleted_at IS NULL",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    let mut out = HashMap::new();
    for row in &rows {
        let id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        out.insert(
            id,
            WorkspaceOwned {
                id,
                name: row.try_get(1).map_err(ViewError::from)?,
                slug: row.try_get(2).map_err(ViewError::from)?,
            },
        );
    }
    Ok(out)
}

pub(crate) async fn load_projects(
    client: &Client,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, ProjectOwned>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = client
        .query(
            "SELECT projects.id, projects.identifier, projects.name, \
                    projects.cover_image, projects.icon_prop, projects.emoji, \
                    projects.description, assets.id, assets.entity_type, \
                    assets.project_id, assets.issue_id, assets_ws.slug, \
                    projects.archived_at \
             FROM projects \
             LEFT JOIN file_assets assets ON assets.id = projects.cover_image_asset_id \
             LEFT JOIN workspaces assets_ws ON assets_ws.id = assets.workspace_id \
             WHERE projects.id = ANY($1) AND projects.deleted_at IS NULL",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    let mut out = HashMap::new();
    for row in &rows {
        let id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        let cover_image: Option<String> = row.try_get(3).map_err(ViewError::from)?;
        let asset_id: Option<Uuid> = row.try_get(7).map_err(ViewError::from)?;
        let entity_type: Option<String> = row.try_get(8).map_err(ViewError::from)?;
        let asset_project: Option<Uuid> = row.try_get(9).map_err(ViewError::from)?;
        let asset_issue: Option<Uuid> = row.try_get(10).map_err(ViewError::from)?;
        let workspace_slug: Option<String> = row.try_get(11).map_err(ViewError::from)?;
        let cover_image_url = match (asset_id, entity_type.as_deref()) {
            (Some(asset_id), Some(entity_type)) => asset_url(
                entity_type,
                asset_project,
                asset_issue,
                workspace_slug.as_deref(),
                asset_id,
            ),
            _ => cover_image.clone(),
        };
        out.insert(
            id,
            ProjectOwned {
                id,
                identifier: row.try_get(1).map_err(ViewError::from)?,
                name: row.try_get(2).map_err(ViewError::from)?,
                cover_image,
                icon_prop: row.try_get(4).map_err(ViewError::from)?,
                emoji: row.try_get(5).map_err(ViewError::from)?,
                description: row.try_get(6).map_err(ViewError::from)?,
                cover_image_url,
                archived_at: row.try_get(12).map_err(ViewError::from)?,
            },
        );
    }
    Ok(out)
}

async fn load_states(
    client: &Client,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, StateOwned>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = client
        .query(
            "SELECT id, name, color, \"group\" FROM states \
             WHERE id = ANY($1) AND deleted_at IS NULL",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    let mut out = HashMap::new();
    for row in &rows {
        let id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        out.insert(
            id,
            StateOwned {
                id,
                name: row.try_get(1).map_err(ViewError::from)?,
                color: row.try_get(2).map_err(ViewError::from)?,
                group: row.try_get(3).map_err(ViewError::from)?,
            },
        );
    }
    Ok(out)
}

async fn load_parents(
    client: &Client,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, ParentOwned>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = client
        .query(
            "SELECT id, sequence_id, project_id FROM issues WHERE id = ANY($1)",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    let mut out = HashMap::new();
    for row in &rows {
        let id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        out.insert(
            id,
            ParentOwned {
                id,
                sequence_id: row.try_get(1).map_err(ViewError::from)?,
                project_id: row.try_get(2).map_err(ViewError::from)?,
            },
        );
    }
    Ok(out)
}

async fn load_estimate_points(
    client: &Client,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, EstimatePointOwned>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = client
        .query(
            // `key` is an `IntegerField` (int4); [`EstimatePointOwned`] stores
            // the value as i64 because the serializer renders it as a number.
            "SELECT id, created_at, updated_at, deleted_at, key::bigint, \
                    description, value, created_by_id, updated_by_id, \
                    project_id, workspace_id, estimate_id \
             FROM estimate_points WHERE id = ANY($1) AND deleted_at IS NULL",
            &[&ids],
        )
        .await
        .map_err(ViewError::from)?;
    let mut out = HashMap::new();
    for row in &rows {
        let id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        out.insert(
            id,
            EstimatePointOwned {
                id,
                created_at: row.try_get(1).map_err(ViewError::from)?,
                updated_at: row.try_get(2).map_err(ViewError::from)?,
                deleted_at: row.try_get(3).map_err(ViewError::from)?,
                key: row.try_get(4).map_err(ViewError::from)?,
                description: row.try_get(5).map_err(ViewError::from)?,
                value: row.try_get(6).map_err(ViewError::from)?,
                created_by: row.try_get(7).map_err(ViewError::from)?,
                updated_by: row.try_get(8).map_err(ViewError::from)?,
                project: row.try_get(9).map_err(ViewError::from)?,
                workspace: row.try_get(10).map_err(ViewError::from)?,
                estimate: row.try_get(11).map_err(ViewError::from)?,
            },
        );
    }
    Ok(out)
}

fn shape_error() -> ViewError {
    ViewError::Unknown("unexpected NULL in a joined row".to_owned())
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The `expand=` values one issue's serializer needs (`base.py:98-139`).
///
/// Every requested name that is a base field must be resolved, because
/// `Expansions::write_injected` refuses an emitted base field without one; a
/// name that is not in v1's mapper becomes `getattr(instance, "<name>_id",
/// None)`, i.e. `null` for all of this serializer's fields.
fn expansion_values<'a>(bundle: &'a IssueBundle, requested: &[&'a str]) -> Vec<Expansion<'a>> {
    let mut values = Vec::with_capacity(requested.len());
    for &name in requested {
        if !ISSUE_FIELDS.contains(&name) {
            continue;
        }
        let value = match name {
            "workspace" => bundle
                .workspace
                .as_ref()
                .map_or(DrfValue::Null, |workspace| {
                    DrfValue::Object(vec![
                        ("name", DrfValue::Str(&workspace.name)),
                        ("slug", DrfValue::Str(&workspace.slug)),
                        ("id", DrfValue::Uuid(workspace.id)),
                    ])
                }),
            "project" => bundle.project.as_ref().map_or(DrfValue::Null, |project| {
                DrfValue::Object(vec![
                    ("id", DrfValue::Uuid(project.id)),
                    ("identifier", DrfValue::Str(&project.identifier)),
                    ("name", DrfValue::Str(&project.name)),
                    (
                        "cover_image",
                        project
                            .cover_image
                            .as_deref()
                            .map_or(DrfValue::Null, DrfValue::Str),
                    ),
                    (
                        "icon_prop",
                        project
                            .icon_prop
                            .as_ref()
                            .map_or(DrfValue::Null, |json| DrfValue::JsonText(json.as_str())),
                    ),
                    (
                        "emoji",
                        project
                            .emoji
                            .as_deref()
                            .map_or(DrfValue::Null, DrfValue::Str),
                    ),
                    ("description", DrfValue::Str(&project.description)),
                    (
                        "cover_image_url",
                        project
                            .cover_image_url
                            .as_deref()
                            .map_or(DrfValue::Null, DrfValue::Str),
                    ),
                    (
                        "archived_at",
                        project
                            .archived_at
                            .map_or(DrfValue::Null, DrfValue::DateTime),
                    ),
                ])
            }),
            "state" => bundle.state.as_ref().map_or(DrfValue::Null, |state| {
                DrfValue::Object(vec![
                    ("id", DrfValue::Uuid(state.id)),
                    ("name", DrfValue::Str(&state.name)),
                    ("color", DrfValue::Str(&state.color)),
                    ("group", DrfValue::Str(&state.group)),
                ])
            }),
            "parent" => bundle.parent.as_ref().map_or(DrfValue::Null, |parent| {
                DrfValue::Object(vec![
                    ("id", DrfValue::Uuid(parent.id)),
                    ("sequence_id", DrfValue::Int(parent.sequence_id)),
                    ("project_id", DrfValue::Uuid(parent.project_id)),
                ])
            }),
            "created_by" => bundle
                .created_by
                .as_ref()
                .map_or(DrfValue::Null, user_value),
            "updated_by" => bundle
                .updated_by
                .as_ref()
                .map_or(DrfValue::Null, user_value),
            "estimate_point" => bundle
                .estimate_point
                .as_ref()
                .map_or(DrfValue::Null, |point| {
                    DrfValue::Object(vec![
                        ("id", DrfValue::Uuid(point.id)),
                        ("created_at", DrfValue::DateTime(point.created_at)),
                        ("updated_at", DrfValue::DateTime(point.updated_at)),
                        (
                            "deleted_at",
                            point.deleted_at.map_or(DrfValue::Null, DrfValue::DateTime),
                        ),
                        ("key", DrfValue::Int(point.key)),
                        ("description", DrfValue::Str(&point.description)),
                        ("value", DrfValue::Str(&point.value)),
                        (
                            "created_by",
                            point.created_by.map_or(DrfValue::Null, DrfValue::Uuid),
                        ),
                        (
                            "updated_by",
                            point.updated_by.map_or(DrfValue::Null, DrfValue::Uuid),
                        ),
                        ("project", DrfValue::Uuid(point.project)),
                        ("workspace", DrfValue::Uuid(point.workspace)),
                        ("estimate", DrfValue::Uuid(point.estimate)),
                    ])
                }),
            // Not in the 14-name mapper: `getattr(instance, "<name>_id",
            // None)` (`base.py:139`). `type` is the one ISSUE_FIELDS name with
            // such an attribute (`type_id`); every other name is `None`.
            "type" => bundle.issue.type_id.map_or(DrfValue::Null, DrfValue::Uuid),
            // Handled by the serializer itself; the value only satisfies the
            // emitted-base-field check.
            _ => DrfValue::Null,
        };
        values.push(Expansion { name, value });
    }
    values
}

pub(crate) fn user_value(user: &UserLiteOwned) -> DrfValue<'_> {
    DrfValue::Object(vec![
        ("id", DrfValue::Uuid(user.id)),
        ("first_name", DrfValue::Str(&user.first_name)),
        ("last_name", DrfValue::Str(&user.last_name)),
        (
            "email",
            user.email.as_deref().map_or(DrfValue::Null, DrfValue::Str),
        ),
        ("avatar", DrfValue::Str(&user.avatar)),
        (
            "avatar_url",
            user.avatar_url
                .as_deref()
                .map_or(DrfValue::Null, DrfValue::Str),
        ),
        ("display_name", DrfValue::Str(&user.display_name)),
    ])
}

/// Render `IssueSerializer(bundle, fields=…, expand=…)`.
fn render_issue(ctx: &ViewCtx<'_>, bundle: &IssueBundle) -> Result<Vec<u8>, ViewError> {
    let prepared = RequestRender::from_ctx(ctx);
    let expand = ctx.expand().unwrap_or_default();
    let requested: Vec<&str> = expand
        .iter()
        .copied()
        .filter(|name| emits(ctx, name))
        .collect();
    let values = expansion_values(bundle, &requested);
    let expansions = plane_serializers::Expansions::new(&requested, &values);
    let render_ctx = prepared.render_ctx().with_expansions(expansions);
    plane_serializers::render(&bundle.serializer(), &render_ctx).map_err(ViewError::from)
}

pub(crate) fn render_issues(
    ctx: &ViewCtx<'_>,
    bundles: &[IssueBundle],
) -> Result<Vec<u8>, ViewError> {
    let mut body = Vec::with_capacity(bundles.len() * 512 + 2);
    body.push(b'[');
    for (index, bundle) in bundles.iter().enumerate() {
        if index != 0 {
            body.push(b',');
        }
        body.extend(render_issue(ctx, bundle)?);
    }
    body.push(b']');
    Ok(body)
}

/// The plain-page envelope with an already-rendered `results` array
/// (`paginator.py:733-755`).
pub(crate) fn render_page(
    total: i64,
    window: &OffsetWindow,
    next: &plane_query::paginator::Cursor,
    prev: &plane_query::paginator::Cursor,
    count: usize,
    results: &[u8],
) -> Vec<u8> {
    let total_pages = ceil_div(total, window.limit);
    let head = format!(
        "{{\"grouped_by\":null,\"sub_grouped_by\":null,\"total_count\":{total},\"next_cursor\":\"{next}\",\"prev_cursor\":\"{prev}\",\"next_page_results\":{},\"prev_page_results\":{},\"count\":{count},\"total_pages\":{total_pages},\"total_results\":{total},\"extra_stats\":null,\"results\":",
        next.has_results, prev.has_results,
    );
    let mut body = head.into_bytes();
    body.extend_from_slice(results);
    body.push(b'}');
    body
}

// ---------------------------------------------------------------------------
// 316/328 — GET by project identifier and sequence id
// ---------------------------------------------------------------------------

/// `WorkspaceIssueAPIEndpoint.get` (`issue.py:302-328`).
pub fn workspace_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_identifier = ctx
            .kwarg("project_identifier")
            .ok_or(ViewError::Key)?
            .to_owned();
        let issue_identifier = ctx
            .kwarg("issue_identifier")
            .ok_or(ViewError::Key)?
            .to_owned();

        // `issue_identifier.isdecimal()` (`issue.py:311-312`).
        if !issue_identifier.chars().all(|ch| ch.is_ascii_digit()) {
            return Ok(not_found_work_item());
        }

        let sequence_id: i32 = issue_identifier
            .parse()
            .map_err(|_| ViewError::Unknown("int overflow in sequence_id".to_owned()))?;
        let mut filters = Frag::raw("projects.identifier = ");
        filters.push(Frag::bind(Value::Text(project_identifier)));
        filters.push_raw(" AND issues.sequence_id = ");
        filters.push(Frag::bind(Value::Int(sequence_id)));

        let mut sql = Frag::raw(format!(
            "SELECT {} FROM issues \
             INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
             INNER JOIN projects ON issues.project_id = projects.id \
             WHERE ",
            issue_columns()
        ));
        sql.push_raw(&issue_objects());
        sql.push_raw(" AND workspaces.slug = ");
        sql.push(Frag::bind(Value::Text(slug.clone())));
        sql.push_raw(" AND ");
        sql.push(filters);

        let client = client(ctx)?;
        let bundles = load_bundles(ctx, client, sql).await?;
        let Some(bundle) = bundles.first() else {
            return Ok(not_found_work_item());
        };
        if guest_cannot_view(ctx, client, &slug, &bundle.issue).await? {
            return Ok(not_found_work_item());
        }
        let body = render_issue(ctx, bundle)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `Response({"error": "Work item not found"}, 404)` (`issue.py:312`).
fn not_found_work_item() -> ViewResponse {
    ViewResponse::raw(
        404,
        APPLICATION_JSON,
        br#"{"error":"Work item not found"}"#.to_vec(),
    )
}

/// `guest_cannot_view_issue` (`issue.py:215-219`).
pub(crate) async fn guest_cannot_view(
    ctx: &ViewCtx<'_>,
    client: &Client,
    slug: &str,
    issue: &Issue,
) -> Result<bool, ViewError> {
    let user = user_id(ctx)?;
    if issue.created_by_id == Some(user) {
        return Ok(false);
    }
    let row = client
        .query_opt(
            "SELECT 1 FROM project_members \
             INNER JOIN projects ON projects.id = project_members.project_id \
             INNER JOIN workspaces ON workspaces.id = projects.workspace_id \
             WHERE workspaces.slug = $1 AND project_members.project_id = $2 \
               AND project_members.member_id = $3 AND project_members.is_active \
               AND project_members.deleted_at IS NULL \
               AND project_members.role = 5 AND projects.guest_view_all_features = false \
             LIMIT 1",
            &[&slug, &issue.project_id, &user],
        )
        .await
        .map_err(ViewError::from)?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// 317/329 — list and create
// ---------------------------------------------------------------------------

/// `IssueListCreateAPIEndpoint.get` (`issue.py:385-513`).
pub fn list_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let client = client(ctx)?;

        // `pql`/`filters` (`issue.py:392-404`).
        let unsupported: Vec<&str> = ["pql", "filters"]
            .into_iter()
            .filter(|name| ctx.query().get(name).is_some_and(|value| !value.is_empty()))
            .collect();
        if !unsupported.is_empty() {
            let message = "PQL and structured filters are not supported on this Plane edition. Remove the pql/filters parameter and filter results client-side, or use a Plane edition that supports work item query filtering.";
            let body = plane_query::Json::object(vec![
                ("pql", plane_query::Json::string(message)),
                (
                    "unsupported_parameters",
                    plane_query::Json::Array(
                        unsupported
                            .iter()
                            .map(|name| plane_query::Json::string(*name))
                            .collect(),
                    ),
                ),
            ]);
            return Ok(ViewResponse::raw(400, APPLICATION_JSON, body.to_drf()));
        }

        // `?external_id=&external_source=` returns a single object from
        // `Issue.objects` (`issue.py:406-421`), before any pagination.
        let external_id = ctx.query().get("external_id").filter(|v| !v.is_empty());
        let external_source = ctx.query().get("external_source").filter(|v| !v.is_empty());
        if let (Some(external_id), Some(external_source)) = (external_id, external_source) {
            let mut sql = Frag::raw(format!(
                "SELECT {} FROM issues \
                 INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
                 WHERE issues.deleted_at IS NULL AND workspaces.slug = ",
                issue_columns()
            ));
            sql.push(Frag::bind(Value::Text(slug.clone())));
            sql.push_raw(" AND issues.project_id = ");
            sql.push(Frag::bind(Value::Uuid(project_id)));
            sql.push_raw(" AND issues.external_id = ");
            sql.push(Frag::bind(Value::Text(external_id.to_owned())));
            sql.push_raw(" AND issues.external_source = ");
            sql.push(Frag::bind(Value::Text(external_source.to_owned())));
            let bundles = load_bundles(ctx, client, sql).await?;
            let Some(bundle) = bundles.first() else {
                return Err(ViewError::ObjectDoesNotExist);
            };
            if guest_cannot_view(ctx, client, &slug, &bundle.issue).await? {
                return Err(ViewError::ObjectDoesNotExist);
            }
            let body = render_issue(ctx, bundle)?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        }

        // `is_restricted_guest` (`issue.py:201-212`).
        let user = user_id(ctx)?;
        let restricted = client
            .query_opt(
                "SELECT 1 FROM project_members \
                 INNER JOIN projects ON projects.id = project_members.project_id \
                 INNER JOIN workspaces ON workspaces.id = projects.workspace_id \
                 WHERE workspaces.slug = $1 AND project_members.project_id = $2 \
                   AND project_members.member_id = $3 AND project_members.is_active \
                   AND project_members.deleted_at IS NULL \
                   AND project_members.role = 5 AND projects.guest_view_all_features = false \
                 LIMIT 1",
                &[&slug, &project_id, &user],
            )
            .await
            .map_err(ViewError::from)?
            .is_some();

        // `sanitize_order_by` (`order_queryset.py:135-156`).
        let raw_order = ctx.query().get_or("order_by", "-created_at");
        let order = sanitize_order_by(raw_order);
        let page = page::default_query(ctx)?;
        let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);

        let mut filters = Frag::raw("issues.project_id = ");
        filters.push(Frag::bind(Value::Uuid(project_id)));
        if restricted {
            filters.push_raw(" AND issues.created_by_id = ");
            filters.push(Frag::bind(Value::Uuid(user)));
        }

        // The `issue_module__module__name` ordering joins the through table
        // and projects the module name, because Django adds the ordering
        // expression to a `SELECT DISTINCT`'s list: an issue in two modules
        // with different names then appears twice (`issue.py:505-506`).
        let module_order = needs_module_join(&order);
        let mut select = Frag::raw(format!(
            "SELECT {}{} FROM issues \
             INNER JOIN workspaces ON issues.workspace_id = workspaces.id ",
            issue_columns(),
            if module_order { ", modules.name" } else { "" }
        ));
        if module_order {
            select.push_raw(
                " INNER JOIN issue_modules ON issue_modules.issue_id = issues.id \
                  INNER JOIN modules ON modules.id = issue_modules.module_id ",
            );
        }
        select.push_raw("WHERE ");
        select.push_raw(&issue_objects());
        select.push_raw(" AND workspaces.slug = ");
        select.push(Frag::bind(Value::Text(slug.clone())));
        select.push_raw(" AND ");
        select.push(filters.clone());
        select.push_raw(" ORDER BY ");
        select.push_raw(&order_sql(&order));
        select.push_raw(" LIMIT ");
        select.push(Frag::bind(Value::Int8(window.row_limit() + 1)));
        select.push_raw(" OFFSET ");
        select.push(Frag::bind(Value::Int8(window.offset)));

        let mut bundles = load_bundles(ctx, client, select).await?;
        let fetched = bundles.len();
        bundles.truncate(window.row_limit() as usize);
        let (next, prev) = window.cursors(fetched);

        // `total_count_queryset` (`issue.py:464-468,508-513`): the
        // unannotated, unpaginated queryset, so it counts every issue a
        // restricted guest cannot see.
        let mut total_sql = Frag::raw(
            "SELECT COUNT(*) FROM issues \
             INNER JOIN workspaces ON issues.workspace_id = workspaces.id WHERE ",
        );
        total_sql.push_raw(&issue_objects());
        total_sql.push_raw(" AND workspaces.slug = ");
        total_sql.push(Frag::bind(Value::Text(slug)));
        total_sql.push_raw(" AND ");
        total_sql.push(filters);
        let total_row = total_sql
            .into_sql()
            .query(client)
            .await
            .map_err(ViewError::from)?
            .into_iter()
            .next()
            .ok_or_else(|| ViewError::Unknown("count returned no row".to_owned()))?;
        let total: i64 = total_row.try_get(0).map_err(ViewError::from)?;

        let results = render_issues(ctx, &bundles)?;
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_page(total, &window, &next, &prev, bundles.len(), &results),
        ))
    })
}

// ---------------------------------------------------------------------------
// 318/330 — detail
// ---------------------------------------------------------------------------

/// `IssueDetailAPIEndpoint.get` (`issue.py:655-673`).
pub fn detail_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let mut filters = Frag::raw("issues.project_id = ");
        filters.push(Frag::bind(Value::Uuid(project_id)));
        filters.push_raw(" AND issues.id = ");
        filters.push(Frag::bind(Value::Uuid(pk)));
        let sql = issue_select(&slug, filters);
        let client = client(ctx)?;
        let bundles = load_bundles(ctx, client, sql).await?;
        let Some(bundle) = bundles.first() else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if guest_cannot_view(ctx, client, &slug, &bundle.issue).await? {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let body = render_issue(ctx, bundle)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// Writes — shared validation (the manual `IssueSerializer`)
// ---------------------------------------------------------------------------

/// The extra `validate()` results (`plane/api/serializers/issue.py:94-182`).
#[derive(Default)]
pub(crate) struct WriteExtra {
    pub(crate) description_html: Option<String>,
    pub(crate) assignees: Option<Vec<Uuid>>,
    pub(crate) labels: Option<Vec<Uuid>>,
}

fn fk_value(validated: &Validated, late: &str, early: &str) -> Option<Option<Uuid>> {
    if validated.contains(late) {
        return Some(validated.uuid(late));
    }
    if validated.contains(early) {
        return Some(validated.uuid(early));
    }
    None
}

/// The field validators of `IssueSerializer` (`issue.py:46-92`), in DRF's
/// `self.fields` order, skipping read-only fields.
pub(crate) async fn validate_fields(
    s: &mut Serializer<'_>,
    client: &Client,
    patch: bool,
) -> Result<(), ViewError> {
    // `assignees` / `labels` are `ListField(child=PrimaryKeyRelatedField(…))`.
    let _ = s
        .primary_key_list_field(
            client,
            "assignees",
            PrimaryKeyListOptions {
                allow_null: false,
                allow_empty: true,
                min_length: None,
                max_length: None,
                query: &queries::USERS,
            },
        )
        .await?;
    let _ = s
        .primary_key_list_field(
            client,
            "labels",
            PrimaryKeyListOptions {
                allow_null: false,
                allow_empty: true,
                min_length: None,
                max_length: None,
                query: &queries::LABELS,
            },
        )
        .await?;
    let _ = s
        .primary_key_field(client, "type_id", true, &queries::ISSUE_TYPES)
        .await?;
    let _ = s.integer_field(
        "point",
        true,
        IntegerOptions {
            min_value: Some(0),
            max_value: Some(12),
        },
    );
    let _ = s.char_field_required(
        "name",
        CharOptions {
            allow_null: false,
            allow_blank: false,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    if s.missing_required("name") {
        s.push_error("name", Detail::required());
    }
    let _ = s.char_field(
        "description_html",
        CharOptions {
            allow_null: false,
            allow_blank: true,
            trim_whitespace: true,
            max_length: None,
        },
    );
    let _ = s.choice_field(
        "priority",
        false,
        false,
        &["urgent", "high", "medium", "low", "none"],
    );
    let _ = s.date_field("start_date", true, &["iso-8601"]);
    let _ = s.date_field("target_date", true, &["iso-8601"]);
    let _ = s.float_field("sort_order", false);
    if !patch {
        let _ = s.bool_field("is_draft");
    }
    let _ = s.char_field(
        "external_source",
        CharOptions {
            allow_null: true,
            allow_blank: true,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    let _ = s.char_field(
        "external_id",
        CharOptions {
            allow_null: true,
            allow_blank: true,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    let _ = s
        .primary_key_field(client, "parent", true, &queries::PARENTS)
        .await?;
    let _ = s
        .primary_key_field(client, "state", true, &queries::STATES)
        .await?;
    let _ = s
        .primary_key_field(client, "estimate_point", true, &queries::ESTIMATE_POINTS)
        .await?;
    // The model's own `type` relation is a second field with source `type`; it
    // is visited after the declared `type_id`, so it wins when both are sent.
    let _ = s
        .primary_key_field(client, "type", true, &queries::ISSUE_TYPES)
        .await?;
    Ok(())
}

/// `IssueSerializer.validate` (`issue.py:94-182`).
pub(crate) async fn validate_extra(
    client: &Client,
    validated: &Validated,
    project_id: Uuid,
    workspace_id: Uuid,
) -> Result<WriteExtra, ValidationErrors> {
    if let (Some(start), Some(target)) =
        (validated.date("start_date"), validated.date("target_date"))
        && start > target
    {
        return Err(ValidationErrors::new(
            "non_field_errors",
            Detail::new("Start date cannot exceed target date", "invalid"),
        ));
    }

    let mut extra = WriteExtra::default();
    if let Some(html) = validated.text("description_html") {
        // `html.fromstring("")` raises `ParserError: Document is empty`, so an
        // empty string is the same 400 as malformed markup.
        let normalized = match crate::common::html::normalize(html) {
            Ok(normalized) => normalized,
            Err(_) => {
                return Err(ValidationErrors::new(
                    "non_field_errors",
                    Detail::new("Invalid HTML passed", "invalid"),
                ));
            }
        };
        extra.description_html = Some(plane_http_app::write::html::sanitize(&normalized));
    }

    if let Some(ids) = validated.uuid_list("assignees") {
        let valid = filter_assignees(client, project_id, ids)
            .await
            .map_err(|error| {
                ValidationErrors::new(
                    "non_field_errors",
                    Detail::new(format!("{error:?}"), "invalid"),
                )
            })?;
        let invalid: Vec<Uuid> = ids
            .iter()
            .copied()
            .filter(|id| !valid.contains(id))
            .collect();
        if !invalid.is_empty() {
            // `f"Assignees {list(invalid_assignee_ids)} are not active members
            // of this project"` (`issue.py:136-140`); Python renders each
            // element with `repr(uuid.UUID)`. A set's order is arbitrary
            // (`PYTHONHASHSEED`); this keeps the request order.
            let rendered: Vec<String> = invalid.iter().map(|id| format!("UUID('{id}')")).collect();
            return Err(ValidationErrors::new(
                "non_field_errors",
                Detail::new(
                    format!(
                        "Assignees [{}] are not active members of this project",
                        rendered.join(", ")
                    ),
                    "invalid",
                ),
            ));
        }
        extra.assignees = Some(valid);
    }
    if let Some(ids) = validated.uuid_list("labels") {
        let valid = filter_labels(client, project_id, ids)
            .await
            .map_err(|error| {
                ValidationErrors::new(
                    "non_field_errors",
                    Detail::new(format!("{error:?}"), "invalid"),
                )
            })?;
        let invalid: Vec<Uuid> = ids
            .iter()
            .copied()
            .filter(|id| !valid.contains(id))
            .collect();
        if !invalid.is_empty() {
            let rendered: Vec<String> = invalid.iter().map(|id| format!("UUID('{id}')")).collect();
            return Err(ValidationErrors::new(
                "non_field_errors",
                Detail::new(
                    format!(
                        "Labels [{}] do not belong to this project",
                        rendered.join(", ")
                    ),
                    "invalid",
                ),
            ));
        }
        extra.labels = Some(valid);
    }

    if let Some(Some(state_id)) = fk_value(validated, "state", "state_id") {
        let row = client
            .query_opt(
                "SELECT 1 FROM states WHERE id = $1 AND project_id = $2 \
                 AND deleted_at IS NULL AND \"group\" <> 'triage' LIMIT 1",
                &[&state_id, &project_id],
            )
            .await
            .map_err(|error| {
                ValidationErrors::new(
                    "non_field_errors",
                    Detail::new(format!("{error:?}"), "invalid"),
                )
            })?;
        if row.is_none() {
            return Err(ValidationErrors::new(
                "non_field_errors",
                Detail::new("State is not valid please pass a valid state_id", "invalid"),
            ));
        }
    }
    if let Some(Some(parent_id)) = fk_value(validated, "parent", "parent_id") {
        let row = client
            .query_opt(
                "SELECT 1 FROM issues WHERE id = $1 AND workspace_id = $2 \
                 AND project_id = $3 AND deleted_at IS NULL LIMIT 1",
                &[&parent_id, &workspace_id, &project_id],
            )
            .await
            .map_err(|error| {
                ValidationErrors::new(
                    "non_field_errors",
                    Detail::new(format!("{error:?}"), "invalid"),
                )
            })?;
        if row.is_none() {
            return Err(ValidationErrors::new(
                "non_field_errors",
                Detail::new(
                    "Parent is not valid issue_id please pass a valid issue_id",
                    "invalid",
                ),
            ));
        }
    }
    if let Some(Some(estimate_id)) = fk_value(validated, "estimate_point", "estimate_point") {
        let row = client
            .query_opt(
                "SELECT 1 FROM estimate_points WHERE id = $1 AND workspace_id = $2 \
                 AND project_id = $3 AND deleted_at IS NULL LIMIT 1",
                &[&estimate_id, &workspace_id, &project_id],
            )
            .await
            .map_err(|error| {
                ValidationErrors::new(
                    "non_field_errors",
                    Detail::new(format!("{error:?}"), "invalid"),
                )
            })?;
        if row.is_none() {
            return Err(ValidationErrors::new(
                "non_field_errors",
                Detail::new(
                    "Estimate point is not valid please pass a valid estimate_point_id",
                    "invalid",
                ),
            ));
        }
    }
    Ok(extra)
}

async fn filter_assignees(
    client: &Client,
    project_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, ViewError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            "SELECT member_id FROM project_members \
             WHERE project_id = $1 AND is_active AND role >= 15 \
               AND deleted_at IS NULL AND member_id = ANY($2)",
            &[&project_id, &ids],
        )
        .await
        .map_err(ViewError::from)?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

async fn filter_labels(
    client: &Client,
    project_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, ViewError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            "SELECT id FROM labels WHERE project_id = $1 AND deleted_at IS NULL AND id = ANY($2)",
            &[&project_id, &ids],
        )
        .await
        .map_err(ViewError::from)?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

fn build_issue(validated: &Validated, project_id: Uuid, extra: &WriteExtra) -> Issue {
    let description_html = extra
        .description_html
        .clone()
        .or_else(|| validated.text("description_html").map(str::to_owned))
        .unwrap_or_else(|| plane_db::hooks::issue::DEFAULT_DESCRIPTION_HTML.to_owned());
    let now = plane_http_app::write::now();
    Issue {
        created_at: now,
        updated_at: now,
        created_by_id: None,
        updated_by_id: None,
        deleted_at: None,
        id: Uuid::new_v4(),
        project_id,
        workspace_id: Uuid::nil(),
        parent_id: fk_value(validated, "parent", "parent_id").flatten(),
        state_id: fk_value(validated, "state", "state_id").flatten(),
        point: validated.integer("point").map(|value| value as i32),
        estimate_point_id: validated.uuid("estimate_point"),
        name: validated
            .text("name")
            .expect("the validator requires name")
            .to_owned(),
        description_json: plane_db::hooks::issue::empty_json(),
        description_stripped: strip_html(&description_html),
        description_html,
        description_binary: None,
        priority: validated.text("priority").unwrap_or("none").to_owned(),
        start_date: validated.date("start_date"),
        target_date: validated.date("target_date"),
        sequence_id: 1,
        sort_order: validated.float("sort_order").unwrap_or(65_535.0),
        completed_at: None,
        archived_at: None,
        is_draft: validated.bool("is_draft").unwrap_or(false),
        external_source: validated.text("external_source").map(str::to_owned),
        external_id: validated.text("external_id").map(str::to_owned),
        type_id: fk_value(validated, "type", "type_id").flatten(),
    }
}

pub(crate) fn strip_html(html: &str) -> Option<String> {
    if html.is_empty() {
        None
    } else {
        Some(plane_db::hooks::html::strip_tags(html))
    }
}

pub(crate) fn apply_patch(issue: &mut Issue, validated: &Validated, extra: &WriteExtra) {
    if validated.contains("state") || validated.contains("state_id") {
        issue.state_id = fk_value(validated, "state", "state_id").flatten();
    }
    if validated.contains("parent") || validated.contains("parent_id") {
        issue.parent_id = fk_value(validated, "parent", "parent_id").flatten();
    }
    if validated.contains("estimate_point") {
        issue.estimate_point_id = validated.uuid("estimate_point");
    }
    if validated.contains("type") || validated.contains("type_id") {
        issue.type_id = fk_value(validated, "type", "type_id").flatten();
    }
    if validated.contains("point") {
        issue.point = validated.integer("point").map(|value| value as i32);
    }
    if let Some(name) = validated.text("name") {
        issue.name = name.to_owned();
    }
    if validated.contains("description_html") {
        issue.description_html = extra.description_html.clone().unwrap_or_default();
    }
    if let Some(priority) = validated.text("priority") {
        issue.priority = priority.to_owned();
    }
    if validated.contains("start_date") {
        issue.start_date = validated.date("start_date");
    }
    if validated.contains("target_date") {
        issue.target_date = validated.date("target_date");
    }
    if let Some(sort_order) = validated.float("sort_order") {
        issue.sort_order = sort_order;
    }
    if let Some(is_draft) = validated.bool("is_draft") {
        issue.is_draft = is_draft;
    }
    if validated.contains("external_source") {
        issue.external_source = validated.text("external_source").map(str::to_owned);
    }
    if validated.contains("external_id") {
        issue.external_id = validated.text("external_id").map(str::to_owned);
    }
}

// ---------------------------------------------------------------------------
// Celery publishers for the v1 write views
// ---------------------------------------------------------------------------

pub(crate) async fn publish_issue_activity(
    ctx: &ViewCtx<'_>,
    call: plane_jobs::celery::tasks::IssueActivityCall,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    let message = plane_jobs::celery::tasks::issue_activity(call);
    plane_http_app::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::new(task),
    )
    .await?;
    Ok(())
}

pub(crate) fn activity_base(
    activity_type: &str,
    requested_data: plane_jobs::celery::args::TaskArg,
    actor_id: Uuid,
    issue_id: &str,
    project_id: &str,
    current_instance: plane_jobs::celery::args::TaskArg,
) -> plane_jobs::celery::tasks::IssueActivityBase {
    plane_jobs::celery::tasks::IssueActivityBase::new(
        activity_type,
        requested_data,
        actor_id.to_string(),
        plane_jobs::celery::args::TaskArg::str(issue_id),
        plane_jobs::celery::args::TaskArg::str(project_id),
        current_instance,
        Utc::now().timestamp(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn publish_model_activity(
    ctx: &ViewCtx<'_>,
    model_name: &str,
    model_id: &str,
    requested_data: plane_jobs::celery::args::TaskArg,
    current_instance: Option<String>,
    actor_id: Uuid,
    slug: &str,
    origin: &str,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::MODEL_ACTIVITY;
    let message =
        plane_jobs::celery::tasks::model_activity(plane_jobs::celery::tasks::ModelActivityCall {
            model_name: model_name.to_owned(),
            model_id: model_id.to_owned(),
            requested_data,
            current_instance,
            actor_id,
            slug: slug.to_owned(),
            origin: origin.to_owned(),
        });
    plane_http_app::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::new(task),
    )
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Write helpers
// ---------------------------------------------------------------------------

/// Load one issue by id with the `Issue.objects` (soft-delete only) manager,
/// as every v1 write's pre-image lookup does.
pub(crate) async fn load_issue_any(client: &Client, pk: Uuid) -> Result<Option<Issue>, ViewError> {
    let sql = format!(
        "SELECT {} FROM issues WHERE issues.deleted_at IS NULL AND issues.id = $1",
        issue_columns()
    );
    let row = client
        .query_opt(&sql, &[&pk])
        .await
        .map_err(ViewError::from)?;
    row.map(|row| Issue::from_row(&row, 0).map_err(ViewError::from))
        .transpose()
}

/// The issue's serializer data after a write: [`load_issue_any`] plus the two
/// through-row id lists (no `expand=` on the write responses).
pub(crate) async fn bundle_after_write(
    client: &Client,
    pk: Uuid,
) -> Result<IssueBundle, ViewError> {
    let Some(issue) = load_issue_any(client, pk).await? else {
        return Err(ViewError::Unknown(
            "the written issue disappeared".to_owned(),
        ));
    };
    let mut bundle = IssueBundle {
        issue,
        assignee_ids: Vec::new(),
        label_ids: Vec::new(),
        assignees: Vec::new(),
        labels: Vec::new(),
        workspace: None,
        project: None,
        state: None,
        parent: None,
        created_by: None,
        updated_by: None,
        estimate_point: None,
    };
    let ids = [bundle.issue.id];
    load_assignee_ids(client, &ids, std::slice::from_mut(&mut bundle)).await?;
    load_label_ids(client, &ids, std::slice::from_mut(&mut bundle)).await?;
    Ok(bundle)
}

/// `json.dumps(IssueSerializer(issue).data, cls=DjangoJSONEncoder)`.
pub(crate) fn current_instance(
    ctx: &ViewCtx<'_>,
    bundle: &IssueBundle,
) -> Result<String, ViewError> {
    use plane_query::json::Json as Doc;
    let tz = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let zoned = |value: DateTime<Utc>| Doc::DateTimeZoned(value.with_timezone(&tz).fixed_offset());
    let zoned_opt = |value: Option<DateTime<Utc>>| value.map_or(Doc::Null, zoned);
    let ids = |values: &[Uuid]| Doc::Array(values.iter().copied().map(Doc::uuid).collect());
    let issue = &bundle.issue;
    let document = Doc::Object(vec![
        ("id".to_owned(), Doc::uuid(issue.id)),
        ("type_id".to_owned(), Doc::opt_uuid(issue.type_id)),
        ("created_at".to_owned(), zoned(issue.created_at)),
        ("updated_at".to_owned(), zoned(issue.updated_at)),
        ("deleted_at".to_owned(), zoned_opt(issue.deleted_at)),
        (
            "point".to_owned(),
            issue
                .point
                .map_or(Doc::Null, |point| Doc::Int(i64::from(point))),
        ),
        ("name".to_owned(), Doc::string(issue.name.as_str())),
        (
            "description_html".to_owned(),
            Doc::string(issue.description_html.as_str()),
        ),
        ("description_binary".to_owned(), Doc::Null),
        ("priority".to_owned(), Doc::string(issue.priority.as_str())),
        ("start_date".to_owned(), Doc::opt_date(issue.start_date)),
        ("target_date".to_owned(), Doc::opt_date(issue.target_date)),
        (
            "sequence_id".to_owned(),
            Doc::Int(i64::from(issue.sequence_id)),
        ),
        ("sort_order".to_owned(), Doc::Float(issue.sort_order)),
        ("completed_at".to_owned(), zoned_opt(issue.completed_at)),
        ("archived_at".to_owned(), Doc::opt_date(issue.archived_at)),
        ("is_draft".to_owned(), Doc::Bool(issue.is_draft)),
        (
            "external_source".to_owned(),
            issue
                .external_source
                .as_deref()
                .map_or(Doc::Null, Doc::string),
        ),
        (
            "external_id".to_owned(),
            issue.external_id.as_deref().map_or(Doc::Null, Doc::string),
        ),
        ("created_by".to_owned(), Doc::opt_uuid(issue.created_by_id)),
        ("updated_by".to_owned(), Doc::opt_uuid(issue.updated_by_id)),
        ("project".to_owned(), Doc::uuid(issue.project_id)),
        ("workspace".to_owned(), Doc::uuid(issue.workspace_id)),
        ("parent".to_owned(), Doc::opt_uuid(issue.parent_id)),
        ("state".to_owned(), Doc::opt_uuid(issue.state_id)),
        (
            "estimate_point".to_owned(),
            Doc::opt_uuid(issue.estimate_point_id),
        ),
        ("type".to_owned(), Doc::opt_uuid(issue.type_id)),
        ("assignees".to_owned(), ids(&bundle.assignee_ids)),
        ("labels".to_owned(), ids(&bundle.label_ids)),
    ]);
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, &document)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `request.data` without the serializer's read-only keys, in request order
/// (`writable_data(serializer, request.data)`).
fn writable_data(ctx: &ViewCtx<'_>, patch: bool) -> Result<plane_serde::pyjson::Value, ViewError> {
    use plane_serde::pyjson::{PyDict, Value as PValue};
    const READ_ONLY: [&str; 9] = [
        "id",
        "workspace",
        "project",
        "created_by",
        "updated_by",
        "updated_at",
        "completed_at",
        "deleted_at",
        "archived_at",
    ];
    let value = plane_http_app::write::request_data_value(ctx)?;
    match &value {
        PValue::Dict(object) => {
            let mut out = PyDict::new();
            for (key, value) in object.iter() {
                let name = key.as_str().unwrap_or_default();
                if READ_ONLY.contains(&name)
                    || name == "sequence_id"
                    || (patch && name == "is_draft")
                {
                    continue;
                }
                out.insert(key.clone(), value.clone());
            }
            Ok(PValue::Dict(out))
        }
        _ => Ok(value.clone()),
    }
}

pub(crate) fn dumps(value: &plane_serde::pyjson::Value) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

pub(crate) fn base_host(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))
}

// ---------------------------------------------------------------------------
// 317/329 — POST
// ---------------------------------------------------------------------------

/// `IssueListCreateAPIEndpoint.post` (`issue.py:534-602`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Project.objects.get(pk=project_id)` (`issue.py:540`): id only.
        let statement = queries::PROJECT
            .prepare(client)
            .await
            .map_err(ViewError::from)?;
        let Some(project) = client
            .query_opt(&statement, &[&project_id])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = project.try_get(1).map_err(ViewError::from)?;
        let default_assignee_id: Option<Uuid> = project.try_get(2).map_err(ViewError::from)?;

        let mut serializer = Serializer::new(ctx, false)?;
        validate_fields(&mut serializer, client, false).await?;
        let validated = match serializer.finish() {
            Ok(validated) => validated,
            Err(mut errors) => {
                errors.sort_by_field_order(ISSUE_FIELDS);
                return Ok(errors.into_response());
            }
        };
        let extra = match validate_extra(client, &validated, project_id, workspace_id).await {
            Ok(extra) => extra,
            Err(errors) => return Ok(errors.into_response()),
        };

        let mut issue = build_issue(&validated, project_id, &extra);

        // `IssueSerializer.create` (`issue.py:184-265`): the default issue type
        // is looked up only when the body did not name one.
        if issue.type_id.is_none() {
            let row = client
                .query_opt(
                    "SELECT issue_types.id FROM issue_types \
                     INNER JOIN project_issue_types \
                       ON project_issue_types.issue_type_id = issue_types.id \
                     WHERE project_issue_types.project_id = $1 \
                       AND issue_types.is_default AND issue_types.deleted_at IS NULL \
                     LIMIT 1",
                    &[&project_id],
                )
                .await
                .map_err(ViewError::from)?;
            issue.type_id = row.map(|row| row.get(0));
        }

        plane_db::hooks::issue::create_issue_row(
            client,
            &mut issue,
            Some(user),
            plane_http_app::write::now(),
        )
        .await
        .map_err(issue_hook_error)?;

        let m2m_now = plane_http_app::write::now();
        let has_assignees = extra
            .assignees
            .as_ref()
            .is_some_and(|assignees| !assignees.is_empty());
        if has_assignees {
            plane_db::hooks::issue_m2m::insert_assignees(
                client,
                &issue,
                extra.assignees.as_deref().unwrap_or_default(),
                m2m_now,
            )
            .await
            .map_err(ViewError::from)?;
        } else if let Some(default) =
            plane_db::hooks::issue_m2m::default_assignee(client, project_id, default_assignee_id)
                .await
                .map_err(ViewError::from)?
        {
            plane_db::hooks::issue_m2m::insert_assignees(client, &issue, &[default], m2m_now)
                .await
                .map_err(ViewError::from)?;
        }
        if extra
            .labels
            .as_ref()
            .is_some_and(|labels| !labels.is_empty())
        {
            plane_db::hooks::issue_m2m::insert_labels(
                client,
                &issue,
                extra.labels.as_deref().unwrap_or_default(),
                m2m_now,
            )
            .await
            .map_err(ViewError::from)?;
        }

        let bundle = bundle_after_write(client, issue.id).await?;
        let body = render_issue(ctx, &bundle)?;

        // `issue_activity.delay(...)` (`issue.py:579-589`).
        let requested = plane_http_app::write::request_data_value(ctx)?;
        let origin = base_host(ctx)?;
        publish_issue_activity(
            ctx,
            plane_jobs::celery::tasks::IssueActivityCall::view(
                activity_base(
                    "issue.activity.created",
                    plane_jobs::celery::args::TaskArg::json_text_from(&requested)
                        .map_err(|error| ViewError::Unknown(error.to_string()))?,
                    user,
                    &issue.id.to_string(),
                    &project_id.to_string(),
                    plane_jobs::celery::args::TaskArg::Null,
                ),
                origin.clone(),
            ),
        )
        .await?;
        publish_model_activity(
            ctx,
            "issue",
            &issue.id.to_string(),
            plane_jobs::celery::args::TaskArg::Json(requested),
            None,
            user,
            &slug,
            &origin,
        )
        .await?;

        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

fn issue_hook_error(error: plane_db::hooks::issue::IssueHookError) -> ViewError {
    match error {
        plane_db::hooks::issue::IssueHookError::ProjectNotFound => ViewError::ObjectDoesNotExist,
        other => ViewError::Unknown(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// 318/330 — PATCH and DELETE
// ---------------------------------------------------------------------------

/// `IssueDetailAPIEndpoint.patch` (`issue.py:840-899`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let Some(mut issue) = load_issue_any(client, pk).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if issue.project_id != project_id {
            return Err(ViewError::ObjectDoesNotExist);
        }
        let workspace_id = issue.workspace_id;

        let before = bundle_after_write(client, pk).await?;
        let current = current_instance(ctx, &before)?;

        let mut serializer = Serializer::new(ctx, true)?;
        validate_fields(&mut serializer, client, true).await?;
        let validated = match serializer.finish() {
            Ok(validated) => validated,
            Err(mut errors) => {
                errors.sort_by_field_order(ISSUE_FIELDS);
                return Ok(errors.into_response());
            }
        };
        let extra = match validate_extra(client, &validated, project_id, workspace_id).await {
            Ok(extra) => extra,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `IssueSerializer.update` (`issue.py:267-321`) replaces the through
        // rows before `super().update()`.
        let m2m_now = plane_http_app::write::now();
        if let Some(assignees) = &extra.assignees {
            plane_db::hooks::issue_m2m::replace_assignees(client, &issue, assignees, m2m_now)
                .await
                .map_err(ViewError::from)?;
        }
        if let Some(labels) = &extra.labels {
            plane_db::hooks::issue_m2m::replace_labels(client, &issue, labels, m2m_now)
                .await
                .map_err(ViewError::from)?;
        }

        let old_state = issue.state_id;
        apply_patch(&mut issue, &validated, &extra);
        if issue.state_id.is_none() {
            issue.state_id = plane_db::hooks::issue::ensure_default_state(client, project_id)
                .await
                .map_err(ViewError::from)?;
        }
        let state_changed = issue.state_id != old_state;
        issue.description_stripped = strip_html(&issue.description_html);
        plane_db::hooks::issue::update_issue_row(
            client,
            &mut issue,
            state_changed,
            Some(user),
            plane_http_app::write::now(),
        )
        .await
        .map_err(ViewError::from)?;

        let after = bundle_after_write(client, pk).await?;
        let body = render_issue(ctx, &after)?;

        let accepted = writable_data(ctx, true)?;
        let accepted_text = dumps(&accepted)?;
        let origin = base_host(ctx)?;
        publish_issue_activity(
            ctx,
            plane_jobs::celery::tasks::IssueActivityCall::view(
                activity_base(
                    "issue.activity.updated",
                    plane_jobs::celery::args::TaskArg::json_text(accepted_text),
                    user,
                    &pk.to_string(),
                    &project_id.to_string(),
                    plane_jobs::celery::args::TaskArg::json_text(current.clone()),
                ),
                origin.clone(),
            ),
        )
        .await?;
        publish_model_activity(
            ctx,
            "issue",
            &pk.to_string(),
            plane_jobs::celery::args::TaskArg::Json(accepted),
            Some(current),
            user,
            &slug,
            &origin,
        )
        .await?;

        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `IssueDetailAPIEndpoint.delete` (`issue.py:914-945`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let Some(mut issue) = load_issue_any(client, pk).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if issue.project_id != project_id {
            return Err(ViewError::ObjectDoesNotExist);
        }

        // `issue.created_by_id != request.user.id and not (role=20 …)`
        // (`issue.py:921-933`).
        let admin = client
            .query_opt(
                "SELECT 1 FROM project_members \
                 INNER JOIN workspaces ON workspaces.id = project_members.workspace_id \
                 WHERE workspaces.slug = $1 AND project_members.member_id = $2 \
                   AND project_members.role = 20 AND project_members.project_id = $3 \
                   AND project_members.is_active AND project_members.deleted_at IS NULL \
                 LIMIT 1",
                &[&slug, &user, &project_id],
            )
            .await
            .map_err(ViewError::from)?
            .is_some();
        if issue.created_by_id != Some(user) && !admin {
            return Ok(ViewResponse::raw(
                403,
                APPLICATION_JSON,
                br#"{"error":"Only admin or creator can delete the work item"}"#.to_vec(),
            ));
        }

        let current = current_instance(ctx, &bundle_after_write(client, pk).await?)?;

        if issue.state_id.is_none() {
            issue.state_id = plane_db::hooks::issue::ensure_default_state(client, project_id)
                .await
                .map_err(ViewError::from)?;
        }
        let now = plane_http_app::write::now();
        issue.deleted_at = Some(now);
        issue.description_stripped = strip_html(&issue.description_html);
        plane_db::hooks::issue::update_issue_row(client, &mut issue, false, Some(user), now)
            .await
            .map_err(ViewError::from)?;
        plane_http_app::write::publish_soft_delete_cascade(ctx, "db", "issue", pk).await?;

        // `issue_activity.delay(type="issue.activity.deleted", …)` with the
        // bare shape (no `notification`/`origin`, `issue.py:936-944`).
        let requested = plane_serde::pyjson::Value::Dict(
            [(
                plane_serde::pyjson::PyStr::new("issue_id"),
                plane_serde::pyjson::Value::Str(plane_serde::pyjson::PyStr::from_string(
                    pk.to_string(),
                )),
            )]
            .into_iter()
            .collect(),
        );
        publish_issue_activity(
            ctx,
            plane_jobs::celery::tasks::IssueActivityCall::bare(activity_base(
                "issue.activity.deleted",
                plane_jobs::celery::args::TaskArg::json_text(dumps(&requested)?),
                user,
                &pk.to_string(),
                &project_id.to_string(),
                plane_jobs::celery::args::TaskArg::json_text(current),
            )),
        )
        .await?;

        Ok(ViewResponse::empty(204))
    })
}
