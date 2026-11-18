// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The write-only entity side effects and shared response literals of the
//! asset views (`plane/app/views/asset/v2.py`).
//!
//! The three upload route families each rebind an entity's FK to the new
//! asset, soft-delete the previous asset and invalidate the paths the Python
//! invalidates:
//!
//! | family | save | delete | invalidations |
//! | --- | --- | --- | --- |
//! | workspace/project (`v2.py:289-354`) | `workspace.logo=""` + `logo_asset`, `project.cover_image=""` + `cover_image_asset` | id-only [`asset_delete`] | [`WORKSPACE_ENTITY_INVALIDATIONS`] |
//! | user (`v2.py:83-149`) | `user.avatar=""` / `cover_image=None` + the FK | id+user [`asset_delete`] | [`USER_ENTITY_INVALIDATIONS`] |
//!
//! The saves themselves are the entity models' own hooks (H01 for
//! `Workspace`/`Project`, H30 for `User`), so they stay in their sets; this
//! module owns only the pieces more than one set repeats: the previous-row
//! soft delete, the literals and the key lists.
//!
//! ## Why the invalidation lists are ported even when nothing is cached
//!
//! Only `/api/instances/` is actually cached (`plane/license/api/views/
//! instance.py:34`). The harness compares the Redis diff as well as the HTTP
//! body (`HANDLERS.md` §7), so a `DEL` the Python issues and Rust does not is a
//! difference even on a path that never stored a value.

use deadpool_postgres::Client;
use plane_core::json::Object;
use plane_drf::error::ViewError;
use plane_drf::view::{ViewCtx, ViewResponse};
use uuid::Uuid;

use crate::common::cache;

use super::support;
use super::write_queries;

/// `UserAssetsV2Endpoint.asset_delete` / `WorkspaceFileAssetEndpoint.
/// asset_delete` (`plane/app/views/asset/v2.py:73-81`, `:278-287`).
///
/// Marks the *previous* asset row deleted, writing only `is_deleted` and
/// `deleted_at` — a `save(update_fields=["is_deleted","deleted_at"])`, so
/// `updated_at` and `updated_by` keep their stored values. A row that is
/// already soft-deleted is not touched again (`FileAsset.objects` filters
/// `deleted_at IS NULL`), and a missing row is a no-op: `.first()` returns
/// `None` and the Python returns.
///
/// `user_scope` is the user-side variant's `filter(id=…, user_id=user_id)`; the
/// workspace/project helper passes `None` and filters on the id alone.
///
/// # Errors
///
/// A database failure.
#[allow(
    dead_code,
    reason = "the user and workspace entity helpers (plans/ASSETS.md A/C) call it"
)]
pub async fn asset_delete(
    client: &Client,
    asset_id: Uuid,
    user_scope: Option<Uuid>,
    deleted_at: chrono::DateTime<chrono::Utc>,
) -> Result<(), ViewError> {
    match user_scope {
        Some(user_id) => {
            client
                .execute(
                    write_queries::FILE_ASSET_SOFT_DELETE_FOR_USER.sql,
                    &[&asset_id, &deleted_at, &user_id],
                )
                .await?;
        }
        None => {
            client
                .execute(
                    write_queries::FILE_ASSET_SOFT_DELETE.sql,
                    &[&asset_id, &deleted_at],
                )
                .await?;
        }
    }
    Ok(())
}

/// `Response({"error": "You don't have the required permissions."},
/// status=403)` (`v2.py:421-424`, `:447-450`, `:452-455`, `:574-577`,
/// `:687-691`, `:707-711`, `:791-795`).
///
/// # Errors
///
/// A render failure.
#[allow(
    dead_code,
    reason = "sets A/B/C call it from their permission checks (plans/ASSETS.md); P0 lands it first"
)]
pub fn no_permission() -> Result<ViewResponse, ViewError> {
    support::json_object(
        403,
        &Object::new().field("error", "You don't have the required permissions."),
    )
}

/// `Response({"error": "You don't have access to this asset."}, status=403)`
/// (`v2.py:446-450`, `:478-482`, `:500-504`, `:725-729`, `:901-906`).
///
/// # Errors
///
/// A render failure.
pub fn no_access() -> Result<ViewResponse, ViewError> {
    support::json_object(
        403,
        &Object::new().field("error", "You don't have access to this asset."),
    )
}

/// `Response({"error": "The requested asset could not be found."},
/// status=404)` (`v2.py:507-511`, `:533-537`, `:732-736`, `:781-785`).
///
/// # Errors
///
/// A render failure.
pub fn asset_not_found() -> Result<ViewResponse, ViewError> {
    support::json_object(
        404,
        &Object::new().field("error", "The requested asset could not be found."),
    )
}

/// `Response({"error": "Invalid entity type.", "status": False},
/// status=400)` (`v2.py:364-369`, `:546-551`; the upload family adds
/// `entity_type` validation at `:624-629`).
///
/// # Errors
///
/// A render failure.
pub fn invalid_entity_type() -> Result<ViewResponse, ViewError> {
    support::json_object(
        400,
        &Object::new()
            .field("error", "Invalid entity type.")
            .field("status", false),
    )
}

/// `Response({"error": "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF
/// files are allowed.", "status": False}, status=400)`
/// (`v2.py:390-397`, `:639-646`).
///
/// # Errors
///
/// A render failure.
#[allow(
    dead_code,
    reason = "sets A and C validate the upload MIME allow-list (plans/ASSETS.md)"
)]
pub fn invalid_file_type() -> Result<ViewResponse, ViewError> {
    support::json_object(
        400,
        &Object::new()
            .field(
                "error",
                "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files are allowed.",
            )
            .field("status", false),
    )
}

/// `Response({"error": "Only workspace admins can upload a workspace logo."},
/// status=403)` (`v2.py:371-380`).
///
/// # Errors
///
/// A render failure.
#[allow(
    dead_code,
    reason = "set A's v2 workspace upload (plans/ASSETS.md route 18) calls it"
)]
pub fn workspace_logo_needs_admin() -> Result<ViewResponse, ViewError> {
    support::json_object(
        403,
        &Object::new().field(
            "error",
            "Only workspace admins can upload a workspace logo.",
        ),
    )
}

/// `Response({"error": "No asset ids provided."}, status=400)`
/// (`v2.py:760-762`, the bulk rebind's empty-body guard).
///
/// # Errors
///
/// A render failure.
#[allow(
    dead_code,
    reason = "set A's project bulk rebind (plans/ASSETS.md route 26) calls it"
)]
pub fn no_asset_ids() -> Result<ViewResponse, ViewError> {
    support::json_object(400, &Object::new().field("error", "No asset ids provided."))
}

/// One `invalidate_cache_directly(path=…, url_params=False, user=…,
/// request=request)` call: the path string and the `user` flag. `url_params` is
/// always `False` on the entity helpers, so it is not part of the struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invalidation {
    pub path: &'static str,
    /// Append `str(request.user.id)` to the key.
    pub user: bool,
}

/// The three calls of the workspace/project entity helpers
/// (`v2.py:302-309`, `:336-343`): logo save and logo delete are identical.
#[allow(
    dead_code,
    reason = "set A's workspace/project entity helpers (plans/ASSETS.md) call it"
)]
pub const WORKSPACE_ENTITY_INVALIDATIONS: &[Invalidation] = &[
    Invalidation {
        path: "/api/workspaces/",
        user: false,
    },
    Invalidation {
        path: "/api/users/me/workspaces/",
        user: true,
    },
    Invalidation {
        path: "/api/instances/",
        user: false,
    },
];

/// The two calls of [`super::entities`]' user-side entity helper
/// (`v2.py:94-100`, `:112-118`, `:128-134`, `:142-148`): avatar and cover,
/// save and delete, are identical.
#[allow(
    dead_code,
    reason = "set C's user entity helper (plans/ASSETS.md routes 20/21) calls it"
)]
pub const USER_ENTITY_INVALIDATIONS: &[Invalidation] = &[
    Invalidation {
        path: "/api/users/me/",
        user: true,
    },
    Invalidation {
        path: "/api/users/me/settings/",
        user: true,
    },
];

/// Run one [`Invalidation`] per entry, in list order, **without**
/// `multiple`/`url_params` substitution.
///
/// The Python invokes `invalidate_cache_directly` once per call; each one is a
/// separate Redis `DEL`, and the harness sees every one in the Redis diff.
///
/// # Errors
///
/// A cache failure, which is a 500 out of the view as in Django.
#[allow(
    dead_code,
    reason = "sets A and C call it from their entity helpers (plans/ASSETS.md)"
)]
pub async fn invalidate_entity(ctx: &ViewCtx<'_>, keys: &[Invalidation]) -> Result<(), ViewError> {
    for key in keys {
        cache::invalidate_cache(ctx, Some(key.path), false, key.user, false).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The literals are Python dict literals: key order and the `status` key
    /// are part of the bytes. `json_object` renders them through
    /// `Dialect::Drf` (compact separators).
    fn body(response: Result<ViewResponse, ViewError>) -> (u16, String) {
        let ViewResponse::Raw { status, body, .. } = response.expect("renders") else {
            panic!("the asset literals are raw JSON responses");
        };
        (status, String::from_utf8(body).expect("utf-8"))
    }

    #[test]
    fn the_error_literals_have_pythons_key_order() {
        assert_eq!(
            body(no_permission()),
            (
                403,
                r#"{"error":"You don't have the required permissions."}"#.to_owned()
            )
        );
        assert_eq!(
            body(no_access()),
            (
                403,
                r#"{"error":"You don't have access to this asset."}"#.to_owned()
            )
        );
        assert_eq!(
            body(asset_not_found()),
            (
                404,
                r#"{"error":"The requested asset could not be found."}"#.to_owned()
            )
        );
        assert_eq!(
            body(invalid_entity_type()),
            (
                400,
                r#"{"error":"Invalid entity type.","status":false}"#.to_owned()
            )
        );
        assert_eq!(
            body(invalid_file_type()),
            (
                400,
                concat!(
                    r#"{"error":"Invalid file type. Only JPEG, PNG, WebP, JPG and "#,
                    r#"GIF files are allowed.","status":false}"#
                )
                .to_owned()
            )
        );
        assert_eq!(
            body(workspace_logo_needs_admin()),
            (
                403,
                r#"{"error":"Only workspace admins can upload a workspace logo."}"#.to_owned()
            )
        );
        assert_eq!(
            body(no_asset_ids()),
            (400, r#"{"error":"No asset ids provided."}"#.to_owned())
        );
    }
}
