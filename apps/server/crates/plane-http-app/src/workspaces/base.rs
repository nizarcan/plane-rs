// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/base.py`: the slug check, `WorkSpaceViewSet` and
//! `WorkspaceThemeViewSet`.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspace-slug-check/` | `WorkSpaceAvailabilityCheckEndpoint` | `base.py:215-226` |
//! | `GET /api/workspaces/` | `WorkSpaceViewSet.list` | `base.py:64-80,153-155` |
//! | `GET /api/workspaces/{slug}/` | `WorkSpaceViewSet.retrieve` | `base.py:62,64-80` |
//! | `GET /api/workspaces/{slug}/workspace-themes/` | `WorkspaceThemeViewSet.list` | `base.py:323-329` |
//! | `GET /api/workspaces/{slug}/workspace-themes/{pk}/` | `WorkspaceThemeViewSet.retrieve` | `base.py:323-329` |
//!
//! `WorkSpaceViewSet.list` is unreachable in practice: its `allow_permission`
//! reads `kwargs["slug"]` on a route that has no `slug`
//! (`plane/app/permissions/base.py:62`), so every caller gets the family's
//! `400 {"error": "The required key does not exist."}` before the handler runs.
//! It is ported anyway, because the route exists and the runtime — not the
//! handler — is what answers that 400.

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::workspace::{Workspace, WorkspaceTheme};
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::fields::PgJson;
use plane_serializers::generated::app_workspace::{WorkSpaceSerializer, WorkspaceThemeSerializer};
use uuid::Uuid;

use crate::common::client;
use crate::workspaces::queries;
use crate::workspaces::rows;

/// `RESTRICTED_WORKSPACE_SLUGS` (`plane/utils/constants.py:5-73`), without the
/// duplicates the Python list carries — membership is what it is used for.
static RESTRICTED_WORKSPACE_SLUGS: &[&str] = &[
    "404",
    "accounts",
    "api",
    "create-workspace",
    "god-mode",
    "installations",
    "invitations",
    "onboarding",
    "profile",
    "spaces",
    "workspace-invitations",
    "password",
    "flags",
    "monitor",
    "monitoring",
    "ingest",
    "plane-pro",
    "plane-ultimate",
    "enterprise",
    "plane-enterprise",
    "disco",
    "silo",
    "chat",
    "calendar",
    "drive",
    "channels",
    "upgrade",
    "billing",
    "sign-in",
    "sign-up",
    "signin",
    "signup",
    "config",
    "live",
    "admin",
    "m",
    "import",
    "importers",
    "integrations",
    "integration",
    "configuration",
    "initiatives",
    "initiative",
    "workflow",
    "workflows",
    "epics",
    "epic",
    "story",
    "mobile",
    "dashboard",
    "desktop",
    "onload",
    "real-time",
    "one",
    "pages",
    "business",
    "pro",
    "settings",
    "license",
    "licenses",
    "instances",
    "instance",
];

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.workspace.base.WorkSpaceAvailabilityCheckEndpoint")?
        .method(Method::GET, availability_check)?;
    registry
        .viewset("plane.app.views.workspace.base.WorkSpaceViewSet")?
        .action("list", list)?
        .action("retrieve", retrieve)?;
    registry
        .viewset("plane.app.views.workspace.base.WorkspaceThemeViewSet")?
        .action("list", theme::list)?
        .action("retrieve", theme::retrieve)?;
    Ok(())
}

/// `WorkSpaceAvailabilityCheckEndpoint.get` (`base.py:216-226`).
///
/// `request.GET.get("slug", False)`: an absent parameter and an empty one are
/// the same 400 (`not slug`), and the restricted-slug test only runs when the
/// database lookup missed (`or` short-circuits).
fn availability_check<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.query().get("slug").unwrap_or("");
        if slug.is_empty() {
            return rows::error_body(400, "Workspace Slug is required");
        }
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::WORKSPACE_SLUG_EXISTS.sql)
            .await?;
        let row = client.query_one(&statement, &[&slug]).await?;
        let exists: bool = row.try_get(0)?;
        let taken = exists || RESTRICTED_WORKSPACE_SLUGS.contains(&slug);
        Ok(ViewResponse::json(
            200,
            serde_json::json!({ "status": !taken }),
        ))
    })
}

/// `WorkSpaceViewSet`'s queryset row: the workspace columns, the logo asset's
/// entity type and the `total_members` annotation.
struct WorkspaceRecord {
    model: Workspace,
    /// `Workspace.logo_url` (`plane/db/models/workspace.py:145-154`), resolved
    /// once so the serializer can borrow it.
    logo_url: Option<String>,
    total_members: i64,
}

impl WorkspaceRecord {
    fn from_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        let model = Workspace::from_row(row, 0)?;
        let logo_entity_type: Option<String> = row.try_get(14)?;
        let logo_url = rows::logo_url(
            model.logo_asset_id,
            logo_entity_type.as_deref(),
            model.logo.as_deref(),
        );
        Ok(Self {
            model,
            logo_url,
            total_members: row.try_get(15)?,
        })
    }

    fn serializer(&self) -> WorkSpaceSerializer<'_> {
        WorkSpaceSerializer {
            id: self.model.id,
            total_members: Some(self.total_members),
            logo_url: self.logo_url.as_deref(),
            // No read path of this viewset adds a `role` annotation, so the
            // field is absent from the instance. `IntegerField(read_only=True)`
            // over a missing attribute is a 500, which no reachable request
            // hits (see the module docs).
            role: None,
            created_at: self.model.created_at,
            updated_at: self.model.updated_at,
            deleted_at: self.model.deleted_at,
            name: &self.model.name,
            logo: self.model.logo.as_deref(),
            slug: &self.model.slug,
            organization_size: self.model.organization_size.as_deref(),
            timezone: &self.model.timezone,
            background_color: &self.model.background_color,
            created_by: self.model.created_by_id,
            updated_by: self.model.updated_by_id,
            logo_asset: self.model.logo_asset_id,
            owner: self.model.owner_id,
        }
    }
}

/// `WorkSpaceViewSet.list` (`base.py:153-155`) — `ListModelMixin.list` over
/// `get_queryset()`. Unreachable, see the module docs.
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let Some(slug) = ctx.kwarg("slug") else {
            return Err(ViewError::Key);
        };
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::WORKSPACE_FOR_MEMBER.sql)
            .await?;
        let Some(row) = client.query_opt(&statement, &[&user, &slug]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let record = WorkspaceRecord::from_row(&row)?;
        let body = rows::render_plain(ctx, &record.serializer())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkSpaceViewSet.retrieve` (`RetrieveModelMixin` over `get_queryset()`,
/// `base.py:64-80`).
///
/// The queryset filters on the caller's active membership, so a workspace the
/// caller does not belong to is a 404 with Django's message, not a 403.
fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::WORKSPACE_FOR_MEMBER.sql)
            .await?;
        let rows = client.query(&statement, &[&user, &slug]).await?;
        match rows.len() {
            0 => rows::not_found_detail("Workspace"),
            // `.get()`: two rows are `MultipleObjectsReturned`, a 500.
            1 => {
                let record = WorkspaceRecord::from_row(&rows[0])?;
                let body = rows::render_plain(ctx, &record.serializer())?;
                Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
            }
            _ => Err(rows::multiple_objects("Workspace")),
        }
    })
}

/// `WorkspaceThemeViewSet` (`base.py:323-337`).
pub mod theme {
    use super::*;

    fn serializer(model: &WorkspaceTheme) -> WorkspaceThemeSerializer<'_> {
        WorkspaceThemeSerializer {
            id: model.id,
            created_at: model.created_at,
            updated_at: model.updated_at,
            deleted_at: model.deleted_at,
            name: &model.name,
            colors: Some(PgJson::from(&model.colors)),
            created_by: model.created_by_id,
            updated_by: model.updated_by_id,
            workspace: model.workspace_id,
            actor: model.actor_id,
        }
    }

    /// `get_queryset()` + `ListModelMixin.list` (`base.py:328-329`).
    pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
        Box::pin(async move {
            let slug = ctx.kwarg("slug").unwrap_or_default();
            let client = client(ctx)?;
            let statement = client
                .prepare_cached(queries::THEMES_FOR_WORKSPACE.sql)
                .await?;
            let rows = client.query(&statement, &[&slug]).await?;
            let mut decoded = Vec::with_capacity(rows.len());
            for row in &rows {
                decoded.push(WorkspaceTheme::from_row(row, 0)?);
            }
            let serializers: Vec<_> = decoded.iter().map(serializer).collect();
            let body = rows::render_plain_list(ctx, &serializers)?;
            Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
        })
    }

    /// `RetrieveModelMixin.retrieve` over `get_queryset()` (`base.py:328-329`).
    pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
        Box::pin(async move {
            let slug = ctx.kwarg("slug").unwrap_or_default();
            let Some(pk) = ctx.kwarg("pk") else {
                return Err(ViewError::Key);
            };
            // `pk` is a `UUIDField` lookup: an unparsable value makes DRF's
            // `get_object_or_404` raise a bare `Http404`
            // (`rest_framework/generics.py:18-21`).
            let Ok(pk) = Uuid::parse_str(pk) else {
                return Err(rows::not_found());
            };
            let client = client(ctx)?;
            let statement = client.prepare_cached(queries::THEME_BY_ID.sql).await?;
            let Some(row) = client.query_opt(&statement, &[&slug, &pk]).await? else {
                return rows::not_found_detail("WorkspaceTheme");
            };
            let model = WorkspaceTheme::from_row(&row, 0)?;
            let body = rows::render_plain(ctx, &serializer(&model))?;
            Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
        })
    }
}
