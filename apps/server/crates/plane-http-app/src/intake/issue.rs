// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.intake.base.IntakeIssueViewSet` — `list` and `retrieve`.
//!
//! Both routes (`/intake-issues/` and `/inbox-issues/`, plus their detail
//! forms) are the same two methods; the view does not look at the path.
//!
//! ## `list` (`plane/app/views/intake/base.py:178-227`)
//!
//! ```text
//! Intake.objects.filter(workspace__slug, project_id).first()   (ORDER BY name)
//! Project.objects.get(pk=project_id)
//! IntakeIssue.objects.filter(intake_id, project_id, **issue_filters(GET, "issue__"))
//!   .select_related("issue").prefetch_related("issue__labels")
//!   .annotate(label_ids=ArrayAgg("issue__labels__id"))
//!   .order_by(sanitize_order_by(?order_by, INTAKE_ISSUE_ORDER_BY_ALLOWLIST, "-issue__created_at"))
//!   .filter(status__in=?status.split(","))           (default "-2")
//!   .filter(created_by=user)                         (restricted guest only)
//! paginate(per_page=1000, on_results=IntakeIssueSerializer(many=True))
//! ```
//!
//! The `issue__` prefix means every `issue_filters` lookup lands on the joined
//! `issues` table, which is exactly what `plane-query`'s `LegacyFilters`
//! predicates and joins are written against. Composing with it keeps the whole
//! filter surface — state, labels, assignees, cycle, module, mentions, dates,
//! `type`, `subscriber`, `intake_status`, and the `logged_by` 500 — identical
//! to the issue-list ports. The base `FROM` alias is `ii`, so the
//! `intake_status` filter's own `intake_issues` join does not collide with it.
//!
//! Two Django statements are **not** run because they cannot be observed: the
//! `prefetch_related("issue__labels")` query (the serializer reads the
//! `label_ids` annotation, not the prefetched relation) and the paginator's
//! separate `page_results.count()` (one extra row does the same job).
//!
//! ## `retrieve` (`plane/app/views/intake/base.py:507-552`)
//!
//! The same queryset for one `issue_id`, plus an `assignee_ids` annotation,
//! then `IntakeIssueDetailSerializer` and the view's own guest check
//! (403 `{"error": "You are not allowed to view this issue"}`).
//!
//! ## Suspected upstream bugs, ported as they are
//!
//! * a project with no intake makes `list` return
//!   `404 {"error": "Intake not found"}` but makes `retrieve` dereference
//!   `None.id` — a 500;
//! * the default `?status=-2` means the list only ever shows *pending* intake
//!   issues unless the caller asks for another status;
//! * `?status=` (present and empty) is Django's `IntegerField` raising
//!   `ValueError` → 500, not a 400;
//! * `IntakeIssueSerializer.to_representation` copies `label_ids` onto the
//!   issue, so the nested object has one field the model does not.

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::filter::{FilterBuilder, FilterParams, LegacyFilters};
use plane_query::issue::model::Relation;
use plane_query::paginator::{MAX_LIMIT, PageQuery};
use plane_query::sql::{Frag, Join, Select, Value};
use plane_serializers::FieldValue;
use plane_serializers::fields::{DrfUuid, DrfUuidList, PyDate};
use plane_serializers::generated::app_issue::IssueIntakeSerializer;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, RenderCtx};
use uuid::Uuid;

use super::queries;
use super::render::plain_context;
use crate::issues::lists::list_common::{
    client, error_body, kwarg_text, kwarg_uuid, map_query_error, user_id,
};

/// Register `list` and `retrieve`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.intake.base.IntakeIssueViewSet")?
        .action("list", list)?
        .action("retrieve", retrieve)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The statements
// ---------------------------------------------------------------------------

/// `IntakeIssue`'s columns under the statement's base alias.
const INTAKE_ISSUE_COLUMNS: &str = "\
 ii.\"created_at\", ii.\"updated_at\", ii.\"created_by_id\", ii.\"updated_by_id\", \
 ii.\"deleted_at\", ii.\"id\", ii.\"project_id\", ii.\"workspace_id\", \
 ii.\"intake_id\", ii.\"issue_id\", ii.\"status\", ii.\"snoozed_till\", \
 ii.\"duplicate_to_id\", ii.\"source\", ii.\"source_email\", \
 ii.\"external_source\", ii.\"external_id\", ii.\"extra\"";

/// The `label_ids` annotation of the list
/// (`plane/app/views/intake/base.py:190-199`), as the ORM renders it.
const LABEL_IDS: &str = "\
 COALESCE(ARRAY_AGG(DISTINCT \"issue_labels\".\"label_id\") \
   FILTER (WHERE (NOT (\"issue_labels\".\"label_id\" IS NULL) \
     AND \"issue_labels\".\"deleted_at\" IS NULL)), '{}'::uuid[]) AS \"label_ids\"";

/// `Issue`'s columns under the alias `issues`, with an `i_` prefix so the
/// projection has no duplicate names. These are the model columns
/// `IssueDetailSerializer` reads; every annotation that serializer declares is
/// absent on this route.
const ISSUE_COLUMNS: &str = "\
 issues.\"created_at\" AS \"i_created_at\", issues.\"updated_at\" AS \"i_updated_at\", \
 issues.\"created_by_id\" AS \"i_created_by_id\", issues.\"updated_by_id\" AS \"i_updated_by_id\", \
 issues.\"deleted_at\" AS \"i_deleted_at\", issues.\"id\" AS \"i_id\", \
 issues.\"project_id\" AS \"i_project_id\", issues.\"workspace_id\" AS \"i_workspace_id\", \
 issues.\"parent_id\" AS \"i_parent_id\", issues.\"state_id\" AS \"i_state_id\", \
 issues.\"point\" AS \"i_point\", issues.\"estimate_point_id\" AS \"i_estimate_point_id\", \
 issues.\"name\" AS \"i_name\", issues.\"description_json\" AS \"i_description_json\", \
 issues.\"description_html\" AS \"i_description_html\", \
 issues.\"description_stripped\" AS \"i_description_stripped\", \
 issues.\"description_binary\" AS \"i_description_binary\", issues.\"priority\" AS \"i_priority\", \
 issues.\"start_date\" AS \"i_start_date\", issues.\"target_date\" AS \"i_target_date\", \
 issues.\"sequence_id\" AS \"i_sequence_id\", issues.\"sort_order\" AS \"i_sort_order\", \
 issues.\"completed_at\" AS \"i_completed_at\", issues.\"archived_at\" AS \"i_archived_at\", \
 issues.\"is_draft\" AS \"i_is_draft\", issues.\"external_source\" AS \"i_external_source\", \
 issues.\"external_id\" AS \"i_external_id\", issues.\"type_id\" AS \"i_type_id\"";

/// `INTAKE_ISSUE_ORDER_BY_ALLOWLIST` (`plane/utils/order_queryset.py:49-60`).
const ORDER_ALLOWLIST: &[&str] = &[
    "issue__created_at",
    "issue__updated_at",
    "issue__sequence_id",
    "issue__sort_order",
    "issue__target_date",
    "issue__start_date",
    "issue__priority",
    "issue__state__name",
    "created_at",
    "updated_at",
    "status",
];

const DEFAULT_ORDER: &str = "-issue__created_at";

/// One resolved `order_by`: the grouped SQL column and its direction.
#[derive(Debug, Clone, Copy)]
struct Order {
    column: &'static str,
    descending: bool,
    /// `issue__state__name` needs the `states` join, which Django adds with
    /// `LEFT OUTER` and repeats in `GROUP BY`.
    needs_states: bool,
}

/// `sanitize_order_by(value, INTAKE_ISSUE_ORDER_BY_ALLOWLIST, "-issue__created_at")`
/// (`plane/utils/order_queryset.py:135-156`).
fn resolve_order(value: &str) -> Order {
    let (bare, descending) = match value.strip_prefix('-') {
        Some(rest) if !rest.is_empty() && !rest.starts_with('-') => (rest, true),
        Some(_) => return resolve_order(DEFAULT_ORDER),
        None => (value, false),
    };
    if bare.is_empty() || !ORDER_ALLOWLIST.contains(&bare) {
        return resolve_order(DEFAULT_ORDER);
    }
    let (column, needs_states) = match bare {
        "issue__created_at" => ("issues.created_at", false),
        "issue__updated_at" => ("issues.updated_at", false),
        "issue__sequence_id" => ("issues.sequence_id", false),
        "issue__sort_order" => ("issues.sort_order", false),
        "issue__target_date" => ("issues.target_date", false),
        "issue__start_date" => ("issues.start_date", false),
        "issue__priority" => ("issues.priority", false),
        "issue__state__name" => ("states.name", true),
        "created_at" => ("ii.created_at", false),
        "updated_at" => ("ii.updated_at", false),
        // `status` is the last allowlisted field; the fallback is unreachable.
        _ => ("ii.status", false),
    };
    Order {
        column,
        descending,
        needs_states,
    }
}

/// `[item for item in request.GET.get("status", "-2").split(",") if item != "null"]`
/// followed by Django's `IntegerField.get_prep_value` for every item
/// (`plane/app/views/intake/base.py:208-210`).
///
/// A present-but-empty value is `[""]`, and `int("")` is a `ValueError` inside
/// the compiler — the family handler's 500, not a 400. An integer outside
/// `int4` is Postgres' own `value out of range`, also a 500.
fn status_filter(ctx: &ViewCtx<'_>) -> Result<Option<Vec<i32>>, ViewError> {
    let raw = ctx.query().get_or("status", "-2");
    let items: Vec<&str> = raw.split(',').filter(|item| *item != "null").collect();
    if items.is_empty() {
        return Ok(None);
    }
    let mut statuses = Vec::with_capacity(items.len());
    for item in items {
        let parsed = plane_core::py::int(item).map_err(|_| {
            ViewError::Unknown(format!("invalid literal for int() with base 10: {item:?}"))
        })?;
        statuses.push(i32::try_from(parsed).map_err(|_| {
            ViewError::Unknown(format!(
                "value \"{parsed}\" is out of range for type integer"
            ))
        })?);
    }
    Ok(Some(statuses))
}

/// The joins and predicate every statement of one list request shares.
fn base_query(
    intake_id: Uuid,
    project_id: Uuid,
    guest_creator: Option<Uuid>,
    filters: &LegacyFilters,
    timezone: chrono_tz::Tz,
    statuses: Option<&[i32]>,
    order: Order,
) -> (Vec<Join>, Frag) {
    let mut builder = FilterBuilder::new();
    filters.apply(&mut builder);
    filters.apply_dates(&mut builder, timezone);

    // The `label_ids` annotation always joins the through table. When a
    // `labels` filter already required it, reuse that join (Django does: one
    // `issue_labels` alias serves both).
    if !builder.joins.contains(Relation::IssueLabels) {
        builder.require_left(Relation::IssueLabels);
    }
    if order.needs_states {
        builder.require_left(Relation::States);
    }

    let mut predicate = Frag::raw("ii.\"deleted_at\" IS NULL")
        .and(Frag::compare(
            "ii.\"intake_id\"",
            "=",
            Value::Uuid(intake_id),
        ))
        .and(Frag::compare(
            "ii.\"project_id\"",
            "=",
            Value::Uuid(project_id),
        ));
    if let Some(statuses) = statuses {
        predicate = predicate.and(Frag::in_list(
            "ii.\"status\"",
            statuses.iter().copied().map(Value::Int).collect(),
        ));
    }
    if let Some(guard) = guest_creator {
        predicate = predicate.and(Frag::compare(
            "ii.\"created_by_id\"",
            "=",
            Value::Uuid(guard),
        ));
    }
    predicate = predicate.and(builder.predicate);

    let mut joins = Vec::with_capacity(builder.joins.entries().len() + 1);
    joins.push(Join::inner(
        "issues",
        "issues",
        Frag::raw("(ii.\"issue_id\" = issues.\"id\")"),
    ));
    joins.extend(builder.joins.to_joins());
    (joins, predicate)
}

/// The statement that fetches one page: `SELECT <columns> … GROUP BY … ORDER BY
/// … LIMIT n+1 OFFSET m`.
fn page_statement(
    joins: Vec<Join>,
    predicate: Frag,
    order: Order,
    limit: i64,
    offset: i64,
) -> Select {
    let mut select = Select::new("intake_issues ii");
    select.joins = joins;
    select.predicate = Some(predicate);
    select.projection = vec![
        Frag::raw(INTAKE_ISSUE_COLUMNS),
        Frag::raw(LABEL_IDS),
        Frag::raw(ISSUE_COLUMNS),
    ];
    select.group_by = vec!["ii.\"id\"".to_owned(), "issues.\"id\"".to_owned()];
    if order.needs_states {
        select.group_by.push("states.\"name\"".to_owned());
    }
    select.order_by = vec![Frag::raw(format!(
        "{} {}",
        order.column,
        if order.descending { "DESC" } else { "ASC" }
    ))];
    select.limit = Some(limit + 1);
    select.offset = Some(offset);
    select
}

/// `queryset.count()`: the number of distinct intake issues the filters leave.
/// Django's `get_aggregation` clears the ordering before counting, so the
/// statement has no `issues.created_at` in its `GROUP BY`.
fn count_statement(joins: Vec<Join>, predicate: Frag) -> Select {
    let mut select = Select::new("intake_issues ii");
    select.joins = joins;
    select.predicate = Some(predicate);
    select.distinct = true;
    select.projection = vec![Frag::raw("ii.\"id\"")];
    select
}

async fn fetch_count(client: &Client, select: Select) -> Result<i64, ViewError> {
    let sql = select.count_sql(Some("ii.\"id\""), None);
    let row = sql
        .query(client)
        .await
        .map_err(ViewError::from)?
        .into_iter()
        .next()
        .ok_or_else(|| ViewError::Unknown("count returned no row".to_owned()))?;
    row.try_get(0).map_err(ViewError::from)
}

async fn fetch_rows(
    client: &Client,
    select: Select,
) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let sql = select.into_sql();
    sql.query(client).await.map_err(ViewError::from)
}

// ---------------------------------------------------------------------------
// Serializers
// ---------------------------------------------------------------------------

/// `plane.app.serializers.intake.IntakeIssueSerializer` (`intake.py:28-97`):
/// seven fields, the nested issue, and `label_ids` copied onto it by
/// `to_representation`.
#[derive(Debug)]
struct IntakeIssueSerializer<'a> {
    id: Uuid,
    status: i64,
    duplicate_to: Option<Uuid>,
    snoozed_till: Option<DateTime<Utc>>,
    source: Option<&'a str>,
    issue: IssueIntakeSerializer<'a>,
    created_by: Option<Uuid>,
}

impl DrfSerialize for IntakeIssueSerializer<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(self.id))?;
        obj.field_dyn("status", ctx, &self.status)?;
        obj.field_dyn("duplicate_to", ctx, &self.duplicate_to.map(DrfUuid))?;
        obj.field_dyn(
            "snoozed_till",
            ctx,
            &plane_serializers::fields::drf_datetime_opt(self.snoozed_till, ctx.tz()),
        )?;
        obj.field_dyn("source", ctx, &self.source)?;
        obj.field_nested_dyn("issue", ctx, Some(&self.issue))?;
        obj.field_dyn("created_by", ctx, &self.created_by.map(DrfUuid))?;
        obj.end();
        Ok(())
    }
}

/// `plane.app.serializers.intake.IntakeIssueDetailSerializer`
/// (`intake.py:100-124`).
#[derive(Debug)]
struct IntakeIssueDetailSerializer<'a> {
    id: Uuid,
    status: i64,
    duplicate_to: Option<Uuid>,
    snoozed_till: Option<DateTime<Utc>>,
    duplicate_issue_detail: Option<&'a IssueIntakeSerializer<'a>>,
    source: Option<&'a str>,
    issue: IntakeIssueDetailIssue<'a>,
}

impl DrfSerialize for IntakeIssueDetailSerializer<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(self.id))?;
        obj.field_dyn("status", ctx, &self.status)?;
        obj.field_dyn("duplicate_to", ctx, &self.duplicate_to.map(DrfUuid))?;
        obj.field_dyn(
            "snoozed_till",
            ctx,
            &plane_serializers::fields::drf_datetime_opt(self.snoozed_till, ctx.tz()),
        )?;
        obj.field_nested_dyn(
            "duplicate_issue_detail",
            ctx,
            self.duplicate_issue_detail
                .map(|value| value as &(dyn DrfSerialize + '_)),
        )?;
        obj.field_dyn("source", ctx, &self.source)?;
        obj.field_nested_dyn("issue", ctx, Some(&self.issue))?;
        obj.end();
        Ok(())
    }
}

/// The nested `IssueDetailSerializer` of `retrieve`, restricted to the fields
/// that exist on an `Issue` fetched through `IntakeIssue.select_related`:
/// every annotation (`cycle_id`, `module_ids`, `sub_issues_count`,
/// `attachment_count`, `link_count`, `is_subscribed`, `is_intake`) is absent
/// and DRF skips it. `label_ids`/`assignee_ids` are assigned by the outer
/// serializer's `to_representation`, so they are always present.
#[derive(Debug)]
struct IntakeIssueDetailIssue<'a> {
    id: Uuid,
    name: String,
    state_id: Option<Uuid>,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    estimate_point: Option<Uuid>,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i64,
    project_id: Uuid,
    parent_id: Option<Uuid>,
    label_ids: &'a [Uuid],
    assignee_ids: &'a [Uuid],
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    is_draft: bool,
    archived_at: Option<NaiveDate>,
    description_html: String,
}

impl DrfSerialize for IntakeIssueDetailIssue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(self.id))?;
        obj.field_dyn("name", ctx, &self.name)?;
        obj.field_dyn("state_id", ctx, &self.state_id.map(DrfUuid))?;
        obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        obj.field_dyn(
            "completed_at",
            ctx,
            &plane_serializers::fields::drf_datetime_opt(self.completed_at, ctx.tz()),
        )?;
        obj.field_dyn("estimate_point", ctx, &self.estimate_point.map(DrfUuid))?;
        obj.field_dyn("priority", ctx, &self.priority)?;
        obj.field_dyn("start_date", ctx, &self.start_date.map(PyDate))?;
        obj.field_dyn("target_date", ctx, &self.target_date.map(PyDate))?;
        obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        obj.field_dyn("project_id", ctx, &DrfUuid(self.project_id))?;
        obj.field_dyn("parent_id", ctx, &self.parent_id.map(DrfUuid))?;
        // `IntakeIssueDetailSerializer.to_representation` always assigns both
        // (`plane/app/serializers/intake.py:117-124`).
        obj.field_dyn("label_ids", ctx, &DrfUuidList(self.label_ids))?;
        obj.field_dyn("assignee_ids", ctx, &DrfUuidList(self.assignee_ids))?;
        obj.field_dyn(
            "created_at",
            ctx,
            &plane_serializers::fields::drf_datetime(self.created_at, ctx.tz()),
        )?;
        obj.field_dyn(
            "updated_at",
            ctx,
            &plane_serializers::fields::drf_datetime(self.updated_at, ctx.tz()),
        )?;
        obj.field_dyn("created_by", ctx, &self.created_by.map(DrfUuid))?;
        obj.field_dyn("updated_by", ctx, &self.updated_by.map(DrfUuid))?;
        obj.field_dyn("is_draft", ctx, &self.is_draft)?;
        obj.field_dyn("archived_at", ctx, &self.archived_at.map(PyDate))?;
        obj.field_dyn("description_html", ctx, &self.description_html)?;
        // The seven absent annotations have no key, exactly as
        // `field_optional(FieldValue::Absent)` writes nothing.
        obj.end();
        Ok(())
    }
}

/// One decoded row of the list statement.
struct ListRow {
    id: Uuid,
    status: i64,
    duplicate_to: Option<Uuid>,
    snoozed_till: Option<DateTime<Utc>>,
    source: Option<String>,
    label_ids: Vec<Uuid>,
    created_by: Option<Uuid>,
    issue_id: Uuid,
    issue_name: String,
    issue_priority: String,
    issue_sequence_id: i64,
    issue_project_id: Uuid,
    issue_created_at: DateTime<Utc>,
    issue_created_by: Option<Uuid>,
}

/// Column positions of the list projection: 18 `IntakeIssue` columns, the
/// `label_ids` aggregate, then the `i_`-prefixed `Issue` columns.
fn decode_list_row(row: &tokio_postgres::Row) -> Result<ListRow, ViewError> {
    Ok(ListRow {
        id: row.get(5),
        status: i64::from(row.get::<_, i32>(10)),
        snoozed_till: row.get(11),
        duplicate_to: row.get(12),
        source: row.get(13),
        label_ids: row.get(18),
        issue_created_at: row.get(19),
        issue_created_by: row.get(21),
        issue_id: row.get(24),
        issue_project_id: row.get(25),
        issue_name: row.get(31),
        issue_priority: row.get(36),
        issue_sequence_id: i64::from(row.get::<_, i32>(39)),
        created_by: row.get(2),
    })
}

/// The owned form of the `duplicate_to` expansion.
struct IssueIntakeRow {
    id: Uuid,
    name: String,
    priority: String,
    sequence_id: i64,
    project_id: Uuid,
    created_at: DateTime<Utc>,
    created_by: Option<Uuid>,
}

fn decode_issue_intake(row: &tokio_postgres::Row) -> IssueIntakeRow {
    IssueIntakeRow {
        id: row.get(0),
        name: row.get(1),
        priority: row.get(2),
        sequence_id: i64::from(row.get::<_, i32>(3)),
        project_id: row.get(4),
        created_at: row.get(5),
        created_by: row.get(6),
    }
}

impl IssueIntakeRow {
    fn serializer(&self) -> IssueIntakeSerializer<'_> {
        IssueIntakeSerializer {
            id: self.id,
            name: &self.name,
            priority: &self.priority,
            sequence_id: self.sequence_id,
            project_id: self.project_id,
            created_at: self.created_at,
            // A plain `Issue` has no `label_ids` attribute, so DRF skips it
            // (`rest_framework/fields.py:421-446`).
            label_ids: FieldValue::absent(),
            created_by: self.created_by,
        }
    }
}

/// The nested issue of `retrieve`, decoded from column 20 onwards.
fn decode_detail_issue<'a>(
    row: &tokio_postgres::Row,
    label_ids: &'a [Uuid],
    assignee_ids: &'a [Uuid],
) -> IntakeIssueDetailIssue<'a> {
    IntakeIssueDetailIssue {
        created_at: row.get(20),
        updated_at: row.get(21),
        created_by: row.get(22),
        updated_by: row.get(23),
        id: row.get(25),
        project_id: row.get(26),
        parent_id: row.get(28),
        state_id: row.get(29),
        estimate_point: row.get(31),
        name: row.get(32),
        description_html: row.get(34),
        priority: row.get(37),
        start_date: row.get(38),
        target_date: row.get(39),
        sequence_id: i64::from(row.get::<_, i32>(40)),
        sort_order: row.get(41),
        completed_at: row.get(42),
        archived_at: row.get(43),
        is_draft: row.get(44),
        label_ids,
        assignee_ids,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `IntakeIssueViewSet.list`.
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Intake.objects.filter(...).first()`; a miss is the view's own 404.
        let intake_statement = client
            .prepare_cached(queries::INTAKE_ONLY.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(intake_row) = client
            .query_opt(&intake_statement, &[&slug, &project_id])
            .await
            .map_err(ViewError::from)?
        else {
            return Ok(error_body(404, "Intake not found"));
        };
        let intake_id: Uuid = intake_row.get(0);

        // `Project.objects.get(pk=…)` and the restricted-guest check
        // (`base.py:184,212-222`).
        let project_statement = client
            .prepare_cached(queries::PROJECT_WITH_GUEST.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(project_row) = client
            .query_opt(&project_statement, &[&project_id, &user, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let guest_view_all_features: bool = project_row.get(0);
        let is_restricted_guest: bool = project_row.get(1);

        // `BasePaginator.paginate` validates `per_page` and the cursor before
        // the queryset is evaluated, and the ORM raises the status/date/filter
        // errors while compiling the first statement — so these parse first.
        let page = match PageQuery::parse_with(ctx.query(), MAX_LIMIT, MAX_LIMIT, None, None) {
            Ok(page) => page,
            Err(error) => return map_query_error(error),
        };
        let limit = i64::from(page.per_page.min(MAX_LIMIT));
        let offset = page.cursor.offset * limit;

        let filters =
            match LegacyFilters::parse(&FilterParams::Get(ctx.query()), Utc::now().date_naive()) {
                Ok(filters) => filters,
                Err(error) => return map_query_error(error),
            };
        let statuses = status_filter(ctx)?;
        let order = resolve_order(ctx.query().get_or("order_by", DEFAULT_ORDER));

        let guest_creator = if is_restricted_guest && !guest_view_all_features {
            Some(user)
        } else {
            None
        };
        let (joins, predicate) = base_query(
            intake_id,
            project_id,
            guest_creator,
            &filters,
            ctx.timezone.copied().unwrap_or(chrono_tz::UTC),
            statuses.as_deref(),
            order,
        );
        let total = fetch_count(client, count_statement(joins.clone(), predicate.clone())).await?;
        let rows = fetch_rows(
            client,
            page_statement(joins, predicate, order, limit, offset),
        )
        .await?;
        let has_results = rows.len() as i64 > limit;
        let mut decoded = Vec::with_capacity(rows.len().min(limit as usize));
        for row in rows.iter().take(limit as usize) {
            decoded.push(decode_list_row(row)?);
        }
        let count = decoded.len() as i64;
        let total_pages = if total == 0 {
            0
        } else {
            (total + limit - 1) / limit
        };

        let context = plain_context(ctx);
        let mut results = Vec::with_capacity(decoded.len());
        for row in &decoded {
            let issue = IssueIntakeSerializer {
                id: row.issue_id,
                name: &row.issue_name,
                priority: &row.issue_priority,
                sequence_id: row.issue_sequence_id,
                project_id: row.issue_project_id,
                created_at: row.issue_created_at,
                label_ids: FieldValue::value(row.label_ids.as_slice()),
                created_by: row.issue_created_by,
            };
            let serializer = IntakeIssueSerializer {
                id: row.id,
                status: row.status,
                duplicate_to: row.duplicate_to,
                snoozed_till: row.snoozed_till,
                source: row.source.as_deref(),
                issue,
                created_by: row.created_by,
            };
            results.push(plane_serializers::render(&serializer, &context)?);
        }

        // `BasePaginator.paginate`'s envelope
        // (`plane/utils/paginator.py:740-755`), key order included.
        let mut body = Vec::with_capacity(96 + results.iter().map(Vec::len).sum::<usize>());
        body.extend_from_slice(b"{\"grouped_by\":null,\"sub_grouped_by\":null,");
        body.extend_from_slice(format!("\"total_count\":{total},").as_bytes());
        body.extend_from_slice(
            format!("\"next_cursor\":\"{limit}:{}:0\",", page.cursor.offset + 1).as_bytes(),
        );
        body.extend_from_slice(
            format!("\"prev_cursor\":\"{limit}:{}:1\",", page.cursor.offset - 1).as_bytes(),
        );
        body.extend_from_slice(
            format!(
                "\"next_page_results\":{},\"prev_page_results\":{},",
                has_results,
                page.cursor.offset > 0
            )
            .as_bytes(),
        );
        body.extend_from_slice(
            format!(
                "\"count\":{count},\"total_pages\":{total_pages},\"total_results\":{total},\
                 \"extra_stats\":null,\"results\":"
            )
            .as_bytes(),
        );
        body.push(b'[');
        for (index, rendered) in results.iter().enumerate() {
            if index > 0 {
                body.push(b',');
            }
            body.extend_from_slice(rendered);
        }
        body.extend_from_slice(b"]}");
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `IntakeIssueViewSet.retrieve`.
fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let intake_statement = client
            .prepare_cached(queries::INTAKE_ONLY.sql)
            .await
            .map_err(ViewError::from)?;
        let intake_id: Option<Uuid> = client
            .query_opt(&intake_statement, &[&slug, &project_id])
            .await
            .map_err(ViewError::from)?
            .map(|row| row.get(0));

        let project_statement = client
            .prepare_cached(queries::PROJECT_WITH_GUEST.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(project_row) = client
            .query_opt(&project_statement, &[&project_id, &user, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let guest_view_all_features: bool = project_row.get(0);
        let is_restricted_guest: bool = project_row.get(1);

        // `intake_issue = …get(intake_id=intake_id.id, …)` — `.id` on None.
        let Some(intake_id) = intake_id else {
            return Err(ViewError::Unknown(
                "'NoneType' object has no attribute 'id'".to_owned(),
            ));
        };

        let statement = client
            .prepare_cached(queries::INTAKE_ISSUE_DETAIL.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&intake_id, &pk, &project_id])
            .await
            .map_err(ViewError::from)?;
        let row = match rows.as_slice() {
            [] => return Err(ViewError::ObjectDoesNotExist),
            [row] => row,
            // `QuerySet.get()` raises `MultipleObjectsReturned`, which is not
            // an `ObjectDoesNotExist`.
            _ => {
                return Err(ViewError::Unknown(
                    "IntakeIssue matching query does not exist.".to_owned(),
                ));
            }
        };

        let created_by: Option<Uuid> = row.get(2);
        if is_restricted_guest && !guest_view_all_features && created_by != Some(user) {
            return Ok(error_body(403, "You are not allowed to view this issue"));
        }

        let duplicate_to: Option<Uuid> = row.get(12);
        let duplicate_row = match duplicate_to {
            Some(id) => {
                let statement = client
                    .prepare_cached(queries::ISSUE_INTAKE_BY_ID.sql)
                    .await
                    .map_err(ViewError::from)?;
                let Some(issue) = client
                    .query_opt(&statement, &[&id])
                    .await
                    .map_err(ViewError::from)?
                else {
                    return Err(ViewError::Unknown(format!(
                        "the duplicate target issue {id} does not exist"
                    )));
                };
                Some(decode_issue_intake(&issue))
            }
            None => None,
        };
        let duplicate_serializer = duplicate_row.as_ref().map(IssueIntakeRow::serializer);

        let label_ids: Vec<Uuid> = row.get(18);
        let assignee_ids: Vec<Uuid> = row.get(19);
        let issue = decode_detail_issue(row, &label_ids, &assignee_ids);

        let source: Option<String> = row.get(13);
        let serializer = IntakeIssueDetailSerializer {
            id: row.get(5),
            status: i64::from(row.get::<_, i32>(10)),
            duplicate_to,
            snoozed_till: row.get(11),
            duplicate_issue_detail: duplicate_serializer.as_ref(),
            source: source.as_deref(),
            issue,
        };
        let context = plain_context(ctx);
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            plane_serializers::render(&serializer, &context)?,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_order_is_issue_created_at_desc() {
        let order = resolve_order(DEFAULT_ORDER);
        assert_eq!(order.column, "issues.created_at");
        assert!(order.descending);
    }

    #[test]
    fn unknown_and_malformed_orders_fall_back() {
        for value in ["", "nope", "--created_at", "-", "issue__created_at__x"] {
            let order = resolve_order(value);
            assert_eq!(order.column, "issues.created_at", "{value:?}");
            assert!(order.descending, "{value:?}");
        }
    }

    #[test]
    fn state_name_needs_the_states_join() {
        let order = resolve_order("issue__state__name");
        assert_eq!(order.column, "states.name");
        assert!(!order.descending);
        assert!(order.needs_states);
    }

    #[test]
    fn the_allowlisted_columns_map_to_the_joined_tables() {
        assert_eq!(resolve_order("status").column, "ii.status");
        assert_eq!(resolve_order("-created_at").column, "ii.created_at");
        assert_eq!(resolve_order("-issue__priority").column, "issues.priority");
    }
}
