// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/device.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use std::net::IpAddr;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::Error;

/// `Device` — table `devices`, generated from `apps/api/plane/db/models/device.py`.
///
/// 11 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
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
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `device_id` — `CharField`, `max_length=255`, nullable.
    pub device_id: Option<String>,
    /// `device_type` — `CharField`, `max_length=255`.
    pub device_type: String,
    /// `push_token` — `CharField`, `max_length=255`, nullable.
    pub push_token: Option<String>,
    /// `is_active` — `BooleanField`. Default: `True`.
    pub is_active: bool,
}

impl Device {
    /// The table this row is read from (`devices`).
    pub const TABLE: &'static str = "devices";

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
        ", {alias}.\"user_id\"",
        ", {alias}.\"device_id\"",
        ", {alias}.\"device_type\"",
        ", {alias}.\"push_token\"",
        ", {alias}.\"is_active\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (11).
    pub const COLUMN_COUNT: usize = 11;

    /// No `Meta.ordering`: Django leaves the row order to Postgres.
    pub const ORDER_BY: &'static str = "";

    /// Manager `objects` — `plane.db.mixins.SoftDeletionManager.get_queryset`
    /// (`apps/api/plane/db/mixins.py:57`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return SoftDeletionQuerySet(self.model, using=self._db).filter(deleted_at__isnull=True)
    /// ```
    ///
    /// Django WHERE: `("devices"."deleted_at" IS NULL)`.
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
            user_id: row.try_get(start + 6)?,
            device_id: row.try_get(start + 7)?,
            device_type: row.try_get(start + 8)?,
            push_token: row.try_get(start + 9)?,
            is_active: row.try_get(start + 10)?,
        })
    }
}

/// `DeviceSession` — table `device_sessions`, generated from `apps/api/plane/db/models/device.py`.
///
/// 13 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceSession {
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
    /// `device` → `db.Device` (`devices.id`, `CASCADE`).
    pub device_id: Uuid,
    /// `session` → `db.Session` (`sessions.session_key`, `CASCADE`).
    pub session_id: String,
    /// `is_active` — `BooleanField`. Default: `True`.
    pub is_active: bool,
    /// `user_agent` — `CharField`, `max_length=255`, nullable.
    pub user_agent: Option<String>,
    /// `ip_address` — `GenericIPAddressField`, `max_length=39`, nullable.
    pub ip_address: Option<IpAddr>,
    /// `start_time` — `DateTimeField`, `auto_now_add`.
    pub start_time: DateTime<Utc>,
    /// `end_time` — `DateTimeField`, nullable.
    pub end_time: Option<DateTime<Utc>>,
}

impl DeviceSession {
    /// The table this row is read from (`device_sessions`).
    pub const TABLE: &'static str = "device_sessions";

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
        ", {alias}.\"device_id\"",
        ", {alias}.\"session_id\"",
        ", {alias}.\"is_active\"",
        ", {alias}.\"user_agent\"",
        ", {alias}.\"ip_address\"",
        ", {alias}.\"start_time\"",
        ", {alias}.\"end_time\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (13).
    pub const COLUMN_COUNT: usize = 13;

    /// No `Meta.ordering`: Django leaves the row order to Postgres.
    pub const ORDER_BY: &'static str = "";

    /// Manager `objects` — `plane.db.mixins.SoftDeletionManager.get_queryset`
    /// (`apps/api/plane/db/mixins.py:57`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return SoftDeletionQuerySet(self.model, using=self._db).filter(deleted_at__isnull=True)
    /// ```
    ///
    /// Django WHERE: `("device_sessions"."deleted_at" IS NULL)`.
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
            device_id: row.try_get(start + 6)?,
            session_id: row.try_get(start + 7)?,
            is_active: row.try_get(start + 8)?,
            user_agent: row.try_get(start + 9)?,
            ip_address: row.try_get(start + 10)?,
            start_time: row.try_get(start + 11)?,
            end_time: row.try_get(start + 12)?,
        })
    }
}
