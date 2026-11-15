// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/notification.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `EmailNotificationLog` — table `email_notification_logs`, generated from `apps/api/plane/db/models/notification.py`.
///
/// 16 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct EmailNotificationLog {
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
    /// `receiver` → `db.User` (`users.id`, `CASCADE`).
    pub receiver_id: Uuid,
    /// `triggered_by` → `db.User` (`users.id`, `CASCADE`).
    pub triggered_by_id: Uuid,
    /// `entity_identifier` — `UUIDField`, `max_length=32`, nullable.
    pub entity_identifier: Option<Uuid>,
    /// `entity_name` — `CharField`, `max_length=255`.
    pub entity_name: String,
    /// `data` — `JSONField`, nullable.
    pub data: Option<JsonText>,
    /// `processed_at` — `DateTimeField`, nullable.
    pub processed_at: Option<DateTime<Utc>>,
    /// `sent_at` — `DateTimeField`, nullable.
    pub sent_at: Option<DateTime<Utc>>,
    /// `entity` — `CharField`, `max_length=200`.
    pub entity: String,
    /// `old_value` — `CharField`, `max_length=300`, nullable.
    pub old_value: Option<String>,
    /// `new_value` — `CharField`, `max_length=300`, nullable.
    pub new_value: Option<String>,
}

impl EmailNotificationLog {
    /// The table this row is read from (`email_notification_logs`).
    pub const TABLE: &'static str = "email_notification_logs";

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
        ", {alias}.\"receiver_id\"",
        ", {alias}.\"triggered_by_id\"",
        ", {alias}.\"entity_identifier\"",
        ", {alias}.\"entity_name\"",
        ", {alias}.\"data\"",
        ", {alias}.\"processed_at\"",
        ", {alias}.\"sent_at\"",
        ", {alias}.\"entity\"",
        ", {alias}.\"old_value\"",
        ", {alias}.\"new_value\"",
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
    /// Django WHERE: `("email_notification_logs"."deleted_at" IS NULL)`.
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
            receiver_id: row.try_get(start + 6)?,
            triggered_by_id: row.try_get(start + 7)?,
            entity_identifier: row.try_get(start + 8)?,
            entity_name: row.try_get(start + 9)?,
            data: row.try_get(start + 10)?,
            processed_at: row.try_get(start + 11)?,
            sent_at: row.try_get(start + 12)?,
            entity: row.try_get(start + 13)?,
            old_value: row.try_get(start + 14)?,
            new_value: row.try_get(start + 15)?,
        })
    }
}

/// `Notification` — table `notifications`, generated from `apps/api/plane/db/models/notification.py`.
///
/// 21 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
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
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
    /// `project` → `db.Project` (`projects.id`, `CASCADE`), nullable.
    pub project_id: Option<Uuid>,
    /// `data` — `JSONField`, nullable.
    pub data: Option<JsonText>,
    /// `entity_identifier` — `UUIDField`, `max_length=32`, nullable.
    pub entity_identifier: Option<Uuid>,
    /// `entity_name` — `CharField`, `max_length=255`.
    pub entity_name: String,
    /// `title` — `TextField`.
    pub title: String,
    /// `message` — `JSONField`, nullable.
    pub message: Option<JsonText>,
    /// `message_html` — `TextField`. Default: `'<p></p>'`.
    pub message_html: String,
    /// `message_stripped` — `TextField`, nullable.
    pub message_stripped: Option<String>,
    /// `sender` — `CharField`, `max_length=255`.
    pub sender: String,
    /// `triggered_by` → `db.User` (`users.id`, `SET_NULL`), nullable.
    pub triggered_by_id: Option<Uuid>,
    /// `receiver` → `db.User` (`users.id`, `CASCADE`).
    pub receiver_id: Uuid,
    /// `read_at` — `DateTimeField`, nullable.
    pub read_at: Option<DateTime<Utc>>,
    /// `snoozed_till` — `DateTimeField`, nullable.
    pub snoozed_till: Option<DateTime<Utc>>,
    /// `archived_at` — `DateTimeField`, nullable.
    pub archived_at: Option<DateTime<Utc>>,
}

impl Notification {
    /// The table this row is read from (`notifications`).
    pub const TABLE: &'static str = "notifications";

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
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"project_id\"",
        ", {alias}.\"data\"",
        ", {alias}.\"entity_identifier\"",
        ", {alias}.\"entity_name\"",
        ", {alias}.\"title\"",
        ", {alias}.\"message\"",
        ", {alias}.\"message_html\"",
        ", {alias}.\"message_stripped\"",
        ", {alias}.\"sender\"",
        ", {alias}.\"triggered_by_id\"",
        ", {alias}.\"receiver_id\"",
        ", {alias}.\"read_at\"",
        ", {alias}.\"snoozed_till\"",
        ", {alias}.\"archived_at\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (21).
    pub const COLUMN_COUNT: usize = 21;

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
    /// Django WHERE: `("notifications"."deleted_at" IS NULL)`.
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
            workspace_id: row.try_get(start + 6)?,
            project_id: row.try_get(start + 7)?,
            data: row.try_get(start + 8)?,
            entity_identifier: row.try_get(start + 9)?,
            entity_name: row.try_get(start + 10)?,
            title: row.try_get(start + 11)?,
            message: row.try_get(start + 12)?,
            message_html: row.try_get(start + 13)?,
            message_stripped: row.try_get(start + 14)?,
            sender: row.try_get(start + 15)?,
            triggered_by_id: row.try_get(start + 16)?,
            receiver_id: row.try_get(start + 17)?,
            read_at: row.try_get(start + 18)?,
            snoozed_till: row.try_get(start + 19)?,
            archived_at: row.try_get(start + 20)?,
        })
    }
}

/// `UserNotificationPreference` — table `user_notification_preferences`, generated from `apps/api/plane/db/models/notification.py`.
///
/// 14 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct UserNotificationPreference {
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
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`), nullable.
    pub workspace_id: Option<Uuid>,
    /// `project` → `db.Project` (`projects.id`, `CASCADE`), nullable.
    pub project_id: Option<Uuid>,
    /// `property_change` — `BooleanField`. Default: `True`.
    pub property_change: bool,
    /// `state_change` — `BooleanField`. Default: `True`.
    pub state_change: bool,
    /// `comment` — `BooleanField`. Default: `True`.
    pub comment: bool,
    /// `mention` — `BooleanField`. Default: `True`.
    pub mention: bool,
    /// `issue_completed` — `BooleanField`. Default: `True`.
    pub issue_completed: bool,
}

impl UserNotificationPreference {
    /// The table this row is read from (`user_notification_preferences`).
    pub const TABLE: &'static str = "user_notification_preferences";

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
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"project_id\"",
        ", {alias}.\"property_change\"",
        ", {alias}.\"state_change\"",
        ", {alias}.\"comment\"",
        ", {alias}.\"mention\"",
        ", {alias}.\"issue_completed\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (14).
    pub const COLUMN_COUNT: usize = 14;

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
    /// Django WHERE: `("user_notification_preferences"."deleted_at" IS NULL)`.
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
            workspace_id: row.try_get(start + 7)?,
            project_id: row.try_get(start + 8)?,
            property_change: row.try_get(start + 9)?,
            state_change: row.try_get(start + 10)?,
            comment: row.try_get(start + 11)?,
            mention: row.try_get(start + 12)?,
            issue_completed: row.try_get(start + 13)?,
        })
    }
}
