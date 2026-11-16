// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/workspaces/<slug>/projects/<project_id>/issues/<issue_id>/issue-links/`
//! and `…/issue-links/<pk>/` — `IssueLinkViewSet` (routes 74 and 75).
//!
//! Python: `plane/app/views/issue/link.py:27-78` (the two read actions; the
//! view's `initial()` guest check is run by the runtime from `routes.json`'s
//! view name, `plane-drf`'s `initial_issue_hidden_from_guest`).
//!
//! * `list` is DRF's `ListModelMixin`: the viewset sets no pagination class,
//!   so the body is a bare array.
//! * `retrieve` is DRF's `RetrieveModelMixin`: `get_object_or_404`, so a
//!   missing row is DRF's `{"detail": "Not found."}` 404.
//! * Neither call passes `fields=`/`expand=` (only `IssueSerializer` and
//!   `IssueDetailSerializer` are `DynamicBaseSerializer`s), so both query
//!   parameters are inert here.

use tokio_postgres::Row;

use plane_core::error::APPLICATION_JSON;
use plane_db::models::issue::IssueLink;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::PgJson;
use plane_serializers::generated::app_issue::IssueLinkSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;

use super::sub_common::{self as shared, UserLiteRow};
use super::sub_queries as queries;
use crate::common;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.link.IssueLinkViewSet")?
        .action("list", list)?
        .action("retrieve", retrieve)?;
    Ok(())
}

struct LinkRow {
    link: IssueLink,
    created_by: Option<UserLiteRow>,
}

fn decode(row: &Row) -> Result<LinkRow, ViewError> {
    let link = IssueLink::from_row(row, 0)?;
    let created_by = UserLiteRow::from_row(row, IssueLink::COLUMN_COUNT)?;
    Ok(LinkRow { link, created_by })
}

/// The row with its nested serializer materialised, so the outer serializer can
/// borrow it.
struct LinkEntry<'a> {
    link: &'a IssueLink,
    created_by: Option<UserLiteSerializer<'a>>,
}

impl<'a> LinkEntry<'a> {
    fn new(row: &'a LinkRow) -> Self {
        Self {
            link: &row.link,
            created_by: row.created_by.as_ref().map(UserLiteRow::serializer),
        }
    }

    fn serializer(&self) -> IssueLinkSerializer<'_> {
        IssueLinkSerializer {
            id: self.link.id,
            created_by_detail: self.created_by.as_ref(),
            created_at: self.link.created_at,
            updated_at: self.link.updated_at,
            deleted_at: self.link.deleted_at,
            title: self.link.title.as_deref(),
            url: &self.link.url,
            metadata: Some(PgJson::from_json_text(&self.link.metadata)),
            created_by: self.link.created_by_id,
            updated_by: self.link.updated_by_id,
            project: self.link.project_id,
            workspace: self.link.workspace_id,
            issue: self.link.issue_id,
        }
    }
}

async fn load(ctx: &ViewCtx<'_>, pk: Option<uuid::Uuid>) -> Result<Vec<LinkRow>, ViewError> {
    let client = common::client(ctx)?;
    let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
    let project_id = shared::kwarg_uuid(ctx, "project_id")?;
    let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
    let user_id = shared::user_id(ctx)?;
    let statement = queries::LINKS.prepare(client).await?;
    let rows = client
        .query(&statement, &[&slug, &project_id, &issue_id, &user_id, &pk])
        .await?;
    rows.iter().map(decode).collect()
}

pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let rows = load(ctx, None).await?;
        let entries: Vec<LinkEntry<'_>> = rows.iter().map(LinkEntry::new).collect();
        let prepared = common::RequestRender::from_ctx(ctx);
        let serializers: Vec<IssueLinkSerializer<'_>> =
            entries.iter().map(LinkEntry::serializer).collect();
        let body = common::render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let pk = shared::kwarg_uuid(ctx, "pk")?;
        let rows = load(ctx, Some(pk)).await?;
        let Some(row) = rows.first() else {
            // DRF's `get_object_or_404` raises `Http404` with the queryset
            // model's `object_name` in the detail
            // (`rest_framework/generics.py:17-20`).
            return Ok(ViewResponse::json(
                404,
                serde_json::json!({ "detail": "No IssueLink matches the given query." }),
            ));
        };
        let entry = LinkEntry::new(row);
        let prepared = common::RequestRender::from_ctx(ctx);
        let body = plane_serializers::render(&entry.serializer(), &prepared.render_ctx())
            .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
