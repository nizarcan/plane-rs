// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/workspace.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `Team` — table `teams`, generated from `apps/api/plane/db/models/workspace.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Team {
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
    /// `name` — `CharField`, `max_length=255`.
    pub name: String,
    /// `description` — `TextField`.
    pub description: String,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
    /// `logo_props` — `JSONField`. Default: `builtins.dict`.
    pub logo_props: JsonText,
}

impl Team {
    /// The table this row is read from (`teams`).
    pub const TABLE: &'static str = "teams";

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
        ", {alias}.\"name\"",
        ", {alias}.\"description\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"logo_props\"",
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
    /// Django WHERE: `("teams"."deleted_at" IS NULL)`.
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
            name: row.try_get(start + 6)?,
            description: row.try_get(start + 7)?,
            workspace_id: row.try_get(start + 8)?,
            logo_props: row.try_get(start + 9)?,
        })
    }
}

/// `Workspace` — table `workspaces`, generated from `apps/api/plane/db/models/workspace.py`.
///
/// 14 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
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
    /// `name` — `CharField`, `max_length=80`.
    pub name: String,
    /// `logo` — `TextField`, nullable.
    pub logo: Option<String>,
    /// `logo_asset` → `db.FileAsset` (`file_assets.id`, `SET_NULL`), nullable.
    pub logo_asset_id: Option<Uuid>,
    /// `owner` → `db.User` (`users.id`, `CASCADE`).
    pub owner_id: Uuid,
    /// `slug` — `SlugField`, `max_length=48`, `unique`, `db_index`.
    pub slug: String,
    /// `organization_size` — `CharField`, `max_length=20`, nullable.
    pub organization_size: Option<String>,
    /// `timezone` — `CharField`, `max_length=255`. Default: `'UTC'`.
    pub timezone: String,
    /// `background_color` — `CharField`, `max_length=255`. Default: `plane.utils.color.get_random_color`.
    pub background_color: String,
}

impl Workspace {
    /// The table this row is read from (`workspaces`).
    pub const TABLE: &'static str = "workspaces";

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
        ", {alias}.\"name\"",
        ", {alias}.\"logo\"",
        ", {alias}.\"logo_asset_id\"",
        ", {alias}.\"owner_id\"",
        ", {alias}.\"slug\"",
        ", {alias}.\"organization_size\"",
        ", {alias}.\"timezone\"",
        ", {alias}.\"background_color\"",
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
    /// Django WHERE: `("workspaces"."deleted_at" IS NULL)`.
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
            name: row.try_get(start + 6)?,
            logo: row.try_get(start + 7)?,
            logo_asset_id: row.try_get(start + 8)?,
            owner_id: row.try_get(start + 9)?,
            slug: row.try_get(start + 10)?,
            organization_size: row.try_get(start + 11)?,
            timezone: row.try_get(start + 12)?,
            background_color: row.try_get(start + 13)?,
        })
    }
}

/// `WorkspaceHomePreference` — table `workspace_home_preferences`, generated from `apps/api/plane/db/models/workspace.py`.
///
/// 12 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceHomePreference {
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
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `key` — `CharField`, `max_length=255`.
    pub key: String,
    /// `is_enabled` — `BooleanField`. Default: `True`.
    pub is_enabled: bool,
    /// `config` — `JSONField`. Default: `builtins.dict`.
    pub config: JsonText,
    /// `sort_order` — `FloatField`. Default: `65535`.
    pub sort_order: f64,
}

impl WorkspaceHomePreference {
    /// The table this row is read from (`workspace_home_preferences`).
    pub const TABLE: &'static str = "workspace_home_preferences";

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
        ", {alias}.\"user_id\"",
        ", {alias}.\"key\"",
        ", {alias}.\"is_enabled\"",
        ", {alias}.\"config\"",
        ", {alias}.\"sort_order\"",
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
    /// Django WHERE: `("workspace_home_preferences"."deleted_at" IS NULL)`.
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
            user_id: row.try_get(start + 7)?,
            key: row.try_get(start + 8)?,
            is_enabled: row.try_get(start + 9)?,
            config: row.try_get(start + 10)?,
            sort_order: row.try_get(start + 11)?,
        })
    }
}

/// `WorkspaceMember` — table `workspace_members`, generated from `apps/api/plane/db/models/workspace.py`.
///
/// 17 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceMember {
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
    /// `member` → `db.User` (`users.id`, `CASCADE`).
    pub member_id: Uuid,
    /// `role` — `PositiveSmallIntegerField`. Default: `5`.
    pub role: i16,
    /// `company_role` — `TextField`, nullable.
    pub company_role: Option<String>,
    /// `view_props` — `JSONField`. Default: `plane.db.models.workspace.get_default_props`.
    pub view_props: JsonText,
    /// `default_props` — `JSONField`. Default: `plane.db.models.workspace.get_default_props`.
    pub default_props: JsonText,
    /// `issue_props` — `JSONField`. Default: `plane.db.models.workspace.get_issue_props`.
    pub issue_props: JsonText,
    /// `is_active` — `BooleanField`. Default: `True`.
    pub is_active: bool,
    /// `getting_started_checklist` — `JSONField`. Default: `builtins.dict`.
    pub getting_started_checklist: JsonText,
    /// `tips` — `JSONField`. Default: `builtins.dict`.
    pub tips: JsonText,
    /// `explored_features` — `JSONField`. Default: `builtins.dict`.
    pub explored_features: JsonText,
}

impl WorkspaceMember {
    /// The table this row is read from (`workspace_members`).
    pub const TABLE: &'static str = "workspace_members";

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
        ", {alias}.\"member_id\"",
        ", {alias}.\"role\"",
        ", {alias}.\"company_role\"",
        ", {alias}.\"view_props\"",
        ", {alias}.\"default_props\"",
        ", {alias}.\"issue_props\"",
        ", {alias}.\"is_active\"",
        ", {alias}.\"getting_started_checklist\"",
        ", {alias}.\"tips\"",
        ", {alias}.\"explored_features\"",
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
    /// Django WHERE: `("workspace_members"."deleted_at" IS NULL)`.
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
            member_id: row.try_get(start + 7)?,
            role: row.try_get(start + 8)?,
            company_role: row.try_get(start + 9)?,
            view_props: row.try_get(start + 10)?,
            default_props: row.try_get(start + 11)?,
            issue_props: row.try_get(start + 12)?,
            is_active: row.try_get(start + 13)?,
            getting_started_checklist: row.try_get(start + 14)?,
            tips: row.try_get(start + 15)?,
            explored_features: row.try_get(start + 16)?,
        })
    }
}

/// `WorkspaceMemberInvite` — table `workspace_member_invites`, generated from `apps/api/plane/db/models/workspace.py`.
///
/// 13 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceMemberInvite {
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
    /// `email` — `CharField`, `max_length=255`.
    pub email: String,
    /// `accepted` — `BooleanField`. Default: `False`.
    pub accepted: bool,
    /// `token` — `CharField`, `max_length=255`.
    pub token: String,
    /// `message` — `TextField`, nullable.
    pub message: Option<String>,
    /// `responded_at` — `DateTimeField`, nullable.
    pub responded_at: Option<DateTime<Utc>>,
    /// `role` — `PositiveSmallIntegerField`. Default: `5`.
    pub role: i16,
}

impl WorkspaceMemberInvite {
    /// The table this row is read from (`workspace_member_invites`).
    pub const TABLE: &'static str = "workspace_member_invites";

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
        ", {alias}.\"email\"",
        ", {alias}.\"accepted\"",
        ", {alias}.\"token\"",
        ", {alias}.\"message\"",
        ", {alias}.\"responded_at\"",
        ", {alias}.\"role\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (13).
    pub const COLUMN_COUNT: usize = 13;

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
    /// Django WHERE: `("workspace_member_invites"."deleted_at" IS NULL)`.
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
            email: row.try_get(start + 7)?,
            accepted: row.try_get(start + 8)?,
            token: row.try_get(start + 9)?,
            message: row.try_get(start + 10)?,
            responded_at: row.try_get(start + 11)?,
            role: row.try_get(start + 12)?,
        })
    }
}

/// `WorkspaceTheme` — table `workspace_themes`, generated from `apps/api/plane/db/models/workspace.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceTheme {
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
    /// `name` — `CharField`, `max_length=300`.
    pub name: String,
    /// `actor` → `db.User` (`users.id`, `CASCADE`).
    pub actor_id: Uuid,
    /// `colors` — `JSONField`. Default: `builtins.dict`.
    pub colors: JsonText,
}

impl WorkspaceTheme {
    /// The table this row is read from (`workspace_themes`).
    pub const TABLE: &'static str = "workspace_themes";

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
        ", {alias}.\"name\"",
        ", {alias}.\"actor_id\"",
        ", {alias}.\"colors\"",
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
    /// Django WHERE: `("workspace_themes"."deleted_at" IS NULL)`.
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
            name: row.try_get(start + 7)?,
            actor_id: row.try_get(start + 8)?,
            colors: row.try_get(start + 9)?,
        })
    }
}

/// `WorkspaceUserLink` — table `workspace_user_links`, generated from `apps/api/plane/db/models/workspace.py`.
///
/// 12 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceUserLink {
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
    /// `title` — `CharField`, `max_length=255`, nullable.
    pub title: Option<String>,
    /// `url` — `TextField`.
    pub url: String,
    /// `metadata` — `JSONField`. Default: `builtins.dict`.
    pub metadata: JsonText,
    /// `owner` → `db.User` (`users.id`, `CASCADE`).
    pub owner_id: Uuid,
}

impl WorkspaceUserLink {
    /// The table this row is read from (`workspace_user_links`).
    pub const TABLE: &'static str = "workspace_user_links";

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
        ", {alias}.\"title\"",
        ", {alias}.\"url\"",
        ", {alias}.\"metadata\"",
        ", {alias}.\"owner_id\"",
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
    /// Django WHERE: `("workspace_user_links"."deleted_at" IS NULL)`.
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
            title: row.try_get(start + 8)?,
            url: row.try_get(start + 9)?,
            metadata: row.try_get(start + 10)?,
            owner_id: row.try_get(start + 11)?,
        })
    }
}

/// `WorkspaceUserPreference` — table `workspace_user_preferences`, generated from `apps/api/plane/db/models/workspace.py`.
///
/// 11 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceUserPreference {
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
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `key` — `CharField`, `max_length=255`.
    pub key: String,
    /// `is_pinned` — `BooleanField`. Default: `False`.
    pub is_pinned: bool,
    /// `sort_order` — `FloatField`. Default: `65535`.
    pub sort_order: f64,
}

impl WorkspaceUserPreference {
    /// The table this row is read from (`workspace_user_preferences`).
    pub const TABLE: &'static str = "workspace_user_preferences";

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
        ", {alias}.\"user_id\"",
        ", {alias}.\"key\"",
        ", {alias}.\"is_pinned\"",
        ", {alias}.\"sort_order\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (11).
    pub const COLUMN_COUNT: usize = 11;

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
    /// Django WHERE: `("workspace_user_preferences"."deleted_at" IS NULL)`.
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
            user_id: row.try_get(start + 7)?,
            key: row.try_get(start + 8)?,
            is_pinned: row.try_get(start + 9)?,
            sort_order: row.try_get(start + 10)?,
        })
    }
}

/// `WorkspaceUserProperties` — table `workspace_user_properties`, generated from `apps/api/plane/db/models/workspace.py`.
///
/// 14 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceUserProperties {
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
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `filters` — `JSONField`. Default: `plane.db.models.workspace.get_default_filters`.
    pub filters: JsonText,
    /// `display_filters` — `JSONField`. Default: `plane.db.models.workspace.get_default_display_filters`.
    pub display_filters: JsonText,
    /// `display_properties` — `JSONField`. Default: `plane.db.models.workspace.get_default_display_properties`.
    pub display_properties: JsonText,
    /// `rich_filters` — `JSONField`. Default: `builtins.dict`.
    pub rich_filters: JsonText,
    /// `navigation_project_limit` — `IntegerField`. Default: `10`.
    pub navigation_project_limit: i32,
    /// `navigation_control_preference` — `CharField`, `max_length=25`. Default: `WorkspaceUserProperties.NavigationControlPreference.ACCORDION`.
    pub navigation_control_preference: String,
}

impl WorkspaceUserProperties {
    /// The table this row is read from (`workspace_user_properties`).
    pub const TABLE: &'static str = "workspace_user_properties";

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
        ", {alias}.\"user_id\"",
        ", {alias}.\"filters\"",
        ", {alias}.\"display_filters\"",
        ", {alias}.\"display_properties\"",
        ", {alias}.\"rich_filters\"",
        ", {alias}.\"navigation_project_limit\"",
        ", {alias}.\"navigation_control_preference\"",
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
    /// Django WHERE: `("workspace_user_properties"."deleted_at" IS NULL)`.
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
            user_id: row.try_get(start + 7)?,
            filters: row.try_get(start + 8)?,
            display_filters: row.try_get(start + 9)?,
            display_properties: row.try_get(start + 10)?,
            rich_filters: row.try_get(start + 11)?,
            navigation_project_limit: row.try_get(start + 12)?,
            navigation_control_preference: row.try_get(start + 13)?,
        })
    }
}
