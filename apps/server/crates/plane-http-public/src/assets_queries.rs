// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the v1 asset routes (273–278).
//!
//! Three of the six routes reuse statements the app surface already owns:
//!
//! | Python | Statement |
//! | --- | --- |
//! | `FileAsset.objects.create(...)` (`api/views/asset.py:155,328,595`) | `plane_http_app::assets::write_queries::FILE_ASSET_INSERT` |
//! | `asset.save(update_fields=["is_uploaded","attributes"])` (`:221`, `:377`, `:664`) | `plane_http_app::assets::write_a_queries::FILE_ASSET_SET_UPLOAD_STATE` |
//! | `asset.save(update_fields=["is_deleted","deleted_at"])` (`:244`, `:401`) | `plane_http_app::assets::write_a_queries::FILE_ASSET_SET_DELETED` |
//! | `FileAsset.objects.get(id, user_id)` (`:212`, `:239`, `:368`, `:396`) | `plane_http_app::assets::write_c_queries::FILE_ASSET_BY_PK_FOR_USER` |
//! | `user.save()` after nulling the avatar/cover FK (`:65-74`, `:263-272`) | `plane_http_app::assets::write_c_queries::USER_SAVE_FOR_ASSET` |
//! | `Project.objects.filter(id, workspace_id).exists()` (`:555`) | `plane_http_app::assets::write_c_queries::PROJECT_EXISTS_IN_WORKSPACE` |
//! | `ProjectMember.objects.filter(..., is_active=True).exists()` (`:560-565`) | `plane_http_app::assets::write_c_queries::PROJECT_MEMBER_ACTIVE_EXISTS` |
//! | `Workspace.objects.get(slug=…)` (`:435`, `:545`) | `plane_http_app::common::queries::WORKSPACE_BY_SLUG` |
//!
//! What is new here is the generic-asset lookups, which are **not** the app
//! surface's `v2.py` statements:
//!
//! * `GET /assets/{asset_id}/` filters `workspace_id` and `is_deleted=False`
//!   on the soft-deletion manager (`asset.py:438`);
//! * `PATCH` filters `workspace__slug` and `is_deleted=False` (`:648`);
//! * the external-id dedup (`:576-582`) and the two 409 lookups.
//!
//! The extra `is_deleted = FALSE` matters: the legacy app routes
//! (`plane/app/views/asset/base.py:63`) write only `is_deleted`, so a row can
//! be `is_deleted=true` with `deleted_at IS NULL` and stay visible to
//! `FileAsset.objects` while the v1 routes 404 it.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `FileAsset.objects.get(id=asset_id, workspace_id=workspace.id,
    /// is_deleted=False)` — the generic-asset `GET` (`api/views/asset.py:438`).
    ///
    /// `FileAsset.objects` is the soft-deletion manager, so `deleted_at IS
    /// NULL` is the manager's own predicate; `is_deleted` is the view's.
    ///
    /// Bind order: `$1` asset id, `$2` workspace id.
    GENERIC_ASSET_BY_ID = "v1_assets_generic_asset_by_id",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
         fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
         fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
         fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
         fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
         fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", \
         fa.\"size\", fa.\"is_uploaded\", fa.\"storage_metadata\" FROM file_assets fa \
         WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 \
           AND fa.\"workspace_id\" = $2 AND NOT fa.\"is_deleted\" \
         LIMIT 21",
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
    /// `FileAsset.objects.get(id=asset_id, workspace__slug=slug,
    /// is_deleted=False)` — the generic-asset `PATCH`
    /// (`api/views/asset.py:648`).
    ///
    /// The `workspaces` join filters on `slug` alone: a related-model filter
    /// does not apply the related manager's soft-delete predicate, so a
    /// soft-deleted workspace still resolves its assets (the app surface's
    /// [`plane_http_app::assets::queries::FILE_ASSET_BY_ID_IN_WORKSPACE`] is
    /// the same shape).
    ///
    /// Bind order: `$1` asset id, `$2` workspace slug.
    GENERIC_ASSET_BY_SLUG = "v1_assets_generic_asset_by_slug",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
         fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
         fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
         fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
         fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
         fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", \
         fa.\"size\", fa.\"is_uploaded\", fa.\"storage_metadata\" FROM file_assets fa \
         INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
         WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 AND w.\"slug\" = $2 \
           AND NOT fa.\"is_deleted\" \
         LIMIT 21",
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
    /// `FileAsset.objects.filter(workspace_id=…, project_id=…,
    /// external_source=…, external_id=…, is_deleted=False).first()` —
    /// the generic upload's dedup (`api/views/asset.py:576-582`).
    ///
    /// `.first()` applies `Meta.ordering = ("-created_at",)`, hence the
    /// `ORDER BY` + `LIMIT 1`; the manager adds `deleted_at IS NULL`.
    ///
    /// Bind order: `$1` workspace id, `$2` project id, `$3` external source,
    /// `$4` external id.
    GENERIC_ASSET_BY_EXTERNAL = "v1_assets_generic_asset_by_external",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
         fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
         fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
         fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
         fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
         fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", \
         fa.\"size\", fa.\"is_uploaded\", fa.\"storage_metadata\" FROM file_assets fa \
         WHERE fa.\"deleted_at\" IS NULL AND fa.\"workspace_id\" = $1 \
           AND fa.\"project_id\" = $2 AND fa.\"external_source\" = $3 \
           AND fa.\"external_id\" = $4 AND NOT fa.\"is_deleted\" \
         ORDER BY fa.\"created_at\" DESC \
         LIMIT 1",
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
    /// `asset.save(update_fields=["is_uploaded"])` — the generic `PATCH`
    /// (`api/views/asset.py:664`).
    ///
    /// One column, written by primary key with no manager predicate
    /// (`MODEL_HOOKS.md` §2.1): `updated_at`/`updated_by` keep their stored
    /// values. The app surface's `write_a_queries::FILE_ASSET_SET_UPLOAD_STATE`
    /// is the two-column variant this route does not use.
    ///
    /// Bind order: `$1` asset id, `$2` `is_uploaded`.
    FILE_ASSET_SET_IS_UPLOADED = "v1_assets_file_asset_set_is_uploaded",
    "UPDATE file_assets SET is_uploaded = $2 WHERE id = $1",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &GENERIC_ASSET_BY_ID,
    &GENERIC_ASSET_BY_SLUG,
    &GENERIC_ASSET_BY_EXTERNAL,
    &FILE_ASSET_SET_IS_UPLOADED,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("assets_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "assets_queries.rs declares {declared} statements but ALL_QUERIES has {}",
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

    /// Every lookup is a `SELECT` over the model's columns; `schema_check`
    /// prepares each statement against the migrated database and checks the
    /// column names and types, which is the authoritative guard.
    #[test]
    fn the_lookups_are_selects() {
        assert!(GENERIC_ASSET_BY_ID.sql.starts_with("SELECT "));
        assert!(GENERIC_ASSET_BY_SLUG.sql.starts_with("SELECT "));
        assert!(GENERIC_ASSET_BY_EXTERNAL.sql.starts_with("SELECT "));
    }
}
