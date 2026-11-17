// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `ai` group.
//!
//! Both statements exist only for the assistant's **success path**
//! (`plane/app/views/external/base.py:170-181`), which runs after the model has
//! answered:
//!
//! * `Workspace.objects.get(slug=slug)` and `Project.objects.get(pk=…)` — two
//!   `get()`s, the second deliberately **not** scoped to the workspace (see the
//!   module note in [`super`]).
//! * `ProjectLiteSerializer` / `WorkspaceLiteSerializer` read
//!   `cover_image_url` / `logo_url`, which touch the related `FileAsset` and
//!   its workspace. The joins below resolve both in the same statement, the way
//!   `users_me/queries.rs` does for the activity feed, so the response never
//!   costs an N+1.
//!
//! Only the columns those serializers read are selected: Django's `get()`
//! loads the whole row, but nothing else is observable on this route.
//!
//! `tests/schema_check.rs` prepares both against a Django-migrated database.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Project.objects.get(pk=project_id)` (`external/base.py:171`) plus the
    /// `cover_image_asset` join `Project.cover_image_url` performs
    /// (`plane/db/models/project.py:129-139`).
    ///
    /// The default manager's `deleted_at IS NULL` applies, but — this is the
    /// upstream bug — there is **no** workspace predicate, so a project from
    /// another workspace is serialized when its id is known.
    PROJECT_LITE_BY_PK = "ai_project_lite_by_pk",
    "SELECT p.id, p.identifier, p.name, p.cover_image, p.logo_props, p.description, \
     cv.id AS cover_asset_id, cv.entity_type AS cover_asset_entity_type, \
     cvw.slug AS cover_asset_workspace_slug, cv.project_id AS cover_asset_project_id, \
     cv.issue_id AS cover_asset_issue_id \
     FROM projects p \
     LEFT JOIN file_assets cv ON cv.id = p.cover_image_asset_id \
     LEFT JOIN workspaces cvw ON cvw.id = cv.workspace_id \
     WHERE p.id = $1 AND p.deleted_at IS NULL",
    [
        ("id", Uuid),
        ("identifier", Varchar),
        ("name", Varchar),
        ("cover_image", Text),
        ("logo_props", Jsonb),
        ("description", Text),
        ("cover_asset_id", Uuid),
        ("cover_asset_entity_type", Varchar),
        ("cover_asset_workspace_slug", Varchar),
        ("cover_asset_project_id", Uuid),
        ("cover_asset_issue_id", Uuid),
    ]
}

plane_query! {
    /// `Workspace.objects.get(slug=slug)` (`external/base.py:170`) plus the
    /// `logo_asset` join `Workspace.logo_url` performs
    /// (`plane/db/models/workspace.py:145-154`).
    WORKSPACE_LITE_BY_SLUG = "ai_workspace_lite_by_slug",
    "SELECT w.id, w.name, w.slug, w.logo, \
     lv.id AS logo_asset_id, lv.entity_type AS logo_asset_entity_type, \
     lvw.slug AS logo_asset_workspace_slug, lv.project_id AS logo_asset_project_id, \
     lv.issue_id AS logo_asset_issue_id \
     FROM workspaces w \
     LEFT JOIN file_assets lv ON lv.id = w.logo_asset_id \
     LEFT JOIN workspaces lvw ON lvw.id = lv.workspace_id \
     WHERE w.slug = $1 AND w.deleted_at IS NULL",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("slug", Varchar),
        ("logo", Text),
        ("logo_asset_id", Uuid),
        ("logo_asset_entity_type", Varchar),
        ("logo_asset_workspace_slug", Varchar),
        ("logo_asset_project_id", Uuid),
        ("logo_asset_issue_id", Uuid),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&PROJECT_LITE_BY_PK, &WORKSPACE_LITE_BY_SLUG];
