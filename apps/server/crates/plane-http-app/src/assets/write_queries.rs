// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `file_assets` write statements shared by the v1, v2 and user-asset
//! routes (`plans/ASSETS.md` set P0).
//!
//! The three statements were lifted from
//! `crates/plane-http-app/src/issues/write_attachment.rs`, where they are the
//! only rows `IssueAttachmentV2Endpoint.post`/`patch` and the v1 delete read
//! and write. Their text is unchanged so that issue-attachment behaviour stays
//! byte-identical; the move only lets the asset sets reuse them.
//!
//! * the INSERT/UPDATE column order is the model's, with the audit columns
//!   first — this is the order `FileAsset.objects.create()` / a full
//!   `save()` bind;
//! * a partial `save(update_fields=[…])` never uses [`FILE_ASSET_UPDATE`]: it
//!   writes only the listed columns and never touches `updated_at`
//!   (`MODEL_HOOKS.md` §2.1), so those call sites build their own two-column
//!   UPDATE;
//! * `FILE_ASSET_BY_PK` filters on the manager's `deleted_at IS NULL`
//!   (`plane/db/mixins.py:57`) and the four route kwargs
//!   (`attachment.py:214`-shape), not on a slug — the v1/v2 attachment
//!   handlers resolve the workspace first and pass its id.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `FileAsset.objects.create(...)` (`plane/app/views/issue/attachment.py:145`
    /// and every asset upload). Django emits one INSERT with every column, the
    /// JSON `attributes` as `jsonb`, and no `RETURNING`.
    ///
    /// Bind order: created_at, updated_at, id, attributes, asset, created_by,
    /// updated_by, workspace, is_deleted, deleted_at, is_archived, comment,
    /// entity_type, external_id, external_source, is_uploaded, issue, page,
    /// project, size, storage_metadata, user, draft_issue, entity_identifier.
    FILE_ASSET_INSERT = "assets_write_file_asset_insert",
    "INSERT INTO file_assets (created_at, updated_at, id, attributes, asset, \
     created_by_id, updated_by_id, workspace_id, is_deleted, deleted_at, \
     is_archived, comment_id, entity_type, external_id, external_source, \
     is_uploaded, issue_id, page_id, project_id, size, storage_metadata, \
     user_id, draft_issue_id, entity_identifier) VALUES \
     ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
      $17, $18, $19, $20, $21, $22, $23, $24)",
    []
}

plane_query! {
    /// A full-row `FileAsset.save()` with no `update_fields`: every column but
    /// the pk is written, in the model's order
    /// (`django/db/models/base.py:915-955`, `non_pks`). The in-memory row the
    /// handler binds already carries H01's `updated_at`/`updated_by` and the
    /// values the route changed, exactly like Django's instance.
    ///
    /// Bind order: created_at, updated_at, attributes, asset, created_by,
    /// updated_by, workspace, is_deleted, deleted_at, is_archived, comment,
    /// entity_type, external_id, external_source, is_uploaded, issue, page,
    /// project, size, storage_metadata, user, draft_issue, entity_identifier,
    /// id.
    FILE_ASSET_UPDATE = "assets_write_file_asset_update",
    "UPDATE file_assets SET created_at = $1, updated_at = $2, \
     attributes = $3, asset = $4, created_by_id = $5, updated_by_id = $6, \
     workspace_id = $7, is_deleted = $8, deleted_at = $9, is_archived = $10, \
     comment_id = $11, entity_type = $12, external_id = $13, \
     external_source = $14, is_uploaded = $15, issue_id = $16, page_id = $17, \
     project_id = $18, size = $19, storage_metadata = $20, user_id = $21, \
     draft_issue_id = $22, entity_identifier = $23 WHERE id = $24",
    []
}

plane_query! {
    /// `FileAsset.objects.get(id=…, workspace_id=…, project_id=…, issue_id=…)`
    /// — the v1/v2 attachment lookups (`plane/app/views/issue/attachment.py:214`
    /// and the ported `write_attachment::load_asset`).
    ///
    /// `FileAsset.objects` is the soft-deletion manager, so the statement
    /// carries `deleted_at IS NULL`; a soft-deleted asset is a
    /// `DoesNotExist`/404.
    ///
    /// The select list is [`plane_db::models::asset::FileAsset::COLUMNS`] in
    /// `from_row` order (the `fa.` alias is implied by the unqualified
    /// columns).
    ///
    /// Bind order: `$1` asset id, `$2` workspace id, `$3` project id,
    /// `$4` issue id.
    FILE_ASSET_BY_PK = "assets_write_file_asset_by_pk",
    "SELECT created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, id, attributes, asset, user_id, workspace_id, \
     draft_issue_id, project_id, issue_id, comment_id, page_id, entity_type, \
     entity_identifier, is_deleted, is_archived, external_id, \
     external_source, size, is_uploaded, storage_metadata FROM file_assets \
     WHERE id = $1 AND workspace_id = $2 AND project_id = $3 AND issue_id = $4 \
     AND deleted_at IS NULL",
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
    /// `FileAsset.objects.filter(id=…).first()` + `save(update_fields=
    /// ["is_deleted","deleted_at"])` for an **id-only** scope
    /// (`plane/app/views/asset/v2.py:278-287`, the workspace/project entity
    /// helper). Only the two listed columns are written: `updated_at` and
    /// `updated_by` are **not** touched (`MODEL_HOOKS.md` §2.1).
    ///
    /// One statement instead of the ORM's SELECT-then-UPDATE: the row is keyed
    /// by id, so `.first()` can only return that row, and the manager's
    /// `deleted_at IS NULL` is the same predicate.
    ///
    /// Bind order: `$1` asset id, `$2` deleted_at.
    FILE_ASSET_SOFT_DELETE = "assets_write_file_asset_soft_delete",
    "UPDATE file_assets SET is_deleted = TRUE, deleted_at = $2 \
     WHERE id = $1 AND deleted_at IS NULL",
    []
}

plane_query! {
    /// As [`FILE_ASSET_SOFT_DELETE`], for the user-avatar/cover helper's
    /// `filter(id=…, user_id=…)` scope (`v2.py:73-81`): a row that belongs to
    /// another user is left alone.
    ///
    /// Bind order: `$1` asset id, `$2` deleted_at, `$3` user id.
    FILE_ASSET_SOFT_DELETE_FOR_USER = "assets_write_file_asset_soft_delete_for_user",
    "UPDATE file_assets SET is_deleted = TRUE, deleted_at = $2 \
     WHERE id = $1 AND deleted_at IS NULL AND user_id = $3",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &FILE_ASSET_INSERT,
    &FILE_ASSET_UPDATE,
    &FILE_ASSET_BY_PK,
    &FILE_ASSET_SOFT_DELETE,
    &FILE_ASSET_SOFT_DELETE_FOR_USER,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard every catalog uses: every `plane_query!` in this file is in
    /// [`ALL_QUERIES`], and names are unique crate-wide (the integration test
    /// in `tests/schema_check.rs` checks the cross-catalog half).
    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_queries.rs declares {declared} statements but ALL_QUERIES has {}",
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
        // The DML statements return nothing, so their column lists are empty;
        // the lookup must still decode `FileAsset::from_row`.
        assert!(!FILE_ASSET_BY_PK.columns.is_empty());
    }

    /// The lifted lookup selects the generated model's columns, in its order:
    /// a renamed or reordered column fails here instead of decoding the wrong
    /// index at runtime.
    #[test]
    fn the_lookup_selects_the_model_columns() {
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
            FILE_ASSET_BY_PK.sql.starts_with(&expected),
            "FILE_ASSET_BY_PK does not select FileAsset::COLUMNS:\n{}",
            FILE_ASSET_BY_PK.sql
        );
    }
}
