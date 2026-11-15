// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/webhook.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::Error;

/// `ProjectWebhook` — table `project_webhooks`, generated from `apps/api/plane/db/models/webhook.py`.
///
/// 9 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectWebhook {
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
    /// `webhook` → `db.Webhook` (`webhooks.id`, `CASCADE`).
    pub webhook_id: Uuid,
}

impl ProjectWebhook {
    /// The table this row is read from (`project_webhooks`).
    pub const TABLE: &'static str = "project_webhooks";

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
        ", {alias}.\"webhook_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (9).
    pub const COLUMN_COUNT: usize = 9;

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
    /// Django WHERE: `("project_webhooks"."deleted_at" IS NULL)`.
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
            webhook_id: row.try_get(start + 8)?,
        })
    }
}

/// `Webhook` — table `webhooks`, generated from `apps/api/plane/db/models/webhook.py`.
///
/// 17 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Webhook {
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
    /// `url` — `URLField`, `max_length=1024`.
    pub url: String,
    /// `is_active` — `BooleanField`. Default: `True`.
    pub is_active: bool,
    /// `secret_key` — `CharField`, `max_length=255`. Default: `plane.db.models.webhook.generate_token`.
    pub secret_key: String,
    /// `project` — `BooleanField`. Default: `False`.
    pub project: bool,
    /// `issue` — `BooleanField`. Default: `False`.
    pub issue: bool,
    /// `module` — `BooleanField`. Default: `False`.
    pub module: bool,
    /// `cycle` — `BooleanField`. Default: `False`.
    pub cycle: bool,
    /// `issue_comment` — `BooleanField`. Default: `False`.
    pub issue_comment: bool,
    /// `is_internal` — `BooleanField`. Default: `False`.
    pub is_internal: bool,
    /// `version` — `CharField`, `max_length=50`. Default: `'v1'`.
    pub version: String,
}

impl Webhook {
    /// The table this row is read from (`webhooks`).
    pub const TABLE: &'static str = "webhooks";

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
        ", {alias}.\"url\"",
        ", {alias}.\"is_active\"",
        ", {alias}.\"secret_key\"",
        ", {alias}.\"project\"",
        ", {alias}.\"issue\"",
        ", {alias}.\"module\"",
        ", {alias}.\"cycle\"",
        ", {alias}.\"issue_comment\"",
        ", {alias}.\"is_internal\"",
        ", {alias}.\"version\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (17).
    pub const COLUMN_COUNT: usize = 17;

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
    /// Django WHERE: `("webhooks"."deleted_at" IS NULL)`.
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
            url: row.try_get(start + 7)?,
            is_active: row.try_get(start + 8)?,
            secret_key: row.try_get(start + 9)?,
            project: row.try_get(start + 10)?,
            issue: row.try_get(start + 11)?,
            module: row.try_get(start + 12)?,
            cycle: row.try_get(start + 13)?,
            issue_comment: row.try_get(start + 14)?,
            is_internal: row.try_get(start + 15)?,
            version: row.try_get(start + 16)?,
        })
    }
}

/// `WebhookLog` — table `webhook_logs`, generated from `apps/api/plane/db/models/webhook.py`.
///
/// 16 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct WebhookLog {
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
    /// `webhook` — `UUIDField`, `max_length=32`.
    pub webhook: Uuid,
    /// `event_type` — `CharField`, `max_length=255`, nullable.
    pub event_type: Option<String>,
    /// `request_method` — `CharField`, `max_length=10`, nullable.
    pub request_method: Option<String>,
    /// `request_headers` — `TextField`, nullable.
    pub request_headers: Option<String>,
    /// `request_body` — `TextField`, nullable.
    pub request_body: Option<String>,
    /// `response_status` — `TextField`, nullable.
    pub response_status: Option<String>,
    /// `response_headers` — `TextField`, nullable.
    pub response_headers: Option<String>,
    /// `response_body` — `TextField`, nullable.
    pub response_body: Option<String>,
    /// `retry_count` — `PositiveSmallIntegerField`. Default: `0`.
    pub retry_count: i16,
}

impl WebhookLog {
    /// The table this row is read from (`webhook_logs`).
    pub const TABLE: &'static str = "webhook_logs";

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
        ", {alias}.\"webhook\"",
        ", {alias}.\"event_type\"",
        ", {alias}.\"request_method\"",
        ", {alias}.\"request_headers\"",
        ", {alias}.\"request_body\"",
        ", {alias}.\"response_status\"",
        ", {alias}.\"response_headers\"",
        ", {alias}.\"response_body\"",
        ", {alias}.\"retry_count\"",
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
    /// Django WHERE: `("webhook_logs"."deleted_at" IS NULL)`.
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
            webhook: row.try_get(start + 7)?,
            event_type: row.try_get(start + 8)?,
            request_method: row.try_get(start + 9)?,
            request_headers: row.try_get(start + 10)?,
            request_body: row.try_get(start + 11)?,
            response_status: row.try_get(start + 12)?,
            response_headers: row.try_get(start + 13)?,
            response_body: row.try_get(start + 14)?,
            retry_count: row.try_get(start + 15)?,
        })
    }
}
