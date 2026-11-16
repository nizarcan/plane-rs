// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.project.invite` — the two read endpoints.
//!
//! Python: `apps/api/plane/app/views/project/invite.py:39-305`.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `GET …/projects/{project_id}/invitations/` | `ProjectInvitationsViewset.list` | `:57-59` |
//! | `GET …/projects/{project_id}/invitations/{pk}/` | `ProjectInvitationsViewset.retrieve` | `:61-63` |
//! | `GET …/projects/{project_id}/join/{pk}/` | `ProjectJoinEndpoint.get` | `:302-305` |
//!
//! `ProjectInvitationsViewset` is a `BaseViewSet` with `@allow_permission([ROLE.ADMIN])`
//! on both actions and `select_related("project")` + `select_related("workspace",
//! "workspace__owner")` in `get_queryset`; `ProjectJoinEndpoint` is `AllowAny`
//! and touches no permission at all, so any caller can read an invitation's
//! public half by id.
//!
//! Both serializers nest a `ProjectLiteSerializer` and a
//! `WorkspaceLiteSerializer`. Django loads the project/workspace rows in the
//! main statement on the invitations routes and lazily on the join route, then
//! one `file_assets` query per `*_url`; here all three statements join both
//! relations and the URLs come from one batched asset query.

use std::collections::HashSet;

use plane_core::error::APPLICATION_JSON;
use plane_db::models::project::ProjectMemberInvite;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_project::{
    ProjectMemberInvitePublicSerializer, ProjectMemberInviteSerializer,
};
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;

use crate::common::{client, lookup, render, render_list};
use crate::projects::joined;
use crate::projects::lite;
use crate::projects::queries;

/// `ProjectInvitationsViewset.list` (`plane/app/views/project/invite.py:57-59`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;

        let statement = queries::PROJECT_INVITATIONS.prepare(client(ctx)?).await?;
        let rows = client(ctx)?
            .query(&statement, &[&slug, &project_id])
            .await?;
        let mut invites = Vec::with_capacity(rows.len());
        let mut projects = Vec::with_capacity(rows.len());
        let mut workspaces = Vec::with_capacity(rows.len());
        for row in &rows {
            invites.push(ProjectMemberInvite::from_row(row, 0)?);
            projects.push(joined::project_at(row, 14)?);
            workspaces.push(joined::workspace_at(row, 21)?);
        }

        let mut asset_ids = HashSet::new();
        for project in &projects {
            asset_ids.extend(project.cover_image_asset_id);
        }
        for workspace in &workspaces {
            asset_ids.extend(workspace.logo_asset_id);
        }
        let urls = lite::load_asset_urls(client(ctx)?, &asset_ids).await?;
        for project in &mut projects {
            project.cover_image_url = lite::asset_or_text(
                project.cover_image_asset_id,
                project.cover_image.clone().unwrap_or_default(),
                &urls,
            )?;
        }
        for workspace in &mut workspaces {
            workspace.logo_url = lite::asset_or_text(
                workspace.logo_asset_id,
                workspace.logo.clone().unwrap_or_default(),
                &urls,
            )?;
        }

        let project_lites: Vec<ProjectLiteSerializer<'_>> =
            projects.iter().map(lite::LiteProject::lite).collect();
        let workspace_lites: Vec<WorkspaceLiteSerializer<'_>> =
            workspaces.iter().map(lite::LiteWorkspace::lite).collect();
        let serializers: Vec<ProjectMemberInviteSerializer<'_>> = invites
            .iter()
            .enumerate()
            .map(|(index, invite)| ProjectMemberInviteSerializer {
                id: invite.id,
                project: Some(&project_lites[index]),
                workspace: Some(&workspace_lites[index]),
                created_at: invite.created_at,
                updated_at: invite.updated_at,
                deleted_at: invite.deleted_at,
                email: &invite.email,
                accepted: invite.accepted,
                token: &invite.token,
                message: invite.message.as_deref(),
                responded_at: invite.responded_at,
                role: i64::from(invite.role),
                created_by: invite.created_by_id,
                updated_by: invite.updated_by_id,
            })
            .collect();
        let body = render_list(
            &serializers,
            &crate::common::render::RequestRender::from_ctx(ctx).render_ctx(),
        )?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `ProjectInvitationsViewset.retrieve` (`plane/app/views/project/invite.py:61-63`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;

        let row = client(ctx)?
            .query_opt(
                queries::PROJECT_INVITATION_ONE.sql,
                &[&slug, &project_id, &pk],
            )
            .await?;
        let Some(row) = row else {
            // DRF's `get_object()`: `Http404("No ProjectMemberInvite matches …")`.
            return crate::projects::not_found::no_match("ProjectMemberInvite");
        };
        let invite = ProjectMemberInvite::from_row(&row, 0)?;
        let mut project = joined::project_at(&row, 14)?;
        let mut workspace = joined::workspace_at(&row, 21)?;

        let mut asset_ids = HashSet::new();
        asset_ids.extend(project.cover_image_asset_id);
        asset_ids.extend(workspace.logo_asset_id);
        let urls = lite::load_asset_urls(client(ctx)?, &asset_ids).await?;
        project.cover_image_url = lite::asset_or_text(
            project.cover_image_asset_id,
            project.cover_image.clone().unwrap_or_default(),
            &urls,
        )?;
        workspace.logo_url = lite::asset_or_text(
            workspace.logo_asset_id,
            workspace.logo.clone().unwrap_or_default(),
            &urls,
        )?;

        let project_lite = project.lite();
        let workspace_lite = workspace.lite();
        let serializer = ProjectMemberInviteSerializer {
            id: invite.id,
            project: Some(&project_lite),
            workspace: Some(&workspace_lite),
            created_at: invite.created_at,
            updated_at: invite.updated_at,
            deleted_at: invite.deleted_at,
            email: &invite.email,
            accepted: invite.accepted,
            token: &invite.token,
            message: invite.message.as_deref(),
            responded_at: invite.responded_at,
            role: i64::from(invite.role),
            created_by: invite.created_by_id,
            updated_by: invite.updated_by_id,
        };
        let body = render(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `ProjectJoinEndpoint.get` (`plane/app/views/project/invite.py:302-305`).
pub fn join<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;

        let row = client(ctx)?
            .query_opt(
                queries::PROJECT_INVITATION_PUBLIC.sql,
                &[&slug, &project_id, &pk],
            )
            .await?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let invite = ProjectMemberInvite::from_row(&row, 0)?;
        let mut project = joined::project_at(&row, 14)?;
        let mut workspace = joined::workspace_at(&row, 21)?;

        let mut asset_ids = HashSet::new();
        asset_ids.extend(project.cover_image_asset_id);
        asset_ids.extend(workspace.logo_asset_id);
        let urls = lite::load_asset_urls(client(ctx)?, &asset_ids).await?;
        project.cover_image_url = lite::asset_or_text(
            project.cover_image_asset_id,
            project.cover_image.clone().unwrap_or_default(),
            &urls,
        )?;
        workspace.logo_url = lite::asset_or_text(
            workspace.logo_asset_id,
            workspace.logo.clone().unwrap_or_default(),
            &urls,
        )?;

        let project_lite = project.lite();
        let workspace_lite = workspace.lite();
        let serializer = ProjectMemberInvitePublicSerializer {
            id: invite.id,
            project: Some(&project_lite),
            workspace: Some(&workspace_lite),
            role: i64::from(invite.role),
            message: invite.message.as_deref(),
            accepted: invite.accepted,
            responded_at: invite.responded_at,
        };
        // `ProjectMemberInvitePublicSerializer` is a plain `BaseSerializer`, so
        // `?fields=`/`?expand=` never reach it.
        let body = render(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
