// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET …/issues/<issue_id>/reactions/` — `IssueReactionViewSet.list` (route 86).
//!
//! Python: `plane/app/views/issue/reaction.py:37-47`. `get_queryset`
//! (`:21-35`) binds the row to the URL workspace/project/issue and to an active
//! membership of the URL project, then orders `-created_at`; `list` runs the
//! restricted-guest check first and then DRF's `ListModelMixin` (no paginator,
//! so a bare array).
//!
//! `IssueReactionSerializer` is a plain `BaseSerializer`: `fields=`/`expand=`
//! are inert.

use tokio_postgres::Row;

use plane_core::error::APPLICATION_JSON;
use plane_db::models::issue::IssueReaction;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_issue::IssueReactionSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;

use super::sub_common::{self as shared, UserLiteRow};
use super::sub_queries as queries;
use crate::common;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.reaction.IssueReactionViewSet")?
        .action("list", list)?;
    Ok(())
}

struct ReactionRow {
    reaction: IssueReaction,
    actor: Option<UserLiteRow>,
}

impl ReactionRow {
    fn decode(row: &Row) -> Result<Self, ViewError> {
        Ok(Self {
            reaction: IssueReaction::from_row(row, 0)?,
            actor: UserLiteRow::from_row(row, IssueReaction::COLUMN_COUNT)?,
        })
    }
}

struct ReactionEntry<'a> {
    reaction: &'a IssueReaction,
    actor: Option<UserLiteSerializer<'a>>,
}

impl<'a> ReactionEntry<'a> {
    fn new(row: &'a ReactionRow) -> Self {
        Self {
            reaction: &row.reaction,
            actor: row.actor.as_ref().map(UserLiteRow::serializer),
        }
    }

    fn serializer(&self) -> IssueReactionSerializer<'_> {
        IssueReactionSerializer {
            id: self.reaction.id,
            actor_detail: self.actor.as_ref(),
            created_at: self.reaction.created_at,
            updated_at: self.reaction.updated_at,
            deleted_at: self.reaction.deleted_at,
            reaction: &self.reaction.reaction,
            created_by: self.reaction.created_by_id,
            updated_by: self.reaction.updated_by_id,
            project: self.reaction.project_id,
            workspace: self.reaction.workspace_id,
            actor: self.reaction.actor_id,
            issue: self.reaction.issue_id,
        }
    }
}

/// `IssueReactionViewSet.list` (`reaction.py:37-47`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user_id = shared::user_id(ctx)?;

        if shared::issue_hidden_from_guest(client, slug, project_id, issue_id, user_id).await? {
            return Ok(shared::hidden_response());
        }

        let statement = queries::ISSUE_REACTIONS.prepare(client).await?;
        let rows = client
            .query(&statement, &[&slug, &project_id, &issue_id, &user_id])
            .await?;
        let decoded: Vec<ReactionRow> = rows
            .iter()
            .map(ReactionRow::decode)
            .collect::<Result<_, _>>()?;
        let entries: Vec<ReactionEntry<'_>> = decoded.iter().map(ReactionEntry::new).collect();
        let prepared = common::RequestRender::from_ctx(ctx);
        let serializers: Vec<IssueReactionSerializer<'_>> =
            entries.iter().map(ReactionEntry::serializer).collect();
        let body = common::render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
