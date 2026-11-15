// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/api.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use std::net::IpAddr;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::Error;

/// `APIActivityLog` — table `api_activity_logs`, generated from `apps/api/plane/db/models/api.py`.
///
/// 16 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct APIActivityLog {
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
    /// `token_identifier` — `CharField`, `max_length=255`.
    pub token_identifier: String,
    /// `path` — `CharField`, `max_length=255`.
    pub path: String,
    /// `method` — `CharField`, `max_length=10`.
    pub method: String,
    /// `query_params` — `TextField`, nullable.
    pub query_params: Option<String>,
    /// `headers` — `TextField`, nullable.
    pub headers: Option<String>,
    /// `body` — `TextField`, nullable.
    pub body: Option<String>,
    /// `response_code` — `PositiveIntegerField`.
    pub response_code: i32,
    /// `response_body` — `TextField`, nullable.
    pub response_body: Option<String>,
    /// `ip_address` — `GenericIPAddressField`, `max_length=39`, nullable.
    pub ip_address: Option<IpAddr>,
    /// `user_agent` — `CharField`, `max_length=512`, nullable.
    pub user_agent: Option<String>,
}

impl APIActivityLog {
    /// The table this row is read from (`api_activity_logs`).
    pub const TABLE: &'static str = "api_activity_logs";

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
        ", {alias}.\"token_identifier\"",
        ", {alias}.\"path\"",
        ", {alias}.\"method\"",
        ", {alias}.\"query_params\"",
        ", {alias}.\"headers\"",
        ", {alias}.\"body\"",
        ", {alias}.\"response_code\"",
        ", {alias}.\"response_body\"",
        ", {alias}.\"ip_address\"",
        ", {alias}.\"user_agent\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (16).
    pub const COLUMN_COUNT: usize = 16;

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
    /// Django WHERE: `("api_activity_logs"."deleted_at" IS NULL)`.
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
            token_identifier: row.try_get(start + 6)?,
            path: row.try_get(start + 7)?,
            method: row.try_get(start + 8)?,
            query_params: row.try_get(start + 9)?,
            headers: row.try_get(start + 10)?,
            body: row.try_get(start + 11)?,
            response_code: row.try_get(start + 12)?,
            response_body: row.try_get(start + 13)?,
            ip_address: row.try_get(start + 14)?,
            user_agent: row.try_get(start + 15)?,
        })
    }
}

/// `APIToken` — table `api_tokens`, generated from `apps/api/plane/db/models/api.py`.
///
/// 15 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct APIToken {
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
    /// `label` — `CharField`, `max_length=255`. Default: `plane.db.models.api.generate_label_token`.
    pub label: String,
    /// `description` — `TextField`.
    pub description: String,
    /// `is_active` — `BooleanField`. Default: `True`.
    pub is_active: bool,
    /// `last_used` — `DateTimeField`, nullable.
    pub last_used: Option<DateTime<Utc>>,
    /// `token` — `CharField`, `max_length=255`, `unique`, `db_index`. Default: `plane.db.models.api.generate_token`.
    pub token: String,
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `user_type` — `PositiveSmallIntegerField`. Default: `0`.
    pub user_type: i16,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`), nullable.
    pub workspace_id: Option<Uuid>,
    /// `expired_at` — `DateTimeField`, nullable.
    pub expired_at: Option<DateTime<Utc>>,
}

impl APIToken {
    /// The table this row is read from (`api_tokens`).
    pub const TABLE: &'static str = "api_tokens";

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
        ", {alias}.\"label\"",
        ", {alias}.\"description\"",
        ", {alias}.\"is_active\"",
        ", {alias}.\"last_used\"",
        ", {alias}.\"token\"",
        ", {alias}.\"user_id\"",
        ", {alias}.\"user_type\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"expired_at\"",
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
    /// Django WHERE: `("api_tokens"."deleted_at" IS NULL)`.
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
            label: row.try_get(start + 6)?,
            description: row.try_get(start + 7)?,
            is_active: row.try_get(start + 8)?,
            last_used: row.try_get(start + 9)?,
            token: row.try_get(start + 10)?,
            user_id: row.try_get(start + 11)?,
            user_type: row.try_get(start + 12)?,
            workspace_id: row.try_get(start + 13)?,
            expired_at: row.try_get(start + 14)?,
        })
    }
}
