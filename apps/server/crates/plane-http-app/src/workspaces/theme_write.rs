// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkspaceThemeViewSet`'s write actions
//! (`plane/app/views/workspace/base.py:323-337`) — workspace-core set A.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 199 `POST …/workspace-themes/` | `POST` | `create` (`base.py:331-337`) | [`create`] |
//! | 200 `PATCH …/workspace-themes/{pk}/` | `PATCH` | `UpdateModelMixin.partial_update` over `get_queryset` | [`partial_update`] |
//! | 200 | `DELETE` | `DestroyModelMixin.destroy` over `get_queryset` | [`destroy`] |
//!
//! `WorkSpaceAdminPermission` guards every verb (`permissions/workspace.py:61-71`):
//! ADMIN **or** MEMBER, checked before the handler, so an outsider is 403 even
//! when the workspace does not exist.
//!
//! ## What Django does per action
//!
//! **create**: `Workspace.objects.get(slug=slug)` — a soft-deleted workspace is
//! the family 404 — then the serializer; `is_valid()` without
//! `raise_exception`, so the errors are the body (and no row is written). A
//! duplicate **name** is not a serializer error: `workspace` is read-only, so
//! DRF cannot check `unique_together` and the INSERT hits the partial unique
//! index instead — an `IntegrityError`, i.e. the family's
//! `400 {"error": "The payload is not valid"}`.
//!
//! `serializer.save(workspace=…, actor=request.user)` runs H01: on a create
//! `created_by` is the caller and `updated_by` is NULL, whatever the body said
//! (`plane/db/models/base.py:36-42`).
//!
//! **partial_update** and **destroy** resolve `get_queryset()`
//! (`base.py:328-329`): `WorkspaceTheme.objects.filter(workspace__slug=slug)`,
//! so a theme of another workspace is DRF's `{"detail": "No WorkspaceTheme
//! matches the given query."}`. On a PATCH, `created_by` **is** writable and is
//! written verbatim; only `updated_by` is replaced (`base.py:40-42`).
//!
//! **destroy** is H03: one full save with `deleted_at = now`, then the
//! `soft_delete_related_objects` publish for `("db", "workspacetheme")`
//! (`plane/db/mixins.py:65-77`).

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks::audit::Audit;
use plane_db::models::workspace::WorkspaceTheme;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::fields::PgJson;
use plane_serializers::generated::app_workspace::WorkspaceThemeSerializer;
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::views::write_support::integrity;
use crate::workspaces::queries;
use crate::workspaces::rows;
use crate::workspaces::write_a_queries as write_queries;
use crate::write::{self, Serializer};

/// `plane.app.views.workspace.base.WorkspaceThemeViewSet`.
const VIEW: &str = "plane.app.views.workspace.base.WorkspaceThemeViewSet";

/// Register the write verbs of routes 199 and 200.
///
/// `base::register` already registers `list` and `retrieve` for this viewset;
/// the registry accumulates actions per view.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset(VIEW)?
        .action("create", create)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?;
    Ok(())
}

/// `WorkspaceThemeSerializer`'s writable fields in `self.fields` order — the
/// order DRF reports `serializer.errors` in. `workspace` and `actor` are
/// read-only (`Meta.read_only_fields`), so they never produce one.
const FIELD_ORDER: &[&str] = &["deleted_at", "name", "colors", "created_by", "updated_by"];

/// A theme body from one row (`serializer.data`).
fn render(
    ctx: &ViewCtx<'_>,
    theme: &WorkspaceTheme,
    status: u16,
) -> Result<ViewResponse, ViewError> {
    let serializer = WorkspaceThemeSerializer {
        id: theme.id,
        created_at: theme.created_at,
        updated_at: theme.updated_at,
        deleted_at: theme.deleted_at,
        name: &theme.name,
        colors: Some(PgJson::from(&theme.colors)),
        created_by: theme.created_by_id,
        updated_by: theme.updated_by_id,
        workspace: theme.workspace_id,
        actor: theme.actor_id,
    };
    let body = rows::render_plain(ctx, &serializer)?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// `WorkspaceThemeViewSet.create` (`plane/app/views/workspace/base.py:331-337`).
fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`:332`).
        let workspace = lookup::workspace_by_slug(client, slug).await?;

        let mut serializer = Serializer::new(ctx, false)?;
        write::generated::workspace_theme_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(mut errors) => {
                errors.sort_by_field_order(FIELD_ORDER);
                return Ok(errors.into_response());
            }
        };

        let now = write::now();
        let theme = WorkspaceTheme {
            created_at: now,
            updated_at: now,
            // H01 on create: the caller wins over a body `created_by`, and
            // `updated_by` is NULL.
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: values.datetime("deleted_at"),
            id: Uuid::new_v4(),
            workspace_id: workspace.id,
            name: values.text("name").unwrap_or_default().to_string(),
            actor_id: user,
            colors: values
                .json("colors")
                .cloned()
                .unwrap_or_else(|| JsonText::new("{}")),
        };

        let statement = write_queries::THEME_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &theme.created_at,
                    &theme.updated_at,
                    &theme.created_by_id,
                    &theme.updated_by_id,
                    &theme.deleted_at,
                    &theme.id,
                    &theme.workspace_id,
                    &theme.name,
                    &theme.actor_id,
                    &theme.colors,
                ],
            )
            .await
            .map_err(integrity)?;

        render(ctx, &theme, 201)
    })
}

/// `PATCH …/workspace-themes/{pk}/` (DRF's `UpdateModelMixin.partial_update`).
fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = client(ctx)?;
        let Some(mut theme) = get_object(ctx, client).await? else {
            return rows::not_found_detail("WorkspaceTheme");
        };

        let mut serializer = Serializer::new(ctx, true)?;
        write::generated::workspace_theme_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(mut errors) => {
                errors.sort_by_field_order(FIELD_ORDER);
                return Ok(errors.into_response());
            }
        };

        // `ModelSerializer.update`: assign every validated field, then `save()`.
        if values.contains("deleted_at") {
            theme.deleted_at = values.datetime("deleted_at");
        }
        if let Some(name) = values.text("name") {
            theme.name = name.to_string();
        }
        if let Some(colors) = values.json("colors") {
            theme.colors = colors.clone();
        }
        if values.contains("created_by") {
            theme.created_by_id = values.uuid("created_by");
        }
        // `updated_by` is writable — DRF validates it, H01 then overwrites it.

        // H01 on update: `updated_at` from `auto_now`, `updated_by` from the user.
        theme.updated_at = write::now();
        theme.updated_by_id = Audit::on_update(Some(user));

        let statement = write_queries::THEME_SAVE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &theme.created_at,
                    &theme.updated_at,
                    &theme.created_by_id,
                    &theme.updated_by_id,
                    &theme.deleted_at,
                    &theme.workspace_id,
                    &theme.name,
                    &theme.actor_id,
                    &theme.colors,
                    &theme.id,
                ],
            )
            .await
            .map_err(integrity)?;

        render(ctx, &theme, 200)
    })
}

/// `DELETE …/workspace-themes/{pk}/` (DRF's `DestroyModelMixin.destroy`).
fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = client(ctx)?;
        let Some(theme) = get_object(ctx, client).await? else {
            return rows::not_found_detail("WorkspaceTheme");
        };

        // H03: `deleted_at = now`, a full save (`updated_at`, `updated_by`) and
        // the cascade publish.
        let now = write::now();
        write::soft_delete(
            client,
            "workspace_themes",
            "id",
            theme.id,
            Audit::on_update(Some(user)),
            now,
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "workspacetheme", theme.id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `get_object()` over `WorkspaceThemeViewSet.get_queryset()`
/// (`plane/app/views/workspace/base.py:328-329`).
///
/// `Ok(None)` is the `Http404` `get_object_or_404` raises for a missing row —
/// the handler answers it with the query-set message — while an unparsable `pk`
/// is DRF's bare `Http404` (`rest_framework/generics.py:18-21`).
async fn get_object(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
) -> Result<Option<WorkspaceTheme>, ViewError> {
    let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
    let Some(pk) = ctx.kwarg("pk") else {
        return Err(ViewError::Key);
    };
    let Ok(pk) = Uuid::parse_str(pk) else {
        return Err(rows::not_found());
    };
    let statement = queries::THEME_BY_ID.prepare(client).await?;
    match client.query_opt(&statement, &[&slug, &pk]).await? {
        Some(row) => Ok(Some(WorkspaceTheme::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_order_matches_the_generated_table() {
        let generated: Vec<&str> = write::generated::workspace_theme_serializer::FIELDS
            .iter()
            .map(|field| field.name)
            .collect();
        assert_eq!(generated, FIELD_ORDER);
    }
}
