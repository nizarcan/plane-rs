// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The statements set A of the `users-me` writes adds
//! (`plans/USERS_ME.md` §3).
//!
//! Everything else these handlers issue is already catalogued:
//!
//! | Handler | Statement | Where |
//! | --- | --- | --- |
//! | 162 PATCH | `USER_UPDATE` | [`super::write_queries`] |
//! | 167 PATCH, 171, 172 | `USER_PROFILE` + `PROFILE_UPDATE` | [`super::queries`], [`super::write_queries`] |
//! | 169 DELETE | `USER_ACCOUNT_BY_ID` | [`super::queries`] |
//! | 226 POST, 227 PATCH/DELETE | `API_TOKEN_INSERT`/`BY_PK`/`UPDATE` | [`super::write_queries`] |
//!
//! `tests/schema_check.rs` prepares every statement against a Django-migrated
//! database, so a column rename fails in CI.

use plane_db::plane_query;

plane_query! {
    /// `AccountEndpoint.delete`'s `account.delete()`
    /// (`plane/app/views/user/base.py:436-439`).
    ///
    /// `Account` is a `TimeAuditModel` (`plane/db/models/user.py:271`), **not**
    /// a `SoftDeleteModel`: `Model.delete()` is Django's deletion collector, a
    /// real `DELETE` of the row (and of nothing else — `accounts` has no
    /// dependants). There is no `deleted_at` column to set and no
    /// `soft_delete_related_objects` task to publish.
    ACCOUNT_DELETE = "users_me_a_account_delete",
    "DELETE FROM accounts WHERE \"id\" = $1",
    []
}

plane_query! {
    /// The two columns `UserSerializer._validate_own_asset` reads off the
    /// `FileAsset` instance DRF's `PrimaryKeyRelatedField` loaded
    /// (`plane/app/serializers/user.py:26-38`):
    /// `value.user_id != self.instance.id or value.entity_type != entity_type`.
    ///
    /// Python runs one query per field — `queryset.get(pk=…)` — and keeps the
    /// row, so the ownership test costs no statement of its own. The generated
    /// `PK_AVATAR_ASSET`/`PK_COVER_IMAGE_ASSET` statements
    /// (`write/generated/user_serializer.rs`) only select the pk, because that
    /// is all DRF's `to_internal_value` needs; this second statement reads the
    /// two columns the field hook needs. Bind order: `$1` the asset id.
    ///
    /// No `deleted_at` predicate: the id already passed the field's own
    /// `FileAsset.objects` lookup, and this only re-reads the accepted row.
    FILE_ASSET_OWNER = "users_me_a_file_asset_owner",
    "SELECT a.\"user_id\" AS user_id, a.\"entity_type\" AS entity_type \
     FROM file_assets a WHERE a.\"id\" = $1",
    [
        ("user_id", Uuid),
        ("entity_type", Varchar),
    ]
}

/// Every catalogued statement of set A, in declaration order.
pub static ALL_QUERIES: &[&plane_db::Query] = &[&ACCOUNT_DELETE, &FILE_ASSET_OWNER];

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
