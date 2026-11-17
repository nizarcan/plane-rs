// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkspaceHomePreferenceViewSet.patch` (`plane/app/views/workspace/home.py:
//! 67-77`) — workspace-core set C, routes 220 and 221.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 220 `PATCH …/home-preferences/` | `PATCH` | the signature 500 | [`patch`] |
//! | 221 `PATCH …/home-preferences/{key}/` | `PATCH` | `home.py:67-77` | [`patch`] |
//!
//! ## 220 is the signature 500
//!
//! The row maps `patch` for both routes and the handler is
//! `patch(self, request, slug, key)` (`home.py:68`). Route 220 binds no `key`,
//! so DRF's dispatch raises `TypeError: patch() missing 1 required positional
//! argument: 'key'` and the family handler answers
//! `500 {"error": "Something went wrong please try again later"}`
//! (`plane/app/views/base.py:235-239`). `@allow_permission` has already run by
//! then, so a non-member is the decorator's 403 and an anonymous caller 401
//! before the 500 — a **guest** is allowed (GUEST is one of the three roles)
//! and reaches it. That is the reverse of `GET …/home-preferences/{key}/`,
//! which is the 500 (`workspaces/home.rs`).
//!
//! ## What 221 does
//!
//! The lookup comes **first** (`home.py:69-70`), so a missing preference is
//! this view's own `400 {"detail": "Preference not found"}` before the body is
//! even parsed. Then the three-field serializer with `partial=True`, a full
//! `save()` (every column, H01's `updated_at`/`updated_by`) and
//! `serializer.data` from the in-memory instance.
//!
//! `config` is **not** writable — the serializer declares only
//! `key,is_enabled,sort_order` (`serializers/workspace.py:339-343`) — so a body
//! `config` is silently ignored, while `key` **is** writable and can rename the
//! row.

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::hooks::audit::Audit;
use plane_db::models::workspace::WorkspaceHomePreference;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_workspace::WorkspaceHomePreferenceSerializer;

use crate::common::client;
use crate::workspaces::rows;
use crate::workspaces::write_c_queries as queries;
use crate::write::{self, Serializer};

/// `plane.app.views.workspace.home.WorkspaceHomePreferenceViewSet`.
const VIEW: &str = "plane.app.views.workspace.home.WorkspaceHomePreferenceViewSet";

/// `WorkspaceHomePreferenceSerializer`'s writable fields in `self.fields` order
/// (`serializers/workspace.py:339-343`).
const FIELD_ORDER: &[&str] = &["key", "is_enabled", "sort_order"];

/// Register the `PATCH` verb both routes resolve to.
///
/// `home::register` already registers `get` for this view; the registry
/// accumulates methods per view.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.api_view(VIEW)?.method(Method::PATCH, patch)?;
    Ok(())
}

/// `WorkspaceHomePreferenceViewSet.patch` (`home.py:67-77`).
fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // Route 220 resolves here without the kwarg the signature requires.
        let Some(key) = ctx.kwarg("key") else {
            return Err(ViewError::Unknown(
                "WorkspaceHomePreferenceViewSet.patch() missing 1 required positional argument: \
                 'key'"
                    .to_string(),
            ));
        };
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = client(ctx)?;

        // `.first()` orders `-created_at` and caps at one row (`home.py:69`).
        let statement = queries::HOME_PREFERENCE_BY_KEY.prepare(client).await?;
        let Some(source) = client.query_opt(&statement, &[&key, &slug, &user]).await? else {
            // This view's own body, not the family 404 (`home.py:77`).
            return rows::detail_body(400, "Preference not found");
        };
        let mut row = WorkspaceHomePreference::from_row(&source, 0)?;

        let mut serializer = Serializer::new(ctx, true)?;
        write::generated::workspace_home_preference_serializer::validate(&mut serializer, client)
            .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(mut errors) => {
                errors.sort_by_field_order(FIELD_ORDER);
                return Ok(errors.into_response());
            }
        };

        // `ModelSerializer.update`: assign every validated field, then `save()`.
        if let Some(key) = values.text("key") {
            row.key = key.to_owned();
        }
        if let Some(enabled) = values.bool("is_enabled") {
            row.is_enabled = enabled;
        }
        if let Some(sort_order) = values.float("sort_order") {
            row.sort_order = sort_order;
        }

        // H01 on update: `updated_at` from `auto_now`, `updated_by` from the
        // user; every other column keeps its value.
        row.updated_at = write::now();
        row.updated_by_id = Audit::on_update(Some(user));

        let statement = queries::HOME_PREFERENCE_SAVE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &row.created_at,
                    &row.updated_at,
                    &row.created_by_id,
                    &row.updated_by_id,
                    &row.deleted_at,
                    &row.workspace_id,
                    &row.user_id,
                    &row.key,
                    &row.is_enabled,
                    &row.config,
                    &row.sort_order,
                    &row.id,
                ],
            )
            .await?;

        render(ctx, &row)
    })
}

/// `serializer.data` for route 221 — `key`, `is_enabled`, `sort_order`
/// (`plane/app/serializers/workspace.py:339-343`).
fn render(ctx: &ViewCtx<'_>, row: &WorkspaceHomePreference) -> Result<ViewResponse, ViewError> {
    let serializer = WorkspaceHomePreferenceSerializer {
        key: &row.key,
        is_enabled: row.is_enabled,
        sort_order: row.sort_order,
    };
    let body = rows::render_plain(ctx, &serializer)?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_order_matches_the_generated_table() {
        let generated: Vec<&str> = write::generated::workspace_home_preference_serializer::FIELDS
            .iter()
            .map(|field| field.name)
            .collect();
        assert_eq!(generated, FIELD_ORDER);
    }

    #[test]
    fn a_preference_body_is_three_keys() {
        assert_eq!(FIELD_ORDER.len(), 3);
    }
}
