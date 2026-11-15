// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/favorite.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::Error;

/// `UserFavorite` — table `user_favorites`, generated from `apps/api/plane/db/models/favorite.py`.
///
/// 15 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct UserFavorite {
    /// `created_at` — `DateTimeField`, `auto_now_add`.
    pub created_at: DateTime<Utc>,
    /// `updated_at` — `DateTimeField`, `auto_now`.
    pub updated_at: DateTime<Utc>,
    /// `created_by` → `db.User` (`users.id`, `SET_NULL`), nullable.
    pub created_by_id: Option<Uuid>,
    /// `updated_by` → `db.User` (`users.id`, `SET_NULL`), nullable.
    pub updated_by_id: Option<Uuid>,
    /// `deleted_at` — `DateTimeField`, nullable.
    pub deleted_at: Option<DateTime<Utc>>,
    /// `id` — `UUIDField`, `max_length=32`, `primary key`, `unique`, `db_index`. Default: `uuid.uuid4`.
    pub id: Uuid,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
    /// `project` → `db.Project` (`projects.id`, `CASCADE`), nullable.
    pub project_id: Option<Uuid>,
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `entity_type` — `CharField`, `max_length=100`.
    pub entity_type: String,
    /// `entity_identifier` — `UUIDField`, `max_length=32`, nullable.
    pub entity_identifier: Option<Uuid>,
    /// `name` — `CharField`, `max_length=255`, nullable.
    pub name: Option<String>,
    /// `is_folder` — `BooleanField`. Default: `False`.
    pub is_folder: bool,
    /// `sequence` — `FloatField`. Default: `65535`.
    pub sequence: f64,
    /// `parent` → `db.UserFavorite` (`user_favorites.id`, `CASCADE`), nullable.
    pub parent_id: Option<Uuid>,
}

impl UserFavorite {
    /// The table this row is read from (`user_favorites`).
    pub const TABLE: &'static str = "user_favorites";

    /// The `<alias>`-qualified select list, in `from_row` order.
    ///
    /// Replace `{alias}` with the alias of the statement
    /// (`plane_db::models::with_alias`).
    pub const COLUMNS: &'static str = concat!(
        "{alias}.\"created_at\"",
        ", {alias}.\"updated_at\"",
        ", {alias}.\"created_by_id\"",
        ", {alias}.\"updated_by_id\"",
        ", {alias}.\"deleted_at\"",
        ", {alias}.\"id\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"project_id\"",
        ", {alias}.\"user_id\"",
        ", {alias}.\"entity_type\"",
        ", {alias}.\"entity_identifier\"",
        ", {alias}.\"name\"",
        ", {alias}.\"is_folder\"",
        ", {alias}.\"sequence\"",
        ", {alias}.\"parent_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (15).
    pub const COLUMN_COUNT: usize = 15;

    /// Default ordering (`Meta.ordering = ['-created_at']`).
    ///
    /// Django's SQL: `"created_at" DESC`
    /// with the alias placeholder inserted.
    pub const ORDER_BY: &'static str = "{alias}.\"created_at\" DESC";

    /// Manager `objects` — `plane.db.mixins.SoftDeletionManager.get_queryset`
    /// (`apps/api/plane/db/mixins.py:57`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return SoftDeletionQuerySet(self.model, using=self._db).filter(deleted_at__isnull=True)
    /// ```
    ///
    /// Django WHERE: `("user_favorites"."deleted_at" IS NULL)`.
    pub const OBJECTS: &'static str = "({alias}.\"deleted_at\" IS NULL)";

    /// Manager `all_objects` — `django.db.models.manager.Manager.get_queryset`
    /// (`site-packages/django/db/models/manager.py:150`).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         """
    ///         Return a new QuerySet object. Subclasses can override this method to
    ///         customize the behavior of the Manager.
    ///         """
    ///         return self._queryset_class(model=self.model, using=self._db, hints=self._hints)
    /// ```
    ///
    /// Django WHERE: none (every row).
    pub const ALL_OBJECTS: &'static str = "TRUE";

    /// Decode this model's row, starting at column `start`.
    ///
    /// Columns are read by index, which is why the select list and this
    /// function are generated together.
    pub fn from_row(row: &Row, start: usize) -> Result<Self, Error> {
        Ok(Self {
            created_at: row.try_get(start)?,
            updated_at: row.try_get(start + 1)?,
            created_by_id: row.try_get(start + 2)?,
            updated_by_id: row.try_get(start + 3)?,
            deleted_at: row.try_get(start + 4)?,
            id: row.try_get(start + 5)?,
            workspace_id: row.try_get(start + 6)?,
            project_id: row.try_get(start + 7)?,
            user_id: row.try_get(start + 8)?,
            entity_type: row.try_get(start + 9)?,
            entity_identifier: row.try_get(start + 10)?,
            name: row.try_get(start + 11)?,
            is_folder: row.try_get(start + 12)?,
            sequence: row.try_get(start + 13)?,
            parent_id: row.try_get(start + 14)?,
        })
    }
}
