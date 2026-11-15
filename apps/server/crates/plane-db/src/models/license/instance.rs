// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/license/models/instance.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `ChangeLog` — table `changelogs`, generated from `apps/api/plane/license/models/instance.py`.
///
/// 12 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeLog {
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
    /// `title` — `CharField`, `max_length=255`.
    pub title: String,
    /// `description` — `TextField`.
    pub description: String,
    /// `version` — `CharField`, `max_length=255`.
    pub version: String,
    /// `tags` — `JSONField`. Default: `builtins.list`.
    pub tags: JsonText,
    /// `release_date` — `DateTimeField`, nullable.
    pub release_date: Option<DateTime<Utc>>,
    /// `is_release_candidate` — `BooleanField`. Default: `False`.
    pub is_release_candidate: bool,
}

impl ChangeLog {
    /// The table this row is read from (`changelogs`).
    pub const TABLE: &'static str = "changelogs";

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
        ", {alias}.\"title\"",
        ", {alias}.\"description\"",
        ", {alias}.\"version\"",
        ", {alias}.\"tags\"",
        ", {alias}.\"release_date\"",
        ", {alias}.\"is_release_candidate\"",
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
    /// Django WHERE: `("changelogs"."deleted_at" IS NULL)`.
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
            title: row.try_get(start + 6)?,
            description: row.try_get(start + 7)?,
            version: row.try_get(start + 8)?,
            tags: row.try_get(start + 9)?,
            release_date: row.try_get(start + 10)?,
            is_release_candidate: row.try_get(start + 11)?,
        })
    }
}

/// `Instance` — table `instances`, generated from `apps/api/plane/license/models/instance.py`.
///
/// 22 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
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
    /// `instance_name` — `CharField`, `max_length=255`.
    pub instance_name: String,
    /// `whitelist_emails` — `TextField`, nullable.
    pub whitelist_emails: Option<String>,
    /// `instance_id` — `CharField`, `max_length=255`, `unique`.
    pub instance_id: String,
    /// `current_version` — `CharField`, `max_length=255`.
    pub current_version: String,
    /// `latest_version` — `CharField`, `max_length=255`, nullable.
    pub latest_version: Option<String>,
    /// `edition` — `CharField`, `max_length=255`. Default: `'PLANE_COMMUNITY'`.
    pub edition: String,
    /// `domain` — `TextField`.
    pub domain: String,
    /// `last_checked_at` — `DateTimeField`.
    pub last_checked_at: DateTime<Utc>,
    /// `namespace` — `CharField`, `max_length=255`, nullable.
    pub namespace: Option<String>,
    /// `is_telemetry_enabled` — `BooleanField`. Default: `True`.
    pub is_telemetry_enabled: bool,
    /// `is_support_required` — `BooleanField`. Default: `True`.
    pub is_support_required: bool,
    /// `is_setup_done` — `BooleanField`. Default: `False`.
    pub is_setup_done: bool,
    /// `is_signup_screen_visited` — `BooleanField`. Default: `False`.
    pub is_signup_screen_visited: bool,
    /// `is_verified` — `BooleanField`. Default: `False`.
    pub is_verified: bool,
    /// `is_test` — `BooleanField`. Default: `False`.
    pub is_test: bool,
    /// `is_current_version_deprecated` — `BooleanField`. Default: `False`.
    pub is_current_version_deprecated: bool,
}

impl Instance {
    /// The table this row is read from (`instances`).
    pub const TABLE: &'static str = "instances";

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
        ", {alias}.\"instance_name\"",
        ", {alias}.\"whitelist_emails\"",
        ", {alias}.\"instance_id\"",
        ", {alias}.\"current_version\"",
        ", {alias}.\"latest_version\"",
        ", {alias}.\"edition\"",
        ", {alias}.\"domain\"",
        ", {alias}.\"last_checked_at\"",
        ", {alias}.\"namespace\"",
        ", {alias}.\"is_telemetry_enabled\"",
        ", {alias}.\"is_support_required\"",
        ", {alias}.\"is_setup_done\"",
        ", {alias}.\"is_signup_screen_visited\"",
        ", {alias}.\"is_verified\"",
        ", {alias}.\"is_test\"",
        ", {alias}.\"is_current_version_deprecated\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (22).
    pub const COLUMN_COUNT: usize = 22;

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
    /// Django WHERE: `("instances"."deleted_at" IS NULL)`.
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
            instance_name: row.try_get(start + 6)?,
            whitelist_emails: row.try_get(start + 7)?,
            instance_id: row.try_get(start + 8)?,
            current_version: row.try_get(start + 9)?,
            latest_version: row.try_get(start + 10)?,
            edition: row.try_get(start + 11)?,
            domain: row.try_get(start + 12)?,
            last_checked_at: row.try_get(start + 13)?,
            namespace: row.try_get(start + 14)?,
            is_telemetry_enabled: row.try_get(start + 15)?,
            is_support_required: row.try_get(start + 16)?,
            is_setup_done: row.try_get(start + 17)?,
            is_signup_screen_visited: row.try_get(start + 18)?,
            is_verified: row.try_get(start + 19)?,
            is_test: row.try_get(start + 20)?,
            is_current_version_deprecated: row.try_get(start + 21)?,
        })
    }
}

/// `InstanceAdmin` — table `instance_admins`, generated from `apps/api/plane/license/models/instance.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceAdmin {
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
    /// `user` → `db.User` (`users.id`, `SET_NULL`), nullable.
    pub user_id: Option<Uuid>,
    /// `instance` → `license.Instance` (`instances.id`, `CASCADE`).
    pub instance_id: Uuid,
    /// `role` — `PositiveIntegerField`. Default: `20`.
    pub role: i32,
    /// `is_verified` — `BooleanField`. Default: `False`.
    pub is_verified: bool,
}

impl InstanceAdmin {
    /// The table this row is read from (`instance_admins`).
    pub const TABLE: &'static str = "instance_admins";

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
        ", {alias}.\"instance_id\"",
        ", {alias}.\"role\"",
        ", {alias}.\"is_verified\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (10).
    pub const COLUMN_COUNT: usize = 10;

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
    /// Django WHERE: `("instance_admins"."deleted_at" IS NULL)`.
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
            instance_id: row.try_get(start + 7)?,
            role: row.try_get(start + 8)?,
            is_verified: row.try_get(start + 9)?,
        })
    }
}

/// `InstanceConfiguration` — table `instance_configurations`, generated from `apps/api/plane/license/models/instance.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceConfiguration {
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
    /// `key` — `CharField`, `max_length=100`, `unique`.
    pub key: String,
    /// `value` — `TextField`, nullable. Default: `None`.
    pub value: Option<String>,
    /// `category` — `TextField`.
    pub category: String,
    /// `is_encrypted` — `BooleanField`. Default: `False`.
    pub is_encrypted: bool,
}

impl InstanceConfiguration {
    /// The table this row is read from (`instance_configurations`).
    pub const TABLE: &'static str = "instance_configurations";

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
        ", {alias}.\"key\"",
        ", {alias}.\"value\"",
        ", {alias}.\"category\"",
        ", {alias}.\"is_encrypted\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (10).
    pub const COLUMN_COUNT: usize = 10;

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
    /// Django WHERE: `("instance_configurations"."deleted_at" IS NULL)`.
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
            key: row.try_get(start + 6)?,
            value: row.try_get(start + 7)?,
            category: row.try_get(start + 8)?,
            is_encrypted: row.try_get(start + 9)?,
        })
    }
}
