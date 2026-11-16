// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Issue comments (routes 81/82) and their reactions (route 88).
//!
//! Python:
//!
//! * `plane/app/views/issue/comment.py:33-71` — `IssueCommentViewSet.get_queryset`
//!   (workspace/project/issue binding, `is_member` annotation);
//! * `:73-92` — `list`, which runs `issue_in_project(include_archived=True)` and
//!   the restricted-guest check before DRF's `ListModelMixin`;
//! * `:94-102` — `retrieve` (guest check only, then `get_object_or_404`);
//! * `:227-283` — `CommentReactionViewSet` (`:206-296`), whose `list` guest
//!   check resolves the comment's parent issue first (`:226-246`).
//!
//! Neither viewset passes `fields=`/`expand=`; `IssueCommentSerializer` is a
//! plain `BaseSerializer`, so both parameters are inert.
//!
//! Round trips: Django's list costs 31 statements on the fixture (one
//! `comment_reactions` query and one user/avatar query per comment and per
//! reaction; `apps/server/HANDLERS.md` §3). This port reads the comments with
//! all nested rows in one statement and their reactions in a second, then
//! groups the reactions in memory.

use std::collections::HashMap;

use uuid::Uuid;

use plane_core::ApiError;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_issue::CommentReactionSerializer;

use super::sub_common::{self as shared, CommentEntry, CommentReactionRow, CommentRow};
use super::sub_queries as queries;
use crate::common;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.comment.IssueCommentViewSet")?
        .action("list", list)?
        .action("retrieve", retrieve)?;
    registry
        .viewset("plane.app.views.issue.comment.CommentReactionViewSet")?
        .action("list", reaction_list)?;
    Ok(())
}

/// `CommentReaction.objects.select_related("actor")` for a set of comments,
/// grouped by comment id in the statement's `-created_at` order.
async fn reactions_by_comment(
    ctx: &ViewCtx<'_>,
    comments: &[Uuid],
) -> Result<(Vec<CommentReactionRow>, Vec<Vec<usize>>), ViewError> {
    let client = common::client(ctx)?;
    let statement = queries::COMMENT_REACTIONS.prepare(client).await?;
    let rows = client.query(&statement, &[&comments]).await?;
    let decoded: Vec<CommentReactionRow> = rows
        .iter()
        .map(|row| CommentReactionRow::from_row(row, 0))
        .collect::<Result<_, _>>()?;
    let mut index: HashMap<Uuid, usize> = HashMap::with_capacity(comments.len());
    for (position, comment) in comments.iter().enumerate() {
        index.insert(*comment, position);
    }
    let mut grouped: Vec<Vec<usize>> = vec![Vec::new(); comments.len()];
    for (position, reaction) in decoded.iter().enumerate() {
        if let Some(slot) = index.get(&reaction.reaction.comment_id) {
            grouped[*slot].push(position);
        }
    }
    Ok((decoded, grouped))
}

/// One `django-filter` UUID parameter of `IssueCommentViewSet.filterset_fields`
/// (`plane/app/views/issue/comment.py:42`). An empty value is ignored by the
/// filter (`EMPTY_VALUES`), an unparsable one is the form's
/// `Enter a valid UUID.` validation error.
fn filter_uuid(
    ctx: &ViewCtx<'_>,
    name: &'static str,
    errors: &mut Vec<(&'static str, serde_json::Value)>,
) -> Option<Uuid> {
    let value = ctx.query().get(name)?.trim();
    if value.is_empty() {
        return None;
    }
    match Uuid::parse_str(value) {
        Ok(uuid) => Some(uuid),
        Err(_) => {
            errors.push((name, serde_json::json!(["Enter a valid UUID."])));
            None
        }
    }
}

fn filter_values(ctx: &ViewCtx<'_>) -> Result<(Option<Uuid>, Option<Uuid>), ViewError> {
    let mut errors: Vec<(&'static str, serde_json::Value)> = Vec::new();
    let issue_id = filter_uuid(ctx, "issue__id", &mut errors);
    let workspace_id = filter_uuid(ctx, "workspace__id", &mut errors);
    if errors.is_empty() {
        return Ok((issue_id, workspace_id));
    }
    let mut detail = plane_core::Object::new();
    for (field, messages) in errors {
        detail = detail.field(field, messages);
    }
    Err(ViewError::Api(ApiError::ValidationError(detail.into())))
}

async fn comments(ctx: &ViewCtx<'_>, pk: Option<Uuid>) -> Result<Vec<CommentRow>, ViewError> {
    let (issue_filter, workspace_filter) = filter_values(ctx)?;
    let client = common::client(ctx)?;
    let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
    let project_id = shared::kwarg_uuid(ctx, "project_id")?;
    let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
    let user_id = shared::user_id(ctx)?;
    let statement = queries::COMMENTS.prepare(client).await?;
    let rows = client
        .query(
            &statement,
            &[
                &slug,
                &project_id,
                &issue_id,
                &user_id,
                &pk,
                &issue_filter,
                &workspace_filter,
            ],
        )
        .await?;
    rows.iter()
        .map(|row| CommentRow::from_row(row, 0, true))
        .collect()
}

/// `IssueCommentViewSet.list` (`comment.py:73-92`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user_id = shared::user_id(ctx)?;

        if !shared::issue_in_project(client, issue_id, project_id, slug).await? {
            return Ok(shared::issue_not_found_response());
        }
        if shared::issue_hidden_from_guest(client, slug, project_id, issue_id, user_id).await? {
            return Ok(shared::hidden_response());
        }

        let rows = comments(ctx, None).await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.comment.id).collect();
        let (reactions, grouped) = if ids.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            reactions_by_comment(ctx, &ids).await?
        };
        let grouped: Vec<Vec<&CommentReactionRow>> = grouped
            .iter()
            .map(|indexes| indexes.iter().map(|index| &reactions[*index]).collect())
            .collect();
        let entries: Vec<CommentEntry<'_>> = rows
            .iter()
            .enumerate()
            .map(|(position, row)| {
                CommentEntry::new(
                    row,
                    grouped.get(position).map(Vec::as_slice).unwrap_or_default(),
                )
            })
            .collect();
        let prepared = common::RequestRender::from_ctx(ctx);
        let serializers: Vec<_> = entries.iter().map(CommentEntry::serializer).collect();
        let body = common::render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `IssueCommentViewSet.retrieve` (`comment.py:94-102`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user_id = shared::user_id(ctx)?;
        let pk = shared::kwarg_uuid(ctx, "pk")?;

        if shared::issue_hidden_from_guest(client, slug, project_id, issue_id, user_id).await? {
            return Ok(shared::hidden_response());
        }

        let rows = comments(ctx, Some(pk)).await?;
        let Some(row) = rows.first() else {
            // DRF's `get_object_or_404` (`comment.py:102`).
            return Ok(ViewResponse::json(
                404,
                serde_json::json!({ "detail": "No IssueComment matches the given query." }),
            ));
        };
        let ids = [row.comment.id];
        let (reactions, grouped) = reactions_by_comment(ctx, &ids).await?;
        let references: Vec<&CommentReactionRow> = grouped
            .first()
            .map(|indexes| indexes.iter().map(|index| &reactions[*index]).collect())
            .unwrap_or_default();
        let entry = CommentEntry::new(row, &references);
        let prepared = common::RequestRender::from_ctx(ctx);
        let body = plane_serializers::render(&entry.serializer(), &prepared.render_ctx())
            .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `CommentReactionViewSet.list` (`comment.py:250-265`).
pub fn reaction_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let comment_id = shared::kwarg_uuid(ctx, "comment_id")?;
        let user_id = shared::user_id(ctx)?;

        // `_comment_issue_hidden_from_guest` (`comment.py:250-257`): a missing
        // comment resolves to `None` and is *not* hidden, so the list answers
        // `[]` rather than a 404.
        let statement = queries::COMMENT_ISSUE.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&comment_id, &project_id, &slug])
            .await?;
        if let Some(row) = row {
            let issue_id: Uuid = row.get(0);
            if shared::issue_hidden_from_guest(client, slug, project_id, issue_id, user_id).await? {
                return Ok(shared::hidden_response());
            }
        }

        let statement = queries::COMMENT_REACTIONS_BY_COMMENT
            .prepare(client)
            .await?;
        let rows = client
            .query(&statement, &[&slug, &project_id, &comment_id, &user_id])
            .await?;
        let decoded: Vec<CommentReactionRow> = rows
            .iter()
            .map(|row| CommentReactionRow::from_row(row, 0))
            .collect::<Result<_, _>>()?;
        let prepared = common::RequestRender::from_ctx(ctx);
        let entries: Vec<CommentReactionSerializer<'_>> =
            decoded.iter().map(CommentReactionRow::serializer).collect();
        let body = common::render_list(&entries, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
