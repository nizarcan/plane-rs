// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET …/issue-attachments/` (route 76) and `…/issue-attachments/<pk>/`
//! (route 77) — both answered by `IssueAttachmentEndpoint.get`.
//!
//! Python: `plane/app/views/issue/attachment.py:32-112`. The collection branch
//! lists every `ISSUE_ATTACHMENT` `FileAsset` of the issue.
//!
//! The detail route is a Django bug, ported as it is: `get()` has no `pk`
//! parameter, so DRF's dispatch raises `TypeError: IssueAttachmentEndpoint.get()
//! got an unexpected keyword argument 'pk'` and Plane's family
//! `handle_exception` answers 500 `{"error": "Something went wrong please try
//! again later"}` for every permitted caller (`COVERAGE.md` records the same
//! 403/500 split). The presigned-redirect branch belongs to
//! `IssueAttachmentV2Endpoint`, a different route group; nothing on these two
//! routes needs S3 presigning — `asset_url` is a local `/api/assets/v2/…` path
//! (`plane/db/models/asset.py:84-104`).
//!
//! The guest check is the handler's own (`attachment.py:99-103`): it returns
//! `{"error": …}` with a 403, unlike the `initial()`-level checks that raise.

use tokio_postgres::Row;

use plane_core::error::APPLICATION_JSON;
use plane_db::models::asset::FileAsset;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::PgJson;
use plane_serializers::generated::app_issue::IssueAttachmentSerializer;

use super::sub_common::{self as shared, AssetParts};
use super::sub_queries as queries;
use crate::common;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.attachment.IssueAttachmentEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

struct AttachmentRow {
    asset: FileAsset,
    asset_url: Option<String>,
}

fn decode(row: &Row) -> Result<AttachmentRow, ViewError> {
    let asset = FileAsset::from_row(row, 0)?;
    // `asset_url` reads `self.workspace.slug`, `self.project_id`, `self.issue_id`
    // and `self.id` (`plane/db/models/asset.py:93-95`); the query joins the
    // asset's workspace for the slug.
    let slug: String = row.try_get(FileAsset::COLUMN_COUNT)?;
    let parts = AssetParts {
        id: asset.id,
        entity_type: asset.entity_type.clone(),
        project_id: asset.project_id,
        issue_id: asset.issue_id,
        workspace_slug: Some(slug),
    };
    Ok(AttachmentRow {
        asset_url: parts.url(),
        asset,
    })
}

pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("pk").is_some() {
            // `IssueAttachmentEndpoint.get(self, request, slug, project_id,
            // issue_id)` called with the detail route's `pk` kwarg
            // (`plane/app/views/issue/attachment.py:100`). Python raises before
            // the body runs, and Section "Things that are easy to get wrong"
            // rule 5 of `RUST_REWRITE.md` keeps the behaviour.
            return Err(ViewError::Unknown(
                "TypeError: IssueAttachmentEndpoint.get() got an unexpected keyword argument 'pk'"
                    .to_string(),
            ));
        }
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user_id = shared::user_id(ctx)?;

        if shared::issue_hidden_from_guest(client, slug, project_id, issue_id, user_id).await? {
            return Ok(shared::hidden_response());
        }

        let statement = queries::ATTACHMENTS.prepare(client).await?;
        let rows = client
            .query(&statement, &[&issue_id, &project_id, &slug])
            .await?;
        let decoded: Vec<AttachmentRow> = rows.iter().map(decode).collect::<Result<_, _>>()?;

        let prepared = common::RequestRender::from_ctx(ctx);
        let entries: Vec<IssueAttachmentSerializer<'_>> = decoded
            .iter()
            .map(|row| IssueAttachmentSerializer {
                id: row.asset.id,
                asset_url: row.asset_url.as_deref(),
                created_at: row.asset.created_at,
                updated_at: row.asset.updated_at,
                deleted_at: row.asset.deleted_at,
                attributes: Some(PgJson::from_json_text(&row.asset.attributes)),
                asset: &row.asset.asset,
                entity_type: row.asset.entity_type.as_deref(),
                entity_identifier: row.asset.entity_identifier.as_deref(),
                is_deleted: row.asset.is_deleted,
                is_archived: row.asset.is_archived,
                external_id: row.asset.external_id.as_deref(),
                external_source: row.asset.external_source.as_deref(),
                size: row.asset.size,
                is_uploaded: row.asset.is_uploaded,
                storage_metadata: row
                    .asset
                    .storage_metadata
                    .as_ref()
                    .map(PgJson::from_json_text),
                created_by: row.asset.created_by_id,
                updated_by: row.asset.updated_by_id,
                user: row.asset.user_id,
                workspace: row.asset.workspace_id,
                draft_issue: row.asset.draft_issue_id,
                project: row.asset.project_id,
                issue: row.asset.issue_id,
                comment: row.asset.comment_id,
                page: row.asset.page_id,
            })
            .collect();
        let body = common::render_list(&entries, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
