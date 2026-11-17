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
//! This module ports the group's fourteen `GET` routes plus the v1 writes
//! ([`legacy`], routes 13-17, `plans/ASSETS.md` set B). The `v2.py` writes
//! (`POST`/`PATCH`/`DELETE`, upload presigns and entity rebinding) are sets A
//! and C and still answer the pipeline's 501 until they land.
//!
//! | Module | Python |
//! | --- | --- |
//! | [`base`] | `plane/app/views/asset/base.py` — `FileAssetEndpoint`, `UserAssetsEndpoint` |
//! | [`legacy`] | the same file's v1 writes — `POST`/`DELETE` and `restore` (set B) |
//! | [`v2`] | `plane/app/views/asset/v2.py` — workspace, project, static, check and download |
//! | [`attachment`] | `plane/app/views/issue/attachment.py` — `IssueAttachmentV2Endpoint` |
//! | [`support`] | `FileAsset`'s model rules, `issue_hidden_from_guest`, the presign call site |
//! | [`entities`] | the write-only entity helpers: `asset_delete`, the shared `{"error": …}` literals, the invalidation key lists |
//! | [`write_queries`] | the `file_assets` INSERT/UPDATE/lookup the write sets share |
//! | [`write_b_queries`] | the v1 writes' two `get()` lookups and the `is_deleted` column UPDATE |
//!
//! The six routes that answer `302` sign for the **request's** host
//! (`request.scheme://request.get_host()`, `plane/settings/storage.py:42-54`),
//! which is why the scenarios pin `Host: plane-fixture.test`: the `Location`
//! header is compared byte for byte, `X-Amz-Signature` included.
//!
//! ## Not ported (blocked on the write path)
//!
//! Every `POST`/`PATCH`/`DELETE` of `v2.py` (upload presigns, entity rebinding,
//! cache invalidation, the `storage_metadata` task) is Phase 5 work. The two
//! `user-assets` routes of `v2.py` answer `POST`/`PATCH`/`DELETE` only — a `GET`
//! there is a `405` the runtime derives from the route table — so they are not
//! part of this group's reads.
//!
//! The v1 writes are [`legacy`]: three unconditional upload 500s
//! (`S3Storage.__init__` never calls the parent), three wrong-verb `TypeError`
//! 500s (13 DELETE, 15 DELETE, 16 POST), route 14's POST 405, and the
//! `is_deleted`-only delete and restore.
//!
//! The shared write pieces land first, so the set files do not each port them:
//! [`support`]'s `can_modify_asset`/`sanitize_filename`/presign bodies,
//! [`write_queries`]'s `file_assets` statements and [`entities`]' error
//! literals, `asset_delete` and invalidation key lists. `plans/ASSETS.md` set
//! P0 owns them; sets A/B/C add one module each and call them.

use plane_drf::registry::{Registry, RegistryError};

pub mod attachment;
pub mod base;
pub mod entities;
pub mod legacy;
pub mod queries;
pub mod support;
pub mod v2;
pub mod write_b_queries;
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
    legacy::register(registry)
}
