// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `assets` (`deploy/route-groups.yaml`): Direct file uploads and
//! downloads, v1-style and v2-style.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/assets/**`
//! * `/api/workspaces/*/file-assets/**`
//! * `/api/workspaces/file-assets/**`
//! * `/api/users/file-assets/**`
//!
//! This module ports the group's fourteen `GET` routes and the v2 write verbs
//! of `plans/ASSETS.md` set A (18/19/24/25/26). The legacy v1 writes
//! (`FileAssetEndpoint`/`UserAssetsEndpoint`), `FileAssetViewSet.restore`,
//! `AssetRestoreEndpoint`, the user-asset routes and `DuplicateAssetEndpoint`
//! are sets B and C and still answer the pipeline's 501.
//!
//! | Module | Python |
//! | --- | --- |
//! | [`base`] | `plane/app/views/asset/base.py` — `FileAssetEndpoint`, `UserAssetsEndpoint` |
//! | [`v2`] | `plane/app/views/asset/v2.py` — workspace, project, static, check and download |
//! | [`workspace_write`] | `v2.py` — `WorkspaceFileAssetEndpoint.post`/`patch`/`delete` (18/19) |
//! | [`project_write`] | `v2.py` — `ProjectAssetEndpoint` (24/25) and `ProjectBulkAssetEndpoint` (26) |
//! | [`attachment`] | `plane/app/views/issue/attachment.py` — `IssueAttachmentV2Endpoint` |
//! | [`support`] | `FileAsset`'s model rules, `issue_hidden_from_guest`, the presign call site |
//! | [`entities`] | the write-only entity helpers: `asset_delete`, the shared `{"error": …}` literals, the invalidation key lists |
//! | [`write_queries`] | the `file_assets` INSERT/UPDATE/lookup the write sets share |
//! | [`write_a_queries`] | set A's own `file_assets`/`workspaces`/`projects` statements |
//!
//! The six routes that answer `302` sign for the **request's** host
//! (`request.scheme://request.get_host()`, `plane/settings/storage.py:42-54`),
//! which is why the scenarios pin `Host: plane-fixture.test`: the `Location`
//! header is compared byte for byte, `X-Amz-Signature` included.
//!
//! ## The shared write pieces
//!
//! [`support`]'s `can_modify_asset`/`sanitize_filename`/presign bodies,
//! [`write_queries`]'s `file_assets` statements and [`entities`]' error
//! literals, `asset_delete` and invalidation key lists came first (set P0), so
//! sets A/B/C each add one module and call them. The two `user-assets` routes
//! of `v2.py` answer `POST`/`PATCH`/`DELETE` only — a `GET` there is a `405`
//! the runtime derives from the route table — so they are not part of this
//! group's reads.

use http::Method;
use plane_drf::registry::{Registry, RegistryError};

pub mod attachment;
pub mod base;
pub mod entities;
pub mod project_write;
pub mod queries;
pub mod support;
pub mod v2;
pub mod workspace_write;
pub mod write_a_queries;
pub mod write_queries;

/// Register this group's handlers, in the order they were ported.
///
/// `apps/server/HANDLERS.md` has the step list. Return the first
/// [`RegistryError`]: a route that cannot be registered must fail startup
/// rather than fall back to a 501 nobody notices.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    base::register(registry)?;
    v2::register(registry)?;
    attachment::register(registry)?;

    // The v2 upload/confirm/delete verbs (`plans/ASSETS.md` set A). Each class
    // serves two URL shapes, so both shapes of a view share one handler that
    // branches on the trailing kwarg — exactly what Python's `TypeError` 500 is
    // (`v2.rs` does the same for `GET`).
    registry
        .api_view("plane.app.views.asset.v2.WorkspaceFileAssetEndpoint")?
        .method(Method::POST, workspace_write::post)?
        .method(Method::PATCH, workspace_write::patch)?
        .method(Method::DELETE, workspace_write::delete)?;
    registry
        .api_view("plane.app.views.asset.v2.ProjectAssetEndpoint")?
        .method(Method::POST, project_write::post)?
        .method(Method::PATCH, project_write::patch)?
        .method(Method::DELETE, project_write::delete)?;
    registry
        .api_view("plane.app.views.asset.v2.ProjectBulkAssetEndpoint")?
        .method(Method::POST, project_write::bulk)?;
    Ok(())
}
