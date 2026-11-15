// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/session.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;

use crate::models::{Error, JsonText};

/// `Session` — table `sessions`, generated from `apps/api/plane/db/models/session.py`.
///
/// 5 columns, not soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// `session_data` — `TextField`.
    pub session_data: String,
    /// `expire_date` — `DateTimeField`, `db_index`.
    pub expire_date: DateTime<Utc>,
    /// `device_info` — `JSONField`, nullable. Default: `None`.
    pub device_info: Option<JsonText>,
    /// `session_key` — `CharField`, `max_length=128`, `primary key`, `unique`.
    pub session_key: String,
    /// `user_id` — `CharField`, `max_length=50`, `db_index`, nullable.
    pub user_id: Option<String>,
}

impl Session {
    /// The table this row is read from (`sessions`).
    pub const TABLE: &'static str = "sessions";

    /// The `<alias>`-qualified select list, in `from_row` order.
    ///
    /// Replace `{alias}` with the alias of the statement
    /// (`plane_db::models::with_alias`).
    pub const COLUMNS: &'static str = concat!(
        "{alias}.\"session_data\"",
        ", {alias}.\"expire_date\"",
        ", {alias}.\"device_info\"",
        ", {alias}.\"session_key\"",
        ", {alias}.\"user_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (5).
    pub const COLUMN_COUNT: usize = 5;

    /// No `Meta.ordering`: Django leaves the row order to Postgres.
    pub const ORDER_BY: &'static str = "";

    /// Manager `objects` — `django.contrib.sessions.base_session.BaseSessionManager.get_queryset`
    /// (`site-packages/django/db/models/manager.py:150`, the default manager).
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
    pub const OBJECTS: &'static str = "TRUE";

    /// Decode this model's row, starting at column `start`.
    ///
    /// Columns are read by index, which is why the select list and this
    /// function are generated together.
    pub fn from_row(row: &Row, start: usize) -> Result<Self, Error> {
        Ok(Self {
            session_data: row.try_get(start)?,
            expire_date: row.try_get(start + 1)?,
            device_info: row.try_get(start + 2)?,
            session_key: row.try_get(start + 3)?,
            user_id: row.try_get(start + 4)?,
        })
    }
}
