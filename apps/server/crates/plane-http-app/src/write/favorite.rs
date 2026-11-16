// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `UserFavorite.delete(soft=False)` for one instance — the deletion collector
//! over the self-referential `user_favorites.parent` relation.
//!
//! The three favourite destroys this module ports:
//!
//! | Route | Python |
//! | --- | --- |
//! | 126 `DELETE …/favorite-pages/{page_id}/` | `PageFavoriteViewSet.destroy` (`plane/app/views/page/base.py:482-492`) |
//! | 111 `DELETE …/user-favorite-modules/{module_id}/` | `ModuleFavoriteViewSet.destroy` (`plane/app/views/module/base.py:833-844`) |
//! | 184 `DELETE …/user-favorite-views/{view_id}/` | `IssueViewFavoriteViewSet.destroy` (`plane/app/views/view/base.py:471-479`) |
//! | 150 `DELETE …/user-favorite-projects/{project_id}/` | `ProjectFavoritesViewSet.destroy` (`plane/app/views/project/base.py:527-535`) |
//! | 37 `DELETE …/user-favorite-cycles/{cycle_id}/` | `CycleFavoriteViewSet.destroy` (`plane/app/views/cycle/base.py:587-595`) |
//!
//! Each one loads the row, then calls `favourite.delete(soft=False)`. That is
//! **not** a single-row `DELETE`: `SoftDeleteModel.delete` hands the instance to
//! `super().delete()` (`plane/db/mixins.py:72-82`), i.e.
//! `django/db/models/deletion.py`'s `Collector`, and `UserFavorite.parent` is a
//! self-referential `on_delete=CASCADE` FK (`plane/db/models/favorite.py:25-31`).
//! The collector therefore also deletes every row reachable through `parent_id`
//! — at every depth, and including rows whose `deleted_at` is set, because the
//! cascade relation is read through the related model's `_base_manager` (a plain
//! manager, `django/db/models/deletion.py:398-410`).
//!
//! The SQL observed on the dev database, in order:
//!
//! ```sql
//! SELECT "user_favorites"."id" FROM "user_favorites"
//!  WHERE "user_favorites"."parent_id" IN ('…') ORDER BY "user_favorites"."created_at" DESC
//! -- … one such SELECT per depth level, while a level is non-empty
//! BEGIN
//! DELETE FROM "user_favorites" WHERE "user_favorites"."id" IN ('…', '…')
//! COMMIT
//! ```
//!
//! A single `DELETE` of the collected ids is that sequence's only observable
//! effect and is atomic on its own, so the port skips the explicit
//! `BEGIN`/`COMMIT`. Without children the collector's DELETE is the same
//! single-row `DELETE … WHERE id IN (…)` shape.
//!
//! The cascade crosses users and entity types: any favourite parented under the
//! deleted row goes with it, whoever owns it and whatever it points at.

use std::collections::HashSet;

use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use uuid::Uuid;

use super::favorite_queries as queries;

/// Hard-delete favourite `id` and every favourite reachable through
/// `user_favorites.parent`, like `instance.delete(soft=False)`.
///
/// # Errors
///
/// A database failure as the family's 500.
pub async fn hard_delete(client: &Client, id: Uuid) -> Result<(), ViewError> {
    // One probe per depth level, exactly like the collector
    // (`django/db/models/deletion.py:398-410`): `_base_manager`, so
    // soft-deleted children are collected too.
    let probe = queries::FAVORITE_CHILDREN.prepare(client).await?;
    let mut collected = vec![id];
    let mut frontier = vec![id];
    let mut seen = HashSet::from([id]);
    while !frontier.is_empty() {
        let rows = client.query(&probe, &[&frontier]).await?;
        let mut next = Vec::new();
        for row in rows {
            let child: Uuid = row.get(0);
            if seen.insert(child) {
                collected.push(child);
                next.push(child);
            }
        }
        frontier = next;
    }

    // The collector's single `DELETE … WHERE id IN (…)`.
    let statement = queries::FAVORITE_HARD_DELETE.prepare(client).await?;
    client.execute(&statement, &[&collected]).await?;
    Ok(())
}
