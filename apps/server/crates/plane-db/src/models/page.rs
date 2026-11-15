// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/page.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, NaiveDate, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `Page` — table `pages`, generated from `apps/api/plane/db/models/page.py`.
///
/// 26 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
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
    /// `name` — `TextField`.
    pub name: String,
    /// `description_json` — `JSONField`. Default: `builtins.dict`.
    pub description_json: JsonText,
    /// `description_binary` — `BinaryField`, nullable.
    pub description_binary: Option<Vec<u8>>,
    /// `description_html` — `TextField`. Default: `'<p></p>'`.
    pub description_html: String,
    /// `description_stripped` — `TextField`, nullable.
    pub description_stripped: Option<String>,
    /// `owned_by` → `db.User` (`users.id`, `CASCADE`).
    pub owned_by_id: Uuid,
    /// `access` — `PositiveSmallIntegerField`. Default: `0`.
    pub access: i16,
    /// `color` — `CharField`, `max_length=255`.
    pub color: String,
    /// `parent` → `db.Page` (`pages.id`, `CASCADE`), nullable.
    pub parent_id: Option<Uuid>,
    /// `archived_at` — `DateField`, nullable.
    pub archived_at: Option<NaiveDate>,
    /// `is_locked` — `BooleanField`. Default: `False`.
    pub is_locked: bool,
    /// `view_props` — `JSONField`. Default: `plane.db.models.page.get_view_props`.
    pub view_props: JsonText,
    /// `logo_props` — `JSONField`. Default: `builtins.dict`.
    pub logo_props: JsonText,
    /// `is_global` — `BooleanField`. Default: `False`.
    pub is_global: bool,
    /// `moved_to_page` — `UUIDField`, `max_length=32`, nullable.
    pub moved_to_page: Option<Uuid>,
    /// `moved_to_project` — `UUIDField`, `max_length=32`, nullable.
    pub moved_to_project: Option<Uuid>,
    /// `sort_order` — `FloatField`. Default: `65535`.
    pub sort_order: f64,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
}

impl Page {
    /// The table this row is read from (`pages`).
    pub const TABLE: &'static str = "pages";

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
        ", {alias}.\"description_json\"",
        ", {alias}.\"description_binary\"",
        ", {alias}.\"description_html\"",
        ", {alias}.\"description_stripped\"",
        ", {alias}.\"owned_by_id\"",
        ", {alias}.\"access\"",
        ", {alias}.\"color\"",
        ", {alias}.\"parent_id\"",
        ", {alias}.\"archived_at\"",
        ", {alias}.\"is_locked\"",
        ", {alias}.\"view_props\"",
        ", {alias}.\"logo_props\"",
        ", {alias}.\"is_global\"",
        ", {alias}.\"moved_to_page\"",
        ", {alias}.\"moved_to_project\"",
        ", {alias}.\"sort_order\"",
        ", {alias}.\"external_id\"",
        ", {alias}.\"external_source\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (26).
    pub const COLUMN_COUNT: usize = 26;

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
    /// Django WHERE: `("pages"."deleted_at" IS NULL)`.
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
            description_json: row.try_get(start + 8)?,
            description_binary: row.try_get(start + 9)?,
            description_html: row.try_get(start + 10)?,
            description_stripped: row.try_get(start + 11)?,
            owned_by_id: row.try_get(start + 12)?,
            access: row.try_get(start + 13)?,
            color: row.try_get(start + 14)?,
            parent_id: row.try_get(start + 15)?,
            archived_at: row.try_get(start + 16)?,
            is_locked: row.try_get(start + 17)?,
            view_props: row.try_get(start + 18)?,
            logo_props: row.try_get(start + 19)?,
            is_global: row.try_get(start + 20)?,
            moved_to_page: row.try_get(start + 21)?,
            moved_to_project: row.try_get(start + 22)?,
            sort_order: row.try_get(start + 23)?,
            external_id: row.try_get(start + 24)?,
            external_source: row.try_get(start + 25)?,
        })
    }
}

/// `PageLabel` — table `page_labels`, generated from `apps/api/plane/db/models/page.py`.
///
/// 9 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLabel {
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
    /// `label` → `db.Label` (`labels.id`, `CASCADE`).
    pub label_id: Uuid,
    /// `page` → `db.Page` (`pages.id`, `CASCADE`).
    pub page_id: Uuid,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
}

impl PageLabel {
    /// The table this row is read from (`page_labels`).
    pub const TABLE: &'static str = "page_labels";

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
        ", {alias}.\"label_id\"",
        ", {alias}.\"page_id\"",
        ", {alias}.\"workspace_id\"",
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
    /// Django WHERE: `("page_labels"."deleted_at" IS NULL)`.
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
            label_id: row.try_get(start + 6)?,
            page_id: row.try_get(start + 7)?,
            workspace_id: row.try_get(start + 8)?,
        })
    }
}

/// `PageLog` — table `page_logs`, generated from `apps/api/plane/db/models/page.py`.
///
/// 12 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLog {
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
    /// `transaction` — `UUIDField`, `max_length=32`. Default: `uuid.uuid4`.
    pub transaction: Uuid,
    /// `page` → `db.Page` (`pages.id`, `CASCADE`).
    pub page_id: Uuid,
    /// `entity_identifier` — `UUIDField`, `max_length=32`, nullable.
    pub entity_identifier: Option<Uuid>,
    /// `entity_name` — `CharField`, `max_length=30`.
    pub entity_name: String,
    /// `entity_type` — `CharField`, `max_length=30`, nullable.
    pub entity_type: Option<String>,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
}

impl PageLog {
    /// The table this row is read from (`page_logs`).
    pub const TABLE: &'static str = "page_logs";

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
        ", {alias}.\"transaction\"",
        ", {alias}.\"page_id\"",
        ", {alias}.\"entity_identifier\"",
        ", {alias}.\"entity_name\"",
        ", {alias}.\"entity_type\"",
        ", {alias}.\"workspace_id\"",
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
    /// Django WHERE: `("page_logs"."deleted_at" IS NULL)`.
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
            transaction: row.try_get(start + 6)?,
            page_id: row.try_get(start + 7)?,
            entity_identifier: row.try_get(start + 8)?,
            entity_name: row.try_get(start + 9)?,
            entity_type: row.try_get(start + 10)?,
            workspace_id: row.try_get(start + 11)?,
        })
    }
}

/// `PageVersion` — table `page_versions`, generated from `apps/api/plane/db/models/page.py`.
///
/// 15 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct PageVersion {
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
    /// `page` → `db.Page` (`pages.id`, `CASCADE`).
    pub page_id: Uuid,
    /// `last_saved_at` — `DateTimeField`. Default: `django.utils.timezone.now`.
    pub last_saved_at: DateTime<Utc>,
    /// `owned_by` → `db.User` (`users.id`, `CASCADE`).
    pub owned_by_id: Uuid,
    /// `description_binary` — `BinaryField`, nullable.
    pub description_binary: Option<Vec<u8>>,
    /// `description_html` — `TextField`. Default: `'<p></p>'`.
    pub description_html: String,
    /// `description_stripped` — `TextField`, nullable.
    pub description_stripped: Option<String>,
    /// `description_json` — `JSONField`. Default: `builtins.dict`.
    pub description_json: JsonText,
    /// `sub_pages_data` — `JSONField`. Default: `builtins.dict`.
    pub sub_pages_data: JsonText,
}

impl PageVersion {
    /// The table this row is read from (`page_versions`).
    pub const TABLE: &'static str = "page_versions";

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
        ", {alias}.\"page_id\"",
        ", {alias}.\"last_saved_at\"",
        ", {alias}.\"owned_by_id\"",
        ", {alias}.\"description_binary\"",
        ", {alias}.\"description_html\"",
        ", {alias}.\"description_stripped\"",
        ", {alias}.\"description_json\"",
        ", {alias}.\"sub_pages_data\"",
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
    /// Django WHERE: `("page_versions"."deleted_at" IS NULL)`.
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
            page_id: row.try_get(start + 7)?,
            last_saved_at: row.try_get(start + 8)?,
            owned_by_id: row.try_get(start + 9)?,
            description_binary: row.try_get(start + 10)?,
            description_html: row.try_get(start + 11)?,
            description_stripped: row.try_get(start + 12)?,
            description_json: row.try_get(start + 13)?,
            sub_pages_data: row.try_get(start + 14)?,
        })
    }
}

/// `ProjectPage` — table `project_pages`, generated from `apps/api/plane/db/models/page.py`.
///
/// 9 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPage {
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
    /// `page` → `db.Page` (`pages.id`, `CASCADE`).
    pub page_id: Uuid,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
}

impl ProjectPage {
    /// The table this row is read from (`project_pages`).
    pub const TABLE: &'static str = "project_pages";

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
        ", {alias}.\"page_id\"",
        ", {alias}.\"workspace_id\"",
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
    /// Django WHERE: `("project_pages"."deleted_at" IS NULL)`.
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
            page_id: row.try_get(start + 7)?,
            workspace_id: row.try_get(start + 8)?,
        })
    }
}
