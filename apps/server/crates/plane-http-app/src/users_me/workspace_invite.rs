// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/invite.py` — the invitation list addressed to
//! the signed-in user.
//!
//! `GET /api/users/me/workspaces/invitations/`
//! (`UserWorkspaceInvitationsViewSet.list`, `invite.py:259-273`).
//!
//! The view is self-service with no `slug`, so the role decorators do not
//! apply; authorization is the `email=request.user.email` filter in
//! `get_queryset`. `filter_queryset` runs `DjangoFilterBackend` (no
//! `filterset_fields`, so nothing) and `SearchFilter` (no `search_fields`, so
//! nothing); neither can change the result, and `?search=`/`?owner=` are
//! ignored exactly as they are upstream.

use plane_core::error::APPLICATION_JSON;
use plane_db::models::workspace::{Workspace, WorkspaceMemberInvite};
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_workspace::{
    WorkSpaceMemberInviteSerializer, WorkspaceLiteSerializer,
};

use super::queries;
use super::support::{self, AssetRef};

/// Register this module's handler.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.workspace.invite.UserWorkspaceInvitationsViewSet")?
        .action("list", list)?;
    Ok(())
}

/// One decoded invite plus the workspace data its serializer needs.
struct InviteRow {
    invite: WorkspaceMemberInvite,
    workspace: Workspace,
    index: usize,
}

/// `UserWorkspaceInvitationsViewSet.list`
/// (`plane/app/views/workspace/invite.py:263-273`).
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let email = support::user_email(ctx)?;
        let client = crate::common::client(ctx)?;
        let statement = queries::INVITES_BY_EMAIL.prepare(client).await?;
        let rows = client.query(&statement, &[&email]).await?;

        let mut logo_urls: Vec<Option<String>> = Vec::with_capacity(rows.len());
        let mut invites: Vec<InviteRow> = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let invite = WorkspaceMemberInvite::from_row(row, 0)?;
            let workspace = Workspace::from_row(row, WorkspaceMemberInvite::COLUMN_COUNT)?;
            let logo = AssetRef::from_row(
                row,
                WorkspaceMemberInvite::COLUMN_COUNT + Workspace::COLUMN_COUNT,
            )?;
            logo_urls.push(support::workspace_logo_url(
                workspace.logo.as_deref(),
                &logo,
            )?);
            invites.push(InviteRow {
                invite,
                workspace,
                index,
            });
        }

        let workspace_lites: Vec<WorkspaceLiteSerializer> = invites
            .iter()
            .map(|invite| WorkspaceLiteSerializer {
                name: &invite.workspace.name,
                slug: &invite.workspace.slug,
                id: invite.workspace.id,
                logo_url: logo_urls[invite.index].as_deref(),
            })
            .collect();

        let serializers: Vec<WorkSpaceMemberInviteSerializer> = invites
            .iter()
            .zip(&workspace_lites)
            .map(|(invite, workspace)| WorkSpaceMemberInviteSerializer {
                id: invite.invite.id,
                workspace: Some(workspace),
                invite_link: plane_serializers::value::DrfValue::String(format!(
                    "/workspace-invitations/?invitation_id={}&slug={}&token={}",
                    invite.invite.id, invite.workspace.slug, invite.invite.token
                )),
                created_at: invite.invite.created_at,
                updated_at: invite.invite.updated_at,
                deleted_at: invite.invite.deleted_at,
                email: &invite.invite.email,
                accepted: invite.invite.accepted,
                token: &invite.invite.token,
                message: invite.invite.message.as_deref(),
                responded_at: invite.invite.responded_at,
                role: i64::from(invite.invite.role),
                created_by: invite.invite.created_by_id,
                updated_by: invite.invite.updated_by_id,
            })
            .collect();

        let prepared = crate::common::RequestRender::from_ctx(ctx);
        let body = crate::common::render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
