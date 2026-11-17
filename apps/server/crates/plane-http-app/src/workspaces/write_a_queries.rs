// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The statements of workspace-core set A (`plans/WORKSPACES_CORE.md` §3):
//! routes 186, 187, 199, 200 and 138.
//!
//! `workspaces/write_queries.rs` (the P0's file) holds what the sets share —
//! the membership lookups, the slug `UniqueValidator`, the H04 identifier
//! delete. This file holds what only set A runs: the two inserts of a workspace
//! create, the full-row `save()` of a workspace and a theme, the `profiles`
//! clear of 187's destroy and the `Project.objects…exists()` of 138.
//!
//! Each statement's doc names the Python it ports. Two rules of Django's SQL
//! recur here:
//!
//! * a `save()` writes **every** concrete column (Django's `_do_update` builds
//!   the SET list from `_meta.local_concrete_fields`, not from the changed
//!   attributes), which is why the UPDATEs below bind columns the request never
//!   touches;
//! * the default manager's `deleted_at IS NULL` applies to the model the view
//!   queried, never to a model it merely joined (`plane/db/mixins.py:56-58`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `WorkSpaceSerializer.save(owner=request.user)` on a create
    /// (`plane/app/views/workspace/base.py:123`): `Workspace.objects.create`
    /// with the validated fields plus `owner`, the model defaults the serializer
    /// leaves absent (`timezone` `"UTC"`, `background_color`
    /// `get_random_color()`, `plane/db/models/workspace.py:137-139`) and H01's
    /// audit columns.
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` updated_by (NULL on a create), `$5` deleted_at, `$6` id, `$7` name,
    /// `$8` logo, `$9` logo_asset, `$10` owner, `$11` slug,
    /// `$12` organization_size, `$13` timezone, `$14` background_color.
    WORKSPACE_INSERT = "workspaces_a_insert",
    "INSERT INTO workspaces \
       (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, name, \
        logo, logo_asset_id, owner_id, slug, organization_size, timezone, background_color) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    []
}

plane_query! {
    /// `WorkspaceMember.objects.create(workspace_id=…, member=request.user,
    /// role=20, company_role=request.data.get("company_role", ""))`
    /// (`plane/app/views/workspace/base.py:125-130`) with the model defaults
    /// (`plane/db/models/workspace.py:198-213`): the two prop blobs and
    /// `issue_props`, `is_active`, and the three empty dicts.
    ///
    /// `company_role` is a `TextField`, so Django's `get_prep_value` strings a
    /// non-string body value before binding; the handler does that.
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` id, `$4` role,
    /// `$5` created_by, `$6` member, `$7` workspace, `$8` company_role,
    /// `$9` view_props, `$10` default_props, `$11` issue_props.
    WORKSPACE_MEMBER_CREATE = "workspaces_a_member_create",
    "INSERT INTO workspace_members \
       (created_at, updated_at, id, role, created_by_id, member_id, updated_by_id, \
        workspace_id, company_role, view_props, default_props, issue_props, is_active, \
        deleted_at, explored_features, getting_started_checklist, tips) \
     VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $8, $9, $10, $11, TRUE, NULL, '{}', '{}', '{}')",
    []
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace_id=…).count()`
    /// (`plane/app/views/workspace/base.py:133`).
    ///
    /// `.count()` is `COUNT(*)` over the manager's predicate — no `is_active`
    /// and no bot filter, unlike the viewset's `total_members` annotation.
    ///
    /// Bind order: `$1` workspace id.
    WORKSPACE_MEMBER_COUNT = "workspaces_a_member_count",
    "SELECT count(*) FROM workspace_members \
     WHERE deleted_at IS NULL AND workspace_id = $1",
    [("count", Int8)]
}

plane_query! {
    /// `Workspace.save()` on an existing row — `ModelSerializer.update`'s save
    /// for 187's PATCH/PUT (`rest_framework/serializers.py:960-988`) and
    /// `SoftDeleteModel.delete`'s full-row save for 187's DELETE
    /// (`plane/db/mixins.py:65-77`).
    ///
    /// Django's `_do_update` writes every non-pk concrete column, `created_at`
    /// included (unchanged), so the statement binds them all; the two call sites
    /// differ only in `updated_at`/`updated_by`/`deleted_at`.
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` updated_by, `$5` deleted_at, `$6` name, `$7` logo,
    /// `$8` logo_asset, `$9` owner, `$10` slug, `$11` organization_size,
    /// `$12` timezone, `$13` background_color, `$14` id.
    WORKSPACE_SAVE = "workspaces_a_save",
    "UPDATE workspaces SET created_at = $1, updated_at = $2, created_by_id = $3, \
       updated_by_id = $4, deleted_at = $5, name = $6, logo = $7, \
       logo_asset_id = $8, owner_id = $9, slug = $10, organization_size = $11, \
       timezone = $12, background_color = $13 \
     WHERE id = $14",
    []
}

plane_query! {
    /// `WorkSpaceViewSet.get_queryset()` + `get_object()`
    /// (`plane/app/views/workspace/base.py:64-80,171`) for 187's write verbs.
    ///
    /// Same predicate as `queries::WORKSPACE_FOR_MEMBER`, which the read route
    /// uses, but the logo asset is joined as the five columns
    /// `common::asset_url::AssetRef` needs: `Workspace.logo_url` is evaluated
    /// while the response is rendered, and for a non-static entity type it reads
    /// `self.logo_asset.workspace.slug`, `project_id` and `issue_id`
    /// (`plane/db/models/asset.py:83-104`). The read route only sees
    /// `entity_type` and documents that gap; a write response cannot.
    ///
    /// Bind order: `$1` user id, `$2` slug.
    WORKSPACE_FOR_MEMBER_ASSET = "workspaces_a_for_member_asset",
    "SELECT w.created_at, w.updated_at, w.created_by_id, w.updated_by_id, w.deleted_at, \
       w.id, w.name, w.logo, w.logo_asset_id, w.owner_id, w.slug, w.organization_size, \
       w.timezone, w.background_color, \
       fa.id AS asset_id, fa.entity_type AS asset_entity_type, \
       aw.slug AS asset_workspace_slug, fa.project_id AS asset_project_id, \
       fa.issue_id AS asset_issue_id, \
       (SELECT count(*) FROM workspace_members cnt \
          INNER JOIN users ucnt ON ucnt.id = cnt.member_id \
         WHERE cnt.workspace_id = w.id AND cnt.deleted_at IS NULL AND cnt.is_active \
           AND NOT ucnt.is_bot) AS total_members \
     FROM workspaces w \
     INNER JOIN workspace_members wm ON wm.workspace_id = w.id \
     LEFT OUTER JOIN file_assets fa ON fa.id = w.logo_asset_id \
     LEFT OUTER JOIN workspaces aw ON aw.id = fa.workspace_id \
     WHERE wm.is_active AND wm.member_id = $1 AND w.slug = $2 AND w.deleted_at IS NULL",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("logo", Text),
        ("logo_asset_id", Uuid),
        ("owner_id", Uuid),
        ("slug", Varchar),
        ("organization_size", Varchar),
        ("timezone", Varchar),
        ("background_color", Varchar),
        ("asset_id", Uuid),
        ("asset_entity_type", Varchar),
        ("asset_workspace_slug", Varchar),
        ("asset_project_id", Uuid),
        ("asset_issue_id", Uuid),
        ("total_members", Int8),
    ]
}

plane_query! {
    /// The logo asset of a **create** response, with the same five columns as
    /// [`WORKSPACE_FOR_MEMBER_ASSET`]'s asset half.
    ///
    /// DRF's `PrimaryKeyRelatedField` loads the instance with
    /// `FileAsset.objects.get(pk=…)` (`rest_framework/relations.py:252-265`), and
    /// `Workspace.logo_url` then reads `self.logo_asset.asset_url`
    /// (`plane/db/models/workspace.py:145-154`) — one extra query for the
    /// related workspace in the non-static branches. The join here matches
    /// Django's `_base_manager` access, which applies no soft-delete predicate.
    ///
    /// Bind order: `$1` asset id.
    LOGO_ASSET_REF = "workspaces_a_logo_asset_ref",
    "SELECT fa.id, fa.entity_type, aw.slug, fa.project_id, fa.issue_id \
     FROM file_assets fa \
     LEFT OUTER JOIN workspaces aw ON aw.id = fa.workspace_id \
     WHERE fa.id = $1",
    [
        ("id", Uuid),
        ("entity_type", Varchar),
        ("slug", Varchar),
        ("project_id", Uuid),
        ("issue_id", Uuid),
    ]
}

plane_query! {
    /// `Profile.objects.filter(last_workspace_id=id).update(last_workspace_id=None)`
    /// (`plane/app/views/workspace/base.py:161-165`).
    ///
    /// A queryset `update()`: no model hook, no `updated_at`, no audit column.
    /// `profiles` has no `deleted_at` (it is a `TimeAuditModel`), so the
    /// statement is the bare update.
    ///
    /// Bind order: `$1` workspace id.
    PROFILES_CLEAR_LAST_WORKSPACE = "workspaces_a_profiles_clear_last_workspace",
    "UPDATE profiles SET last_workspace_id = NULL WHERE last_workspace_id = $1",
    []
}

plane_query! {
    /// `WorkspaceThemeSerializer.save(workspace=…, actor=request.user)` on a
    /// create (`plane/app/views/workspace/base.py:331-337`): the validated
    /// fields plus `actor` and the workspace, H01's audit columns and the
    /// `colors` default `{}`.
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` updated_by (NULL on a create), `$5` deleted_at, `$6` id,
    /// `$7` workspace, `$8` name, `$9` actor, `$10` colors.
    THEME_INSERT = "workspaces_a_theme_insert",
    "INSERT INTO workspace_themes \
       (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
        workspace_id, name, actor_id, colors) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    []
}

plane_query! {
    /// `WorkspaceTheme.save()` on an existing row — `ModelSerializer.update`'s
    /// save for 200's PATCH and `SoftDeleteModel.delete`'s full-row save for
    /// 200's DELETE (`plane/db/mixins.py:65-77`).
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` updated_by, `$5` deleted_at, `$6` workspace, `$7` name, `$8` actor,
    /// `$9` colors, `$10` id.
    THEME_SAVE = "workspaces_a_theme_save",
    "UPDATE workspace_themes SET created_at = $1, updated_at = $2, created_by_id = $3, \
       updated_by_id = $4, deleted_at = $5, workspace_id = $6, name = $7, \
       actor_id = $8, colors = $9 \
     WHERE id = $10",
    []
}

plane_query! {
    /// `Project.objects.filter(identifier=name, workspace__slug=slug).exists()`
    /// (`plane/app/views/project/base.py:464`) — route 138's refusal to delete
    /// an identifier a live project still uses.
    ///
    /// `Project.objects` is the soft-deletion manager, so a soft-deleted project
    /// does not block the delete; the `workspaces` join has no `deleted_at`
    /// predicate (only the queried model's manager applies).
    ///
    /// Bind order: `$1` identifier (already `strip().upper()`ed), `$2` slug.
    PROJECT_IDENTIFIER_NAME_TAKEN = "workspaces_a_project_identifier_name_taken",
    "SELECT EXISTS ( \
       SELECT 1 FROM projects p \
       INNER JOIN workspaces w ON w.id = p.workspace_id \
       WHERE p.deleted_at IS NULL AND p.identifier = $1 AND w.slug = $2 \
     ) AS taken",
    [("taken", Bool)]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &WORKSPACE_INSERT,
    &WORKSPACE_MEMBER_CREATE,
    &WORKSPACE_MEMBER_COUNT,
    &WORKSPACE_SAVE,
    &WORKSPACE_FOR_MEMBER_ASSET,
    &LOGO_ASSET_REF,
    &PROFILES_CLEAR_LAST_WORKSPACE,
    &THEME_INSERT,
    &THEME_SAVE,
    &PROJECT_IDENTIFIER_NAME_TAKEN,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_a_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_a_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
