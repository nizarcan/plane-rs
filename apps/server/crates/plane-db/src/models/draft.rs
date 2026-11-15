// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/draft.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, NaiveDate, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `DraftIssue` — table `draft_issues`, generated from `apps/api/plane/db/models/draft.py`.
///
/// 24 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftIssue {
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
    /// `parent` → `db.Issue` (`issues.id`, `CASCADE`), nullable.
    pub parent_id: Option<Uuid>,
    /// `state` → `db.State` (`states.id`, `CASCADE`), nullable.
    pub state_id: Option<Uuid>,
    /// `estimate_point` → `db.EstimatePoint` (`estimate_points.id`, `SET_NULL`), nullable.
    pub estimate_point_id: Option<Uuid>,
    /// `name` — `CharField`, `max_length=255`, nullable.
    pub name: Option<String>,
    /// `description_json` — `JSONField`. Default: `builtins.dict`.
    pub description_json: JsonText,
    /// `description_html` — `TextField`. Default: `'<p></p>'`.
    pub description_html: String,
    /// `description_stripped` — `TextField`, nullable.
    pub description_stripped: Option<String>,
    /// `description_binary` — `BinaryField`, nullable.
    pub description_binary: Option<Vec<u8>>,
    /// `priority` — `CharField`, `max_length=30`. Default: `'none'`.
    pub priority: String,
    /// `start_date` — `DateField`, nullable.
    pub start_date: Option<NaiveDate>,
    /// `target_date` — `DateField`, nullable.
    pub target_date: Option<NaiveDate>,
    /// `sort_order` — `FloatField`. Default: `65535`.
    pub sort_order: f64,
    /// `completed_at` — `DateTimeField`, nullable.
    pub completed_at: Option<DateTime<Utc>>,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
    /// `type` → `db.IssueType` (`issue_types.id`, `SET_NULL`), nullable.
    pub type_id: Option<Uuid>,
}

impl DraftIssue {
    /// The table this row is read from (`draft_issues`).
    pub const TABLE: &'static str = "draft_issues";

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
        ", {alias}.\"parent_id\"",
        ", {alias}.\"state_id\"",
        ", {alias}.\"estimate_point_id\"",
        ", {alias}.\"name\"",
        ", {alias}.\"description_json\"",
        ", {alias}.\"description_html\"",
        ", {alias}.\"description_stripped\"",
        ", {alias}.\"description_binary\"",
        ", {alias}.\"priority\"",
        ", {alias}.\"start_date\"",
        ", {alias}.\"target_date\"",
        ", {alias}.\"sort_order\"",
        ", {alias}.\"completed_at\"",
        ", {alias}.\"external_source\"",
        ", {alias}.\"external_id\"",
        ", {alias}.\"type_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (24).
    pub const COLUMN_COUNT: usize = 24;

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
    /// Django WHERE: `("draft_issues"."deleted_at" IS NULL)`.
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
            parent_id: row.try_get(start + 8)?,
            state_id: row.try_get(start + 9)?,
            estimate_point_id: row.try_get(start + 10)?,
            name: row.try_get(start + 11)?,
            description_json: row.try_get(start + 12)?,
            description_html: row.try_get(start + 13)?,
            description_stripped: row.try_get(start + 14)?,
            description_binary: row.try_get(start + 15)?,
            priority: row.try_get(start + 16)?,
            start_date: row.try_get(start + 17)?,
            target_date: row.try_get(start + 18)?,
            sort_order: row.try_get(start + 19)?,
            completed_at: row.try_get(start + 20)?,
            external_source: row.try_get(start + 21)?,
            external_id: row.try_get(start + 22)?,
            type_id: row.try_get(start + 23)?,
        })
    }
}

/// `DraftIssueAssignee` — table `draft_issue_assignees`, generated from `apps/api/plane/db/models/draft.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftIssueAssignee {
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
    /// `draft_issue` → `db.DraftIssue` (`draft_issues.id`, `CASCADE`).
    pub draft_issue_id: Uuid,
    /// `assignee` → `db.User` (`users.id`, `CASCADE`).
    pub assignee_id: Uuid,
}

impl DraftIssueAssignee {
    /// The table this row is read from (`draft_issue_assignees`).
    pub const TABLE: &'static str = "draft_issue_assignees";

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
        ", {alias}.\"draft_issue_id\"",
        ", {alias}.\"assignee_id\"",
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
    /// Django WHERE: `("draft_issue_assignees"."deleted_at" IS NULL)`.
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
            draft_issue_id: row.try_get(start + 8)?,
            assignee_id: row.try_get(start + 9)?,
        })
    }
}

/// `DraftIssueCycle` — table `draft_issue_cycles`, generated from `apps/api/plane/db/models/draft.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftIssueCycle {
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
    /// `draft_issue` → `db.DraftIssue` (`draft_issues.id`, `CASCADE`).
    pub draft_issue_id: Uuid,
    /// `cycle` → `db.Cycle` (`cycles.id`, `CASCADE`).
    pub cycle_id: Uuid,
}

impl DraftIssueCycle {
    /// The table this row is read from (`draft_issue_cycles`).
    pub const TABLE: &'static str = "draft_issue_cycles";

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
        ", {alias}.\"draft_issue_id\"",
        ", {alias}.\"cycle_id\"",
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
    /// Django WHERE: `("draft_issue_cycles"."deleted_at" IS NULL)`.
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
            draft_issue_id: row.try_get(start + 8)?,
            cycle_id: row.try_get(start + 9)?,
        })
    }
}

/// `DraftIssueLabel` — table `draft_issue_labels`, generated from `apps/api/plane/db/models/draft.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftIssueLabel {
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
    /// `draft_issue` → `db.DraftIssue` (`draft_issues.id`, `CASCADE`).
    pub draft_issue_id: Uuid,
    /// `label` → `db.Label` (`labels.id`, `CASCADE`).
    pub label_id: Uuid,
}

impl DraftIssueLabel {
    /// The table this row is read from (`draft_issue_labels`).
    pub const TABLE: &'static str = "draft_issue_labels";

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
        ", {alias}.\"draft_issue_id\"",
        ", {alias}.\"label_id\"",
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
    /// Django WHERE: `("draft_issue_labels"."deleted_at" IS NULL)`.
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
            draft_issue_id: row.try_get(start + 8)?,
            label_id: row.try_get(start + 9)?,
        })
    }
}

/// `DraftIssueModule` — table `draft_issue_modules`, generated from `apps/api/plane/db/models/draft.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftIssueModule {
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
    /// `module` → `db.Module` (`modules.id`, `CASCADE`).
    pub module_id: Uuid,
    /// `draft_issue` → `db.DraftIssue` (`draft_issues.id`, `CASCADE`).
    pub draft_issue_id: Uuid,
}

impl DraftIssueModule {
    /// The table this row is read from (`draft_issue_modules`).
    pub const TABLE: &'static str = "draft_issue_modules";

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
        ", {alias}.\"module_id\"",
        ", {alias}.\"draft_issue_id\"",
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
    /// Django WHERE: `("draft_issue_modules"."deleted_at" IS NULL)`.
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
            module_id: row.try_get(start + 8)?,
            draft_issue_id: row.try_get(start + 9)?,
        })
    }
}
