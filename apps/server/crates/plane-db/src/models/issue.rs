// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/issue.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, NaiveDate, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `CommentReaction` — table `comment_reactions`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 11 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentReaction {
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
    /// `actor` → `db.User` (`users.id`, `CASCADE`).
    pub actor_id: Uuid,
    /// `comment` → `db.IssueComment` (`issue_comments.id`, `CASCADE`).
    pub comment_id: Uuid,
    /// `reaction` — `TextField`.
    pub reaction: String,
}

impl CommentReaction {
    /// The table this row is read from (`comment_reactions`).
    pub const TABLE: &'static str = "comment_reactions";

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
        ", {alias}.\"actor_id\"",
        ", {alias}.\"comment_id\"",
        ", {alias}.\"reaction\"",
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
    /// Django WHERE: `("comment_reactions"."deleted_at" IS NULL)`.
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
            actor_id: row.try_get(start + 8)?,
            comment_id: row.try_get(start + 9)?,
            reaction: row.try_get(start + 10)?,
        })
    }
}

/// `Issue` — table `issues`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 28 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
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
    /// `parent` → `db.Issue` (`issues.id`, `CASCADE`), nullable.
    pub parent_id: Option<Uuid>,
    /// `state` → `db.State` (`states.id`, `CASCADE`), nullable.
    pub state_id: Option<Uuid>,
    /// `point` — `IntegerField`, nullable.
    pub point: Option<i32>,
    /// `estimate_point` → `db.EstimatePoint` (`estimate_points.id`, `SET_NULL`), nullable.
    pub estimate_point_id: Option<Uuid>,
    /// `name` — `CharField`, `max_length=255`.
    pub name: String,
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
    /// `sequence_id` — `IntegerField`. Default: `1`.
    pub sequence_id: i32,
    /// `sort_order` — `FloatField`. Default: `65535`.
    pub sort_order: f64,
    /// `completed_at` — `DateTimeField`, nullable.
    pub completed_at: Option<DateTime<Utc>>,
    /// `archived_at` — `DateField`, nullable.
    pub archived_at: Option<NaiveDate>,
    /// `is_draft` — `BooleanField`. Default: `False`.
    pub is_draft: bool,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
    /// `type` → `db.IssueType` (`issue_types.id`, `SET_NULL`), nullable.
    pub type_id: Option<Uuid>,
}

impl Issue {
    /// The table this row is read from (`issues`).
    pub const TABLE: &'static str = "issues";

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
        ", {alias}.\"parent_id\"",
        ", {alias}.\"state_id\"",
        ", {alias}.\"point\"",
        ", {alias}.\"estimate_point_id\"",
        ", {alias}.\"name\"",
        ", {alias}.\"description_json\"",
        ", {alias}.\"description_html\"",
        ", {alias}.\"description_stripped\"",
        ", {alias}.\"description_binary\"",
        ", {alias}.\"priority\"",
        ", {alias}.\"start_date\"",
        ", {alias}.\"target_date\"",
        ", {alias}.\"sequence_id\"",
        ", {alias}.\"sort_order\"",
        ", {alias}.\"completed_at\"",
        ", {alias}.\"archived_at\"",
        ", {alias}.\"is_draft\"",
        ", {alias}.\"external_source\"",
        ", {alias}.\"external_id\"",
        ", {alias}.\"type_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (28).
    pub const COLUMN_COUNT: usize = 28;

    /// Default ordering (`Meta.ordering = ['-created_at']`).
    ///
    /// Django's SQL: `"created_at" DESC`
    /// with the alias placeholder inserted.
    pub const ORDER_BY: &'static str = "{alias}.\"created_at\" DESC";

    /// Manager `issue_objects` — `plane.db.models.issue.IssueManager.get_queryset`
    /// (`apps/api/plane/db/models/issue.py:93`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return (
    ///             super()
    ///             .get_queryset()
    ///             .exclude(state__group=StateGroup.TRIAGE.value)
    ///             .exclude(archived_at__isnull=False)
    ///             .exclude(project__archived_at__isnull=False)
    ///             .exclude(is_draft=True)
    ///         )
    /// ```
    ///
    /// Django WHERE: `("issues"."deleted_at" IS NULL) AND (NOT (("states"."group" = 'triage') AND ("states"."group" IS NOT NULL))) AND (NOT (("issues"."archived_at" IS NOT NULL))) AND (NOT (("projects"."archived_at" IS NOT NULL))) AND (NOT (("issues"."is_draft")))`.
    pub const ISSUE_OBJECTS: &'static str = "({alias}.\"deleted_at\" IS NULL) AND (NOT (EXISTS (SELECT 1 FROM \"states\" WHERE \"states\".\"id\" = {alias}.\"state_id\" AND \"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL))) AND (NOT (({alias}.\"archived_at\" IS NOT NULL))) AND (NOT (EXISTS (SELECT 1 FROM \"projects\" WHERE \"projects\".\"id\" = {alias}.\"project_id\" AND \"projects\".\"archived_at\" IS NOT NULL))) AND (NOT (({alias}.\"is_draft\")))";

    /// Manager `objects` — `plane.db.mixins.SoftDeletionManager.get_queryset`
    /// (`apps/api/plane/db/mixins.py:57`).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return SoftDeletionQuerySet(self.model, using=self._db).filter(deleted_at__isnull=True)
    /// ```
    ///
    /// Django WHERE: `("issues"."deleted_at" IS NULL)`.
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
            parent_id: row.try_get(start + 8)?,
            state_id: row.try_get(start + 9)?,
            point: row.try_get(start + 10)?,
            estimate_point_id: row.try_get(start + 11)?,
            name: row.try_get(start + 12)?,
            description_json: row.try_get(start + 13)?,
            description_html: row.try_get(start + 14)?,
            description_stripped: row.try_get(start + 15)?,
            description_binary: row.try_get(start + 16)?,
            priority: row.try_get(start + 17)?,
            start_date: row.try_get(start + 18)?,
            target_date: row.try_get(start + 19)?,
            sequence_id: row.try_get(start + 20)?,
            sort_order: row.try_get(start + 21)?,
            completed_at: row.try_get(start + 22)?,
            archived_at: row.try_get(start + 23)?,
            is_draft: row.try_get(start + 24)?,
            external_source: row.try_get(start + 25)?,
            external_id: row.try_get(start + 26)?,
            type_id: row.try_get(start + 27)?,
        })
    }
}

/// `IssueActivity` — table `issue_activities`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 20 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueActivity {
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
    /// `issue` → `db.Issue` (`issues.id`, `DO_NOTHING`), nullable.
    pub issue_id: Option<Uuid>,
    /// `verb` — `CharField`, `max_length=255`. Default: `'created'`.
    pub verb: String,
    /// `field` — `CharField`, `max_length=255`, nullable.
    pub field: Option<String>,
    /// `old_value` — `TextField`, nullable.
    pub old_value: Option<String>,
    /// `new_value` — `TextField`, nullable.
    pub new_value: Option<String>,
    /// `comment` — `TextField`.
    pub comment: String,
    /// `attachments` — `ArrayField`. Default: `builtins.list`.
    pub attachments: Vec<String>,
    /// `issue_comment` → `db.IssueComment` (`issue_comments.id`, `DO_NOTHING`), nullable.
    pub issue_comment_id: Option<Uuid>,
    /// `actor` → `db.User` (`users.id`, `SET_NULL`), nullable.
    pub actor_id: Option<Uuid>,
    /// `old_identifier` — `UUIDField`, `max_length=32`, nullable.
    pub old_identifier: Option<Uuid>,
    /// `new_identifier` — `UUIDField`, `max_length=32`, nullable.
    pub new_identifier: Option<Uuid>,
    /// `epoch` — `FloatField`, nullable.
    pub epoch: Option<f64>,
}

impl IssueActivity {
    /// The table this row is read from (`issue_activities`).
    pub const TABLE: &'static str = "issue_activities";

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
        ", {alias}.\"issue_id\"",
        ", {alias}.\"verb\"",
        ", {alias}.\"field\"",
        ", {alias}.\"old_value\"",
        ", {alias}.\"new_value\"",
        ", {alias}.\"comment\"",
        ", {alias}.\"attachments\"",
        ", {alias}.\"issue_comment_id\"",
        ", {alias}.\"actor_id\"",
        ", {alias}.\"old_identifier\"",
        ", {alias}.\"new_identifier\"",
        ", {alias}.\"epoch\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (20).
    pub const COLUMN_COUNT: usize = 20;

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
    /// Django WHERE: `("issue_activities"."deleted_at" IS NULL)`.
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
            issue_id: row.try_get(start + 8)?,
            verb: row.try_get(start + 9)?,
            field: row.try_get(start + 10)?,
            old_value: row.try_get(start + 11)?,
            new_value: row.try_get(start + 12)?,
            comment: row.try_get(start + 13)?,
            attachments: row.try_get(start + 14)?,
            issue_comment_id: row.try_get(start + 15)?,
            actor_id: row.try_get(start + 16)?,
            old_identifier: row.try_get(start + 17)?,
            new_identifier: row.try_get(start + 18)?,
            epoch: row.try_get(start + 19)?,
        })
    }
}

/// `IssueAssignee` — table `issue_assignees`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueAssignee {
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
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `assignee` → `db.User` (`users.id`, `CASCADE`).
    pub assignee_id: Uuid,
}

impl IssueAssignee {
    /// The table this row is read from (`issue_assignees`).
    pub const TABLE: &'static str = "issue_assignees";

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
        ", {alias}.\"issue_id\"",
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
    /// Django WHERE: `("issue_assignees"."deleted_at" IS NULL)`.
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
            issue_id: row.try_get(start + 8)?,
            assignee_id: row.try_get(start + 9)?,
        })
    }
}

/// `IssueAttachment` — table `issue_attachments`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 13 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueAttachment {
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
    /// `attributes` — `JSONField`. Default: `builtins.dict`.
    pub attributes: JsonText,
    /// `asset` — `FileField`, `max_length=100`.
    pub asset: String,
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
}

impl IssueAttachment {
    /// The table this row is read from (`issue_attachments`).
    pub const TABLE: &'static str = "issue_attachments";

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
        ", {alias}.\"attributes\"",
        ", {alias}.\"asset\"",
        ", {alias}.\"issue_id\"",
        ", {alias}.\"external_source\"",
        ", {alias}.\"external_id\"",
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
    /// Django WHERE: `("issue_attachments"."deleted_at" IS NULL)`.
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
            attributes: row.try_get(start + 8)?,
            asset: row.try_get(start + 9)?,
            issue_id: row.try_get(start + 10)?,
            external_source: row.try_get(start + 11)?,
            external_id: row.try_get(start + 12)?,
        })
    }
}

/// `IssueBlocker` — table `issue_blockers`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueBlocker {
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
    /// `block` → `db.Issue` (`issues.id`, `CASCADE`).
    pub block_id: Uuid,
    /// `blocked_by` → `db.Issue` (`issues.id`, `CASCADE`).
    pub blocked_by_id: Uuid,
}

impl IssueBlocker {
    /// The table this row is read from (`issue_blockers`).
    pub const TABLE: &'static str = "issue_blockers";

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
        ", {alias}.\"block_id\"",
        ", {alias}.\"blocked_by_id\"",
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
    /// Django WHERE: `("issue_blockers"."deleted_at" IS NULL)`.
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
            block_id: row.try_get(start + 8)?,
            blocked_by_id: row.try_get(start + 9)?,
        })
    }
}

/// `IssueComment` — table `issue_comments`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 20 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueComment {
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
    /// `comment_stripped` — `TextField`.
    pub comment_stripped: String,
    /// `comment_json` — `JSONField`. Default: `builtins.dict`.
    pub comment_json: JsonText,
    /// `comment_html` — `TextField`. Default: `'<p></p>'`.
    pub comment_html: String,
    /// `description` → `db.Description` (`descriptions.id`, `CASCADE`, one-to-one), nullable.
    pub description_id: Option<Uuid>,
    /// `attachments` — `ArrayField`. Default: `builtins.list`.
    pub attachments: Vec<String>,
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `actor` → `db.User` (`users.id`, `CASCADE`), nullable.
    pub actor_id: Option<Uuid>,
    /// `access` — `CharField`, `max_length=100`. Default: `'INTERNAL'`.
    pub access: String,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
    /// `edited_at` — `DateTimeField`, nullable.
    pub edited_at: Option<DateTime<Utc>>,
    /// `parent` → `db.IssueComment` (`issue_comments.id`, `CASCADE`), nullable.
    pub parent_id: Option<Uuid>,
}

impl IssueComment {
    /// The table this row is read from (`issue_comments`).
    pub const TABLE: &'static str = "issue_comments";

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
        ", {alias}.\"comment_stripped\"",
        ", {alias}.\"comment_json\"",
        ", {alias}.\"comment_html\"",
        ", {alias}.\"description_id\"",
        ", {alias}.\"attachments\"",
        ", {alias}.\"issue_id\"",
        ", {alias}.\"actor_id\"",
        ", {alias}.\"access\"",
        ", {alias}.\"external_source\"",
        ", {alias}.\"external_id\"",
        ", {alias}.\"edited_at\"",
        ", {alias}.\"parent_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (20).
    pub const COLUMN_COUNT: usize = 20;

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
    /// Django WHERE: `("issue_comments"."deleted_at" IS NULL)`.
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
            comment_stripped: row.try_get(start + 8)?,
            comment_json: row.try_get(start + 9)?,
            comment_html: row.try_get(start + 10)?,
            description_id: row.try_get(start + 11)?,
            attachments: row.try_get(start + 12)?,
            issue_id: row.try_get(start + 13)?,
            actor_id: row.try_get(start + 14)?,
            access: row.try_get(start + 15)?,
            external_source: row.try_get(start + 16)?,
            external_id: row.try_get(start + 17)?,
            edited_at: row.try_get(start + 18)?,
            parent_id: row.try_get(start + 19)?,
        })
    }
}

/// `IssueDescriptionVersion` — table `issue_description_versions`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 15 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueDescriptionVersion {
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
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `description_binary` — `BinaryField`, nullable.
    pub description_binary: Option<Vec<u8>>,
    /// `description_html` — `TextField`. Default: `'<p></p>'`.
    pub description_html: String,
    /// `description_stripped` — `TextField`, nullable.
    pub description_stripped: Option<String>,
    /// `description_json` — `JSONField`. Default: `builtins.dict`.
    pub description_json: JsonText,
    /// `last_saved_at` — `DateTimeField`. Default: `django.utils.timezone.now`.
    pub last_saved_at: DateTime<Utc>,
    /// `owned_by` → `db.User` (`users.id`, `CASCADE`).
    pub owned_by_id: Uuid,
}

impl IssueDescriptionVersion {
    /// The table this row is read from (`issue_description_versions`).
    pub const TABLE: &'static str = "issue_description_versions";

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
        ", {alias}.\"issue_id\"",
        ", {alias}.\"description_binary\"",
        ", {alias}.\"description_html\"",
        ", {alias}.\"description_stripped\"",
        ", {alias}.\"description_json\"",
        ", {alias}.\"last_saved_at\"",
        ", {alias}.\"owned_by_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (15).
    pub const COLUMN_COUNT: usize = 15;

    /// No `Meta.ordering`: Django leaves the row order to Postgres.
    pub const ORDER_BY: &'static str = "";

    /// Manager `objects` — `plane.db.mixins.SoftDeletionManager.get_queryset`
    /// (`apps/api/plane/db/mixins.py:57`, the default manager).
    ///
    /// ```python
    ///     def get_queryset(self):
    ///         return SoftDeletionQuerySet(self.model, using=self._db).filter(deleted_at__isnull=True)
    /// ```
    ///
    /// Django WHERE: `("issue_description_versions"."deleted_at" IS NULL)`.
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
            issue_id: row.try_get(start + 8)?,
            description_binary: row.try_get(start + 9)?,
            description_html: row.try_get(start + 10)?,
            description_stripped: row.try_get(start + 11)?,
            description_json: row.try_get(start + 12)?,
            last_saved_at: row.try_get(start + 13)?,
            owned_by_id: row.try_get(start + 14)?,
        })
    }
}

/// `IssueLabel` — table `issue_labels`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueLabel {
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
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `label` → `db.Label` (`labels.id`, `CASCADE`).
    pub label_id: Uuid,
}

impl IssueLabel {
    /// The table this row is read from (`issue_labels`).
    pub const TABLE: &'static str = "issue_labels";

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
        ", {alias}.\"issue_id\"",
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
    /// Django WHERE: `("issue_labels"."deleted_at" IS NULL)`.
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
            issue_id: row.try_get(start + 8)?,
            label_id: row.try_get(start + 9)?,
        })
    }
}

/// `IssueLink` — table `issue_links`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 12 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueLink {
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
    /// `title` — `CharField`, `max_length=255`, nullable.
    pub title: Option<String>,
    /// `url` — `TextField`.
    pub url: String,
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `metadata` — `JSONField`. Default: `builtins.dict`.
    pub metadata: JsonText,
}

impl IssueLink {
    /// The table this row is read from (`issue_links`).
    pub const TABLE: &'static str = "issue_links";

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
        ", {alias}.\"title\"",
        ", {alias}.\"url\"",
        ", {alias}.\"issue_id\"",
        ", {alias}.\"metadata\"",
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
    /// Django WHERE: `("issue_links"."deleted_at" IS NULL)`.
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
            title: row.try_get(start + 8)?,
            url: row.try_get(start + 9)?,
            issue_id: row.try_get(start + 10)?,
            metadata: row.try_get(start + 11)?,
        })
    }
}

/// `IssueMention` — table `issue_mentions`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueMention {
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
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `mention` → `db.User` (`users.id`, `CASCADE`).
    pub mention_id: Uuid,
}

impl IssueMention {
    /// The table this row is read from (`issue_mentions`).
    pub const TABLE: &'static str = "issue_mentions";

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
        ", {alias}.\"issue_id\"",
        ", {alias}.\"mention_id\"",
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
    /// Django WHERE: `("issue_mentions"."deleted_at" IS NULL)`.
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
            issue_id: row.try_get(start + 8)?,
            mention_id: row.try_get(start + 9)?,
        })
    }
}

/// `IssueReaction` — table `issue_reactions`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 11 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueReaction {
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
    /// `actor` → `db.User` (`users.id`, `CASCADE`).
    pub actor_id: Uuid,
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `reaction` — `TextField`.
    pub reaction: String,
}

impl IssueReaction {
    /// The table this row is read from (`issue_reactions`).
    pub const TABLE: &'static str = "issue_reactions";

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
        ", {alias}.\"actor_id\"",
        ", {alias}.\"issue_id\"",
        ", {alias}.\"reaction\"",
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
    /// Django WHERE: `("issue_reactions"."deleted_at" IS NULL)`.
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
            actor_id: row.try_get(start + 8)?,
            issue_id: row.try_get(start + 9)?,
            reaction: row.try_get(start + 10)?,
        })
    }
}

/// `IssueRelation` — table `issue_relations`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 11 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueRelation {
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
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `related_issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub related_issue_id: Uuid,
    /// `relation_type` — `CharField`, `max_length=20`. Default: `IssueRelationChoices.BLOCKED_BY`.
    pub relation_type: String,
}

impl IssueRelation {
    /// The table this row is read from (`issue_relations`).
    pub const TABLE: &'static str = "issue_relations";

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
        ", {alias}.\"issue_id\"",
        ", {alias}.\"related_issue_id\"",
        ", {alias}.\"relation_type\"",
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
    /// Django WHERE: `("issue_relations"."deleted_at" IS NULL)`.
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
            issue_id: row.try_get(start + 8)?,
            related_issue_id: row.try_get(start + 9)?,
            relation_type: row.try_get(start + 10)?,
        })
    }
}

/// `IssueSequence` — table `issue_sequences`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 11 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueSequence {
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
    /// `issue` → `db.Issue` (`issues.id`, `SET_NULL`), nullable.
    pub issue_id: Option<Uuid>,
    /// `sequence` — `PositiveBigIntegerField`, `db_index`. Default: `1`.
    pub sequence: i64,
    /// `deleted` — `BooleanField`. Default: `False`.
    pub deleted: bool,
}

impl IssueSequence {
    /// The table this row is read from (`issue_sequences`).
    pub const TABLE: &'static str = "issue_sequences";

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
        ", {alias}.\"issue_id\"",
        ", {alias}.\"sequence\"",
        ", {alias}.\"deleted\"",
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
    /// Django WHERE: `("issue_sequences"."deleted_at" IS NULL)`.
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
            issue_id: row.try_get(start + 8)?,
            sequence: row.try_get(start + 9)?,
            deleted: row.try_get(start + 10)?,
        })
    }
}

/// `IssueSubscriber` — table `issue_subscribers`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 10 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueSubscriber {
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
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `subscriber` → `db.User` (`users.id`, `CASCADE`).
    pub subscriber_id: Uuid,
}

impl IssueSubscriber {
    /// The table this row is read from (`issue_subscribers`).
    pub const TABLE: &'static str = "issue_subscribers";

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
        ", {alias}.\"issue_id\"",
        ", {alias}.\"subscriber_id\"",
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
    /// Django WHERE: `("issue_subscribers"."deleted_at" IS NULL)`.
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
            issue_id: row.try_get(start + 8)?,
            subscriber_id: row.try_get(start + 9)?,
        })
    }
}

/// `IssueVersion` — table `issue_versions`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 33 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueVersion {
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
    /// `parent` — `UUIDField`, `max_length=32`, nullable.
    pub parent: Option<Uuid>,
    /// `state` — `UUIDField`, `max_length=32`, nullable.
    pub state: Option<Uuid>,
    /// `estimate_point` — `UUIDField`, `max_length=32`, nullable.
    pub estimate_point: Option<Uuid>,
    /// `name` — `CharField`, `max_length=255`.
    pub name: String,
    /// `priority` — `CharField`, `max_length=30`. Default: `'none'`.
    pub priority: String,
    /// `start_date` — `DateField`, nullable.
    pub start_date: Option<NaiveDate>,
    /// `target_date` — `DateField`, nullable.
    pub target_date: Option<NaiveDate>,
    /// `assignees` — `ArrayField`. Default: `builtins.list`.
    pub assignees: Vec<Uuid>,
    /// `sequence_id` — `IntegerField`. Default: `1`.
    pub sequence_id: i32,
    /// `labels` — `ArrayField`. Default: `builtins.list`.
    pub labels: Vec<Uuid>,
    /// `sort_order` — `FloatField`. Default: `65535`.
    pub sort_order: f64,
    /// `completed_at` — `DateTimeField`, nullable.
    pub completed_at: Option<DateTime<Utc>>,
    /// `archived_at` — `DateField`, nullable.
    pub archived_at: Option<NaiveDate>,
    /// `is_draft` — `BooleanField`. Default: `False`.
    pub is_draft: bool,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
    /// `type` — `UUIDField`, `max_length=32`, nullable.
    pub r#type: Option<Uuid>,
    /// `cycle` — `UUIDField`, `max_length=32`, nullable.
    pub cycle: Option<Uuid>,
    /// `modules` — `ArrayField`. Default: `builtins.list`.
    pub modules: Vec<Uuid>,
    /// `properties` — `JSONField`. Default: `builtins.dict`.
    pub properties: JsonText,
    /// `meta` — `JSONField`. Default: `builtins.dict`.
    pub meta: JsonText,
    /// `last_saved_at` — `DateTimeField`. Default: `django.utils.timezone.now`.
    pub last_saved_at: DateTime<Utc>,
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `activity` → `db.IssueActivity` (`issue_activities.id`, `SET_NULL`), nullable.
    pub activity_id: Option<Uuid>,
    /// `owned_by` → `db.User` (`users.id`, `CASCADE`).
    pub owned_by_id: Uuid,
}

impl IssueVersion {
    /// The table this row is read from (`issue_versions`).
    pub const TABLE: &'static str = "issue_versions";

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
        ", {alias}.\"parent\"",
        ", {alias}.\"state\"",
        ", {alias}.\"estimate_point\"",
        ", {alias}.\"name\"",
        ", {alias}.\"priority\"",
        ", {alias}.\"start_date\"",
        ", {alias}.\"target_date\"",
        ", {alias}.\"assignees\"",
        ", {alias}.\"sequence_id\"",
        ", {alias}.\"labels\"",
        ", {alias}.\"sort_order\"",
        ", {alias}.\"completed_at\"",
        ", {alias}.\"archived_at\"",
        ", {alias}.\"is_draft\"",
        ", {alias}.\"external_source\"",
        ", {alias}.\"external_id\"",
        ", {alias}.\"type\"",
        ", {alias}.\"cycle\"",
        ", {alias}.\"modules\"",
        ", {alias}.\"properties\"",
        ", {alias}.\"meta\"",
        ", {alias}.\"last_saved_at\"",
        ", {alias}.\"issue_id\"",
        ", {alias}.\"activity_id\"",
        ", {alias}.\"owned_by_id\"",
    );

    /// Number of columns in [`Self::COLUMNS`] (33).
    pub const COLUMN_COUNT: usize = 33;

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
    /// Django WHERE: `("issue_versions"."deleted_at" IS NULL)`.
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
            parent: row.try_get(start + 8)?,
            state: row.try_get(start + 9)?,
            estimate_point: row.try_get(start + 10)?,
            name: row.try_get(start + 11)?,
            priority: row.try_get(start + 12)?,
            start_date: row.try_get(start + 13)?,
            target_date: row.try_get(start + 14)?,
            assignees: row.try_get(start + 15)?,
            sequence_id: row.try_get(start + 16)?,
            labels: row.try_get(start + 17)?,
            sort_order: row.try_get(start + 18)?,
            completed_at: row.try_get(start + 19)?,
            archived_at: row.try_get(start + 20)?,
            is_draft: row.try_get(start + 21)?,
            external_source: row.try_get(start + 22)?,
            external_id: row.try_get(start + 23)?,
            r#type: row.try_get(start + 24)?,
            cycle: row.try_get(start + 25)?,
            modules: row.try_get(start + 26)?,
            properties: row.try_get(start + 27)?,
            meta: row.try_get(start + 28)?,
            last_saved_at: row.try_get(start + 29)?,
            issue_id: row.try_get(start + 30)?,
            activity_id: row.try_get(start + 31)?,
            owned_by_id: row.try_get(start + 32)?,
        })
    }
}

/// `IssueVote` — table `issue_votes`, generated from `apps/api/plane/db/models/issue.py`.
///
/// 11 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueVote {
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
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`).
    pub issue_id: Uuid,
    /// `actor` → `db.User` (`users.id`, `CASCADE`).
    pub actor_id: Uuid,
    /// `vote` — `IntegerField`. Default: `1`.
    pub vote: i32,
}

impl IssueVote {
    /// The table this row is read from (`issue_votes`).
    pub const TABLE: &'static str = "issue_votes";

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
        ", {alias}.\"issue_id\"",
        ", {alias}.\"actor_id\"",
        ", {alias}.\"vote\"",
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
    /// Django WHERE: `("issue_votes"."deleted_at" IS NULL)`.
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
            issue_id: row.try_get(start + 8)?,
            actor_id: row.try_get(start + 9)?,
            vote: row.try_get(start + 10)?,
        })
    }
}
