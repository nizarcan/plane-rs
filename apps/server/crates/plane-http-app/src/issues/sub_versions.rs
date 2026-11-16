// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Issue versions (routes 97/98) and work-item description versions
//! (routes 99/100).
//!
//! Python:
//!
//! * `plane/app/views/issue/version.py:32-96` — `IssueVersionEndpoint.get`
//!   (guest check, then `get()` for the detail or `paginate(...)` for the
//!   list);
//! * `plane/app/views/issue/version.py:99-163` —
//!   `WorkItemDescriptionVersionEndpoint.get` (it loads the `Project` and the
//!   `Issue` itself, then runs the same guest rule inline, then the same
//!   paginator with an explicit `-created_at` ordering).
//!
//! Both classes are `APIView`s with a `pk=None` keyword, so one registered
//! handler answers the collection and the detail route of each view; the
//! runtime keys a handler by view + method, exactly as Django's `get(**kwargs)`
//! sees both.
//!
//! The list body is `plane/utils/global_paginator.py`'s envelope — *not*
//! DRF's paginator — so the cursor is `"<size>:<page>:0"`, `next_cursor` is
//! `null` on the last page and `page_count` is the number of rows. Each row is
//! a `.values(*fields)` dict, rendered in the user's timezone
//! (`user_timezone_converter`, then DRF's `DateTimeField`).

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use tokio_postgres::Row;
use uuid::Uuid;

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_issue::{
    IssueDescriptionVersionDetailSerializer, IssueVersionDetailSerializer,
};
use plane_serializers::{DrfDateTime, PgJson};

use super::sub_common::{self as shared, GlobalPage, PageEnvelope};
use super::sub_queries as queries;
use crate::common;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.version.IssueVersionEndpoint")?
        .method(http::Method::GET, issue_version_get)?;
    registry
        .api_view("plane.app.views.issue.version.WorkItemDescriptionVersionEndpoint")?
        .method(http::Method::GET, description_version_get)?;
    Ok(())
}

/// One row of `paginate`'s `.values(*required_fields)` projection.
///
/// `created_at`/`updated_at` are the two fields `user_timezone_converter`
/// rewrites before the dict reaches DRF (`plane/utils/timezone_converter.py`);
/// every other datetime column stays a UTC-aware `datetime` and the renderer's
/// encoder writes it with `isoformat()` — **not** in the active timezone the way
/// a serializer's `DateTimeField` would. `last_saved_at` is exactly that case,
/// so it renders as `…Z` for every caller.
#[derive(serde::Serialize)]
struct VersionValue {
    id: Uuid,
    workspace: Uuid,
    project: Uuid,
    issue: Uuid,
    last_saved_at: DrfDateTime<Utc>,
    owned_by: Uuid,
    created_at: DrfDateTime<Tz>,
    updated_at: DrfDateTime<Tz>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

impl VersionValue {
    fn decode(row: &Row, timezone: Tz) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0)?,
            workspace: row.try_get(1)?,
            project: row.try_get(2)?,
            issue: row.try_get(3)?,
            last_saved_at: DrfDateTime(row.try_get::<_, DateTime<Utc>>(4)?),
            owned_by: row.try_get(5)?,
            created_at: DrfDateTime(row.try_get::<_, DateTime<Utc>>(6)?.with_timezone(&timezone)),
            updated_at: DrfDateTime(row.try_get::<_, DateTime<Utc>>(7)?.with_timezone(&timezone)),
            created_by: row.try_get(8)?,
            updated_by: row.try_get(9)?,
        })
    }
}

async fn page(
    ctx: &ViewCtx<'_>,
    count_query: &plane_db::Query,
    page_query: &plane_db::Query,
    issue_id: Uuid,
    project_id: Uuid,
    slug: &str,
) -> Result<PageEnvelope<VersionValue>, ViewError> {
    let client = common::client(ctx)?;
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let statement = count_query.prepare(client).await?;
    let row = client
        .query_one(&statement, &[&issue_id, &project_id, &slug])
        .await?;
    let total_results: i64 = row.get(0);
    let plan: GlobalPage = shared::global_page(ctx.query().get("cursor"), total_results)?;
    let rows = if plan.limit > 0 {
        let statement = page_query.prepare(client).await?;
        client
            .query(
                &statement,
                &[&issue_id, &project_id, &slug, &plan.limit, &plan.offset],
            )
            .await?
    } else {
        Vec::new()
    };
    let values = rows
        .iter()
        .map(|row| VersionValue::decode(row, timezone))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PageEnvelope::new(plan, values))
}

// ---------------------------------------------------------------------------
// Issue versions (routes 97/98)
// ---------------------------------------------------------------------------

struct IssueVersionRow {
    id: Uuid,
    workspace: Uuid,
    project: Uuid,
    issue: Uuid,
    parent: Option<Uuid>,
    state: Option<Uuid>,
    estimate_point: Option<Uuid>,
    name: String,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    assignees: Vec<Uuid>,
    sequence_id: i32,
    labels: Vec<Uuid>,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    archived_at: Option<NaiveDate>,
    is_draft: bool,
    external_source: Option<String>,
    external_id: Option<String>,
    r#type: Option<Uuid>,
    cycle: Option<Uuid>,
    modules: Vec<Uuid>,
    meta: JsonText,
    last_saved_at: DateTime<Utc>,
    owned_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

impl IssueVersionRow {
    fn decode(row: &Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0)?,
            workspace: row.try_get(1)?,
            project: row.try_get(2)?,
            issue: row.try_get(3)?,
            parent: row.try_get(4)?,
            state: row.try_get(5)?,
            estimate_point: row.try_get(6)?,
            name: row.try_get(7)?,
            priority: row.try_get(8)?,
            start_date: row.try_get(9)?,
            target_date: row.try_get(10)?,
            assignees: row.try_get(11)?,
            sequence_id: row.try_get(12)?,
            labels: row.try_get(13)?,
            sort_order: row.try_get(14)?,
            completed_at: row.try_get(15)?,
            archived_at: row.try_get(16)?,
            is_draft: row.try_get(17)?,
            external_source: row.try_get(18)?,
            external_id: row.try_get(19)?,
            r#type: row.try_get(20)?,
            cycle: row.try_get(21)?,
            modules: row.try_get(22)?,
            meta: row.try_get(23)?,
            last_saved_at: row.try_get(24)?,
            owned_by: row.try_get(25)?,
            created_at: row.try_get(26)?,
            updated_at: row.try_get(27)?,
            created_by: row.try_get(28)?,
            updated_by: row.try_get(29)?,
        })
    }

    fn serializer(&self) -> IssueVersionDetailSerializer<'_> {
        IssueVersionDetailSerializer {
            id: self.id,
            workspace: self.workspace,
            project: self.project,
            issue: self.issue,
            parent: self.parent,
            state: self.state,
            estimate_point: self.estimate_point,
            name: &self.name,
            priority: &self.priority,
            start_date: self.start_date,
            target_date: self.target_date,
            assignees: &self.assignees,
            sequence_id: i64::from(self.sequence_id),
            labels: &self.labels,
            sort_order: self.sort_order,
            completed_at: self.completed_at,
            archived_at: self.archived_at,
            is_draft: self.is_draft,
            external_source: self.external_source.as_deref(),
            external_id: self.external_id.as_deref(),
            r#type: self.r#type,
            cycle: self.cycle,
            modules: &self.modules,
            meta: Some(PgJson::from_json_text(&self.meta)),
            last_saved_at: self.last_saved_at,
            owned_by: self.owned_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
            created_by: self.created_by,
            updated_by: self.updated_by,
        }
    }
}

/// `IssueVersionEndpoint.get` (`version.py:44-96`).
pub fn issue_version_get<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user_id = shared::user_id(ctx)?;

        if shared::issue_hidden_from_guest(client, slug, project_id, issue_id, user_id).await? {
            return Ok(shared::hidden_response());
        }

        if let Some(pk) = ctx.kwarg("pk") {
            let pk = Uuid::parse_str(pk).map_err(|_| ViewError::Validation)?;
            let statement = queries::VERSION_BY_ID.prepare(client).await?;
            let row = client
                .query_opt(&statement, &[&pk, &issue_id, &project_id, &slug])
                .await?;
            let Some(row) = row else {
                return Err(ViewError::ObjectDoesNotExist);
            };
            let version = IssueVersionRow::decode(&row)?;
            let prepared = common::RequestRender::from_ctx(ctx);
            let body = plane_serializers::render(&version.serializer(), &prepared.render_ctx())
                .map_err(ViewError::from)?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        }

        let envelope = page(
            ctx,
            &queries::VERSIONS_COUNT,
            &queries::VERSIONS_PAGE,
            issue_id,
            project_id,
            slug,
        )
        .await?;
        let body = plane_serde::to_vec(plane_serde::Dialect::Drf, &envelope)
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// Work-item description versions (routes 99/100)
// ---------------------------------------------------------------------------

struct DescriptionVersionRow {
    id: Uuid,
    workspace: Uuid,
    project: Uuid,
    issue: Uuid,
    description_binary: Option<Vec<u8>>,
    description_html: String,
    description_stripped: Option<String>,
    description_json: JsonText,
    last_saved_at: DateTime<Utc>,
    owned_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

impl DescriptionVersionRow {
    fn decode(row: &Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0)?,
            workspace: row.try_get(1)?,
            project: row.try_get(2)?,
            issue: row.try_get(3)?,
            description_binary: row.try_get(4)?,
            description_html: row.try_get(5)?,
            description_stripped: row.try_get(6)?,
            description_json: row.try_get(7)?,
            last_saved_at: row.try_get(8)?,
            owned_by: row.try_get(9)?,
            created_at: row.try_get(10)?,
            updated_at: row.try_get(11)?,
            created_by: row.try_get(12)?,
            updated_by: row.try_get(13)?,
        })
    }

    fn serializer(&self) -> Result<IssueDescriptionVersionDetailSerializer<'_>, ViewError> {
        if self.description_binary.is_some() {
            // DRF's `ModelField` returns the raw `bytes` and its JSON encoder
            // cannot serialise them, so a non-null `description_binary` is a
            // 500 (`rest_framework/fields.py`, `JSONEncoder.default`). The
            // fixture stores NULL; the write path copies the issue's binary
            // column, which is also NULL today.
            return Err(ViewError::Unknown(
                "TypeError: Object of type bytes is not JSON serializable".to_string(),
            ));
        }
        Ok(IssueDescriptionVersionDetailSerializer {
            id: self.id,
            workspace: self.workspace,
            project: self.project,
            issue: self.issue,
            description_binary: None,
            description_html: &self.description_html,
            description_stripped: self.description_stripped.as_deref(),
            description_json: Some(PgJson::from_json_text(&self.description_json)),
            last_saved_at: self.last_saved_at,
            owned_by: self.owned_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
            created_by: self.created_by,
            updated_by: self.updated_by,
        })
    }
}

/// `WorkItemDescriptionVersionEndpoint.get` (`version.py:99-163`).
pub fn description_version_get<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "work_item_id")?;
        let user_id = shared::user_id(ctx)?;

        // `Project.objects.get(pk=project_id)` (`version.py:100`).
        let statement = queries::PROJECT_GUEST_VIEW.prepare(client).await?;
        let row = client.query_opt(&statement, &[&project_id]).await?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let guest_view_all_features: bool = row.get(0);

        // `Issue.objects.get(workspace__slug, project_id, pk)` (`version.py:101`).
        let statement = queries::ISSUE_CREATED_BY.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&slug, &project_id, &issue_id])
            .await?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let created_by: Option<Uuid> = row.get(0);

        // The inline guest rule (`version.py:103-111`): an active GUEST project
        // member, on a project that does not let guests see everything, who did
        // not create the issue.
        let statement = queries::GUEST_MEMBERSHIP.prepare(client).await?;
        let row = client
            .query_one(&statement, &[&user_id, &project_id, &slug])
            .await?;
        let is_guest: bool = row.get(0);
        if is_guest && !guest_view_all_features && created_by != Some(user_id) {
            return Ok(shared::hidden_response());
        }

        if let Some(pk) = ctx.kwarg("pk") {
            let pk = Uuid::parse_str(pk).map_err(|_| ViewError::Validation)?;
            let statement = queries::DESC_VERSION_BY_ID.prepare(client).await?;
            let row = client
                .query_opt(&statement, &[&pk, &issue_id, &project_id, &slug])
                .await?;
            let Some(row) = row else {
                return Err(ViewError::ObjectDoesNotExist);
            };
            let version = DescriptionVersionRow::decode(&row)?;
            let prepared = common::RequestRender::from_ctx(ctx);
            let body = plane_serializers::render(&version.serializer()?, &prepared.render_ctx())
                .map_err(ViewError::from)?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        }

        let envelope = page(
            ctx,
            &queries::DESC_VERSIONS_COUNT,
            &queries::DESC_VERSIONS_PAGE,
            issue_id,
            project_id,
            slug,
        )
        .await?;
        let body = plane_serde::to_vec(plane_serde::Dialect::Drf, &envelope)
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
