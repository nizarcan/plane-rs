// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `file_assets` statements of the legacy (`v1`) asset writes
//! (`plans/ASSETS.md` set B, routes 13-17).
//!
//! The three statements reproduce the two `QuerySet.get()` lookups and the
//! column-only `save(update_fields=["is_deleted"])` of
//! `plane/app/views/asset/base.py`:
//!
//! | Python | Statement |
//! | --- | --- |
//! | `FileAsset.objects.get(asset=asset_key)` (`base.py:57`, `:73`) | [`FILE_ASSET_BY_ASSET_KEY`] |
//! | `FileAsset.objects.get(asset=asset_key, created_by=request.user)` (`base.py:106`) | [`FILE_ASSET_BY_ASSET_KEY_FOR_USER`] |
//! | `file_asset.save(update_fields=["is_deleted"])` (`base.py:64`, `:80`, `:108`) | [`FILE_ASSET_SET_IS_DELETED`] |
//!
//! None of them filters on the URL's `workspace_id`: the key a legacy route
//! builds is `str(workspace_id) + "/" + asset_key` (`base.py:56`, `:72`), so the
//! workspace is part of the key, and the user-asset routes filter on
//! `created_by` alone. `WorkspaceMemberPermission` is what checks the workspace
//! (`plane/app/permissions/workspace.py:121-137`), not these queries.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `FileAsset.objects.get(asset=asset_key)` — `base.py:57` (DELETE) and
    /// `base.py:73` (`FileAssetViewSet.restore`).
    ///
    /// `FileAsset.objects` is the soft-deletion manager, so the statement
    /// carries `deleted_at IS NULL` (`plane/db/mixins.py:57`): a row the v2
    /// routes soft-deleted (both columns) is a `DoesNotExist` → 404, while the
    /// legacy routes' own `is_deleted = True` leaves it visible.
    ///
    /// `.get()` raises `MultipleObjectsReturned` when two live rows share the
    /// key (`base.py` has no uniqueness constraint on `asset`), which Plane's
    /// `handle_exception` renders as the family 500. `LIMIT 2` is enough to
    /// tell one row from many; Django's own cap is `MAX_GET_RESULTS` (21).
    ///
    /// Bind order: `$1` the asset key.
    FILE_ASSET_BY_ASSET_KEY = "assets_write_b_file_asset_by_asset_key",
    "SELECT created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, id, attributes, asset, user_id, workspace_id, \
     draft_issue_id, project_id, issue_id, comment_id, page_id, entity_type, \
     entity_identifier, is_deleted, is_archived, external_id, \
     external_source, size, is_uploaded, storage_metadata FROM file_assets \
     WHERE deleted_at IS NULL AND asset = $1 \
     ORDER BY created_at DESC LIMIT 2",
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
    /// `FileAsset.objects.get(asset=asset_key, created_by=request.user)` —
    /// `UserAssetsEndpoint.delete` (`base.py:106`).
    ///
    /// As [`FILE_ASSET_BY_ASSET_KEY`], plus the `created_by` scope that makes
    /// another user's asset a 404 rather than a 403. A workspace asset key
    /// contains a `/` and cannot reach this route (Django's `<str:asset_key>`
    /// converter excludes it), but the statement does not care: the key is
    /// compared as a whole string.
    ///
    /// Bind order: `$1` the asset key, `$2` the caller's user id.
    FILE_ASSET_BY_ASSET_KEY_FOR_USER = "assets_write_b_file_asset_by_asset_key_for_user",
    "SELECT created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, id, attributes, asset, user_id, workspace_id, \
     draft_issue_id, project_id, issue_id, comment_id, page_id, entity_type, \
     entity_identifier, is_deleted, is_archived, external_id, \
     external_source, size, is_uploaded, storage_metadata FROM file_assets \
     WHERE deleted_at IS NULL AND asset = $1 AND created_by_id = $2 \
     ORDER BY created_at DESC LIMIT 2",
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
    /// `file_asset.save(update_fields=["is_deleted"])` — the legacy DELETE
    /// (`base.py:63-64`, `base.py:107-108`) and restore (`base.py:79-80`).
    ///
    /// Django writes **only** the listed column (`django/db/models/base.py`
    /// `_save_table`'s `non_pks` filter, `MODEL_HOOKS.md` §2.1): `updated_at`
    /// and `updated_by` keep their stored values, and `deleted_at` is left
    /// NULL — that is why the row stays visible to `FileAsset.objects` and a
    /// restore finds it. The `_base_manager` update is `WHERE id = $1` with no
    /// soft-delete predicate.
    ///
    /// Bind order: `$1` the asset id, `$2` `is_deleted`.
    FILE_ASSET_SET_IS_DELETED = "assets_write_b_file_asset_set_is_deleted",
    "UPDATE file_assets SET is_deleted = $2 WHERE id = $1",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &FILE_ASSET_BY_ASSET_KEY,
    &FILE_ASSET_BY_ASSET_KEY_FOR_USER,
    &FILE_ASSET_SET_IS_DELETED,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard every catalog uses: every `plane_query!` in this file is in
    /// [`ALL_QUERIES`], and the names are unique (the integration test in
    /// `tests/schema_check.rs` checks the cross-catalog half).
    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_b_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_b_queries.rs declares {declared} statements but ALL_QUERIES has {}",
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
        // The two lookups must decode `FileAsset::from_row`; the UPDATE returns
        // nothing.
        assert!(!FILE_ASSET_BY_ASSET_KEY.columns.is_empty());
        assert!(!FILE_ASSET_BY_ASSET_KEY_FOR_USER.columns.is_empty());
        assert!(FILE_ASSET_SET_IS_DELETED.columns.is_empty());
    }

    /// The lookups select the generated model's columns, in its order.
    #[test]
    fn the_lookups_select_the_model_columns() {
        let expected: Vec<String> = plane_db::models::asset::FileAsset::COLUMNS
            .split(", ")
            .map(|column| {
                let column = column.trim();
                let start = column.find('"').expect("quoted column") + 1;
                let end = column.rfind('"').expect("quoted column");
                column[start..end].to_owned()
            })
            .collect();
        let expected = format!("SELECT {} FROM file_assets", expected.join(", "));
        assert!(
            FILE_ASSET_BY_ASSET_KEY.sql.starts_with(&expected),
            "FILE_ASSET_BY_ASSET_KEY does not select FileAsset::COLUMNS:\n{}",
            FILE_ASSET_BY_ASSET_KEY.sql
        );
        assert!(
            FILE_ASSET_BY_ASSET_KEY_FOR_USER.sql.starts_with(&expected),
            "FILE_ASSET_BY_ASSET_KEY_FOR_USER does not select FileAsset::COLUMNS:\n{}",
            FILE_ASSET_BY_ASSET_KEY_FOR_USER.sql
        );
    }
}
