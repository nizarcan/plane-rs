// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/integration/base.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `Integration` — table `integrations`, generated from `apps/api/plane/db/models/integration/base.py`.
///
/// 17 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Integration {
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
    /// `title` — `CharField`, `max_length=400`.
    pub title: String,
    /// `provider` — `CharField`, `max_length=400`, `unique`.
    pub provider: String,
    /// `network` — `PositiveIntegerField`. Default: `1`.
    pub network: i32,
    /// `description` — `JSONField`. Default: `builtins.dict`.
    pub description: JsonText,
    /// `author` — `CharField`, `max_length=400`.
    pub author: String,
    /// `webhook_url` — `TextField`.
    pub webhook_url: String,
    /// `webhook_secret` — `TextField`.
    pub webhook_secret: String,
    /// `redirect_url` — `TextField`.
    pub redirect_url: String,
    /// `metadata` — `JSONField`. Default: `builtins.dict`.
    pub metadata: JsonText,
    /// `verified` — `BooleanField`. Default: `False`.
    pub verified: bool,
    /// `avatar_url` — `TextField`, nullable.
    pub avatar_url: Option<String>,
}

impl Integration {
    /// The table this row is read from (`integrations`).
    pub const TABLE: &'static str = "integrations";

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
        ", {alias}.\"provider\"",
        ", {alias}.\"network\"",
        ", {alias}.\"description\"",
        ", {alias}.\"author\"",
        ", {alias}.\"webhook_url\"",
        ", {alias}.\"webhook_secret\"",
        ", {alias}.\"redirect_url\"",
        ", {alias}.\"metadata\"",
        ", {alias}.\"verified\"",
        ", {alias}.\"avatar_url\"",
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
    /// Django WHERE: `("integrations"."deleted_at" IS NULL)`.
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
            provider: row.try_get(start + 7)?,
            network: row.try_get(start + 8)?,
            description: row.try_get(start + 9)?,
            author: row.try_get(start + 10)?,
            webhook_url: row.try_get(start + 11)?,
            webhook_secret: row.try_get(start + 12)?,
            redirect_url: row.try_get(start + 13)?,
            metadata: row.try_get(start + 14)?,
            verified: row.try_get(start + 15)?,
            avatar_url: row.try_get(start + 16)?,
        })
    }
}

/// `WorkspaceIntegration` — table `workspace_integrations`, generated from `apps/api/plane/db/models/integration/base.py`.
///
/// 12 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceIntegration {
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
    /// `actor` → `db.User` (`users.id`, `CASCADE`).
    pub actor_id: Uuid,
    /// `integration` → `db.Integration` (`integrations.id`, `CASCADE`).
    pub integration_id: Uuid,
    /// `api_token` → `db.APIToken` (`api_tokens.id`, `CASCADE`).
    pub api_token_id: Uuid,
    /// `metadata` — `JSONField`. Default: `builtins.dict`.
    pub metadata: JsonText,
    /// `config` — `JSONField`. Default: `builtins.dict`.
    pub config: JsonText,
}

impl WorkspaceIntegration {
    /// The table this row is read from (`workspace_integrations`).
    pub const TABLE: &'static str = "workspace_integrations";

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
        ", {alias}.\"actor_id\"",
        ", {alias}.\"integration_id\"",
        ", {alias}.\"api_token_id\"",
        ", {alias}.\"metadata\"",
        ", {alias}.\"config\"",
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
    /// Django WHERE: `("workspace_integrations"."deleted_at" IS NULL)`.
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
            actor_id: row.try_get(start + 7)?,
            integration_id: row.try_get(start + 8)?,
            api_token_id: row.try_get(start + 9)?,
            metadata: row.try_get(start + 10)?,
            config: row.try_get(start + 11)?,
        })
    }
}
