// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/invite.py`: workspace invitations, read paths.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/invitations/` | `WorkspaceInvitationsViewset.list` | `invite.py:44-50` |
//! | `GET /api/workspaces/{slug}/invitations/{pk}/` | `WorkspaceInvitationsViewset.retrieve` | `invite.py:44-50` |
//! | `GET /api/workspaces/{slug}/invitations/{pk}/join/` | `WorkspaceJoinEndpoint.get` | `invite.py:250-256` |
//!
//! `get_invite_link` (`invite.py:44-47` of the serializer, `plane/app/
//! serializers/workspace.py:96-98`) builds the frontend link from the row's
//! `id`, its workspace slug and its token; it is a `SerializerMethodField`, so
//! the string is built here.
//!
//! The `join` route is `AllowAny` and uses the *public* serializer, which omits
//! `token` and `invite_link` (`serializers/workspace.py:112-139`, the
//! GHSA-86mg-259g-pwgg fix).

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::workspace::WorkspaceMemberInvite;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_workspace::{
    WorkSpaceMemberInvitePublicSerializer, WorkSpaceMemberInviteSerializer,
};
use plane_serializers::value::DrfValue;
use uuid::Uuid;

use crate::common::client;
use crate::workspaces::queries;
use crate::workspaces::rows::{self, WorkspaceLite};

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.workspace.invite.WorkspaceInvitationsViewset")?
        .action("list", list)?
        .action("retrieve", retrieve)?;
    registry
        .api_view("plane.app.views.workspace.invite.WorkspaceJoinEndpoint")?
        .method(Method::GET, join::get)?;
    Ok(())
}

/// One decoded invite plus the nested workspace the serializer needs.
struct InviteRecord {
    model: WorkspaceMemberInvite,
    workspace: WorkspaceLite,
}

impl InviteRecord {
    fn from_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        let model = WorkspaceMemberInvite::from_row(row, 0)?;
        // Columns 13..19: the workspace lite fields and its logo asset type.
        let name: String = row.try_get(13)?;
        let slug: String = row.try_get(14)?;
        let id: Uuid = row.try_get(15)?;
        let logo_asset_id: Option<Uuid> = row.try_get(16)?;
        let logo: Option<String> = row.try_get(17)?;
        let logo_entity_type: Option<String> = row.try_get(18)?;
        let workspace = WorkspaceLite::new(
            id,
            name,
            slug,
            logo_asset_id,
            logo_entity_type.as_deref(),
            logo.as_deref(),
        );
        Ok(Self { model, workspace })
    }

    /// `WorkSpaceMemberInviteSerializer`
    /// (`plane/app/serializers/workspace.py:88-110`).
    fn serializer<'a>(
        &'a self,
        workspace: &'a plane_serializers::generated::app_workspace::WorkspaceLiteSerializer<'a>,
    ) -> WorkSpaceMemberInviteSerializer<'a> {
        let invite_link = format!(
            "/workspace-invitations/?invitation_id={}&slug={}&token={}",
            self.model.id, self.workspace.slug, self.model.token
        );
        WorkSpaceMemberInviteSerializer {
            id: self.model.id,
            workspace: Some(workspace),
            invite_link: DrfValue::String(invite_link),
            created_at: self.model.created_at,
            updated_at: self.model.updated_at,
            deleted_at: self.model.deleted_at,
            email: &self.model.email,
            accepted: self.model.accepted,
            token: &self.model.token,
            message: self.model.message.as_deref(),
            responded_at: self.model.responded_at,
            role: i64::from(self.model.role),
            created_by: self.model.created_by_id,
            updated_by: self.model.updated_by_id,
        }
    }

    /// `WorkSpaceMemberInvitePublicSerializer`
    /// (`plane/app/serializers/workspace.py:112-139`).
    fn public_serializer<'a>(
        &'a self,
        workspace: &'a plane_serializers::generated::app_workspace::WorkspaceLiteSerializer<'a>,
    ) -> WorkSpaceMemberInvitePublicSerializer<'a> {
        WorkSpaceMemberInvitePublicSerializer {
            id: self.model.id,
            email: &self.model.email,
            workspace: Some(workspace),
            role: i64::from(self.model.role),
            message: self.model.message.as_deref(),
            accepted: self.model.accepted,
            responded_at: self.model.responded_at,
            created_at: self.model.created_at,
            updated_at: self.model.updated_at,
            created_by: self.model.created_by_id,
        }
    }
}

/// `WorkspaceInvitationsViewset`'s `ListModelMixin.list`
/// (`invite.py:44-50`, `rest_framework/mixins.py`).
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::INVITES_FOR_WORKSPACE.sql)
            .await?;
        let rows = client.query(&statement, &[&slug]).await?;
        let mut decoded = Vec::with_capacity(rows.len());
        for row in &rows {
            decoded.push(InviteRecord::from_row(row)?);
        }
        let workspaces: Vec<_> = decoded
            .iter()
            .map(|record| record.workspace.serializer())
            .collect();
        let serializers: Vec<_> = decoded
            .iter()
            .zip(workspaces.iter())
            .map(|(record, workspace)| record.serializer(workspace))
            .collect();
        let body = rows::render_plain_list(ctx, &serializers)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `RetrieveModelMixin.retrieve` over the same queryset
/// (`invite.py:44-50`): a miss is Django's `No WorkspaceMemberInvite matches
/// the given query.`; an unparsable `pk` is a bare `Http404`.
fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(ViewError::Key);
        };
        let Ok(pk) = Uuid::parse_str(pk) else {
            return Err(rows::not_found());
        };
        let client = client(ctx)?;
        let statement = client.prepare_cached(queries::INVITE_BY_ID.sql).await?;
        let Some(row) = client.query_opt(&statement, &[&slug, &pk]).await? else {
            return rows::not_found_detail("WorkspaceMemberInvite");
        };
        let record = InviteRecord::from_row(&row)?;
        let workspace = record.workspace.serializer();
        let body = rows::render_plain(ctx, &record.serializer(&workspace))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkspaceJoinEndpoint.get` (`invite.py:250-256`).
pub mod join {
    use super::*;

    /// `WorkspaceMemberInvite.objects.get(workspace__slug=slug, pk=pk)` — a
    /// `Model.DoesNotExist` is the family's 404, not `get_object_or_404`.
    pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
        Box::pin(async move {
            let slug = ctx.kwarg("slug").unwrap_or_default();
            let Some(pk) = ctx.kwarg("pk") else {
                return Err(ViewError::Key);
            };
            // `get(pk=…)` coerces the kwarg through `UUIDField.to_python`.
            let Ok(pk) = Uuid::parse_str(pk) else {
                return Err(ViewError::Validation);
            };
            let client = client(ctx)?;
            let statement = client.prepare_cached(queries::INVITE_BY_ID.sql).await?;
            let rows = client.query(&statement, &[&slug, &pk]).await?;
            match rows.len() {
                0 => Err(ViewError::ObjectDoesNotExist),
                1 => {
                    let record = InviteRecord::from_row(&rows[0])?;
                    let workspace = record.workspace.serializer();
                    let body = rows::render_plain(ctx, &record.public_serializer(&workspace))?;
                    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
                }
                _ => Err(rows::multiple_objects("WorkspaceMemberInvite")),
            }
        })
    }
}
