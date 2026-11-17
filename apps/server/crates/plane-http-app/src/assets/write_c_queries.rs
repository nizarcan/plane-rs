// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `file_assets`, `users` and `projects` statements of the user-asset and
//! duplicate writes (`plans/ASSETS.md` set C: routes 20, 21, 22 and 28).
//!
//! ## Managers
//!
//! `FileAsset.objects` is the soft-deletion manager (`plane/db/mixins.py:57`),
//! so every lookup that starts from it carries `deleted_at IS NULL`.
//! `FileAsset.all_objects` does not, which is what makes 22's restore reach a
//! soft-deleted row (`v2.py:572`). A `filter(<relation>__field=…)` join does
//! **not** apply the related model's predicate, so the `workspaces` join below
//! filters on `slug` alone, exactly like the ORM.
//!
//! `users` has no soft-delete column and `User` is not a `BaseModel` (it is
//! `AbstractBaseUser, PermissionsMixin`, `plane/db/models/user.py:56`), so its
//! statements have no audit columns and no manager predicate.
//!
//! ## `update_fields` on the asset row
//!
//! 21's `save(update_fields=["is_uploaded","attributes"])` and
//! `save(update_fields=["is_deleted","deleted_at"])` write only the listed
//! columns (`MODEL_HOOKS.md` §2.1), so they reuse
//! `write_a_queries::FILE_ASSET_SET_UPLOAD_STATE`/`FILE_ASSET_SET_DELETED`
//! rather than the full-row `write_queries::FILE_ASSET_UPDATE`. 28's
//! `FileAsset.objects.filter(id=…).update(is_uploaded=True)` is a queryset
//! update: one column, and never `updated_at`/`updated_by`.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `FileAsset.objects.get(id=asset_id, user_id=request.user.id)`
    /// (`v2.py:214`, `:234`): the user-asset detail routes only ever touch the
    /// caller's own uploads.
    ///
    /// Bind order: `$1` asset id, `$2` user id.
    FILE_ASSET_BY_PK_FOR_USER = "assets_write_c_file_asset_by_pk_for_user",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 AND fa.\"user_id\" = $2 \
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
    /// `FileAsset.all_objects.get(id=asset_id, workspace__slug=slug)`
    /// (`v2.py:572`, `AssetRestoreEndpoint.post`). `all_objects` skips the
    /// soft-deletion predicate, so a deleted row is found and restored.
    ///
    /// Bind order: `$1` asset id, `$2` workspace slug.
    FILE_ASSET_ANY_STATE_IN_WORKSPACE = "assets_write_c_file_asset_any_state_in_workspace",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"id\" = $1 AND w.\"slug\" = $2 \
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
    /// `asset.is_deleted = False; asset.deleted_at = None;
    /// asset.save(update_fields=["is_deleted", "deleted_at"])`
    /// (`v2.py:578-580`).
    ///
    /// Only the two listed columns are written: `updated_at` and `updated_by`
    /// keep their stored values (`MODEL_HOOKS.md` §2.1), and the row is
    /// addressed by its primary key with no manager predicate.
    ///
    /// Bind order: `$1` asset id.
    FILE_ASSET_RESTORE = "assets_write_c_file_asset_restore",
    "UPDATE file_assets SET is_deleted = FALSE, deleted_at = NULL WHERE id = $1",
    []
}

plane_query! {
    /// `FileAsset.objects.filter(id=asset_id, is_uploaded=True,
    /// workspace=workspace).first()` (`v2.py:892-896`): the duplicate's source
    /// lookup, scoped to the destination workspace so an asset from another
    /// workspace is a 404.
    ///
    /// `Meta.ordering = ("-created_at",)` is what `.first()` applies; the id is
    /// the primary key, so the statement can only return one row.
    ///
    /// Bind order: `$1` asset id, `$2` workspace id.
    FILE_ASSET_DUPLICATE_SOURCE = "assets_write_c_file_asset_duplicate_source",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 AND fa.\"is_uploaded\" \
       AND fa.\"workspace_id\" = $2 \
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
    /// `FileAsset.objects.filter(id=duplicated_asset.id).update(is_uploaded=True)`
    /// (`v2.py:954`): a queryset update, so `updated_at`/`updated_by` are not
    /// touched and the manager's predicate still applies.
    ///
    /// Bind order: `$1` asset id.
    FILE_ASSET_SET_UPLOADED = "assets_write_c_file_asset_set_uploaded",
    "UPDATE file_assets SET is_uploaded = TRUE \
     WHERE \"deleted_at\" IS NULL AND \"id\" = $1",
    []
}

plane_query! {
    /// `user.save()`'s **observable** half for the avatar/cover helpers
    /// (`plane/db/models/user.py:169-187` + `Model.save`'s `auto_now`).
    ///
    /// A full save writes all 39 non-pk columns; the row is otherwise
    /// unchanged, and the harness compares the final row, so this writes the
    /// ten columns a save can change:
    ///
    /// * `email` — H30's `email.lower().strip()`;
    /// * `display_name` — H30's derivation when the stored value is empty;
    /// * `is_staff` — H30's `is_superuser` implication;
    /// * `token`, `token_updated_at` — H30's rotation whenever
    ///   `token_updated_at` is non-NULL (the fixture makes it non-NULL for
    ///   every user, so every avatar/cover write rotates the token);
    /// * `updated_at` — `DateTimeField(auto_now=True).pre_save`
    ///   (`django/db/models/fields/__init__.py`), a **second** `now` after
    ///   H30's own `timezone.now()`;
    /// * `avatar`/`avatar_asset_id`/`cover_image`/`cover_image_asset_id` — the
    ///   four columns the entity helpers assign (`v2.py:87-92,105-110,
    ///   126,139`): `avatar = ""` and `cover_image = None` before the FK, and
    ///   the FK set to the new asset or to NULL.
    ///
    /// Bind order: `$1` user id, `$2` email, `$3` display_name, `$4` is_staff,
    /// `$5` token, `$6` token_updated_at, `$7` updated_at, `$8` avatar,
    /// `$9` avatar_asset_id, `$10` cover_image, `$11` cover_image_asset_id.
    USER_SAVE_FOR_ASSET = "assets_write_c_user_save_for_asset",
    "UPDATE users SET email = $2, display_name = $3, is_staff = $4, \
     token = $5, token_updated_at = $6, updated_at = $7, avatar = $8, \
     avatar_asset_id = $9, cover_image = $10, cover_image_asset_id = $11 \
     WHERE id = $1",
    []
}

plane_query! {
    /// `Project.objects.filter(id=project_id, workspace=workspace).exists()`
    /// (`v2.py:917`, the duplicate's destination check). `Project.objects`
    /// filters `deleted_at IS NULL` (`plane/db/mixins.py:57`).
    ///
    /// Bind order: `$1` project id, `$2` workspace id.
    PROJECT_EXISTS_IN_WORKSPACE = "assets_write_c_project_exists_in_workspace",
    "SELECT 1::int4 AS present FROM projects \
     WHERE \"deleted_at\" IS NULL AND \"id\" = $1 AND \"workspace_id\" = $2 \
     LIMIT 1",
    [("present", Int4)]
}

plane_query! {
    /// `ProjectMember.objects.filter(member=request.user, workspace=workspace,
    /// project_id=project_id, is_active=True).exists()` (`v2.py:919-924`).
    ///
    /// Bind order: `$1` member id, `$2` workspace id, `$3` project id.
    PROJECT_MEMBER_ACTIVE_EXISTS = "assets_write_c_project_member_active_exists",
    "SELECT 1::int4 AS present FROM project_members \
     WHERE \"deleted_at\" IS NULL AND \"member_id\" = $1 \
       AND \"workspace_id\" = $2 AND \"project_id\" = $3 AND \"is_active\" \
     LIMIT 1",
    [("present", Int4)]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &FILE_ASSET_BY_PK_FOR_USER,
    &FILE_ASSET_ANY_STATE_IN_WORKSPACE,
    &FILE_ASSET_RESTORE,
    &FILE_ASSET_DUPLICATE_SOURCE,
    &FILE_ASSET_SET_UPLOADED,
    &USER_SAVE_FOR_ASSET,
    &PROJECT_EXISTS_IN_WORKSPACE,
    &PROJECT_MEMBER_ACTIVE_EXISTS,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard every catalog uses: every `plane_query!` in this file is in
    /// [`ALL_QUERIES`], and the names are unique crate-wide (the integration
    /// test in `tests/schema_check.rs` checks the cross-catalog half).
    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_c_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_c_queries.rs declares {declared} statements but ALL_QUERIES has {}",
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

    /// The four `FileAsset` lookups select the generated model's columns, in
    /// `from_row` order: a renamed or reordered column fails here instead of
    /// decoding the wrong index at runtime.
    #[test]
    fn the_asset_lookups_select_the_model_columns() {
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
        for query in [
            &FILE_ASSET_BY_PK_FOR_USER,
            &FILE_ASSET_ANY_STATE_IN_WORKSPACE,
            &FILE_ASSET_DUPLICATE_SOURCE,
        ] {
            assert!(
                query.sql.starts_with(&expected),
                "{} does not select FileAsset::COLUMNS:\n{}",
                query.name,
                query.sql
            );
        }
    }
}
