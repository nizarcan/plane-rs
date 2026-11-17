// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The deploy-board resolution every `/api/public/` view shares, and its 404s
//! (`PUBLIC_INSTANCES.md` §2a).
//!
//! The public surface has no session and no workspace permission check: the
//! board **is** the authorization. Every handler starts by turning its
//! `anchor` into a `DeployBoard` row and then scopes everything it reads or
//! writes to that row's `workspace_id` / `project_id`. `is_disabled` and
//! `Project.network` are never read (`PUBLIC_INSTANCES.md` §5), and neither is
//! `entity_identifier != project_id` here — that mismatch is the runtime's
//! `DeployBoardAnchorMixin` 404, which runs before the handler.
//!
//! ## The three lookups
//!
//! `apps/api/plane/space/` uses three different ORM lookups for the same table,
//! and the difference is observable:
//!
//! | Python | Rows | Miss |
//! | --- | --- | --- |
//! | `.filter(anchor, entity_name="project").first()` (`issue.py:113`) | project boards | the view's own 404 body |
//! | `.filter(anchor).first()` (`asset.py:48`, `project.py:69`) | any board | the view's own 404 body |
//! | `.get(anchor, entity_name="project")` (`intake.py:58`, `meta.py:21`, `project.py:23`) | project boards | `DeployBoard.DoesNotExist` → the family's `ObjectDoesNotExist` 404 |
//! | `.get(anchor)` (`issue.py:690`, `asset.py:48`) | any board | the same `ObjectDoesNotExist` 404 |
//!
//! The `.first()` forms answer a page the view wrote itself
//! ([`project_not_published`], [`requested_resource_not_found`],
//! [`invalid_anchor`]); the `.get()` forms are a `DoesNotExist` that
//! `BaseAPIView.handle_exception` catches as `ObjectDoesNotExist`
//! (`plane/space/views/base.py:107-124`) and renders `{"error": "The required
//! object does not exist."}`. [`require_published_board`] and [`require_board`]
//! are those two.
//!
//! A `DeployBoard` soft-delete is a miss on all four, because `.objects` is
//! `SoftDeletionManager` (`plane/db/mixins.py:57`).
//!
//! ## The write predicates
//!
//! Every public write re-resolves the board and then binds the body's object
//! ids to it, so a caller cannot reach another board's issue through a board
//! they know (`PUBLIC_INSTANCES.md` §5, "Public write scoping"):
//!
//! * [`issue_in_board_scope`] — `_issue_in_board_scope`
//!   (`plane/space/views/issue.py:76-87`), which uses `Issue.issue_objects` so
//!   only what the board *displays* is writable;
//! * [`comment_in_board_scope`] — `_comment_in_board_scope`
//!   (`plane/space/views/issue.py:90-97`), restricted to `access="EXTERNAL"`;
//! * [`is_own_board_comment`] — `_is_own_board_comment`
//!   (`plane/space/views/asset.py:27-35`), the same scope **plus** the caller
//!   as the actor, which is what the public upload/restore/bulk routes check.
//!
//! All three are `.exists()`: a denial is a 404 the view writes, never a 403.

use deadpool_postgres::Client;
use plane_core::error::ApiError;
use plane_db::models::deploy_board::DeployBoard;
use plane_drf::error::ViewError;
use plane_drf::view::ViewResponse;
use uuid::Uuid;

use crate::public::queries;
use crate::write::error_body;

/// `Response({"error": "Project is not published"}, 404)` — the answer of every
/// view whose `anchor` does not resolve to a **project** board
/// (`plane/space/views/issue.py:115`, `meta.py:23,29`, `project.py:72` uses its
/// own body, `asset.py:92,182,210,236,261`).
#[must_use]
pub fn project_not_published() -> ViewResponse {
    error_body(404, "Project is not published")
}

/// `Response({"error": "Requested resource could not be found."}, 404)` — the
/// asset **GET**'s answer for an unknown anchor
/// (`plane/space/views/asset.py:50-54`). Its siblings use
/// [`project_not_published`] even on the same route.
#[must_use]
pub fn requested_resource_not_found() -> ViewResponse {
    error_body(404, "Requested resource could not be found.")
}

/// `Response({"error": "Invalid anchor"}, 404)` — `ProjectMembersEndpoint`
/// (`plane/space/views/project.py:69-74`), which resolves the board without an
/// `entity_name` filter.
#[must_use]
pub fn invalid_anchor() -> ViewResponse {
    error_body(404, "Invalid anchor")
}

/// `DeployBoard.objects.filter(anchor=anchor, entity_name="project").first()`.
///
/// `None` is "this anchor is not a published project board"; the caller writes
/// the 404 body ([`project_not_published`]).
///
/// # Errors
///
/// A database failure as the family's 500.
pub async fn published_board(
    client: &Client,
    anchor: &str,
) -> Result<Option<DeployBoard>, ViewError> {
    let statement = queries::PUBLISHED_BOARD_BY_ANCHOR.prepare(client).await?;
    let row = client.query_opt(&statement, &[&anchor]).await?;
    match row {
        Some(row) => Ok(Some(DeployBoard::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

/// `DeployBoard.objects.filter(anchor=anchor).first()` — no `entity_name`
/// filter, so a page board answers too (`plane/space/views/asset.py:48`,
/// `plane/space/views/project.py:69`).
///
/// # Errors
///
/// A database failure as the family's 500.
pub async fn board_by_anchor(
    client: &Client,
    anchor: &str,
) -> Result<Option<DeployBoard>, ViewError> {
    let statement = queries::BOARD_BY_ANCHOR.prepare(client).await?;
    let row = client.query_opt(&statement, &[&anchor]).await?;
    match row {
        Some(row) => Ok(Some(DeployBoard::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

/// `DeployBoard.objects.get(anchor=anchor, entity_name="project")` — the
/// `.get()` form of [`published_board`].
///
/// A miss is `DeployBoard.DoesNotExist`, which the group's `handle_exception`
/// renders as `ApiError::ObjectDoesNotExist` (404 `{"error": "The required
/// object does not exist."}`). `anchor` is unique
/// (`plane/db/models/deploy_board.py:19`), so `MultipleObjectsReturned` is
/// unreachable.
///
/// # Errors
///
/// 404 when the anchor is not a published project board; a database failure as
/// the family's 500.
pub async fn require_published_board(
    client: &Client,
    anchor: &str,
) -> Result<DeployBoard, ViewError> {
    published_board(client, anchor)
        .await?
        .ok_or(ViewError::Api(ApiError::ObjectDoesNotExist))
}

/// `DeployBoard.objects.get(anchor=anchor)` — the `.get()` form of
/// [`board_by_anchor`] (`plane/space/views/issue.py:690`,
/// `plane/space/views/asset.py:48`).
///
/// # Errors
///
/// 404 when the anchor is unknown; a database failure as the family's 500.
pub async fn require_board(client: &Client, anchor: &str) -> Result<DeployBoard, ViewError> {
    board_by_anchor(client, anchor)
        .await?
        .ok_or(ViewError::Api(ApiError::ObjectDoesNotExist))
}

/// `_issue_in_board_scope(issue_id, project_deploy_board)`
/// (`plane/space/views/issue.py:76-87`): a board-visible issue in the board's
/// own project and workspace.
///
/// # Errors
///
/// A database failure as the family's 500.
pub async fn issue_in_board_scope(
    client: &Client,
    issue_id: Uuid,
    board: &DeployBoard,
) -> Result<bool, ViewError> {
    let statement = queries::ISSUE_IN_BOARD_SCOPE.prepare(client).await?;
    let row = client
        .query_one(
            &statement,
            &[&issue_id, &board.project_id, &board.workspace_id],
        )
        .await?;
    Ok(row.get(0))
}

/// `_comment_in_board_scope(comment_id, project_deploy_board)`
/// (`plane/space/views/issue.py:90-97`): an `EXTERNAL` comment in the board's
/// own project and workspace.
///
/// # Errors
///
/// A database failure as the family's 500.
pub async fn comment_in_board_scope(
    client: &Client,
    comment_id: Uuid,
    board: &DeployBoard,
) -> Result<bool, ViewError> {
    let statement = queries::COMMENT_IN_BOARD_SCOPE.prepare(client).await?;
    let row = client
        .query_one(
            &statement,
            &[&comment_id, &board.project_id, &board.workspace_id],
        )
        .await?;
    Ok(row.get(0))
}

/// `_is_own_board_comment(comment_id, deploy_board, user)`
/// (`plane/space/views/asset.py:27-35`): an `EXTERNAL` comment in the board's
/// own project and workspace **authored by the caller**.
///
/// # Errors
///
/// A database failure as the family's 500.
pub async fn is_own_board_comment(
    client: &Client,
    comment_id: Uuid,
    board: &DeployBoard,
    actor_id: Uuid,
) -> Result<bool, ViewError> {
    let statement = queries::OWN_BOARD_COMMENT.prepare(client).await?;
    let row = client
        .query_one(
            &statement,
            &[
                &comment_id,
                &board.project_id,
                &board.workspace_id,
                &actor_id,
            ],
        )
        .await?;
    Ok(row.get(0))
}
