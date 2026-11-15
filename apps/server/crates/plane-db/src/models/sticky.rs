// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/sticky.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `Sticky` — table `stickies`, generated from `apps/api/plane/db/models/sticky.py`.
///
/// 17 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Sticky {
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
    /// `name` — `TextField`, nullable.
    pub name: Option<String>,
    /// `description` — `JSONField`. Default: `builtins.dict`.
    pub description: JsonText,
    /// `description_html` — `TextField`. Default: `'<p></p>'`.
    pub description_html: String,
    /// `description_stripped` — `TextField`, nullable.
    pub description_stripped: Option<String>,
    /// `description_binary` — `BinaryField`, nullable.
    pub description_binary: Option<Vec<u8>>,
    /// `logo_props` — `JSONField`. Default: `builtins.dict`.
    pub logo_props: JsonText,
    /// `color` — `CharField`, `max_length=255`, nullable.
    pub color: Option<String>,
    /// `background_color` — `CharField`, `max_length=255`, nullable.
    pub background_color: Option<String>,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
    /// `owner` → `db.User` (`users.id`, `CASCADE`).
    pub owner_id: Uuid,
    /// `sort_order` — `FloatField`. Default: `65535`.
    pub sort_order: f64,
}

impl Sticky {
    /// The table this row is read from (`stickies`).
    pub const TABLE: &'static str = "stickies";

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
        ", {alias}.\"name\"",
        ", {alias}.\"description\"",
        ", {alias}.\"description_html\"",
        ", {alias}.\"description_stripped\"",
        ", {alias}.\"description_binary\"",
        ", {alias}.\"logo_props\"",
        ", {alias}.\"color\"",
        ", {alias}.\"background_color\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"owner_id\"",
        ", {alias}.\"sort_order\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (17).
    pub const COLUMN_COUNT: usize = 17;

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
    /// Django WHERE: `("stickies"."deleted_at" IS NULL)`.
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
            name: row.try_get(start + 6)?,
            description: row.try_get(start + 7)?,
            description_html: row.try_get(start + 8)?,
            description_stripped: row.try_get(start + 9)?,
            description_binary: row.try_get(start + 10)?,
            logo_props: row.try_get(start + 11)?,
            color: row.try_get(start + 12)?,
            background_color: row.try_get(start + 13)?,
            workspace_id: row.try_get(start + 14)?,
            owner_id: row.try_get(start + 15)?,
            sort_order: row.try_get(start + 16)?,
        })
    }
}
