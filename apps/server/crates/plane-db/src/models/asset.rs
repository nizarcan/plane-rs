// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated from `apps/api/plane/db/models/asset.py` by `apps/server/tools/codegen/models.py --rust`.
//! Never edit this file by hand; regenerate it instead.

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::models::{Error, JsonText};

/// `FileAsset` — table `file_assets`, generated from `apps/api/plane/db/models/asset.py`.
///
/// 24 columns, soft-deletable.
#[derive(Debug, Clone, PartialEq)]
pub struct FileAsset {
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
    /// `attributes` — `JSONField`. Default: `builtins.dict`.
    pub attributes: JsonText,
    /// `asset` — `FileField`, `max_length=800`.
    pub asset: String,
    /// `user` → `db.User` (`users.id`, `CASCADE`), nullable.
    pub user_id: Option<Uuid>,
    /// `workspace` → `db.Workspace` (`workspaces.id`, `CASCADE`), nullable.
    pub workspace_id: Option<Uuid>,
    /// `draft_issue` → `db.DraftIssue` (`draft_issues.id`, `CASCADE`), nullable.
    pub draft_issue_id: Option<Uuid>,
    /// `project` → `db.Project` (`projects.id`, `CASCADE`), nullable.
    pub project_id: Option<Uuid>,
    /// `issue` → `db.Issue` (`issues.id`, `CASCADE`), nullable.
    pub issue_id: Option<Uuid>,
    /// `comment` → `db.IssueComment` (`issue_comments.id`, `CASCADE`), nullable.
    pub comment_id: Option<Uuid>,
    /// `page` → `db.Page` (`pages.id`, `CASCADE`), nullable.
    pub page_id: Option<Uuid>,
    /// `entity_type` — `CharField`, `max_length=255`, nullable.
    pub entity_type: Option<String>,
    /// `entity_identifier` — `CharField`, `max_length=255`, nullable.
    pub entity_identifier: Option<String>,
    /// `is_deleted` — `BooleanField`. Default: `False`.
    pub is_deleted: bool,
    /// `is_archived` — `BooleanField`. Default: `False`.
    pub is_archived: bool,
    /// `external_id` — `CharField`, `max_length=255`, nullable.
    pub external_id: Option<String>,
    /// `external_source` — `CharField`, `max_length=255`, nullable.
    pub external_source: Option<String>,
    /// `size` — `FloatField`. Default: `0`.
    pub size: f64,
    /// `is_uploaded` — `BooleanField`. Default: `False`.
    pub is_uploaded: bool,
    /// `storage_metadata` — `JSONField`, nullable. Default: `builtins.dict`.
    pub storage_metadata: Option<JsonText>,
}

impl FileAsset {
    /// The table this row is read from (`file_assets`).
    pub const TABLE: &'static str = "file_assets";

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
        ", {alias}.\"attributes\"",
        ", {alias}.\"asset\"",
        ", {alias}.\"user_id\"",
        ", {alias}.\"workspace_id\"",
        ", {alias}.\"draft_issue_id\"",
        ", {alias}.\"project_id\"",
        ", {alias}.\"issue_id\"",
        ", {alias}.\"comment_id\"",
        ", {alias}.\"page_id\"",
        ", {alias}.\"entity_type\"",
        ", {alias}.\"entity_identifier\"",
        ", {alias}.\"is_deleted\"",
        ", {alias}.\"is_archived\"",
        ", {alias}.\"external_id\"",
        ", {alias}.\"external_source\"",
        ", {alias}.\"size\"",
        ", {alias}.\"is_uploaded\"",
        ", {alias}.\"storage_metadata\"",
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
    /// Django WHERE: `("file_assets"."deleted_at" IS NULL)`.
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
            attributes: row.try_get(start + 6)?,
            asset: row.try_get(start + 7)?,
            user_id: row.try_get(start + 8)?,
            workspace_id: row.try_get(start + 9)?,
            draft_issue_id: row.try_get(start + 10)?,
            project_id: row.try_get(start + 11)?,
            issue_id: row.try_get(start + 12)?,
            comment_id: row.try_get(start + 13)?,
            page_id: row.try_get(start + 14)?,
            entity_type: row.try_get(start + 15)?,
            entity_identifier: row.try_get(start + 16)?,
            is_deleted: row.try_get(start + 17)?,
            is_archived: row.try_get(start + 18)?,
            external_id: row.try_get(start + 19)?,
            external_source: row.try_get(start + 20)?,
            size: row.try_get(start + 21)?,
            is_uploaded: row.try_get(start + 22)?,
            storage_metadata: row.try_get(start + 23)?,
        })
    }
}
