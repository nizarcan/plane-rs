// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/state.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::Error;

/// `State` — table `states`, generated from `apps/api/plane/db/models/state.py`.
///
/// 18 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
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
    /// `color` — `CharField`, `max_length=255`.
    pub color: String,
    /// `slug` — `SlugField`, `max_length=100`, `db_index`.
    pub slug: String,
    /// `sequence` — `FloatField`. Default: `65535`.
    pub sequence: f64,
    /// `group` — `CharField`, `max_length=20`. Default: `StateGroup.BACKLOG`.
    pub group: String,
    /// `is_triage` — `BooleanField`. Default: `False`.
    pub is_triage: bool,
    /// `default` — `BooleanField`. Default: `False`.
    pub default: bool,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
}

impl State {
    /// The table this row is read from (`states`).
    pub const TABLE: &'static str = "states";

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
        ", {alias}.\"color\"",
        ", {alias}.\"slug\"",
        ", {alias}.\"sequence\"",
        ", {alias}.\"group\"",
        ", {alias}.\"is_triage\"",
        ", {alias}.\"default\"",
        ", {alias}.\"external_source\"",
        ", {alias}.\"external_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (18).
    pub const COLUMN_COUNT: usize = 18;

    /// Default ordering (`Meta.ordering = ['sequence']`).
    ///
    /// Django's SQL: `"sequence" ASC`
    /// with the alias placeholder inserted.
    pub const ORDER_BY: &'static str = "{alias}.\"sequence\" ASC";

    /// Manager `objects` — `plane.db.models.state.StateManager.get_queryset`
    /// (`apps/api/plane/db/models/state.py:68`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return super().get_queryset().exclude(group=StateGroup.TRIAGE.value)
    /// ```
    ///
    /// Django WHERE: `("states"."deleted_at" IS NULL) AND (NOT (("states"."group" = 'triage')))`.
    pub const OBJECTS: &'static str =
        "({alias}.\"deleted_at\" IS NULL) AND (NOT (({alias}.\"group\" = 'triage')))";

    /// Manager `all_state_objects` — `django.db.models.manager.Manager.get_queryset`
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
    pub const ALL_STATE_OBJECTS: &'static str = "TRUE";

    /// Manager `triage_objects` — `plane.db.models.state.TriageStateManager.get_queryset`
    /// (`apps/api/plane/db/models/state.py:75`).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return super().get_queryset().filter(group=StateGroup.TRIAGE.value)
    /// ```
    ///
    /// Django WHERE: `("states"."deleted_at" IS NULL) AND ("states"."group" = 'triage')`.
    pub const TRIAGE_OBJECTS: &'static str =
        "({alias}.\"deleted_at\" IS NULL) AND ({alias}.\"group\" = 'triage')";

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
            color: row.try_get(start + 10)?,
            slug: row.try_get(start + 11)?,
            sequence: row.try_get(start + 12)?,
            group: row.try_get(start + 13)?,
            is_triage: row.try_get(start + 14)?,
            default: row.try_get(start + 15)?,
            external_source: row.try_get(start + 16)?,
            external_id: row.try_get(start + 17)?,
        })
    }
}
