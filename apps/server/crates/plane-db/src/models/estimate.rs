// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/estimate.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::Error;

/// `Estimate` — table `estimates`, generated from `apps/api/plane/db/models/estimate.py`.
///
/// 12 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
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
    /// `project` → `db.Project` (`projects.id`, `CASCADE`).
    pub project_id: Uuid,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
    /// `name` — `CharField`, `max_length=255`.
    pub name: String,
    /// `description` — `TextField`.
    pub description: String,
    /// `type` — `CharField`, `max_length=255`. Default: `EstimateType.CATEGORIES`.
    pub r#type: String,
    /// `last_used` — `BooleanField`. Default: `False`.
    pub last_used: bool,
}

impl Estimate {
    /// The table this row is read from (`estimates`).
    pub const TABLE: &'static str = "estimates";

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
        ", {alias}.\"project_id\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"name\"",
        ", {alias}.\"description\"",
        ", {alias}.\"type\"",
        ", {alias}.\"last_used\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (12).
    pub const COLUMN_COUNT: usize = 12;

    /// Default ordering (`Meta.ordering = ['name']`).
    ///
    /// Django's SQL: `"name" ASC`
    /// with the alias placeholder inserted.
    pub const ORDER_BY: &'static str = "{alias}.\"name\" ASC";

    /// Manager `objects` — `plane.db.mixins.SoftDeletionManager.get_queryset`
    /// (`apps/api/plane/db/mixins.py:57`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return SoftDeletionQuerySet(self.model, using=self._db).filter(deleted_at__isnull=True)
    /// ```
    ///
    /// Django WHERE: `("estimates"."deleted_at" IS NULL)`.
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
            project_id: row.try_get(start + 6)?,
            workspace_id: row.try_get(start + 7)?,
            name: row.try_get(start + 8)?,
            description: row.try_get(start + 9)?,
            r#type: row.try_get(start + 10)?,
            last_used: row.try_get(start + 11)?,
        })
    }
}

/// `EstimatePoint` — table `estimate_points`, generated from `apps/api/plane/db/models/estimate.py`.
///
/// 12 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimatePoint {
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
    /// `project` → `db.Project` (`projects.id`, `CASCADE`).
    pub project_id: Uuid,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
    /// `estimate` → `db.Estimate` (`estimates.id`, `CASCADE`).
    pub estimate_id: Uuid,
    /// `key` — `IntegerField`. Default: `0`.
    pub key: i32,
    /// `description` — `TextField`.
    pub description: String,
    /// `value` — `CharField`, `max_length=255`.
    pub value: String,
}

impl EstimatePoint {
    /// The table this row is read from (`estimate_points`).
    pub const TABLE: &'static str = "estimate_points";

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
        ", {alias}.\"project_id\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"estimate_id\"",
        ", {alias}.\"key\"",
        ", {alias}.\"description\"",
        ", {alias}.\"value\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (12).
    pub const COLUMN_COUNT: usize = 12;

    /// Default ordering (`Meta.ordering = ['value']`).
    ///
    /// Django's SQL: `"value" ASC`
    /// with the alias placeholder inserted.
    pub const ORDER_BY: &'static str = "{alias}.\"value\" ASC";

    /// Manager `objects` — `plane.db.mixins.SoftDeletionManager.get_queryset`
    /// (`apps/api/plane/db/mixins.py:57`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return SoftDeletionQuerySet(self.model, using=self._db).filter(deleted_at__isnull=True)
    /// ```
    ///
    /// Django WHERE: `("estimate_points"."deleted_at" IS NULL)`.
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
            project_id: row.try_get(start + 6)?,
            workspace_id: row.try_get(start + 7)?,
            estimate_id: row.try_get(start + 8)?,
            key: row.try_get(start + 9)?,
            description: row.try_get(start + 10)?,
            value: row.try_get(start + 11)?,
        })
    }
}
