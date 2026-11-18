// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The public comment, reaction and vote **reads** (routes 239-245, GET
//! halves).
//!
//! Python: `plane/space/views/issue.py`.
//!
//! | Route | ViewSet action | Python | Result |
//! | --- | --- | --- | --- |
//! | 239 | `IssueCommentPublicViewSet.list` | `:261-289` | the board's `EXTERNAL` comments |
//! | 240 | `IssueCommentPublicViewSet.retrieve` | same queryset | one comment or DRF's 404 |
//! | 241 | `IssueReactionPublicViewSet.list` | `:402-420` | **always `[]`** |
//! | 243 | `CommentReactionPublicViewSet.list` | `:498-517` | the comment's reactions |
//! | 245 | `IssueVotePublicViewSet.list` | `:607-622` | **always `[]`** |
//!
//! ## The two always-empty lists (241 and 245)
//!
//! Both `get_queryset`s read URL kwargs their routes do not supply:
//! `IssueReactionPublicViewSet` filters `DeployBoard.objects.get(
//! workspace__slug=self.kwargs["slug"], project_id=self.kwargs["project_id"])`
//! (`:404-407`) while the URL binds `anchor` and `issue_id`
//! (`space/urls/issue.py:33-43`), and `IssueVotePublicViewSet` reads
//! `workspace__slug=self.kwargs["anchor"]` (`:609-611`) — the anchor is not a
//! workspace slug. Both miss, both catch `DeployBoard.DoesNotExist` and return
//! `.none()`, so an authenticated caller always gets `[]`. Ported as-is
//! (`PUBLIC_INSTANCES.md` §5); only the POST/DELETE halves are meaningful.
//!
//! ## Permissions
//!
//! 239/240 are `AllowAny` for `list`/`retrieve` — the view's own
//! `get_permissions` (`:253-259`) — while 241/243/245 keep the class default
//! `IsAuthenticated`, so anonymous callers get 401 before the queryset runs.
//! The runtime's `dynamic_permissions` policy already implements both
//! (`plane-permissions/src/dynamic.rs`); nothing here re-checks them.
//!
//! ## The comment queryset
//!
//! `IssueCommentPublicViewSet.get_queryset` (`:261-289`) is a
//! `.get(anchor, entity_name="project")`; a miss — or a board with comments
//! disabled — is `IssueComment.objects.none()`, i.e. `[]` and a 404 on
//! retrieve, never an error. When enabled it filters the board's workspace and
//! project plus the URL's `issue_id` and `access="EXTERNAL"`, annotates
//! `is_member` as an `Exists(ProjectMember …)`, and orders by `created_at`.
//! `filterset_fields = ["issue__id", "workspace__id"]` (`:251`), so those two
//! query parameters are validated and applied by `DjangoFilterBackend`.
//!
//! `list` is a `ModelViewSet.list` → DRF's `ListModelMixin` with no paginator
//! (`BasePaginator` has no `paginate_queryset`), so `?per_page=` is inert and
//! the body is a plain array.

use std::collections::HashMap;

use plane_core::{ApiError, Object};
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::json::Json;
use uuid::Uuid;

use super::board;
use super::queries;
use super::{ok, plain_context};
use crate::common::client;
use crate::issues::sub_resources::sub_common::{CommentEntry, CommentReactionRow, CommentRow};
use crate::public::queries as public_queries;

/// Register the five reads.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    let mut comments = registry.viewset("plane.space.views.issue.IssueCommentPublicViewSet")?;
    comments
        .action("list", comment_list)?
        .action("retrieve", comment_retrieve)?;
    registry
        .viewset("plane.space.views.issue.IssueReactionPublicViewSet")?
        .action("list", empty_list)?;
    registry
        .viewset("plane.space.views.issue.CommentReactionPublicViewSet")?
        .action("list", comment_reaction_list)?;
    registry
        .viewset("plane.space.views.issue.IssueVotePublicViewSet")?
        .action("list", empty_list)?;
    Ok(())
}

/// `IssueReactionPublicViewSet.list` / `IssueVotePublicViewSet.list`: the
/// queryset is always `.none()` (see the module docs).
fn empty_list<'a>(_ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { Ok(ok(Json::Array(Vec::new()).to_drf())) })
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

/// The board the comment queryset is scoped to, or `None` for "no rows".
struct CommentScope {
    workspace_id: Uuid,
    project_id: Uuid,
}

/// `IssueCommentPublicViewSet.get_queryset`'s board half (`issue.py:262-264`).
async fn comment_scope(ctx: &ViewCtx<'_>, anchor: &str) -> Result<Option<CommentScope>, ViewError> {
    let client = client(ctx)?;
    let Some(board) = board::published_board(client, anchor).await? else {
        return Ok(None);
    };
    if !board.is_comments_enabled {
        return Ok(None);
    }
    let Some(project_id) = board.project_id else {
        return Ok(None);
    };
    Ok(Some(CommentScope {
        workspace_id: board.workspace_id,
        project_id,
    }))
}

/// `DjangoFilterBackend` over `filterset_fields = ["issue__id", "workspace__id"]`
/// (`issue.py:251`): an empty value is ignored, an unparsable one is
/// `{"<field>": ["Enter a valid UUID."]}`.
fn filter_values(ctx: &ViewCtx<'_>) -> Result<(Option<Uuid>, Option<Uuid>), ViewError> {
    let mut errors: Vec<(&'static str, serde_json::Value)> = Vec::new();
    let mut field = |name: &'static str| -> Option<Uuid> {
        let value = ctx.query().get(name)?.trim().to_owned();
        if value.is_empty() {
            return None;
        }
        match Uuid::parse_str(&value) {
            Ok(uuid) => Some(uuid),
            Err(_) => {
                errors.push((name, serde_json::json!(["Enter a valid UUID."])));
                None
            }
        }
    };
    let issue_id = field("issue__id");
    let workspace_id = field("workspace__id");
    if errors.is_empty() {
        return Ok((issue_id, workspace_id));
    }
    let mut detail = Object::new();
    for (name, messages) in errors {
        detail = detail.field(name, messages);
    }
    Err(ViewError::Api(ApiError::ValidationError(detail.into())))
}

/// The comment rows, in `created_at` order.
async fn comments(ctx: &ViewCtx<'_>, pk: Option<Uuid>) -> Result<Vec<CommentRow>, ViewError> {
    let (issue_filter, workspace_filter) = filter_values(ctx)?;
    let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
    let issue_id: Uuid = ctx
        .kwarg("issue_id")
        .ok_or(ViewError::Key)?
        .parse()
        .map_err(|_| ViewError::Validation)?;
    let anonymous_id: Option<Uuid> = ctx.user.id();
    let Some(scope) = comment_scope(ctx, anchor).await? else {
        return Ok(Vec::new());
    };
    let client = client(ctx)?;
    let statement = queries::PUBLIC_COMMENTS.prepare(client).await?;
    let rows = client
        .query(
            &statement,
            &[
                &scope.workspace_id,
                &scope.project_id,
                &issue_id,
                &anonymous_id,
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

/// `IssueCommentPublicViewSet.list` (`plane/space/views/issue.py:261-289` +
/// DRF's `ListModelMixin`).
fn comment_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let rows = comments(ctx, None).await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.comment.id).collect();
        let (reactions, grouped) = reactions_by_comment(ctx, &ids).await?;
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
        let prepared = plain_context(ctx);
        let serializers: Vec<_> = entries.iter().map(CommentEntry::serializer).collect();
        let body =
            plane_serializers::render_list(&serializers, &prepared).map_err(ViewError::from)?;
        Ok(ok(body))
    })
}

/// `IssueCommentPublicViewSet.retrieve` (`plane/space/views/issue.py:261-289`
/// + `get_object_or_404`).
fn comment_retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let pk: Uuid = ctx
            .kwarg("pk")
            .ok_or(ViewError::Key)?
            .parse()
            .map_err(|_| ViewError::Validation)?;
        let rows = comments(ctx, Some(pk)).await?;
        let Some(row) = rows.first() else {
            // `get_object_or_404(queryset, pk=pk)` (`django/shortcuts.py:76-84`).
            return Err(ViewError::Api(ApiError::not_found_detail(
                "No IssueComment matches the given query.",
            )));
        };
        let ids = [row.comment.id];
        let (reactions, grouped) = reactions_by_comment(ctx, &ids).await?;
        let references: Vec<&CommentReactionRow> = grouped
            .first()
            .map(|indexes| indexes.iter().map(|index| &reactions[*index]).collect())
            .unwrap_or_default();
        let entry = CommentEntry::new(row, &references);
        let prepared = plain_context(ctx);
        let body =
            plane_serializers::render(&entry.serializer(), &prepared).map_err(ViewError::from)?;
        Ok(ok(body))
    })
}

/// `comment.comment_reactions.all()` for each comment
/// (`IssueCommentSerializer.comment_reactions`): one statement, grouped in
/// memory. `CommentReaction.Meta.ordering = ("-created_at",)`
/// (`plane/db/models/issue.py:648`).
async fn reactions_by_comment(
    ctx: &ViewCtx<'_>,
    comments: &[Uuid],
) -> Result<(Vec<CommentReactionRow>, Vec<Vec<usize>>), ViewError> {
    if comments.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let client = client(ctx)?;
    let statement = crate::issues::sub_resources::sub_queries::COMMENT_REACTIONS
        .prepare(client)
        .await?;
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

// ---------------------------------------------------------------------------
// Comment reactions (route 243)
// ---------------------------------------------------------------------------

/// `CommentReactionPublicViewSet.list` (`plane/space/views/issue.py:498-517`).
fn comment_reaction_list<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let comment_id: Uuid = ctx
            .kwarg("comment_id")
            .ok_or(ViewError::Key)?
            .parse()
            .map_err(|_| ViewError::Validation)?;
        let client = client(ctx)?;
        let Some(deploy_board) = board::published_board(client, anchor).await? else {
            return Ok(ok(Json::Array(Vec::new()).to_drf()));
        };
        if !deploy_board.is_reactions_enabled {
            return Ok(ok(Json::Array(Vec::new()).to_drf()));
        }
        let statement = public_queries::COMMENT_REACTION_ROWS
            .prepare(client)
            .await?;
        let rows = client
            .query(
                &statement,
                &[
                    &deploy_board.workspace_id,
                    &deploy_board.project_id,
                    &comment_id,
                ],
            )
            .await?;
        let decoded: Vec<CommentReactionRow> = rows
            .iter()
            .map(|row| CommentReactionRow::from_row(row, 0))
            .collect::<Result<_, _>>()?;
        let prepared = plain_context(ctx);
        let serializers: Vec<_> = decoded.iter().map(CommentReactionRow::serializer).collect();
        let body =
            plane_serializers::render_list(&serializers, &prepared).map_err(ViewError::from)?;
        Ok(ok(body))
    })
}
