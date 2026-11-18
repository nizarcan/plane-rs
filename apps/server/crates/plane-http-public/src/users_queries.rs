// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the v1 user route (314).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `request.user`'s row for `UserLiteSerializer` (`plane/api/views/
    /// user.py:40`). `users` has no `deleted_at` column
    /// (`plane/db/models/user.py:56`), so none is filtered.
    USER_BY_ID = "v1_users_by_id",
    "SELECT u.id, u.first_name, u.last_name, u.email, u.avatar, \
     u.avatar_asset_id, u.display_name \
     FROM users u WHERE u.id = $1 LIMIT 1",
    [
        ("id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("email", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("display_name", Varchar),
    ]
}

/// Every catalogued statement of this route family.
pub static ALL_QUERIES: &[&Query] = &[&USER_BY_ID];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("users_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
    }
}
