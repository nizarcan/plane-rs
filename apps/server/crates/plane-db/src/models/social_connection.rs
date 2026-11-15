// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/social_connection.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `SocialLoginConnection` — table `social_login_connections`, generated from `apps/api/plane/db/models/social_connection.py`.
///
/// 12 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct SocialLoginConnection {
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
    /// `medium` — `CharField`, `max_length=20`. Default: `None`.
    pub medium: String,
    /// `last_login_at` — `DateTimeField`, nullable. Default: `django.utils.timezone.now`.
    pub last_login_at: Option<DateTime<Utc>>,
    /// `last_received_at` — `DateTimeField`, nullable. Default: `django.utils.timezone.now`.
    pub last_received_at: Option<DateTime<Utc>>,
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `token_data` — `JSONField`, nullable.
    pub token_data: Option<JsonText>,
    /// `extra_data` — `JSONField`, nullable.
    pub extra_data: Option<JsonText>,
}

impl SocialLoginConnection {
    /// The table this row is read from (`social_login_connections`).
    pub const TABLE: &'static str = "social_login_connections";

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
        ", {alias}.\"medium\"",
        ", {alias}.\"last_login_at\"",
        ", {alias}.\"last_received_at\"",
        ", {alias}.\"user_id\"",
        ", {alias}.\"token_data\"",
        ", {alias}.\"extra_data\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (12).
    pub const COLUMN_COUNT: usize = 12;

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
    /// Django WHERE: `("social_login_connections"."deleted_at" IS NULL)`.
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
            medium: row.try_get(start + 6)?,
            last_login_at: row.try_get(start + 7)?,
            last_received_at: row.try_get(start + 8)?,
            user_id: row.try_get(start + 9)?,
            token_data: row.try_get(start + 10)?,
            extra_data: row.try_get(start + 11)?,
        })
    }
}
