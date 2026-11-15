// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/intake.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `Intake` — table `intakes`, generated from `apps/api/plane/db/models/intake.py`.
///
/// 13 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Intake {
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
    /// `is_default` — `BooleanField`. Default: `False`.
    pub is_default: bool,
    /// `view_props` — `JSONField`. Default: `builtins.dict`.
    pub view_props: JsonText,
    /// `logo_props` — `JSONField`. Default: `builtins.dict`.
    pub logo_props: JsonText,
}

impl Intake {
    /// The table this row is read from (`intakes`).
    pub const TABLE: &'static str = "intakes";

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
        ", {alias}.\"is_default\"",
        ", {alias}.\"view_props\"",
        ", {alias}.\"logo_props\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (13).
    pub const COLUMN_COUNT: usize = 13;

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
    /// Django WHERE: `("intakes"."deleted_at" IS NULL)`.
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
            is_default: row.try_get(start + 10)?,
            view_props: row.try_get(start + 11)?,
            logo_props: row.try_get(start + 12)?,
        })
    }
}

/// `IntakeIssue` — table `intake_issues`, generated from `apps/api/plane/db/models/intake.py`.
///
/// 18 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IntakeIssue {
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
    /// `intake` → `db.Intake` (`intakes.id`, `CASCADE`).
    pub intake_id: Uuid,
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `status` — `IntegerField`. Default: `-2`.
    pub status: i32,
    /// `snoozed_till` — `DateTimeField`, nullable.
    pub snoozed_till: Option<DateTime<Utc>>,
    /// `duplicate_to` → `db.Issue` (`issues.id`, `SET_NULL`), nullable.
    pub duplicate_to_id: Option<Uuid>,
    /// `source` — `CharField`, `max_length=255`, nullable. Default: `'IN_APP'`.
    pub source: Option<String>,
    /// `source_email` — `TextField`, nullable.
    pub source_email: Option<String>,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
    /// `extra` — `JSONField`. Default: `builtins.dict`.
    pub extra: JsonText,
}

impl IntakeIssue {
    /// The table this row is read from (`intake_issues`).
    pub const TABLE: &'static str = "intake_issues";

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
        ", {alias}.\"intake_id\"",
        ", {alias}.\"issue_id\"",
        ", {alias}.\"status\"",
        ", {alias}.\"snoozed_till\"",
        ", {alias}.\"duplicate_to_id\"",
        ", {alias}.\"source\"",
        ", {alias}.\"source_email\"",
        ", {alias}.\"external_source\"",
        ", {alias}.\"external_id\"",
        ", {alias}.\"extra\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (18).
    pub const COLUMN_COUNT: usize = 18;

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
    /// Django WHERE: `("intake_issues"."deleted_at" IS NULL)`.
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
            intake_id: row.try_get(start + 8)?,
            issue_id: row.try_get(start + 9)?,
            status: row.try_get(start + 10)?,
            snoozed_till: row.try_get(start + 11)?,
            duplicate_to_id: row.try_get(start + 12)?,
            source: row.try_get(start + 13)?,
            source_email: row.try_get(start + 14)?,
            external_source: row.try_get(start + 15)?,
            external_id: row.try_get(start + 16)?,
            extra: row.try_get(start + 17)?,
        })
    }
}
