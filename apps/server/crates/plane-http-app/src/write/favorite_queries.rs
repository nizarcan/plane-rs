// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two statements of [`super::favorite::hard_delete`] — the shared port of
//! `UserFavorite.delete(soft=False)`, Django's deletion collector over the
//! self-referential `user_favorites.parent` relation.
//!
//! One catalog for the helper because three routes call it (page favourites
//! 126, module favourites 111, view favourites 184) and each domain's own
//! catalog already has its lookup; duplicating these two statements per domain
//! would let their texts drift apart.

use plane_db::plane_query;

plane_query! {
    /// The deletion collector's cascade probe, one statement per depth level:
    /// `related_model._base_manager.using(…).filter(parent__in=…)`
    /// (`django/db/models/deletion.py:398-410`). `_base_manager` is a plain
    /// manager, so **soft-deleted children count** too. The ordering is
    /// `UserFavorite.Meta.ordering = ("-created_at",)`
    /// (`plane/db/models/favorite.py:45`), which the collector keeps on the
    /// related-manager query; only the pk is selected because the collector
    /// calls `only("pk")` on the related queryset.
    ///
    /// Bind order: `$1` parent ids.
    FAVORITE_CHILDREN = "write_favorite_children",
    "SELECT uf.id FROM user_favorites uf \
     WHERE uf.parent_id = ANY($1::uuid[]) \
     ORDER BY uf.created_at DESC",
    [("id", Uuid)]
}

plane_query! {
    /// `instance.delete(soft=False)` — `SoftDeleteModel.delete`'s
    /// `super().delete()` (`plane/db/mixins.py:72-82`), i.e. Django's deletion
    /// collector. The collector deletes the favourite **and every row reachable
    /// through `user_favorites.parent`** (`plane/db/models/favorite.py:25-31`,
    /// `on_delete=CASCADE`) in one `DELETE … WHERE id IN (…)`
    /// (`django/db/models/deletion.py:497-510`). Collected by
    /// [`super::favorite::hard_delete`] before this statement runs.
    ///
    /// Bind order: `$1` ids.
    FAVORITE_HARD_DELETE = "write_favorite_hard_delete",
    "DELETE FROM user_favorites WHERE id = ANY($1::uuid[])",
    []
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&plane_db::Query] = &[&FAVORITE_CHILDREN, &FAVORITE_HARD_DELETE];
