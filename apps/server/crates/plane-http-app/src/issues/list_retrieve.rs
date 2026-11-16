// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The three "one issue" read routes:
//!
//! * `GET /api/workspaces/{slug}/projects/{project_id}/issues/{pk}/` —
//!   `IssueViewSet.retrieve` (`plane/app/views/issue/base.py:477-624`);
//! * `GET /api/workspaces/{slug}/projects/{project_id}/issues/{pk}/archive/` —
//!   `IssueArchiveViewSet.retrieve` (`plane/app/views/issue/archive.py:222-255`);
//! * `GET /api/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/`
//!   — `IssueDetailIdentifierEndpoint.get` (`plane/app/views/issue/base.py:1110-1368`).
//!
//! All three end in `IssueDetailSerializer`, whose 28 fields mix model columns
//! with annotations. The generated serializer skips an `Option` field that is
//! `None`, which is right for an *absent* annotation (`retrieve` has no
//! `is_intake`) but wrong for one that is present and `null`
//! (`attachment_count` with no rows, `cycle_id` with no cycle). This module
//! therefore renders the body through [`IssueDetail`], a transcription of the
//! generated `drf_write` that distinguishes the two.

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::plane_query;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_query::sql::{Sql, Value};
use plane_serializers::fields::{DrfPk, DrfUuid, DrfUuidList, PyDate};
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, FilterMode, RenderCtx};
use uuid::Uuid;

use super::list_common::{
    client, error_body, kwarg_text, kwarg_uuid, project_with_membership, user_id,
};
use crate::common::jobs;

/// Register the three routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.base.IssueViewSet")?
        .action("retrieve", retrieve)?;
    registry
        .viewset("plane.app.views.issue.archive.IssueArchiveViewSet")?
        .action("retrieve", archive_retrieve)?;
    registry
        .api_view("plane.app.views.issue.base.IssueDetailIdentifierEndpoint")?
        .method(Method::GET, by_identifier)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The serializer
// ---------------------------------------------------------------------------

/// `IssueDetailSerializer`'s `_readable_fields` order.
const DETAIL_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "description_html",
    "is_subscribed",
    "is_intake",
];

/// One issue's `IssueDetailSerializer` input.
///
/// `None` on an annotation field means "the attribute does not exist", which
/// DRF skips; `Some(None)` means "present and null", which DRF renders as
/// `null`.
#[derive(Debug, Clone)]
struct IssueDetail<'a> {
    id: Uuid,
    name: &'a str,
    state_id: Option<Uuid>,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    estimate_point: Option<Uuid>,
    priority: &'a str,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i64,
    project_id: Uuid,
    parent_id: Option<Uuid>,
    cycle_id: Option<Option<Uuid>>,
    module_ids: Option<Vec<Uuid>>,
    label_ids: Option<Vec<Uuid>>,
    assignee_ids: Option<Vec<Uuid>>,
    sub_issues_count: Option<Option<i64>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    attachment_count: Option<Option<i64>>,
    link_count: Option<Option<i64>>,
    is_draft: bool,
    archived_at: Option<NaiveDate>,
    description_html: &'a str,
    is_subscribed: Option<bool>,
    is_intake: Option<bool>,
}

impl DrfSerialize for IssueDetail<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", FilterMode::OutputOnly) {
            obj.field_dyn("id", ctx, &DrfUuid(self.id))?;
        }
        if ctx.emits("name", FilterMode::OutputOnly) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("state_id", FilterMode::OutputOnly) {
            obj.field_dyn("state_id", ctx, &self.state_id.map(DrfUuid))?;
        }
        if ctx.emits("sort_order", FilterMode::OutputOnly) {
            obj.field_dyn("sort_order", ctx, &self.sort_order)?;
        }
        if ctx.emits("completed_at", FilterMode::OutputOnly) {
            obj.field_dyn(
                "completed_at",
                ctx,
                &plane_serializers::fields::drf_datetime_opt(self.completed_at, ctx.tz()),
            )?;
        }
        if ctx.emits("estimate_point", FilterMode::OutputOnly) {
            obj.field_dyn("estimate_point", ctx, &self.estimate_point.map(DrfUuid))?;
        }
        if ctx.emits("priority", FilterMode::OutputOnly) {
            obj.field_dyn("priority", ctx, &self.priority)?;
        }
        if ctx.emits("start_date", FilterMode::OutputOnly) {
            obj.field_dyn("start_date", ctx, &self.start_date.map(PyDate))?;
        }
        if ctx.emits("target_date", FilterMode::OutputOnly) {
            obj.field_dyn("target_date", ctx, &self.target_date.map(PyDate))?;
        }
        if ctx.emits("sequence_id", FilterMode::OutputOnly) {
            obj.field_dyn("sequence_id", ctx, &self.sequence_id)?;
        }
        if ctx.emits("project_id", FilterMode::OutputOnly) {
            obj.field_dyn("project_id", ctx, &DrfUuid(self.project_id))?;
        }
        if ctx.emits("parent_id", FilterMode::OutputOnly) {
            obj.field_dyn("parent_id", ctx, &self.parent_id.map(DrfUuid))?;
        }
        if ctx.emits("cycle_id", FilterMode::OutputOnly)
            && let Some(cycle_id) = self.cycle_id
        {
            let value = match cycle_id {
                Some(cycle_id) => DrfPk::Uuid(cycle_id),
                None => DrfPk::Null,
            };
            obj.field_dyn("cycle_id", ctx, &value)?;
        }
        if ctx.emits("module_ids", FilterMode::OutputOnly)
            && let Some(ids) = &self.module_ids
        {
            obj.field_dyn("module_ids", ctx, &DrfUuidList(ids))?;
        }
        if ctx.emits("label_ids", FilterMode::OutputOnly)
            && let Some(ids) = &self.label_ids
        {
            obj.field_dyn("label_ids", ctx, &DrfUuidList(ids))?;
        }
        if ctx.emits("assignee_ids", FilterMode::OutputOnly)
            && let Some(ids) = &self.assignee_ids
        {
            obj.field_dyn("assignee_ids", ctx, &DrfUuidList(ids))?;
        }
        if ctx.emits("sub_issues_count", FilterMode::OutputOnly)
            && let Some(count) = self.sub_issues_count
        {
            obj.field_dyn("sub_issues_count", ctx, &count)?;
        }
        if ctx.emits("created_at", FilterMode::OutputOnly) {
            obj.field_dyn(
                "created_at",
                ctx,
                &plane_serializers::fields::drf_datetime(self.created_at, ctx.tz()),
            )?;
        }
        if ctx.emits("updated_at", FilterMode::OutputOnly) {
            obj.field_dyn(
                "updated_at",
                ctx,
                &plane_serializers::fields::drf_datetime(self.updated_at, ctx.tz()),
            )?;
        }
        if ctx.emits("created_by", FilterMode::OutputOnly) {
            obj.field_dyn("created_by", ctx, &self.created_by.map(DrfUuid))?;
        }
        if ctx.emits("updated_by", FilterMode::OutputOnly) {
            obj.field_dyn("updated_by", ctx, &self.updated_by.map(DrfUuid))?;
        }
        if ctx.emits("attachment_count", FilterMode::OutputOnly)
            && let Some(count) = self.attachment_count
        {
            obj.field_dyn("attachment_count", ctx, &count)?;
        }
        if ctx.emits("link_count", FilterMode::OutputOnly)
            && let Some(count) = self.link_count
        {
            obj.field_dyn("link_count", ctx, &count)?;
        }
        if ctx.emits("is_draft", FilterMode::OutputOnly) {
            obj.field_dyn("is_draft", ctx, &self.is_draft)?;
        }
        if ctx.emits("archived_at", FilterMode::OutputOnly) {
            obj.field_dyn("archived_at", ctx, &self.archived_at.map(PyDate))?;
        }
        if ctx.emits("description_html", FilterMode::OutputOnly) {
            obj.field_dyn("description_html", ctx, &self.description_html)?;
        }
        if ctx.emits("is_subscribed", FilterMode::OutputOnly)
            && let Some(value) = self.is_subscribed
        {
            obj.field_dyn("is_subscribed", ctx, &value)?;
        }
        if ctx.emits("is_intake", FilterMode::OutputOnly)
            && let Some(value) = self.is_intake
        {
            obj.field_dyn("is_intake", ctx, &value)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, DETAIL_FIELDS, FilterMode::OutputOnly)?;
        obj.end();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

/// The issue columns of [`IssueDetail`], in table order.
const ISSUE_COLUMNS: &str = "issues.created_at, issues.updated_at, issues.created_by_id, \
     issues.updated_by_id, issues.deleted_at, issues.id, issues.project_id, \
     issues.workspace_id, issues.parent_id, issues.state_id, issues.point, \
     issues.estimate_point_id, issues.name, issues.description_json, \
     issues.description_html, issues.description_stripped, issues.description_binary, \
     issues.priority, issues.start_date, issues.target_date, issues.sequence_id, \
     issues.sort_order, issues.completed_at, issues.archived_at, issues.is_draft, \
     issues.external_source, issues.external_id, issues.type_id";

/// The four count/array annotations of `IssueViewSet.retrieve`
/// (`plane/app/views/issue/base.py:491-578`) and the identifier endpoint.
const DETAIL_ANNOTATIONS: &str = "\
 (SELECT cycle_issues.cycle_id FROM cycle_issues \
   WHERE cycle_issues.issue_id = issues.id \
   ORDER BY cycle_issues.created_at DESC LIMIT 1) AS cycle_id, \
 (SELECT COUNT(issue_links.id) FROM issue_links \
   WHERE issue_links.deleted_at IS NULL AND issue_links.issue_id = issues.id \
   GROUP BY issue_links.issue_id) AS link_count, \
 (SELECT COUNT(file_assets.id) FROM file_assets \
   WHERE file_assets.deleted_at IS NULL AND file_assets.entity_type = 'ISSUE_ATTACHMENT' \
     AND file_assets.issue_id = issues.id \
   GROUP BY file_assets.issue_id) AS attachment_count, \
 (SELECT COUNT(sub_issues.id) FROM issues sub_issues \
   LEFT OUTER JOIN states sub_states ON sub_issues.state_id = sub_states.id \
   INNER JOIN projects sub_projects ON sub_issues.project_id = sub_projects.id \
   WHERE sub_issues.deleted_at IS NULL \
     AND (sub_states.group IS NULL OR sub_states.group <> 'triage') \
     AND sub_issues.archived_at IS NULL AND sub_projects.archived_at IS NULL \
     AND NOT sub_issues.is_draft AND sub_issues.parent_id = issues.id \
   GROUP BY sub_issues.parent_id) AS sub_issues_count, \
 (SELECT ARRAY_AGG(DISTINCT issue_labels.label_id) FROM issue_labels \
   WHERE issue_labels.deleted_at IS NULL AND issue_labels.issue_id = issues.id \
   GROUP BY issue_labels.issue_id) AS label_ids, \
 (SELECT ARRAY_AGG(DISTINCT issue_assignees.assignee_id) FROM issue_assignees \
   INNER JOIN users ON issue_assignees.assignee_id = users.id \
   INNER JOIN project_members ON users.id = project_members.member_id \
   WHERE issue_assignees.deleted_at IS NULL AND project_members.is_active \
     AND project_members.deleted_at IS NULL AND issue_assignees.issue_id = issues.id \
   GROUP BY issue_assignees.issue_id) AS assignee_ids, \
 (SELECT ARRAY_AGG(DISTINCT module_issues.module_id) FROM module_issues \
   INNER JOIN modules ON module_issues.module_id = modules.id \
   WHERE module_issues.deleted_at IS NULL AND module_issues.issue_id = issues.id \
     AND modules.archived_at IS NULL \
   GROUP BY module_issues.issue_id) AS module_ids";

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `IssueViewSet.retrieve`.
fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Project.objects.get(pk=project_id, workspace__slug=slug)`.
        let project = project_with_membership(client, slug, project_id, user).await?;

        let sql = format!(
            "SELECT {ISSUE_COLUMNS}, {DETAIL_ANNOTATIONS}, \
             EXISTS(SELECT 1 FROM issue_subscribers \
               INNER JOIN workspaces ON issue_subscribers.workspace_id = workspaces.id \
               WHERE workspaces.slug = $2 AND issue_subscribers.project_id = $1 \
                 AND issue_subscribers.issue_id = issues.id \
                 AND issue_subscribers.subscriber_id = $4 \
                 AND issue_subscribers.deleted_at IS NULL) AS is_subscribed \
             FROM issues \
             INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
             WHERE issues.deleted_at IS NULL AND issues.project_id = $1 \
               AND workspaces.slug = $2 AND issues.id = $3"
        );
        let query = Sql {
            text: sql,
            params: vec![
                Value::Uuid(project_id),
                Value::Text(slug.to_owned()),
                Value::Uuid(pk),
                Value::Uuid(user),
            ],
        };
        let Some(row) = fetch_one(client, &query).await? else {
            return Ok(error_body(404, "The required object does not exist."));
        };

        if project.is_guest() && !project.guest_view_all_features {
            let created_by: Option<Uuid> = row.get(2);
            if created_by != Some(user) {
                return Ok(error_body(403, "You are not allowed to view this issue"));
            }
        }

        let expand = ctx.expand().unwrap_or_default();
        // `recent_visited_task.delay(slug, entity_name="issue",
        // entity_identifier=pk, user_id, project_id)` (`:617`).
        jobs::recent_visited_entity(
            ctx,
            "issue",
            TaskArg::uuid(pk),
            Some(TaskArg::uuid(project_id)),
            TaskArg::uuid(user),
        )
        .await?;

        detail_response(ctx, &row, false, &expand)
    })
}

/// `IssueArchiveViewSet.retrieve`.
fn archive_retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let sql = "SELECT issues.id, issues.name, issues.state_id, issues.sort_order, \
               issues.completed_at, issues.estimate_point_id, issues.priority, \
               issues.start_date, issues.target_date, issues.sequence_id, \
               issues.project_id, issues.parent_id, issues.created_at, \
               issues.updated_at, issues.created_by_id, issues.updated_by_id, \
               issues.is_draft, issues.archived_at, issues.description_html, \
               EXISTS(SELECT 1 FROM issue_subscribers \
                 INNER JOIN workspaces ON issue_subscribers.workspace_id = workspaces.id \
                 WHERE workspaces.slug = $2 AND issue_subscribers.project_id = $1 \
                   AND issue_subscribers.issue_id = issues.id \
                   AND issue_subscribers.subscriber_id = $4 \
                   AND issue_subscribers.deleted_at IS NULL) AS is_subscribed \
             FROM issues \
             INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
             LEFT OUTER JOIN issue_types ON issues.type_id = issue_types.id \
             WHERE issues.deleted_at IS NULL \
               AND (issues.type_id IS NULL OR issue_types.is_epic = false) \
               AND issues.archived_at IS NOT NULL \
               AND issues.project_id = $1 AND workspaces.slug = $2 AND issues.id = $3";
        let query = Sql {
            text: sql.to_owned(),
            params: vec![
                Value::Uuid(project_id),
                Value::Text(slug.to_owned()),
                Value::Uuid(pk),
                Value::Uuid(user),
            ],
        };
        let Some(row) = fetch_one(client, &query).await? else {
            return Ok(error_body(404, "The required object does not exist."));
        };
        let expand = ctx.expand().unwrap_or_default();
        archived_response(ctx, &row, &expand)
    })
}

/// `IssueDetailIdentifierEndpoint.get`.
fn by_identifier<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_identifier = kwarg_text(ctx, "project_identifier")?;
        let issue_identifier = kwarg_text(ctx, "issue_identifier")?;

        // `strict_str_to_int`: digits with an optional leading `-`.
        let sequence_id = match strict_int(issue_identifier) {
            Some(value) => value,
            None => return Ok(error_body(400, "Invalid issue identifier")),
        };

        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Project.objects.get(identifier__iexact=…, workspace__slug=…)`.
        let statement = client
            .prepare_cached(IDENTIFIER_PROJECT.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&statement, &[&project_identifier, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let project_id: Uuid = row.get(0);
        let guest_view_all_features: bool = row.get(1);

        if !super::list_common::is_project_member(client, slug, project_id, user).await? {
            return Ok(error_body(403, "You are not allowed to view this issue"));
        }

        let annotations = annotations_without_group_by();
        let sql = format!(
            "SELECT {ISSUE_COLUMNS}, {annotations}, \
             EXISTS(SELECT 1 FROM issue_subscribers \
               INNER JOIN workspaces ON issue_subscribers.workspace_id = workspaces.id \
               WHERE workspaces.slug = $2 AND issue_subscribers.project_id = $1 \
                 AND issue_subscribers.issue_id = issues.id \
                 AND issue_subscribers.subscriber_id = $4 \
                 AND issue_subscribers.deleted_at IS NULL) AS is_subscribed, \
             EXISTS(SELECT 1 FROM intake_issues \
               INNER JOIN workspaces ON intake_issues.workspace_id = workspaces.id \
               WHERE intake_issues.issue_id = issues.id \
                 AND intake_issues.status IN (-2, 0) \
                 AND workspaces.slug = $2 AND intake_issues.project_id = $1) AS is_intake \
             FROM issues \
             INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
             WHERE issues.deleted_at IS NULL AND issues.project_id = $1 \
               AND workspaces.slug = $2 AND issues.sequence_id::bigint = $3"
        );
        let query = Sql {
            text: sql,
            params: vec![
                Value::Uuid(project_id),
                Value::Text(slug.to_owned()),
                Value::Int8(sequence_id),
                Value::Uuid(user),
            ],
        };
        let Some(row) = fetch_one(client, &query).await? else {
            return Ok(error_body(404, "The required object does not exist."));
        };

        let created_by: Option<Uuid> = row.get(2);
        if super::list_common::is_guest_member(client, slug, project_id, user).await?
            && !guest_view_all_features
            && created_by != Some(user)
        {
            return Ok(error_body(403, "You are not allowed to view this issue"));
        }

        let issue_id: Uuid = row.get(5);
        let expand = ctx.expand().unwrap_or_default();
        // `recent_visited_task.delay(slug, entity_name="issue",
        // entity_identifier=str(issue.id), user_id=str(user.id),
        // project_id=str(project.id))` (`:1359`).
        jobs::recent_visited_entity(
            ctx,
            "issue",
            TaskArg::str(issue_id.to_string()),
            Some(TaskArg::str(project_id.to_string())),
            TaskArg::str(user.to_string()),
        )
        .await?;

        detail_response(ctx, &row, true, &expand)
    })
}

/// The same annotations without the `GROUP BY`, which makes an empty count
/// `0` rather than `null` — `IssueDetailIdentifierEndpoint` uses
/// `Func(F("id"), function="Count")` (`base.py:1150-1210`) while
/// `IssueViewSet.retrieve` groups by the outer reference.
fn annotations_without_group_by() -> String {
    DETAIL_ANNOTATIONS
        .replace(" GROUP BY issue_links.issue_id", "")
        .replace(" GROUP BY file_assets.issue_id", "")
        .replace(" GROUP BY sub_issues.parent_id", "")
}

plane_query! {
    /// `Project.objects.get(identifier__iexact=project_identifier,
    /// workspace__slug=slug)` (`plane/app/views/issue/base.py:1123`).
    IDENTIFIER_PROJECT = "list_identifier_project",
    "SELECT projects.id, projects.guest_view_all_features \
     FROM projects \
     INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
     WHERE UPPER(projects.identifier::text) = UPPER($1) AND workspaces.slug = $2 \
       AND projects.deleted_at IS NULL",
    [("id", Uuid), ("guest_view_all_features", Bool)]
}

// ---------------------------------------------------------------------------
// Row decoding
// ---------------------------------------------------------------------------

fn strict_int(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let (sign, digits) = match bytes[0] {
        b'-' => (-1i64, &value[1..]),
        _ => (1i64, value),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().map(|value| sign * value)
}

async fn fetch_one(client: &Client, sql: &Sql) -> Result<Option<tokio_postgres::Row>, ViewError> {
    Ok(sql.query(client).await?.into_iter().next())
}

/// The render context: `IssueDetailSerializer(issue, expand=self.expand)` never
/// receives `fields=`, so the mask is `None`.
fn render_context<'a>(ctx: &'a ViewCtx<'a>, expand: &'a [&'a str]) -> RenderCtx<'a> {
    let timezone: &'a chrono_tz::Tz = match ctx.timezone {
        Some(timezone) => timezone,
        None => &chrono_tz::UTC,
    };
    RenderCtx::new(timezone, None, expand)
}

/// `IssueViewSet.retrieve` / `IssueDetailIdentifierEndpoint` response.
fn detail_response(
    ctx: &ViewCtx<'_>,
    row: &tokio_postgres::Row,
    is_intake: bool,
    expand: &[&str],
) -> Result<ViewResponse, ViewError> {
    let name: String = row.get(12);
    let description_html: String = row.get(14);
    let priority: String = row.get(17);
    let cycle_id: Option<Uuid> = row.get(28);
    let label_ids: Option<Vec<Uuid>> = row.get(32);
    let assignee_ids: Option<Vec<Uuid>> = row.get(33);
    let module_ids: Option<Vec<Uuid>> = row.get(34);
    let detail = IssueDetail {
        id: row.get(5),
        name: &name,
        state_id: row.get(9),
        sort_order: row.get(21),
        completed_at: row.get(22),
        estimate_point: row.get(11),
        priority: &priority,
        start_date: row.get(18),
        target_date: row.get(19),
        sequence_id: i64::from(row.get::<_, i32>(20)),
        project_id: row.get(6),
        parent_id: row.get(8),
        cycle_id: Some(cycle_id),
        module_ids: Some(module_ids.unwrap_or_default()),
        label_ids: Some(label_ids.unwrap_or_default()),
        assignee_ids: Some(assignee_ids.unwrap_or_default()),
        sub_issues_count: Some(row.get(31)),
        created_at: row.get(0),
        updated_at: row.get(1),
        created_by: row.get(2),
        updated_by: row.get(3),
        attachment_count: Some(row.get(30)),
        link_count: Some(row.get(29)),
        is_draft: row.get(24),
        archived_at: row.get(23),
        description_html: &description_html,
        is_subscribed: Some(row.get(35)),
        is_intake: if is_intake { Some(row.get(36)) } else { None },
    };
    let context = render_context(ctx, expand);
    Ok(ViewResponse::raw(
        200,
        APPLICATION_JSON,
        plane_serializers::render(&detail, &context)?,
    ))
}

/// `IssueArchiveViewSet.retrieve` response: every annotation is absent, so only
/// the model columns and `is_subscribed` are emitted.
fn archived_response(
    ctx: &ViewCtx<'_>,
    row: &tokio_postgres::Row,
    expand: &[&str],
) -> Result<ViewResponse, ViewError> {
    let name: String = row.get(1);
    let description_html: String = row.get(18);
    let priority: String = row.get(6);
    let detail = IssueDetail {
        id: row.get(0),
        name: &name,
        state_id: row.get(2),
        sort_order: row.get(3),
        completed_at: row.get(4),
        estimate_point: row.get(5),
        priority: &priority,
        start_date: row.get(7),
        target_date: row.get(8),
        sequence_id: i64::from(row.get::<_, i32>(9)),
        project_id: row.get(10),
        parent_id: row.get(11),
        cycle_id: None,
        module_ids: None,
        label_ids: None,
        assignee_ids: None,
        sub_issues_count: None,
        created_at: row.get(12),
        updated_at: row.get(13),
        created_by: row.get(14),
        updated_by: row.get(15),
        attachment_count: None,
        link_count: None,
        is_draft: row.get(16),
        archived_at: row.get(17),
        description_html: &description_html,
        is_subscribed: Some(row.get(19)),
        is_intake: None,
    };
    let context = render_context(ctx, expand);
    Ok(ViewResponse::raw(
        200,
        APPLICATION_JSON,
        plane_serializers::render(&detail, &context)?,
    ))
}
