// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IntakeIssuePublicViewSet.list` / `retrieve` (`plane/space/views/intake.py`,
//! routes 234-236 GET halves).
//!
//! Both start with `DeployBoard.objects.get(anchor, entity_name="project")`
//! (`:58`, `:251`) — a miss is the family 404 — and answer
//! `{"error": "Intake is not enabled for this Project Board"}` 400 when the
//! board has no intake (`:59-63`, `:252-256`).
//!
//! `list` (`:57-106`) filters the board's issues by `issue_intake__intake_id`
//! plus `issue_filters(request.query_params, "GET")`, orders by
//! `issue_intake__snoozed_till, issue_intake__status`, annotates
//! `bridge_id = F("issue_intake__id")` and the three `COUNT` subqueries, and
//! renders `IssueStateIntakeSerializer` — the **app** serializer
//! (`:20-25` imports `plane.app.serializers`), 36 fields and no `bridge_id`.
//!
//! `retrieve` (`:250-270`) resolves one `IntakeIssue` in the board's
//! workspace/project/intake, then its issue, and renders the same serializer.
//! That path has no `sub_issues_count` annotation, so the field is **absent**
//! from the response (DRF skips a missing attribute), not `null`.
//!
//! The nested lists are the related managers: `labels` and `assignees` join
//! their through table without its manager (so a soft-deleted link still
//! shows) and take their order from `Label`/`User`'s `-created_at`;
//! `issue_intake` is `IntakeIssue.objects` ordered by `-created_at`.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{ViewCtx, ViewResponse};
use plane_query::issue::filter::{FilterBuilder, FilterParams, LegacyFilters};
use plane_query::sql::{Frag, Join, Select, Value};
use plane_serializers::FieldValue;
use plane_serializers::PgJson;
use plane_serializers::generated::app_intake::{
    IntakeIssueLiteSerializer, IssueStateIntakeSerializer,
};
use plane_serializers::generated::app_issue::LabelLiteSerializer;
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_state::StateLiteSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;
use tokio_postgres::Row;
use uuid::Uuid;

use super::board;
use super::queries;
use super::{ok, plain_context};
use crate::common::client;
use crate::issues::lists::list_common::{map_query_error, user_timezone};
use crate::issues::sub_resources::sub_common::{AssetParts, ProjectLiteRow};
use crate::write::error_body;
use plane_serializers::RenderCtx;

/// `IntakeIssuePublicViewSet.list` (`plane/space/views/intake.py:57-106`).
pub async fn list(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    list_inner(ctx).await
}

async fn list_inner(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
    let intake_id = uuid_kwarg(ctx, "intake_id")?;
    let client = client(ctx)?;
    let board = board::require_published_board(client, anchor).await?;
    let Some(project_id) = board.project_id else {
        return Ok(intake_not_enabled());
    };
    if board.intake_id.is_none() {
        return Ok(intake_not_enabled());
    }
    let timezone = user_timezone(ctx);
    let filters = match LegacyFilters::parse(
        &FilterParams::Get(ctx.query()),
        plane_query::paginator::request_now().date_naive(),
    ) {
        Ok(filters) => filters,
        Err(error) => return map_query_error(error),
    };
    let mut builder = FilterBuilder::new();
    builder.require_left(plane_query::issue::model::Relation::States);
    builder.require_inner(plane_query::issue::model::Relation::Projects);
    builder.and(Frag::is_null("issues.deleted_at"));
    builder.and(Frag::compare("ii.intake_id", "=", Value::Uuid(intake_id)));
    builder.and(Frag::compare(
        "issues.workspace_id",
        "=",
        Value::Uuid(board.workspace_id),
    ));
    builder.and(Frag::compare(
        "issues.project_id",
        "=",
        Value::Uuid(project_id),
    ));
    filters.apply(&mut builder);
    filters.apply_dates(&mut builder, timezone);
    let mut select = Select::new("issues");
    select.joins = vec![Join::inner(
        "intake_issues",
        "ii",
        Frag::raw("(issues.id = ii.issue_id)"),
    )];
    select.joins.extend(builder.joins.to_joins());
    select.joins.push(Join::left(
        "file_assets",
        "pca",
        Frag::raw("(pca.id = projects.cover_image_asset_id)"),
    ));
    select.joins.push(Join::left(
        "workspaces",
        "pca_w",
        Frag::raw("(pca_w.id = pca.workspace_id)"),
    ));
    select.predicate = Some(builder.predicate);
    select.projection = vec![Frag::raw(queries::INTAKE_LIST_PROJECTION)];
    select.order_by = vec![Frag::raw("ii.snoozed_till ASC"), Frag::raw("ii.status ASC")];
    let rows = select
        .into_sql()
        .query(client)
        .await
        .map_err(ViewError::from)?;
    let entries = collections(client, &rows).await?;
    let prepared = plain_context(ctx);
    let mut body = Vec::new();
    body.push(b'[');
    for (position, entry) in entries.iter().enumerate() {
        if position > 0 {
            body.push(b',');
        }
        body.extend_from_slice(&entry.render(&prepared)?);
    }
    body.push(b']');
    Ok(ok(body))
}

/// `IntakeIssuePublicViewSet.retrieve` (`plane/space/views/intake.py:250-270`).
pub async fn retrieve(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    retrieve_inner(ctx).await
}

async fn retrieve_inner(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
    let intake_id = uuid_kwarg(ctx, "intake_id")?;
    let pk = uuid_kwarg(ctx, "pk")?;
    let client = client(ctx)?;
    let board = board::require_published_board(client, anchor).await?;
    let Some(project_id) = board.project_id else {
        return Ok(intake_not_enabled());
    };
    if board.intake_id.is_none() {
        return Ok(intake_not_enabled());
    }
    let statement = queries::PUBLIC_INTAKE_ISSUE.prepare(client).await?;
    let row = client
        .query_opt(
            &statement,
            &[&pk, &board.workspace_id, &project_id, &intake_id],
        )
        .await?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let issue_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
    let statement = queries::PUBLIC_INTAKE_ISSUE_VALUES.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&issue_id, &board.workspace_id, &project_id])
        .await?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let rows = vec![row];
    let entries = collections(client, &rows).await?;
    let Some(entry) = entries.first() else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let prepared = plain_context(ctx);
    let body = entry.render(&prepared)?;
    Ok(ok(body))
}

fn intake_not_enabled() -> ViewResponse {
    error_body(400, "Intake is not enabled for this Project Board")
}

fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &str) -> Result<Uuid, ViewError> {
    ctx.kwarg(name)
        .ok_or(ViewError::Key)?
        .parse()
        .map_err(|_| ViewError::Validation)
}

/// One issue plus its related rows.
struct Entry {
    issue: IssueRow,
    labels: Vec<LabelRow>,
    assignees: Vec<AssigneeRow>,
    links: Vec<LinkRow>,
}

impl Entry {
    /// Render one `IssueStateIntakeSerializer`; the nested serializers borrow
    /// the rows stored on the entry, so the whole tree is built here.
    fn render(&self, prepared: &RenderCtx<'_>) -> Result<Vec<u8>, ViewError> {
        let issue = &self.issue;
        let state = issue.state.as_ref().map(|state| StateLiteSerializer {
            id: state.id,
            name: &state.name,
            color: &state.color,
            group: &state.group,
        });
        let project: Option<ProjectLiteSerializer<'_>> =
            issue.project.as_ref().map(ProjectLiteRow::serializer);
        let labels: Vec<LabelLiteSerializer<'_>> = self
            .labels
            .iter()
            .map(|label| LabelLiteSerializer {
                id: label.id,
                name: &label.name,
                color: &label.color,
            })
            .collect();
        let assignees: Vec<UserLiteSerializer<'_>> =
            self.assignees.iter().map(AssigneeRow::serializer).collect();
        let links: Vec<IntakeIssueLiteSerializer<'_>> =
            self.links.iter().map(LinkRow::serializer).collect();
        let assignee_ids: Vec<Uuid> = self.assignees.iter().map(|row| row.user_id).collect();
        let label_ids: Vec<Uuid> = self.labels.iter().map(|row| row.id).collect();
        let serializer = IssueStateIntakeSerializer {
            id: issue.id,
            state_detail: state.as_ref(),
            project_detail: project.as_ref(),
            label_details: &labels,
            assignee_details: &assignees,
            sub_issues_count: issue.sub_issues_count,
            issue_intake: &links,
            created_at: issue.created_at,
            updated_at: issue.updated_at,
            deleted_at: issue.deleted_at,
            point: issue.point,
            name: &issue.name,
            description_json: issue.description_json.as_ref().map(PgJson::from_json_text),
            description_html: &issue.description_html,
            description_stripped: issue.description_stripped.as_deref(),
            description_binary: issue.description_binary.as_deref(),
            priority: &issue.priority,
            start_date: issue.start_date,
            target_date: issue.target_date,
            sequence_id: i64::from(issue.sequence_id),
            sort_order: issue.sort_order,
            completed_at: issue.completed_at,
            archived_at: issue.archived_at,
            is_draft: issue.is_draft,
            external_source: issue.external_source.as_deref(),
            external_id: issue.external_id.as_deref(),
            created_by: issue.created_by,
            updated_by: issue.updated_by,
            project: issue.project_id,
            workspace: issue.workspace_id,
            parent: issue.parent_id,
            state: issue.state_id,
            estimate_point: issue.estimate_point_id,
            r#type: issue.type_id,
            assignees: &assignee_ids,
            labels: &label_ids,
        };
        plane_serializers::render(&serializer, prepared).map_err(ViewError::from)
    }
}

/// `IssueStateIntakeSerializer`'s model columns plus the nested blocks.
struct IssueRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    point: Option<i64>,
    name: String,
    description_json: Option<JsonText>,
    description_html: String,
    description_stripped: Option<String>,
    description_binary: Option<Vec<u8>>,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i32,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    archived_at: Option<NaiveDate>,
    is_draft: bool,
    external_source: Option<String>,
    external_id: Option<String>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    project_id: Uuid,
    workspace_id: Uuid,
    parent_id: Option<Uuid>,
    state_id: Option<Uuid>,
    estimate_point_id: Option<Uuid>,
    type_id: Option<Uuid>,
    state: Option<StateRow>,
    project: Option<ProjectLiteRow>,
    sub_issues_count: FieldValue<i64>,
}

struct StateRow {
    id: Uuid,
    name: String,
    color: String,
    group: String,
}

struct LabelRow {
    id: Uuid,
    name: String,
    color: String,
}

struct AssigneeRow {
    user_id: Uuid,
    first_name: String,
    last_name: String,
    avatar: String,
    is_bot: bool,
    display_name: String,
    avatar_url: Option<String>,
}

impl AssigneeRow {
    fn serializer(&self) -> UserLiteSerializer<'_> {
        UserLiteSerializer {
            id: self.user_id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.is_bot,
            display_name: &self.display_name,
        }
    }
}

struct LinkRow {
    id: Uuid,
    status: i64,
    duplicate_to: Option<Uuid>,
    snoozed_till: Option<DateTime<Utc>>,
    source: Option<String>,
}

impl LinkRow {
    fn serializer(&self) -> IntakeIssueLiteSerializer<'_> {
        IntakeIssueLiteSerializer {
            id: self.id,
            status: self.status,
            duplicate_to: self.duplicate_to,
            snoozed_till: self.snoozed_till,
            source: self.source.as_deref(),
        }
    }
}

/// Decode the page's issues and fetch the three related lists in three
/// statements (Django's prefetches), grouped in memory.
async fn collections(client: &Client, rows: &[Row]) -> Result<Vec<Entry>, ViewError> {
    let mut issues = Vec::with_capacity(rows.len());
    for row in rows {
        issues.push(decode_issue(row)?);
    }
    let ids: Vec<Uuid> = issues.iter().map(|issue| issue.id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let labels = fetch_rows(client, &queries::PUBLIC_INTAKE_LABELS, &ids).await?;
    let assignees = fetch_rows(client, &queries::PUBLIC_INTAKE_ASSIGNEES, &ids).await?;
    let links = fetch_rows(client, &queries::PUBLIC_INTAKE_LINKS, &ids).await?;

    let mut by_label: HashMap<Uuid, Vec<LabelRow>> = HashMap::new();
    for row in &labels {
        by_label.entry(get(row, 0)?).or_default().push(LabelRow {
            id: get(row, 1)?,
            name: get(row, 2)?,
            color: get(row, 3)?,
        });
    }
    let mut by_assignee: HashMap<Uuid, Vec<AssigneeRow>> = HashMap::new();
    for row in &assignees {
        let avatar: String = get(row, 4)?;
        let asset: Option<AssetParts> = AssetParts::from_row(row, 7)?;
        let avatar_url = match &asset {
            Some(asset) => asset.url(),
            None if !avatar.is_empty() => Some(avatar.clone()),
            None => None,
        };
        by_assignee
            .entry(get(row, 0)?)
            .or_default()
            .push(AssigneeRow {
                user_id: get(row, 1)?,
                first_name: get(row, 2)?,
                last_name: get(row, 3)?,
                avatar,
                is_bot: get(row, 5)?,
                display_name: get(row, 6)?,
                avatar_url,
            });
    }
    let mut by_link: HashMap<Uuid, Vec<LinkRow>> = HashMap::new();
    for row in &links {
        by_link.entry(get(row, 0)?).or_default().push(LinkRow {
            id: get(row, 1)?,
            status: i64::from(get::<i32>(row, 2)?),
            duplicate_to: get(row, 3)?,
            snoozed_till: get(row, 4)?,
            source: get(row, 5)?,
        });
    }

    let mut entries = Vec::with_capacity(issues.len());
    for issue in issues {
        let labels = by_label.remove(&issue.id).unwrap_or_default();
        let assignees = by_assignee.remove(&issue.id).unwrap_or_default();
        let links = by_link.remove(&issue.id).unwrap_or_default();
        entries.push(Entry {
            issue,
            labels,
            assignees,
            links,
        });
    }
    Ok(entries)
}

async fn fetch_rows(
    client: &Client,
    query: &plane_db::Query,
    ids: &[Uuid],
) -> Result<Vec<Row>, ViewError> {
    let statement = query.prepare(client).await?;
    Ok(client.query(&statement, &[&ids]).await?)
}

fn get<T>(row: &Row, index: usize) -> Result<T, ViewError>
where
    T: for<'a> tokio_postgres::types::FromSql<'a>,
{
    row.try_get(index).map_err(ViewError::from)
}

/// The projection's first block: `issues.id` then the 27 model columns
/// (`queries::INTAKE_LIST_PROJECTION`, `PUBLIC_INTAKE_ISSUE_VALUES`), then the
/// optional `bridge_id`/`sub_issues_count` pair (list only), then the state and
/// project blocks.
fn decode_issue(row: &Row) -> Result<IssueRow, ViewError> {
    let point: Option<i32> = get(row, 4)?;
    let mut issue = IssueRow {
        id: get(row, 0)?,
        created_at: get(row, 1)?,
        updated_at: get(row, 2)?,
        deleted_at: get(row, 3)?,
        point: point.map(i64::from),
        name: get(row, 5)?,
        description_json: get(row, 6)?,
        description_html: get(row, 7)?,
        description_stripped: get(row, 8)?,
        description_binary: get(row, 9)?,
        priority: get(row, 10)?,
        start_date: get(row, 11)?,
        target_date: get(row, 12)?,
        sequence_id: get(row, 13)?,
        sort_order: get(row, 14)?,
        completed_at: get(row, 15)?,
        archived_at: get(row, 16)?,
        is_draft: get(row, 17)?,
        external_source: get(row, 18)?,
        external_id: get(row, 19)?,
        created_by: get(row, 20)?,
        updated_by: get(row, 21)?,
        project_id: get(row, 22)?,
        workspace_id: get(row, 23)?,
        parent_id: get(row, 24)?,
        state_id: get(row, 25)?,
        estimate_point_id: get(row, 26)?,
        type_id: get(row, 27)?,
        state: None,
        project: None,
        sub_issues_count: FieldValue::Absent,
    };
    // `list` projects `bridge_id` then `sub_issues_count`; `retrieve` selects
    // the same two slots as typed `NULL`s, so both paths skip two columns and
    // only a non-NULL count becomes a field value (a missing annotation is
    // DRF's `SkipField`, i.e. `FieldValue::Absent`).
    let mut offset = 28;
    let _bridge: Option<Uuid> = get(row, offset)?;
    let count: Option<i64> = get(row, offset + 1)?;
    if let Some(count) = count {
        issue.sub_issues_count = FieldValue::Value(count);
    }
    offset += 2;
    let state_id: Option<Uuid> = get(row, offset)?;
    if let Some(state_id) = state_id {
        issue.state = Some(StateRow {
            id: state_id,
            name: get(row, offset + 1)?,
            color: get(row, offset + 2)?,
            group: get(row, offset + 3)?,
        });
    }
    offset += 4;
    issue.project = ProjectLiteRow::from_row(row, offset)?;
    Ok(issue)
}
