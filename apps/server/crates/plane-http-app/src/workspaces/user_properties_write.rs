// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkspaceUserPropertiesEndpoint.patch` (`plane/app/views/workspace/user.py:
//! 274-286`) — workspace-core set C, route 207.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 207 `PATCH /api/workspaces/{slug}/user-properties/` | `PATCH` | `user.py:274-286` | [`patch`] |
//!
//! ## The 400 that writes a row
//!
//! The view creates the properties row **before** the serializer runs
//! (`user.py:277-279`), and no view in this family opens a transaction
//! (`ATOMIC_REQUESTS` is unset), so an invalid PATCH answers 400 with the row
//! committed — and a `get_or_create` miss is not a 404 the way a lookup would
//! be. The statement order is therefore: `Workspace.objects.get` → the
//! `get_or_create` → body parse → field validation → `save()` → render from the
//! in-memory instance.
//!
//! ## What the in-memory instance renders
//!
//! `serializer.data` comes from the instance, not a re-read. A row this request
//! created still holds the model's Python default dicts (their insertion
//! order); a row read from the table holds `json.loads(jsonb_text)` (Postgres'
//! length-then-bytes key order); a field the body carried holds the request's
//! order. All three are reproduced by choosing the starting text per field and
//! replacing only the validated ones.
//!
//! `created_by` is writable here (only `workspace` and `user` are read-only),
//! so a body value is written verbatim and only `updated_by` is replaced by
//! H01 (`plane/db/models/base.py:36-42`). `deleted_at` is writable too: a body
//! value soft-deletes the row with **no** cascade publish, because the view
//! calls `save()`, not `delete()`.

use http::Method;
use plane_db::JsonText;
use plane_db::hooks::audit::Audit;
use plane_db::models::workspace::WorkspaceUserProperties;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::fields::PgJson;
use plane_serializers::generated::app_workspace::WorkspaceUserPropertiesSerializer;

use crate::common::{client, lookup};
use crate::workspaces::rows;
use crate::workspaces::user::{
    DEFAULT_DISPLAY_FILTERS, DEFAULT_DISPLAY_PROPERTIES, DEFAULT_FILTERS, get_or_create,
};
use crate::workspaces::write_c_queries as queries;
use crate::write::{self, Serializer};

/// `plane.app.views.workspace.user.WorkspaceUserPropertiesEndpoint`.
const VIEW: &str = "plane.app.views.workspace.user.WorkspaceUserPropertiesEndpoint";

/// `WorkspaceUserPropertiesSerializer`'s writable fields in `self.fields`
/// order — the order DRF reports `serializer.errors` in. `workspace` and `user`
/// are read-only (`Meta.read_only_fields`), so they never produce one.
const FIELD_ORDER: &[&str] = &[
    "deleted_at",
    "filters",
    "display_filters",
    "display_properties",
    "rich_filters",
    "navigation_project_limit",
    "navigation_control_preference",
    "created_by",
    "updated_by",
];

/// Register route 207's write verb.
///
/// `user::register` already registers `get` for this view; the registry
/// accumulates methods per view.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.api_view(VIEW)?.method(Method::PATCH, patch)?;
    Ok(())
}

/// `WorkspaceUserPropertiesEndpoint.patch` (`user.py:274-286`).
fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`:275`): a soft-deleted workspace
        // is the family 404, even though the class permission passed.
        let workspace = lookup::workspace_by_slug(client, slug).await?;

        // `get_or_create(user=request.user, workspace_id=workspace.id)`
        // (`:277-279`) — commits before anything is validated.
        let (mut row, created) = get_or_create(client, workspace.id, user).await?;

        // `WorkspaceUserPropertiesSerializer(workspace_properties,
        // data=request.data, partial=True)` (`:281`): the body is parsed here,
        // after the row exists.
        let mut serializer = Serializer::new(ctx, true)?;
        write::generated::workspace_user_properties_serializer::validate(&mut serializer, client)
            .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(mut errors) => {
                errors.sort_by_field_order(FIELD_ORDER);
                return Ok(errors.into_response());
            }
        };

        // A row Django created carries the model defaults in memory, not the
        // `jsonb` text a re-read would produce (`plane/db/models/workspace.py:
        // 352-390`).
        if created {
            row.filters = JsonText::new(DEFAULT_FILTERS);
            row.display_filters = JsonText::new(DEFAULT_DISPLAY_FILTERS);
            row.display_properties = JsonText::new(DEFAULT_DISPLAY_PROPERTIES);
            row.rich_filters = JsonText::new("{}");
            row.navigation_project_limit = 10;
            row.navigation_control_preference = "ACCORDION".to_owned();
        }

        // `ModelSerializer.update`: assign every validated field, then `save()`.
        if values.contains("deleted_at") {
            row.deleted_at = values.datetime("deleted_at");
        }
        if let Some(filters) = values.json("filters") {
            row.filters = filters.clone();
        }
        if let Some(display_filters) = values.json("display_filters") {
            row.display_filters = display_filters.clone();
        }
        if let Some(display_properties) = values.json("display_properties") {
            row.display_properties = display_properties.clone();
        }
        if let Some(rich_filters) = values.json("rich_filters") {
            row.rich_filters = rich_filters.clone();
        }
        if let Some(limit) = values.integer("navigation_project_limit") {
            row.navigation_project_limit = i32::try_from(limit)
                .map_err(|_| ViewError::Unknown("navigation_project_limit out of range".into()))?;
        }
        if let Some(preference) = values.text("navigation_control_preference") {
            row.navigation_control_preference = preference.to_owned();
        }
        if values.contains("created_by") {
            row.created_by_id = values.uuid("created_by");
        }
        // `updated_by` is writable — DRF validates it, H01 then overwrites it.

        // H01 on update: `updated_at` from `auto_now`, `updated_by` from the
        // user; `created_at`, `created_by` and `deleted_at` keep their values.
        row.updated_at = write::now();
        row.updated_by_id = Audit::on_update(Some(user));

        let statement = queries::USER_PROPERTIES_SAVE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &row.created_at,
                    &row.updated_at,
                    &row.created_by_id,
                    &row.updated_by_id,
                    &row.deleted_at,
                    &row.filters,
                    &row.display_filters,
                    &row.display_properties,
                    &row.rich_filters,
                    &row.navigation_project_limit,
                    &row.navigation_control_preference,
                    &row.workspace_id,
                    &row.user_id,
                    &row.id,
                ],
            )
            .await?;

        render(ctx, &row)
    })
}

/// `serializer.data` for route 207 — the 14 keys of
/// `WorkspaceUserPropertiesSerializer` (`plane/app/serializers/workspace.py:
/// 164-170`) in `_readable_fields` order.
fn render(ctx: &ViewCtx<'_>, row: &WorkspaceUserProperties) -> Result<ViewResponse, ViewError> {
    let serializer = WorkspaceUserPropertiesSerializer {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        filters: Some(PgJson::from(&row.filters)),
        display_filters: Some(PgJson::from(&row.display_filters)),
        display_properties: Some(PgJson::from(&row.display_properties)),
        rich_filters: Some(PgJson::from(&row.rich_filters)),
        navigation_project_limit: i64::from(row.navigation_project_limit),
        navigation_control_preference: &row.navigation_control_preference,
        created_by: row.created_by_id,
        updated_by: row.updated_by_id,
        workspace: row.workspace_id,
        user: row.user_id,
    };
    let body = rows::render_plain(ctx, &serializer)?;
    Ok(ViewResponse::raw(
        200,
        plane_core::error::APPLICATION_JSON,
        body,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_order_matches_the_generated_table() {
        let generated: Vec<&str> = write::generated::workspace_user_properties_serializer::FIELDS
            .iter()
            .map(|field| field.name)
            .collect();
        assert_eq!(generated, FIELD_ORDER);
    }

    #[test]
    fn the_defaults_are_the_python_dict_text() {
        assert!(DEFAULT_FILTERS.contains("\"priority\": null"));
        assert!(DEFAULT_DISPLAY_FILTERS.starts_with("{\"display_filters\": {"));
        assert!(DEFAULT_DISPLAY_PROPERTIES.starts_with("{\"display_properties\": {"));
    }
}
