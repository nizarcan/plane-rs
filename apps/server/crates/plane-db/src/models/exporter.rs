// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/exporter.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `ExporterHistory` — table `exporters`, generated from `apps/api/plane/db/models/exporter.py`.
///
/// 19 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ExporterHistory {
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
    /// `name` — `CharField`, `max_length=255`, nullable.
    pub name: Option<String>,
    /// `type` — `CharField`, `max_length=50`. Default: `'issue_exports'`.
    pub r#type: String,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
    /// `project` — `ArrayField`, nullable.
    pub project: Option<Vec<Uuid>>,
    /// `provider` — `CharField`, `max_length=50`.
    pub provider: String,
    /// `status` — `CharField`, `max_length=50`. Default: `'queued'`.
    pub status: String,
    /// `reason` — `TextField`.
    pub reason: String,
    /// `key` — `TextField`.
    pub key: String,
    /// `url` — `URLField`, `max_length=800`, nullable.
    pub url: Option<String>,
    /// `token` — `CharField`, `max_length=255`, `unique`. Default: `plane.db.models.exporter.generate_token`.
    pub token: String,
    /// `initiated_by` → `db.User` (`users.id`, `CASCADE`).
    pub initiated_by_id: Uuid,
    /// `filters` — `JSONField`, nullable.
    pub filters: Option<JsonText>,
    /// `rich_filters` — `JSONField`, nullable. Default: `builtins.dict`.
    pub rich_filters: Option<JsonText>,
}

impl ExporterHistory {
    /// The table this row is read from (`exporters`).
    pub const TABLE: &'static str = "exporters";

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
        ", {alias}.\"type\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"project\"",
        ", {alias}.\"provider\"",
        ", {alias}.\"status\"",
        ", {alias}.\"reason\"",
        ", {alias}.\"key\"",
        ", {alias}.\"url\"",
        ", {alias}.\"token\"",
        ", {alias}.\"initiated_by_id\"",
        ", {alias}.\"filters\"",
        ", {alias}.\"rich_filters\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (19).
    pub const COLUMN_COUNT: usize = 19;

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
    /// Django WHERE: `("exporters"."deleted_at" IS NULL)`.
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
            r#type: row.try_get(start + 7)?,
            workspace_id: row.try_get(start + 8)?,
            project: row.try_get(start + 9)?,
            provider: row.try_get(start + 10)?,
            status: row.try_get(start + 11)?,
            reason: row.try_get(start + 12)?,
            key: row.try_get(start + 13)?,
            url: row.try_get(start + 14)?,
            token: row.try_get(start + 15)?,
            initiated_by_id: row.try_get(start + 16)?,
            filters: row.try_get(start + 17)?,
            rich_filters: row.try_get(start + 18)?,
        })
    }
}
