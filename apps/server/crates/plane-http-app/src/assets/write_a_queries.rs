// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `file_assets`, `workspaces` and `projects` statements of the v2
//! workspace/project asset writes (`plans/ASSETS.md` set A: routes 18, 19, 24,
//! 25 and 26).
//!
//! ## Entity writes are partial `UPDATE`s, and that is observational
//!
//! The Python calls `workspace.save()` / `project.save()` (`v2.py:301,323,335,
//! 351,754`), i.e. a full-row `UPDATE` after H01 stamped `updated_by` and
//! `updated_at` (and H09 normalised `projects.identifier`). The harness
//! compares the *final row*, so writing the columns the route changes plus the
//! audit ones is the same row — with one exception this module keeps: H09 is
//! applied to `identifier` because `Project.save` really does rewrite it
//! (`plane/db/models/project.py:196`). `Workspace.save` has no hook of its own
//! (`plane/db/models/workspace.py` only overrides `delete`), so the workspace
//! statements carry no derived column.
//!
//! ## `update_fields` on the asset row
//!
//! `asset.save(update_fields=["is_uploaded","attributes"])` (`v2.py:230,471,
//! 701`) and `save(update_fields=["is_deleted","deleted_at"])`
//! (`v2.py:239,492,717`) write **only** those columns: `updated_at` and
//! `updated_by` keep their stored values (`MODEL_HOOKS.md` §2.1). That is why
//! [`FILE_ASSET_SET_UPLOAD_STATE`] and [`FILE_ASSET_SET_DELETED`] exist beside
//! `write_queries::FILE_ASSET_UPDATE`, which is the full-row form.
//!
//! ## Managers
//!
//! `FileAsset.objects` is the soft-deletion manager, so `deleted_at IS NULL`
//! is part of every statement that starts from `file_assets`
//! (`plane/db/mixins.py:57`). A `filter(<relation>__field=…)` join does **not**
//! apply the related model's predicate; the `INNER JOIN workspaces` below
//! therefore filters on `slug` alone, exactly like the ORM.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Workspace.objects.filter(id=asset.workspace_id).first()`
    /// (`plane/app/views/asset/v2.py:292`, the logo save's "find the current
    /// logo" lookup). `Workspace.Meta.ordering` is `("-created_at",)`
    /// (`plane/db/models/workspace.py:177`), which `.first()` applies; the id is
    /// the primary key, so the statement can only return one row anyway.
    ///
    /// Bind order: `$1` workspace id.
    WORKSPACE_LOGO_STATE = "assets_write_a_workspace_logo_state",
    "SELECT w.\"logo_asset_id\" \
     FROM workspaces w \
     WHERE w.\"id\" = $1 AND w.\"deleted_at\" IS NULL \
     ORDER BY w.\"created_at\" DESC \
     LIMIT 1",
    [("logo_asset_id", Uuid)]
}

plane_query! {
    /// `workspace.logo = ""; workspace.logo_asset_id = asset_id;
    /// workspace.save()` (`v2.py:299-301`).
    ///
    /// `logo` is `TextField` (never NULL) and is always set to the empty string,
    /// not to `None`. `updated_at`/`updated_by_id` are H01's
    /// (`plane/db/models/base.py:23-45`).
    ///
    /// Bind order: `$1` workspace id, `$2` the new logo asset id, `$3`
    /// `updated_at`, `$4` `updated_by_id`.
    WORKSPACE_SET_LOGO = "assets_write_a_workspace_set_logo",
    "UPDATE workspaces \
     SET logo = '', logo_asset_id = $2, updated_at = $3, updated_by_id = $4 \
     WHERE id = $1",
    []
}

plane_query! {
    /// `workspace.logo_asset_id = None; workspace.save()`
    /// (`v2.py:334-335`): the logo *delete* helper, which leaves `logo` alone.
    ///
    /// Bind order: `$1` workspace id, `$2` `updated_at`, `$3` `updated_by_id`.
    WORKSPACE_CLEAR_LOGO = "assets_write_a_workspace_clear_logo",
    "UPDATE workspaces \
     SET logo_asset_id = NULL, updated_at = $2, updated_by_id = $3 \
     WHERE id = $1",
    []
}

plane_query! {
    /// `Project.objects.filter(id=asset.project_id, workspace_id=asset.workspace_id)
    /// .first()` (`v2.py:314`, `:347`, the cover save and delete).
    ///
    /// `identifier` is selected because `Project.save` normalises it on every
    /// save (`plane/db/models/project.py:196`, H09) and the partial `UPDATE`
    /// has to write the normalised value.
    ///
    /// Bind order: `$1` project id, `$2` workspace id.
    PROJECT_COVER_STATE = "assets_write_a_project_cover_state",
    "SELECT p.\"cover_image_asset_id\", p.\"identifier\" \
     FROM projects p \
     WHERE p.\"id\" = $1 AND p.\"workspace_id\" = $2 AND p.\"deleted_at\" IS NULL \
     ORDER BY p.\"created_at\" DESC \
     LIMIT 1",
    [("cover_image_asset_id", Uuid), ("identifier", Varchar)]
}

plane_query! {
    /// `project.cover_image = ""; project.cover_image_asset_id = asset_id;
    /// project.save()` (`v2.py:321-323`).
    ///
    /// Bind order: `$1` project id, `$2` the new cover asset id, `$3`
    /// `updated_at`, `$4` `updated_by_id`, `$5` the H09-normalised
    /// `identifier`.
    PROJECT_SET_COVER = "assets_write_a_project_set_cover",
    "UPDATE projects \
     SET cover_image = '', cover_image_asset_id = $2, updated_at = $3, \
     updated_by_id = $4, identifier = $5 \
     WHERE id = $1",
    []
}

plane_query! {
    /// `project.cover_image_asset_id = None; project.save()`
    /// (`v2.py:350-351`): the cover *delete* helper, which leaves `cover_image`
    /// alone.
    ///
    /// Bind order: `$1` project id, `$2` `updated_at`, `$3` `updated_by_id`,
    /// `$4` the H09-normalised `identifier`.
    PROJECT_CLEAR_COVER = "assets_write_a_project_clear_cover",
    "UPDATE projects \
     SET cover_image_asset_id = NULL, updated_at = $2, updated_by_id = $3, \
     identifier = $4 \
     WHERE id = $1",
    []
}

plane_query! {
    /// `Project.objects.get(id=project_id)`
    /// (`plane/app/views/asset/v2.py:752`, the bulk endpoint's
    /// `save_project_cover`). No workspace predicate: the decorator already
    /// scoped the caller to this project, and the Python really does load the
    /// project by primary key alone.
    ///
    /// Bind order: `$1` project id.
    PROJECT_BY_ID_FOR_COVER = "assets_write_a_project_by_id_for_cover",
    "SELECT p.\"cover_image_asset_id\", p.\"identifier\" \
     FROM projects p \
     WHERE p.\"id\" = $1 AND p.\"deleted_at\" IS NULL",
    [("cover_image_asset_id", Uuid), ("identifier", Varchar)]
}

plane_query! {
    /// The bulk endpoint's `project.cover_image_asset_id = asset.id;
    /// project.save()` (`v2.py:753-754`), once per matching asset.
    ///
    /// Unlike [`PROJECT_SET_COVER`] this **keeps** `cover_image`: the bulk path
    /// never clears it (`v2.py:751-754`).
    ///
    /// Bind order: `$1` project id, `$2` the cover asset id, `$3` `updated_at`,
    /// `$4` `updated_by_id`, `$5` the H09-normalised `identifier`.
    PROJECT_SET_COVER_FROM_BULK = "assets_write_a_project_set_cover_from_bulk",
    "UPDATE projects \
     SET cover_image_asset_id = $2, updated_at = $3, updated_by_id = $4, \
     identifier = $5 \
     WHERE id = $1",
    []
}

plane_query! {
    /// `FileAsset.objects.filter(id__in=asset_ids, workspace__slug=slug,
    /// created_by=request.user).filter(Q(project_id=project_id) |
    /// Q(project_id__isnull=True))` (`plane/app/views/asset/v2.py:772-776`).
    ///
    /// Django's `workspace__slug` join does not filter the workspace's own
    /// `deleted_at`; the base model's manager adds `fa.deleted_at IS NULL`.
    /// `Meta.ordering = ("-created_at",)` (`plane/db/models/asset.py:72`)
    /// is what `.first()` and the later iteration both use.
    ///
    /// Bind order: `$1` asset ids, `$2` workspace slug, `$3` `created_by_id`,
    /// `$4` project id.
    FILE_ASSETS_FOR_BULK = "assets_write_a_file_assets_for_bulk",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = ANY($1) \
       AND w.\"slug\" = $2 AND fa.\"created_by_id\" = $3 \
       AND (fa.\"project_id\" = $4 OR fa.\"project_id\" IS NULL) \
     ORDER BY fa.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("attributes", Jsonb),
        ("asset", Varchar),
        ("user_id", Uuid),
        ("workspace_id", Uuid),
        ("draft_issue_id", Uuid),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("comment_id", Uuid),
        ("page_id", Uuid),
        ("entity_type", Varchar),
        ("entity_identifier", Varchar),
        ("is_deleted", Bool),
        ("is_archived", Bool),
        ("external_id", Varchar),
        ("external_source", Varchar),
        ("size", Float8),
        ("is_uploaded", Bool),
        ("storage_metadata", Jsonb),
    ]
}

plane_query! {
    /// `assets.update(project_id=project_id)` (`v2.py:796`), the
    /// `PROJECT_COVER` branch of the bulk rebind.
    ///
    /// `QuerySet.update()` writes one column and never runs `save()`, so
    /// `updated_at`/`updated_by` are untouched — and it carries no `ORDER BY`.
    /// The predicate is [`FILE_ASSETS_FOR_BULK`]'s, unchanged by the update
    /// (the rows already have `project_id` NULL or equal).
    ///
    /// Bind order: `$1` project id, `$2` asset ids, `$3` workspace slug, `$4`
    /// `created_by_id`, `$5` the filter's project id.
    FILE_ASSETS_SET_PROJECT = "assets_write_a_file_assets_set_project",
    "UPDATE file_assets fa \
     SET project_id = $1 \
     FROM workspaces w \
     WHERE fa.\"workspace_id\" = w.\"id\" AND fa.\"deleted_at\" IS NULL \
       AND fa.\"id\" = ANY($2) AND w.\"slug\" = $3 AND fa.\"created_by_id\" = $4 \
       AND (fa.\"project_id\" = $5 OR fa.\"project_id\" IS NULL)",
    []
}

plane_query! {
    /// `assets.update(issue_id=entity_id, project_id=project_id)`
    /// (`v2.py:803`), the `ISSUE_DESCRIPTION` branch. The Python wraps it in
    /// `try/except IntegrityError` because the issue may have been deleted in
    /// the meantime; the caller swallows class-23 failures, not this statement.
    ///
    /// Bind order: `$1` issue id, `$2` project id, `$3` asset ids, `$4`
    /// workspace slug, `$5` `created_by_id`, `$6` the filter's project id.
    FILE_ASSETS_SET_ISSUE_PROJECT = "assets_write_a_file_assets_set_issue_project",
    "UPDATE file_assets fa \
     SET issue_id = $1, project_id = $2 \
     FROM workspaces w \
     WHERE fa.\"workspace_id\" = w.\"id\" AND fa.\"deleted_at\" IS NULL \
       AND fa.\"id\" = ANY($3) AND w.\"slug\" = $4 AND fa.\"created_by_id\" = $5 \
       AND (fa.\"project_id\" = $6 OR fa.\"project_id\" IS NULL)",
    []
}

plane_query! {
    /// `assets.update(comment_id=entity_id)` (`v2.py:811`), the
    /// `COMMENT_DESCRIPTION` branch. Note that this one does **not** set
    /// `project_id`, unlike the issue branch.
    ///
    /// Bind order: `$1` comment id, `$2` asset ids, `$3` workspace slug, `$4`
    /// `created_by_id`, `$5` the filter's project id.
    FILE_ASSETS_SET_COMMENT = "assets_write_a_file_assets_set_comment",
    "UPDATE file_assets fa \
     SET comment_id = $1 \
     FROM workspaces w \
     WHERE fa.\"workspace_id\" = w.\"id\" AND fa.\"deleted_at\" IS NULL \
       AND fa.\"id\" = ANY($2) AND w.\"slug\" = $3 AND fa.\"created_by_id\" = $4 \
       AND (fa.\"project_id\" = $5 OR fa.\"project_id\" IS NULL)",
    []
}

plane_query! {
    /// `assets.update(page_id=entity_id)` (`v2.py:816`), the
    /// `PAGE_DESCRIPTION` branch. Not wrapped in `try/except`.
    ///
    /// Bind order: `$1` page id, `$2` asset ids, `$3` workspace slug, `$4`
    /// `created_by_id`, `$5` the filter's project id.
    FILE_ASSETS_SET_PAGE = "assets_write_a_file_assets_set_page",
    "UPDATE file_assets fa \
     SET page_id = $1 \
     FROM workspaces w \
     WHERE fa.\"workspace_id\" = w.\"id\" AND fa.\"deleted_at\" IS NULL \
       AND fa.\"id\" = ANY($2) AND w.\"slug\" = $3 AND fa.\"created_by_id\" = $4 \
       AND (fa.\"project_id\" = $5 OR fa.\"project_id\" IS NULL)",
    []
}

plane_query! {
    /// `assets.update(draft_issue_id=entity_id)` (`v2.py:822`), the
    /// `DRAFT_ISSUE_DESCRIPTION` branch, wrapped in `try/except IntegrityError`.
    ///
    /// Bind order: `$1` draft issue id, `$2` asset ids, `$3` workspace slug,
    /// `$4` `created_by_id`, `$5` the filter's project id.
    FILE_ASSETS_SET_DRAFT_ISSUE = "assets_write_a_file_assets_set_draft_issue",
    "UPDATE file_assets fa \
     SET draft_issue_id = $1 \
     FROM workspaces w \
     WHERE fa.\"workspace_id\" = w.\"id\" AND fa.\"deleted_at\" IS NULL \
       AND fa.\"id\" = ANY($2) AND w.\"slug\" = $3 AND fa.\"created_by_id\" = $4 \
       AND (fa.\"project_id\" = $5 OR fa.\"project_id\" IS NULL)",
    []
}

plane_query! {
    /// `asset.save(update_fields=["is_uploaded", "attributes"])` — the confirm
    /// step of 19 PATCH (`v2.py:471`) and 25 PATCH (`v2.py:701`).
    ///
    /// Only the two listed columns are written: `updated_at` and `updated_by`
    /// are **not** refreshed (`MODEL_HOOKS.md` §2.1). The row is addressed by
    /// its primary key with no manager predicate, because `Model.save` does not
    /// apply one — which is what lets the "confirm twice" case write to a row
    /// the entity save just soft-deleted (`v2.py:462-471`).
    ///
    /// Bind order: `$1` asset id, `$2` `attributes` (any JSON value, including
    /// `NULL`).
    FILE_ASSET_SET_UPLOAD_STATE = "assets_write_a_file_asset_set_upload_state",
    "UPDATE file_assets SET is_uploaded = TRUE, attributes = $2 WHERE id = $1",
    []
}

plane_query! {
    /// `asset.is_deleted = True; asset.deleted_at = timezone.now();
    /// asset.save(update_fields=["is_deleted", "deleted_at"])`
    /// (`v2.py:488-492`, `:714-717`).
    ///
    /// A primary-key update with no manager predicate, unlike the entity
    /// helper's `filter(...)`-based soft delete.
    ///
    /// Bind order: `$1` asset id, `$2` `deleted_at`.
    FILE_ASSET_SET_DELETED = "assets_write_a_file_asset_set_deleted",
    "UPDATE file_assets SET is_deleted = TRUE, deleted_at = $2 WHERE id = $1",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &WORKSPACE_LOGO_STATE,
    &WORKSPACE_SET_LOGO,
    &WORKSPACE_CLEAR_LOGO,
    &PROJECT_COVER_STATE,
    &PROJECT_SET_COVER,
    &PROJECT_CLEAR_COVER,
    &PROJECT_BY_ID_FOR_COVER,
    &PROJECT_SET_COVER_FROM_BULK,
    &FILE_ASSETS_FOR_BULK,
    &FILE_ASSETS_SET_PROJECT,
    &FILE_ASSETS_SET_ISSUE_PROJECT,
    &FILE_ASSETS_SET_COMMENT,
    &FILE_ASSETS_SET_PAGE,
    &FILE_ASSETS_SET_DRAFT_ISSUE,
    &FILE_ASSET_SET_UPLOAD_STATE,
    &FILE_ASSET_SET_DELETED,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard every catalog uses: every `plane_query!` in this file is in
    /// [`ALL_QUERIES`], and the names are unique.
    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_a_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_a_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );

        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
        }
    }

    /// [`FILE_ASSETS_FOR_BULK`] selects the generated model's columns, in
    /// `FileAsset::from_row` order: a renamed or reordered column fails here
    /// instead of decoding the wrong index at runtime.
    #[test]
    fn the_bulk_lookup_selects_the_model_columns() {
        let expected: Vec<String> = plane_db::models::asset::FileAsset::COLUMNS
            .split(", ")
            .map(|column| {
                let column = column.trim();
                let start = column.find('"').expect("quoted column") + 1;
                let end = column.rfind('"').expect("quoted column");
                format!("fa.\"{}\"", &column[start..end])
            })
            .collect();
        let expected = format!("SELECT {} FROM file_assets fa", expected.join(", "));
        assert!(
            FILE_ASSETS_FOR_BULK.sql.starts_with(&expected),
            "FILE_ASSETS_FOR_BULK does not select FileAsset::COLUMNS:\n{}",
            FILE_ASSETS_FOR_BULK.sql
        );
    }
}
