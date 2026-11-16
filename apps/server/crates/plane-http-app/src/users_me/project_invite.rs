// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/project/invite.py` — project invitations addressed to the
//! signed-in user.
//!
//! `GET /api/users/me/workspaces/{slug}/projects/invitations/`
//! (`UserProjectInvitationsViewset.list`, `invite.py:132-142`).
//!
//! This `GET` answers **405** in Django: the router maps it to the
//! mixin-provided `list`, only `IsAuthenticated` is configured, and
//! `BaseViewSet._resolved_action_is_authorized` refuses an action whose
//! implementation comes from `rest_framework` (`plane/app/views/base.py:103-172`).
//! `plane-drf` runs the same guard before the handler, so the handler below is
//! never called for `GET`; it is a complete port of `list` anyway so the route
//! does not depend on that guard being the only thing that makes it correct.

use plane_core::error::APPLICATION_JSON;
use plane_db::models::project::ProjectMemberInvite;
use plane_db::models::workspace::Workspace;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::PgJson;
use plane_serializers::generated::app_project::{
    ProjectLiteSerializer, ProjectMemberInviteSerializer,
};
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use uuid::Uuid;

use super::queries;
use super::support::{self, AssetRef};

/// Register this module's handler.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.project.invite.UserProjectInvitationsViewset")?
        .action("list", list)?;
    Ok(())
}

struct ProjectParts {
    id: Uuid,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    logo_props: plane_db::JsonText,
    description: String,
}

struct InviteRow {
    invite: ProjectMemberInvite,
    project: ProjectParts,
    workspace: Workspace,
    project_asset: AssetRef,
    workspace_asset: AssetRef,
}

impl InviteRow {
    fn from_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        // `PROJECT_INVITES_BY_EMAIL`'s column order: invite (14), project (7),
        // cover asset (5), workspace (14), logo asset (5).
        const PROJECT_START: usize = ProjectMemberInvite::COLUMN_COUNT;
        const COVER_START: usize = PROJECT_START + 7;
        const WORKSPACE_START: usize = COVER_START + 5;
        const LOGO_START: usize = WORKSPACE_START + Workspace::COLUMN_COUNT;
        let invite = ProjectMemberInvite::from_row(row, 0)?;
        let project = ProjectParts {
            id: row.try_get(PROJECT_START)?,
            identifier: row.try_get(PROJECT_START + 1)?,
            name: row.try_get(PROJECT_START + 2)?,
            cover_image: row.try_get(PROJECT_START + 3)?,
            logo_props: row.try_get(PROJECT_START + 5)?,
            description: row.try_get(PROJECT_START + 6)?,
        };
        let project_asset = AssetRef::from_row(row, COVER_START)?;
        let workspace = Workspace::from_row(row, WORKSPACE_START)?;
        let workspace_asset = AssetRef::from_row(row, LOGO_START)?;
        Ok(Self {
            invite,
            project,
            workspace,
            project_asset,
            workspace_asset,
        })
    }
}

/// `UserProjectInvitationsViewset.list`.
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let email = support::user_email(ctx)?;
        let client = crate::common::client(ctx)?;
        let statement = queries::PROJECT_INVITES_BY_EMAIL.prepare(client).await?;
        let rows = client.query(&statement, &[&email]).await?;
        let mut invites = Vec::with_capacity(rows.len());
        for row in &rows {
            invites.push(InviteRow::from_row(row)?);
        }

        let mut project_cover_urls: Vec<Option<String>> = Vec::with_capacity(invites.len());
        let mut workspace_logo_urls: Vec<Option<String>> = Vec::with_capacity(invites.len());
        for invite in &invites {
            project_cover_urls.push(support::project_cover_image_url(
                invite.project.cover_image.as_deref(),
                &invite.project_asset,
            )?);
            workspace_logo_urls.push(support::workspace_logo_url(
                invite.workspace.logo.as_deref(),
                &invite.workspace_asset,
            )?);
        }

        let project_lites: Vec<ProjectLiteSerializer> = invites
            .iter()
            .zip(&project_cover_urls)
            .map(|(invite, cover_url)| ProjectLiteSerializer {
                id: invite.project.id,
                identifier: &invite.project.identifier,
                name: &invite.project.name,
                cover_image: invite.project.cover_image.as_deref(),
                cover_image_url: cover_url.as_deref(),
                logo_props: Some(PgJson::from_json_text(&invite.project.logo_props)),
                description: &invite.project.description,
            })
            .collect();
        let workspace_lites: Vec<WorkspaceLiteSerializer> = invites
            .iter()
            .zip(&workspace_logo_urls)
            .map(|(invite, logo_url)| WorkspaceLiteSerializer {
                name: &invite.workspace.name,
                slug: &invite.workspace.slug,
                id: invite.workspace.id,
                logo_url: logo_url.as_deref(),
            })
            .collect();

        let serializers: Vec<ProjectMemberInviteSerializer> = invites
            .iter()
            .zip(project_lites.iter().zip(&workspace_lites))
            .map(
                |(invite, (project, workspace))| ProjectMemberInviteSerializer {
                    id: invite.invite.id,
                    project: Some(project),
                    workspace: Some(workspace),
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
                },
            )
            .collect();

        let prepared = crate::common::RequestRender::from_ctx(ctx);
        let body = crate::common::render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
