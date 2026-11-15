// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/project.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `Project` — table `projects`, generated from `apps/api/plane/db/models/project.py`.
///
/// 36 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
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
    /// `description_text` — `JSONField`, nullable.
    pub description_text: Option<JsonText>,
    /// `description_html` — `JSONField`, nullable.
    pub description_html: Option<JsonText>,
    /// `network` — `PositiveSmallIntegerField`. Default: `2`.
    pub network: i16,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`).
    pub workspace_id: Uuid,
    /// `identifier` — `CharField`, `max_length=12`, `db_index`.
    pub identifier: String,
    /// `default_assignee` → `db.User` (`users.id`, `CASCADE`), nullable.
    pub default_assignee_id: Option<Uuid>,
    /// `project_lead` → `db.User` (`users.id`, `CASCADE`), nullable.
    pub project_lead_id: Option<Uuid>,
    /// `emoji` — `CharField`, `max_length=255`, nullable.
    pub emoji: Option<String>,
    /// `icon_prop` — `JSONField`, nullable.
    pub icon_prop: Option<JsonText>,
    /// `module_view` — `BooleanField`. Default: `False`.
    pub module_view: bool,
    /// `cycle_view` — `BooleanField`. Default: `False`.
    pub cycle_view: bool,
    /// `issue_views_view` — `BooleanField`. Default: `False`.
    pub issue_views_view: bool,
    /// `page_view` — `BooleanField`. Default: `True`.
    pub page_view: bool,
    /// `intake_view` — `BooleanField`. Default: `False`.
    pub intake_view: bool,
    /// `is_time_tracking_enabled` — `BooleanField`. Default: `False`.
    pub is_time_tracking_enabled: bool,
    /// `is_issue_type_enabled` — `BooleanField`. Default: `False`.
    pub is_issue_type_enabled: bool,
    /// `guest_view_all_features` — `BooleanField`. Default: `False`.
    pub guest_view_all_features: bool,
    /// `cover_image` — `TextField`, nullable.
    pub cover_image: Option<String>,
    /// `cover_image_asset` → `db.FileAsset` (`file_assets.id`, `SET_NULL`), nullable.
    pub cover_image_asset_id: Option<Uuid>,
    /// `estimate` → `db.Estimate` (`estimates.id`, `SET_NULL`), nullable.
    pub estimate_id: Option<Uuid>,
    /// `archive_in` — `IntegerField`. Default: `0`.
    pub archive_in: i32,
    /// `close_in` — `IntegerField`. Default: `0`.
    pub close_in: i32,
    /// `logo_props` — `JSONField`. Default: `builtins.dict`.
    pub logo_props: JsonText,
    /// `default_state` → `db.State` (`states.id`, `SET_NULL`), nullable.
    pub default_state_id: Option<Uuid>,
    /// `archived_at` — `DateTimeField`, nullable.
    pub archived_at: Option<DateTime<Utc>>,
    /// `timezone` — `CharField`, `max_length=255`. Default: `'UTC'`.
    pub timezone: String,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
}

impl Project {
    /// The table this row is read from (`projects`).
    pub const TABLE: &'static str = "projects";

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
        ", {alias}.\"description_text\"",
        ", {alias}.\"description_html\"",
        ", {alias}.\"network\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"identifier\"",
        ", {alias}.\"default_assignee_id\"",
        ", {alias}.\"project_lead_id\"",
        ", {alias}.\"emoji\"",
        ", {alias}.\"icon_prop\"",
        ", {alias}.\"module_view\"",
        ", {alias}.\"cycle_view\"",
        ", {alias}.\"issue_views_view\"",
        ", {alias}.\"page_view\"",
        ", {alias}.\"intake_view\"",
        ", {alias}.\"is_time_tracking_enabled\"",
        ", {alias}.\"is_issue_type_enabled\"",
        ", {alias}.\"guest_view_all_features\"",
        ", {alias}.\"cover_image\"",
        ", {alias}.\"cover_image_asset_id\"",
        ", {alias}.\"estimate_id\"",
        ", {alias}.\"archive_in\"",
        ", {alias}.\"close_in\"",
        ", {alias}.\"logo_props\"",
        ", {alias}.\"default_state_id\"",
        ", {alias}.\"archived_at\"",
        ", {alias}.\"timezone\"",
        ", {alias}.\"external_source\"",
        ", {alias}.\"external_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (36).
    pub const COLUMN_COUNT: usize = 36;

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
    /// Django WHERE: `("projects"."deleted_at" IS NULL)`.
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
            description_text: row.try_get(start + 8)?,
            description_html: row.try_get(start + 9)?,
            network: row.try_get(start + 10)?,
            workspace_id: row.try_get(start + 11)?,
            identifier: row.try_get(start + 12)?,
            default_assignee_id: row.try_get(start + 13)?,
            project_lead_id: row.try_get(start + 14)?,
            emoji: row.try_get(start + 15)?,
            icon_prop: row.try_get(start + 16)?,
            module_view: row.try_get(start + 17)?,
            cycle_view: row.try_get(start + 18)?,
            issue_views_view: row.try_get(start + 19)?,
            page_view: row.try_get(start + 20)?,
            intake_view: row.try_get(start + 21)?,
            is_time_tracking_enabled: row.try_get(start + 22)?,
            is_issue_type_enabled: row.try_get(start + 23)?,
            guest_view_all_features: row.try_get(start + 24)?,
            cover_image: row.try_get(start + 25)?,
            cover_image_asset_id: row.try_get(start + 26)?,
            estimate_id: row.try_get(start + 27)?,
            archive_in: row.try_get(start + 28)?,
            close_in: row.try_get(start + 29)?,
            logo_props: row.try_get(start + 30)?,
            default_state_id: row.try_get(start + 31)?,
            archived_at: row.try_get(start + 32)?,
            timezone: row.try_get(start + 33)?,
            external_source: row.try_get(start + 34)?,
            external_id: row.try_get(start + 35)?,
        })
    }
}

/// `ProjectDeployBoard` — table `project_deploy_boards`, generated from `apps/api/plane/db/models/project.py`.
///
/// 14 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectDeployBoard {
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
    /// `anchor` — `CharField`, `max_length=255`, `unique`, `db_index`. Default: `plane.db.models.project.get_anchor`.
    pub anchor: String,
    /// `comments` — `BooleanField`. Default: `False`.
    pub comments: bool,
    /// `reactions` — `BooleanField`. Default: `False`.
    pub reactions: bool,
    /// `intake` → `db.Intake` (`intakes.id`, `SET_NULL`), nullable.
    pub intake_id: Option<Uuid>,
    /// `votes` — `BooleanField`. Default: `False`.
    pub votes: bool,
    /// `views` — `JSONField`. Default: `plane.db.models.project.get_default_views`.
    pub views: JsonText,
}

impl ProjectDeployBoard {
    /// The table this row is read from (`project_deploy_boards`).
    pub const TABLE: &'static str = "project_deploy_boards";

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
        ", {alias}.\"anchor\"",
        ", {alias}.\"comments\"",
        ", {alias}.\"reactions\"",
        ", {alias}.\"intake_id\"",
        ", {alias}.\"votes\"",
        ", {alias}.\"views\"",
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
    /// Django WHERE: `("project_deploy_boards"."deleted_at" IS NULL)`.
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
            anchor: row.try_get(start + 8)?,
            comments: row.try_get(start + 9)?,
            reactions: row.try_get(start + 10)?,
            intake_id: row.try_get(start + 11)?,
            votes: row.try_get(start + 12)?,
            views: row.try_get(start + 13)?,
        })
    }
}

/// `ProjectIdentifier` — table `project_identifiers`, generated from `apps/api/plane/db/models/project.py`.
///
/// 9 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectIdentifier {
    /// `id` — `BigAutoField`, `primary key`, `unique`.
    pub id: i64,
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
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`), nullable.
    pub workspace_id: Option<Uuid>,
    /// `project` → `db.Project` (`projects.id`, `CASCADE`, one-to-one).
    pub project_id: Uuid,
    /// `name` — `CharField`, `max_length=12`, `db_index`.
    pub name: String,
}

impl ProjectIdentifier {
    /// The table this row is read from (`project_identifiers`).
    pub const TABLE: &'static str = "project_identifiers";

    /// The `<alias>`-qualified select list, in `from_row` order.
    ///
    /// Replace `{alias}` with the alias of the statement
    /// (`plane_db::models::with_alias`).
    pub const COLUMNS: &'static str = concat!(
        "{alias}.\"id\"",
        ", {alias}.\"created_at\"",
        ", {alias}.\"updated_at\"",
        ", {alias}.\"created_by_id\"",
        ", {alias}.\"updated_by_id\"",
        ", {alias}.\"deleted_at\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"project_id\"",
        ", {alias}.\"name\"",
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
    /// Django WHERE: `("project_identifiers"."deleted_at" IS NULL)`.
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
            id: row.try_get(start)?,
            created_at: row.try_get(start + 1)?,
            updated_at: row.try_get(start + 2)?,
            created_by_id: row.try_get(start + 3)?,
            updated_by_id: row.try_get(start + 4)?,
            deleted_at: row.try_get(start + 5)?,
            workspace_id: row.try_get(start + 6)?,
            project_id: row.try_get(start + 7)?,
            name: row.try_get(start + 8)?,
        })
    }
}

/// `ProjectMember` — table `project_members`, generated from `apps/api/plane/db/models/project.py`.
///
/// 16 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMember {
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
    /// `member` → `db.User` (`users.id`, `CASCADE`), nullable.
    pub member_id: Option<Uuid>,
    /// `comment` — `TextField`, nullable.
    pub comment: Option<String>,
    /// `role` — `PositiveSmallIntegerField`. Default: `5`.
    pub role: i16,
    /// `view_props` — `JSONField`. Default: `plane.db.models.project.get_default_props`.
    pub view_props: JsonText,
    /// `default_props` — `JSONField`. Default: `plane.db.models.project.get_default_props`.
    pub default_props: JsonText,
    /// `preferences` — `JSONField`. Default: `plane.db.models.project.get_default_preferences`.
    pub preferences: JsonText,
    /// `sort_order` — `FloatField`. Default: `65535`.
    pub sort_order: f64,
    /// `is_active` — `BooleanField`. Default: `True`.
    pub is_active: bool,
}

impl ProjectMember {
    /// The table this row is read from (`project_members`).
    pub const TABLE: &'static str = "project_members";

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
        ", {alias}.\"member_id\"",
        ", {alias}.\"comment\"",
        ", {alias}.\"role\"",
        ", {alias}.\"view_props\"",
        ", {alias}.\"default_props\"",
        ", {alias}.\"preferences\"",
        ", {alias}.\"sort_order\"",
        ", {alias}.\"is_active\"",
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
    /// Django WHERE: `("project_members"."deleted_at" IS NULL)`.
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
            member_id: row.try_get(start + 8)?,
            comment: row.try_get(start + 9)?,
            role: row.try_get(start + 10)?,
            view_props: row.try_get(start + 11)?,
            default_props: row.try_get(start + 12)?,
            preferences: row.try_get(start + 13)?,
            sort_order: row.try_get(start + 14)?,
            is_active: row.try_get(start + 15)?,
        })
    }
}

/// `ProjectMemberInvite` — table `project_member_invites`, generated from `apps/api/plane/db/models/project.py`.
///
/// 14 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMemberInvite {
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

impl ProjectMemberInvite {
    /// The table this row is read from (`project_member_invites`).
    pub const TABLE: &'static str = "project_member_invites";

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
        ", {alias}.\"email\"",
        ", {alias}.\"accepted\"",
        ", {alias}.\"token\"",
        ", {alias}.\"message\"",
        ", {alias}.\"responded_at\"",
        ", {alias}.\"role\"",
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
    /// Django WHERE: `("project_member_invites"."deleted_at" IS NULL)`.
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
            email: row.try_get(start + 8)?,
            accepted: row.try_get(start + 9)?,
            token: row.try_get(start + 10)?,
            message: row.try_get(start + 11)?,
            responded_at: row.try_get(start + 12)?,
            role: row.try_get(start + 13)?,
        })
    }
}

/// `ProjectPublicMember` — table `project_public_members`, generated from `apps/api/plane/db/models/project.py`.
///
/// 9 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPublicMember {
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
    /// `member` → `db.User` (`users.id`, `CASCADE`).
    pub member_id: Uuid,
}

impl ProjectPublicMember {
    /// The table this row is read from (`project_public_members`).
    pub const TABLE: &'static str = "project_public_members";

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
        ", {alias}.\"member_id\"",
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
    /// Django WHERE: `("project_public_members"."deleted_at" IS NULL)`.
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
            member_id: row.try_get(start + 8)?,
        })
    }
}

/// `ProjectUserProperty` — table `project_user_properties`, generated from `apps/api/plane/db/models/project.py`.
///
/// 15 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectUserProperty {
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
    /// `user` → `db.User` (`users.id`, `CASCADE`).
    pub user_id: Uuid,
    /// `filters` — `JSONField`. Default: `plane.db.models.issue.get_default_filters`.
    pub filters: JsonText,
    /// `display_filters` — `JSONField`. Default: `plane.db.models.issue.get_default_display_filters`.
    pub display_filters: JsonText,
    /// `display_properties` — `JSONField`. Default: `plane.db.models.issue.get_default_display_properties`.
    pub display_properties: JsonText,
    /// `rich_filters` — `JSONField`. Default: `builtins.dict`.
    pub rich_filters: JsonText,
    /// `preferences` — `JSONField`. Default: `plane.db.models.project.get_default_preferences`.
    pub preferences: JsonText,
    /// `sort_order` — `FloatField`. Default: `65535`.
    pub sort_order: f64,
}

impl ProjectUserProperty {
    /// The table this row is read from (`project_user_properties`).
    pub const TABLE: &'static str = "project_user_properties";

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
        ", {alias}.\"user_id\"",
        ", {alias}.\"filters\"",
        ", {alias}.\"display_filters\"",
        ", {alias}.\"display_properties\"",
        ", {alias}.\"rich_filters\"",
        ", {alias}.\"preferences\"",
        ", {alias}.\"sort_order\"",
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
    /// Django WHERE: `("project_user_properties"."deleted_at" IS NULL)`.
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
            user_id: row.try_get(start + 8)?,
            filters: row.try_get(start + 9)?,
            display_filters: row.try_get(start + 10)?,
            display_properties: row.try_get(start + 11)?,
            rich_filters: row.try_get(start + 12)?,
            preferences: row.try_get(start + 13)?,
            sort_order: row.try_get(start + 14)?,
        })
    }
}
