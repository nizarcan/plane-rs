// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `public` (`deploy/route-groups.yaml`): the anonymous project
//! board under `/api/public/`.
//!
//! Python: `apps/api/plane/space/` — 25 routes, mounted by `plane/urls.py` at
//! `/api/public/` and wired in `plane/space/urls/{issue,intake,project,asset}.py`.
//!
//! This module holds the pieces every route of the group shares; the handlers
//! land with the porting sets:
//!
//! * [`board`] — the deploy-board resolution every view starts with, its 404s
//!   and the write-scope predicates (`PUBLIC_INSTANCES.md` §2a);
//! * [`queries`] — the statements [`board`] and the handlers prepare;
//! * [`board_read`] — the reads that only need the board row
//!   (`WorkspaceProjectDeployBoardEndpoint`, `WorkspaceProjectAnchorEndpoint`,
//!   and the intake-issue list/retrieve: set A);
//! * [`meta`] — `ProjectMetaDataEndpoint` and
//!   `ProjectDeployBoardPublicSettingsEndpoint`;
//! * [`lists`] — the five `{id, name}`-style list endpoints (cycles, modules,
//!   states, labels, members);
//! * [`project`] — the public comment list/retrieve and the reaction/vote
//!   lists (set A's GET halves of 239-245);
//! * [`assets_read`] — the asset `GET`s (`assets/v2/anchor/{anchor}/` and
//!   `assets/v2/anchor/{anchor}/{pk}/`);
//! * [`issue_query`] — `ProjectIssuesPublicEndpoint` and
//!   `IssueRetrievePublicEndpoint`.
//!
//! Set B adds the write verbs; each module registers them on the view that
//! already serves the corresponding read, because one Django view can serve
//! both:
//!
//! * [`intake_write`] — routes 234/235 `create` and 236
//!   `partial_update`/`destroy` on `IntakeIssuePublicViewSet`;
//! * [`comments_write`] — routes 239 `create` and 240
//!   `partial_update`/`destroy` on `IssueCommentPublicViewSet`;
//! * [`reactions_write`] — routes 241-244 on the two reaction viewsets;
//! * [`votes_write`] — route 245's `create`/`destroy` on
//!   `IssueVotePublicViewSet`;
//! * [`assets_write`] — routes 255/256 `post`/`patch`/`delete`,
//!   `AssetRestoreEndpoint.post` (257) and `EntityBulkAssetEndpoint.post`
//!   (258).
//!
//! Two shared behaviours live in the runtime, not here:
//!
//! * `DeployBoardAnchorMixin.initial` (`plane/space/views/base.py:47-59`) —
//!   the 404 for a board whose `entity_identifier` is not its `project_id` —
//!   is `plane_drf::dispatch`'s `InitialExtra::DeployBoardAnchor`
//!   (`plane-drf/src/dispatch.rs:472-477,740-753`), and it runs **after**
//!   `check_permissions`, so an anonymous request on an `IsAuthenticated` route
//!   is a 401 before the anchor is read;
//! * the 500 body of the group's `handle_exception`
//!   (`plane/space/views/base.py:91-124`) is the family error type the
//!   dispatcher already renders (`ApiError::FamilyServerError`).

use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;

pub mod assets_read;
pub mod assets_write;
pub mod board;
pub mod board_read;
pub mod comments_write;
pub mod intake_read;
pub mod intake_write;
pub mod issue_list;
pub mod issue_query;
pub mod lists;
pub mod meta;
pub mod project;
pub mod queries;
pub mod reactions_write;
pub mod votes_write;

/// Register this group's handlers, in route order.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    board_read::register(registry)?;
    issue_query::register(registry)?;
    project::register(registry)?;
    meta::register(registry)?;
    lists::register(registry)?;
    assets_read::register(registry)?;
    assets_write::register(registry)?;
    comments_write::register(registry)?;
    intake_write::register(registry)?;
    reactions_write::register(registry)?;
    votes_write::register(registry)?;
    Ok(())
}

/// The context a serializer renders under on this surface.
///
/// None of the serializers the group renders is a `DynamicBaseSerializer`
/// (`DeployBoardSerializer`, `ProjectLiteSerializer`,
/// `IssueStateIntakeSerializer`, `IssueCommentSerializer` and their nested
/// serializers are plain `BaseSerializer`s, `SERIALIZERS.md`), and the group's
/// base classes (`plane/space/views/base.py`) do not extend Plane's dynamic
/// view base, so `?fields=` and `?expand=` never reach a serializer here. The
/// context is therefore the timezone and nothing else.
pub fn plain_context<'a>(ctx: &'a ViewCtx<'a>) -> RenderCtx<'a> {
    let timezone: &'a Tz = match ctx.timezone {
        Some(timezone) => timezone,
        None => &chrono_tz::UTC,
    };
    RenderCtx::new(timezone, None, &[])
}

/// `DeployBoard.workspace.slug` — the FK read every board-scoped view makes
/// (`plane/space/views/cycle.py:24`, `issue.py:118,695`). A soft-deleted
/// workspace is `Workspace.DoesNotExist` → the family 404.
pub async fn workspace_slug(
    client: &deadpool_postgres::Client,
    workspace_id: uuid::Uuid,
) -> Result<String, ViewError> {
    let statement = queries::WORKSPACE_SLUG.prepare(client).await?;
    let row = client.query_opt(&statement, &[&workspace_id]).await?;
    row.map(|row| row.get(0))
        .ok_or(ViewError::ObjectDoesNotExist)
}

/// `Response(status=200)` of an already-rendered body.
pub fn ok(body: Vec<u8>) -> ViewResponse {
    ViewResponse::raw(200, plane_core::error::APPLICATION_JSON, body)
}

/// A handler that only ever raises: the ported 500 of a view whose Python
/// always raises (see [`board_read::workspace_project_deploy_board`]).
pub fn always_server_error<'a>(
    reason: &'static str,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { Err(ViewError::Unknown(reason.to_owned())) })
}

#[cfg(test)]
mod tests {
    use plane_query::json::Json;

    /// `JSONRenderer.render(None)` is the empty body, and the DRF dialect
    /// reproduces that only at depth 0 — which is why a field value must be
    /// rendered at depth 1 (`issue_query::value_bytes`).
    #[test]
    fn a_top_level_null_is_the_empty_body() {
        assert_eq!(Json::Null.to_drf(), b"");
        assert_eq!(Json::opt_uuid(None).to_drf(), b"");
    }
}
