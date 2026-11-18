// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Static statements of the v1 module routes (`plane/api/views/module.py`).
//!
//! `ModuleListCreateAPIEndpoint.get_queryset` (`module.py:88-174`) is the same
//! shape as the cycle one with three differences that change the body or the
//! row set:
//!
//! * the counts use `COUNT(DISTINCT module_issues.id)`;
//! * there is **no** `project_members` join — the queryset is scoped by
//!   `project_id`/`workspace__slug` only and `ProjectEntityPermission` does the
//!   membership check;
//! * `members` is a separate `users INNER JOIN module_members` query ordered by
//!   `users.created_at DESC` (the target model's `Meta.ordering`), and it does
//!   **not** filter `module_members.deleted_at`.
//!
//! The module-issue lists are `ModuleIssue.objects` (soft delete only, so a
//! soft-deleted issue still matches) plus the archived-project filter the view
//! adds (`module.py:611`).

use plane_db::Query;
use plane_db::catalog::{Column, PgType};

/// The 22 `modules` columns `ModuleSerializer` renders, in its field order
/// (id first because `BaseSerializer` declares it, then the model fields, and
/// `members` is appended by `to_representation`).
macro_rules! module_select {
    () => {
        "SELECT modules.id, COUNT(DISTINCT module_issues.id) FILTER (WHERE \
             module_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
             AND NOT issues.is_draft) AS total_issues, \
         COUNT(DISTINCT module_issues.id) FILTER (WHERE \
             module_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
             AND NOT issues.is_draft AND states.\"group\" = 'cancelled') \
             AS cancelled_issues, \
         COUNT(DISTINCT module_issues.id) FILTER (WHERE \
             module_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
             AND NOT issues.is_draft AND states.\"group\" = 'completed') \
             AS completed_issues, \
         COUNT(DISTINCT module_issues.id) FILTER (WHERE \
             module_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
             AND NOT issues.is_draft AND states.\"group\" = 'started') \
             AS started_issues, \
         COUNT(DISTINCT module_issues.id) FILTER (WHERE \
             module_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
             AND NOT issues.is_draft AND states.\"group\" = 'unstarted') \
             AS unstarted_issues, \
         COUNT(DISTINCT module_issues.id) FILTER (WHERE \
             module_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
             AND NOT issues.is_draft AND states.\"group\" = 'backlog') \
             AS backlog_issues, \
         modules.created_at, modules.updated_at, modules.deleted_at, \
         modules.name, modules.description, modules.description_text, \
         modules.description_html, modules.start_date, modules.target_date, \
         modules.status, modules.view_props, modules.sort_order, \
         modules.external_source, modules.external_id, modules.archived_at, \
         modules.logo_props, modules.created_by_id, modules.updated_by_id, \
         modules.project_id, modules.workspace_id, modules.lead_id"
    };
}

/// The counts-then-columns row shape of the read statements (42 columns).
static MODULE_READ_COLUMNS: [Column; 28] = [
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "total_issues",
        pg_type: PgType::Int8,
    },
    Column {
        name: "cancelled_issues",
        pg_type: PgType::Int8,
    },
    Column {
        name: "completed_issues",
        pg_type: PgType::Int8,
    },
    Column {
        name: "started_issues",
        pg_type: PgType::Int8,
    },
    Column {
        name: "unstarted_issues",
        pg_type: PgType::Int8,
    },
    Column {
        name: "backlog_issues",
        pg_type: PgType::Int8,
    },
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_text",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "start_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "target_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "status",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "view_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "lead_id",
        pg_type: PgType::Uuid,
    },
];

/// The bare-row shape (22 columns) shared by the write re-reads.
static MODULE_ROW_COLUMNS: [Column; 22] = [
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_text",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "start_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "target_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "status",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "view_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "lead_id",
        pg_type: PgType::Uuid,
    },
];

/// A statement whose result columns are one of the shared lists above.
macro_rules! module_query {
    (
        $(#[$meta:meta])*
        $ident:ident = $name:literal,
        $sql:expr,
        $columns:ident
    ) => {
        $(#[$meta])*
        pub static $ident: Query = Query {
            name: $name,
            sql: $sql,
            columns: &$columns,
        };
    };
}

/// The joins and filters the three read statements share.
macro_rules! module_from {
    () => {
        " FROM modules \
         INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
         LEFT OUTER JOIN module_issues ON modules.id = module_issues.module_id \
         LEFT OUTER JOIN issues ON module_issues.issue_id = issues.id \
         LEFT OUTER JOIN states ON issues.state_id = states.id \
         WHERE modules.deleted_at IS NULL AND modules.project_id = $2 \
           AND workspaces.slug = $1"
    };
}

module_query! {
    /// `ModuleListCreateAPIEndpoint.get` (`module.py:267-279`):
    /// `archived_at IS NULL`, `ORDER BY modules.created_at DESC`.
    MODULE_LIST = "v1_modules_list",
    concat!(module_select!(), module_from!(),
            " AND modules.archived_at IS NULL GROUP BY modules.id \
             ORDER BY modules.created_at DESC"),
    MODULE_READ_COLUMNS
}

module_query! {
    /// `ModuleDetailAPIEndpoint.get` (`module.py:519-526`): `$3` the module id.
    MODULE_DETAIL = "v1_modules_detail",
    concat!(module_select!(), module_from!(),
            " AND modules.archived_at IS NULL AND modules.id = $3 \
             GROUP BY modules.id ORDER BY modules.created_at DESC"),
    MODULE_READ_COLUMNS
}

module_query! {
    /// `ModuleArchiveUnarchiveAPIEndpoint.get` (`module.py:956-1079`).
    MODULE_ARCHIVED_LIST = "v1_modules_archived_list",
    concat!(module_select!(), module_from!(),
            " AND modules.archived_at IS NOT NULL GROUP BY modules.id \
             ORDER BY modules.created_at DESC"),
    MODULE_READ_COLUMNS
}

module_query! {
    /// `Module.objects.get(pk, project_id, workspace__slug)` — the write
    /// lookups (`module.py:459`, `:547`, `:728`, `:1101`, `:1135`).
    MODULE_ROW = "v1_modules_row",
    "SELECT modules.id, modules.created_at, modules.updated_at, \
         modules.deleted_at, modules.name, modules.description, \
         modules.description_text, modules.description_html, \
         modules.start_date, modules.target_date, modules.status, \
         modules.view_props, modules.sort_order, modules.external_source, \
         modules.external_id, modules.archived_at, modules.logo_props, \
         modules.created_by_id, modules.updated_by_id, modules.project_id, \
         modules.workspace_id, modules.lead_id \
     FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     WHERE modules.deleted_at IS NULL AND workspaces.slug = $1 \
       AND modules.project_id = $2 AND modules.id = $3",
    MODULE_ROW_COLUMNS
}

module_query! {
    /// `Module.objects.get(pk=…)` after a write (`module.py:241`).
    MODULE_BY_PK = "v1_modules_by_pk",
    "SELECT modules.id, modules.created_at, modules.updated_at, \
         modules.deleted_at, modules.name, modules.description, \
         modules.description_text, modules.description_html, \
         modules.start_date, modules.target_date, modules.status, \
         modules.view_props, modules.sort_order, modules.external_source, \
         modules.external_id, modules.archived_at, modules.logo_props, \
         modules.created_by_id, modules.updated_by_id, modules.project_id, \
         modules.workspace_id, modules.lead_id \
     FROM modules WHERE modules.id = $1",
    MODULE_ROW_COLUMNS
}

plane_db::plane_query! {
    /// `Project.objects.get(pk=project_id, workspace__slug=slug)` — 404 on a
    /// miss (`module.py:201`).
    MODULE_PROJECT = "v1_modules_project",
    "SELECT projects.id, projects.module_view, projects.workspace_id \
     FROM projects \
     INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
     WHERE projects.deleted_at IS NULL AND projects.id = $1 \
       AND workspaces.slug = $2",
    [("id", Uuid), ("module_view", Bool), ("workspace_id", Uuid)]
}

plane_db::plane_query! {
    /// `Project.objects.get(id=project_id)` inside `ModuleCreateSerializer
    /// .validate` (`serializers/module.py:64`).
    MODULE_PROJECT_BY_ID = "v1_modules_project_by_id",
    "SELECT projects.id, projects.module_view, projects.workspace_id \
     FROM projects WHERE projects.id = $1 AND projects.deleted_at IS NULL",
    [("id", Uuid), ("module_view", Bool), ("workspace_id", Uuid)]
}

plane_db::plane_query! {
    /// `Module.objects.filter(name=…, project_id=…).first()` — the duplicate
    /// name check on create (`serializers/module.py:92`).
    MODULE_BY_NAME = "v1_modules_by_name",
    "SELECT modules.id FROM modules \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $1 \
       AND modules.name = $2 ORDER BY modules.created_at DESC LIMIT 1",
    [("id", Uuid)]
}

plane_db::plane_query! {
    /// The PATCH duplicate check (`serializers/module.py:145`):
    /// `filter(name, project).exclude(id=instance).exists()`.
    MODULE_NAME_TAKEN = "v1_modules_name_taken",
    "SELECT 1 FROM modules \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $1 \
       AND modules.name = $2 AND modules.id <> $3 LIMIT 1",
    [("?column?", Int4)]
}

plane_db::plane_query! {
    /// `Module.objects.filter(project_id, workspace__slug, external_source,
    /// external_id).first()` (`module.py:210-222`, `:473-478`).
    MODULE_EXTERNAL_ID = "v1_modules_external_id",
    "SELECT modules.id FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $1 \
       AND workspaces.slug = $2 AND modules.external_source = $3 \
       AND modules.external_id = $4 \
     ORDER BY modules.created_at DESC LIMIT 1",
    [("id", Uuid)]
}

plane_db::plane_query! {
    /// `Module.objects.filter(project=self.project).aggregate(Min("sort_order"))`
    /// (`db/models/module.py:115-124`).
    MODULE_MIN_SORT_ORDER = "v1_modules_min_sort_order",
    "SELECT MIN(modules.sort_order) FROM modules \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $1",
    [("min", Float8)]
}

plane_db::plane_query! {
    /// `Module.objects.create(**validated_data, project_id=…)`
    /// (`serializers/module.py:103`).
    ///
    /// `status` defaults to `planned` and the two `jsonb` columns to `{}`
    /// (both `NOT NULL`, so they are written explicitly); `description_text`
    /// and `description_html` are nullable and left out.
    ///
    /// Bind order: id, created_at, updated_at, created_by, project, workspace,
    /// name, description, start_date, target_date, status, lead, sort_order,
    /// external_source, external_id.
    MODULE_INSERT = "v1_modules_insert",
    "INSERT INTO modules (id, created_at, updated_at, created_by_id, \
         updated_by_id, deleted_at, project_id, workspace_id, name, description, \
         description_text, description_html, start_date, target_date, status, \
         lead_id, view_props, sort_order, external_source, external_id, \
         archived_at, logo_props) \
     VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, NULL, NULL, $9, $10, \
             $11, $12, '{}'::jsonb, $13, $14, $15, NULL, '{}'::jsonb)",
    []
}

plane_db::plane_query! {
    /// The PATCH write: the seven optional fields plus audit columns.
    ///
    /// Bind order: name, description, start_date, target_date, status, lead,
    /// external_source, external_id, updated_at, updated_by, id.
    MODULE_UPDATE = "v1_modules_update",
    "UPDATE modules SET name = $1, description = $2, start_date = $3, \
         target_date = $4, status = $5, lead_id = $6, external_source = $7, \
         external_id = $8, updated_at = $9, updated_by_id = $10 \
     WHERE modules.id = $11",
    []
}

plane_db::plane_query! {
    /// `Module.members.all()` (`db/models/module.py:145-150`): the target
    /// model's `Meta.ordering` (`users.created_at DESC`), no
    /// `module_members.deleted_at` predicate.
    MODULE_MEMBER_IDS = "v1_modules_member_ids",
    "SELECT users.id FROM users \
     INNER JOIN module_members ON (users.id = module_members.member_id) \
     WHERE module_members.module_id = $1 ORDER BY users.created_at DESC",
    [("id", Uuid)]
}

plane_db::plane_query! {
    /// `ModuleMember.objects.filter(module=instance).delete()` — a soft delete
    /// of every through row (`serializers/module.py:149`).
    MODULE_MEMBERS_SOFT_DELETE = "v1_modules_members_soft_delete",
    "UPDATE module_members SET deleted_at = $1 \
     WHERE module_members.module_id = $2 AND module_members.deleted_at IS NULL",
    []
}

plane_db::plane_query! {
    /// `ModuleMember.objects.bulk_create([...], ignore_conflicts=True)`
    /// (`serializers/module.py:105-119`, `:150-164`). One row per member.
    ///
    /// Bind order: id, created_at, updated_at, created_by, updated_by, project,
    /// workspace, module, member.
    MODULE_MEMBER_INSERT = "v1_modules_member_insert",
    "INSERT INTO module_members (id, created_at, updated_at, created_by_id, \
         updated_by_id, project_id, workspace_id, module_id, member_id) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
     ON CONFLICT DO NOTHING",
    []
}

plane_db::plane_query! {
    /// `ProjectMember.objects.filter(project_id=…, member_id__in=…)
    /// .values_list("member_id", flat=True)` — `ModuleCreateSerializer
    /// .validate`'s member filter (`serializers/module.py:76-79`).
    MODULE_PROJECT_MEMBERS = "v1_modules_project_members",
    "SELECT project_members.member_id FROM project_members \
     WHERE project_members.deleted_at IS NULL AND project_members.project_id = $1 \
       AND project_members.member_id = ANY($2) \
     ORDER BY project_members.created_at DESC",
    [("member_id", Uuid)]
}

plane_db::plane_query! {
    /// `ModuleIssue.objects.filter(module_id=pk).values_list("issue",
    /// flat=True)` (`module.py:562`).
    MODULE_ISSUE_IDS = "v1_modules_issue_ids",
    "SELECT module_issues.issue_id FROM module_issues \
     WHERE module_issues.module_id = $1 \
     ORDER BY module_issues.created_at DESC",
    [("issue_id", Uuid)]
}

plane_db::plane_query! {
    /// `module.delete()`'s soft delete (`db/mixins.py:72-80`).
    MODULE_SOFT_DELETE = "v1_modules_soft_delete",
    "UPDATE modules SET deleted_at = $1, updated_at = $2, updated_by_id = $3 \
     WHERE modules.id = $4",
    []
}

plane_db::plane_query! {
    /// `ModuleIssue.objects.filter(module=pk, project_id=…).delete()` — the
    /// queryset soft delete, which writes only `deleted_at` (`module.py:581`).
    MODULE_ISSUES_SOFT_DELETE = "v1_modules_issues_soft_delete",
    "UPDATE module_issues SET deleted_at = $1 \
     WHERE module_issues.deleted_at IS NULL AND module_issues.module_id = $2 \
       AND module_issues.project_id = $3",
    []
}

plane_db::plane_query! {
    /// `UserFavorite.objects.filter(entity_type="module", entity_identifier=pk,
    /// project_id=…).delete()` (`module.py:583`, `:1109-1114`), newest shape
    /// with the workspace join for the archive POST.
    MODULE_FAVORITE_SOFT_DELETE_WS = "v1_modules_favorite_soft_delete_ws",
    "UPDATE user_favorites SET deleted_at = $1 \
     FROM workspaces \
     WHERE workspaces.id = user_favorites.workspace_id \
       AND user_favorites.deleted_at IS NULL \
       AND user_favorites.entity_type = 'module' \
       AND user_favorites.entity_identifier = $2 \
       AND user_favorites.project_id = $3 AND workspaces.slug = $4",
    []
}

plane_db::plane_query! {
    /// `Module.objects.get(pk, project_id, workspace__slug)` plus `status` and
    /// `archived_at` for the archive pair (`module.py:1101`, `:1135`).
    MODULE_ARCHIVE_LOOKUP = "v1_modules_archive_lookup",
    "SELECT modules.status, modules.archived_at FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     WHERE modules.deleted_at IS NULL AND modules.id = $1 \
       AND modules.project_id = $2 AND workspaces.slug = $3",
    [("status", Varchar), ("archived_at", Timestamptz)]
}

plane_db::plane_query! {
    /// `module.archived_at = timezone.now(); module.save()` (`module.py:1107`).
    MODULE_SET_ARCHIVED = "v1_modules_set_archived",
    "UPDATE modules SET archived_at = $1, updated_at = $2, updated_by_id = $3 \
     WHERE modules.id = $4",
    []
}

plane_db::plane_query! {
    /// `module.archived_at = None; module.save()` (`module.py:1136`).
    MODULE_CLEAR_ARCHIVED = "v1_modules_clear_archived",
    "UPDATE modules SET archived_at = NULL, updated_at = $1, updated_by_id = $2 \
     WHERE modules.id = $3",
    []
}

plane_db::plane_query! {
    /// The module DELETE's creator-or-project-admin check (`module.py:548-556`):
    /// `ProjectMember.objects.filter(workspace__slug, member, role=20,
    /// project_id, is_active).exists()`. Note it does **not** accept a
    /// workspace admin, unlike the cycle delete.
    MODULE_PROJECT_ADMIN = "v1_modules_project_admin",
    "SELECT 1 FROM project_members \
     INNER JOIN workspaces ON project_members.workspace_id = workspaces.id \
     WHERE workspaces.slug = $1 AND project_members.member_id = $2 \
       AND project_members.project_id = $3 AND project_members.is_active \
       AND project_members.deleted_at IS NULL AND project_members.role = 20 \
     LIMIT 1",
    [("?column?", Int4)]
}

plane_db::plane_query! {
    /// `ModuleIssue.objects.get(workspace__slug, project_id, module_id,
    /// issue_id)` (`module.py:931`).
    MODULE_ISSUE_ROW = "v1_modules_issue_row",
    "SELECT module_issues.id, module_issues.created_at, \
         module_issues.updated_at, module_issues.deleted_at, \
         module_issues.created_by_id, module_issues.updated_by_id, \
         module_issues.project_id, module_issues.workspace_id, \
         module_issues.module_id, module_issues.issue_id \
     FROM module_issues \
     INNER JOIN workspaces ON module_issues.workspace_id = workspaces.id \
     WHERE module_issues.deleted_at IS NULL AND workspaces.slug = $1 \
       AND module_issues.project_id = $2 AND module_issues.module_id = $3 \
       AND module_issues.issue_id = $4",
    [("id", Uuid), ("created_at", Timestamptz), ("updated_at", Timestamptz),
     ("deleted_at", Timestamptz), ("created_by_id", Uuid), ("updated_by_id", Uuid),
     ("project_id", Uuid), ("workspace_id", Uuid), ("module_id", Uuid),
     ("issue_id", Uuid)]
}

// ---------------------------------------------------------------------------
// Module-issue bulk add (302 POST, 303 DELETE)
// ---------------------------------------------------------------------------

/// The `ModuleIssueSerializer` read row: id, `sub_issues_count`, then the model
/// columns in `_readable_fields` order.
static MODULE_ISSUE_READ_COLUMNS: [Column; 11] = [
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "sub_issues_count",
        pg_type: PgType::Int8,
    },
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "module_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
];

/// `Issue.objects.filter(workspace__slug=slug, project_id=project_id,
/// pk__in=issues).values_list("id", flat=True)` (`module.py:730-732`) — the
/// default manager, so drafts and archived work items are included, in
/// `Issue.Meta.ordering` (`-created_at`).
///
/// Bind order: `$1` the workspace slug, `$2` the project id, `$3` the candidate
/// ids.
pub static MODULE_ISSUE_NEW_IDS: Query = Query {
    name: "v1_modules_issue_new_ids",
    sql: "SELECT issues.id FROM issues \
          INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
          WHERE issues.deleted_at IS NULL AND workspaces.slug = $1 \
            AND issues.project_id = $2 AND issues.id = ANY($3) \
          ORDER BY issues.created_at DESC",
    columns: &[Column {
        name: "id",
        pg_type: PgType::Uuid,
    }],
};

plane_db::plane_query! {
    /// One real row of the bulk add (`module.py:755-764`): the constructor sets
    /// both audit columns to the request user, and `ignore_conflicts=True`
    /// turns a duplicate of the same module into a no-op.
    ///
    /// Bind order: id, created_at, updated_at, created_by, updated_by, project,
    /// workspace, module, issue.
    MODULE_ISSUE_INSERT = "v1_modules_issue_insert",
    "INSERT INTO module_issues \
     (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
      project_id, workspace_id, module_id, issue_id) \
     VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9) \
     ON CONFLICT DO NOTHING",
    []
}

/// The `ModuleIssueSerializer(get_queryset(), many=True)` read of the bulk
/// add's 200 body (`module.py:596-620`, `:787-790`).
///
/// Bind order: `$1` workspace slug, `$2` project id, `$3` user id, `$4` module
/// id.
pub static MODULE_ISSUE_LIST_ROWS: Query = Query {
    name: "v1_modules_issue_list_rows",
    sql: "SELECT DISTINCT module_issues.id, \
          (SELECT COUNT(sub.id) FROM issues sub \
           WHERE (sub.deleted_at IS NULL) \
             AND (NOT (EXISTS (SELECT 1 FROM states WHERE states.id = sub.state_id \
                   AND states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL))) \
             AND (NOT ((sub.archived_at IS NOT NULL))) \
             AND (NOT (EXISTS (SELECT 1 FROM projects WHERE projects.id = sub.project_id \
                   AND projects.archived_at IS NOT NULL))) \
             AND (NOT ((sub.is_draft))) \
             AND sub.parent_id = module_issues.issue_id) AS sub_issues_count, \
          module_issues.created_at, module_issues.updated_at, \
          module_issues.deleted_at, module_issues.created_by_id, \
          module_issues.updated_by_id, module_issues.project_id, \
          module_issues.workspace_id, module_issues.module_id, \
          module_issues.issue_id \
          FROM module_issues \
          INNER JOIN workspaces ON module_issues.workspace_id = workspaces.id \
          INNER JOIN projects ON projects.id = module_issues.project_id \
          INNER JOIN project_members \
              ON project_members.project_id = module_issues.project_id \
          WHERE module_issues.deleted_at IS NULL AND workspaces.slug = $1 \
            AND module_issues.project_id = $2 \
            AND project_members.member_id = $3 AND project_members.is_active \
            AND module_issues.module_id = $4 AND projects.archived_at IS NULL \
          ORDER BY module_issues.created_at DESC",
    columns: &MODULE_ISSUE_READ_COLUMNS,
};

/// `ModuleIssue.objects.get(workspace__slug, project_id, module_id, issue_id)`
/// (`module.py:931-936`), plus the joined `module.name` the activity copies
/// (`:938`).
///
/// Bind order: `$1` workspace slug, `$2` project id, `$3` module id, `$4` issue
/// id.
pub static MODULE_ISSUE_DELETE_ROW: Query = Query {
    name: "v1_modules_issue_delete_row",
    sql: "SELECT module_issues.id, module_issues.issue_id, modules.name \
          FROM module_issues \
          INNER JOIN workspaces ON module_issues.workspace_id = workspaces.id \
          LEFT OUTER JOIN modules ON modules.id = module_issues.module_id \
          WHERE module_issues.deleted_at IS NULL AND workspaces.slug = $1 \
            AND module_issues.project_id = $2 AND module_issues.module_id = $3 \
            AND module_issues.issue_id = $4",
    columns: &[
        Column {
            name: "id",
            pg_type: PgType::Uuid,
        },
        Column {
            name: "issue_id",
            pg_type: PgType::Uuid,
        },
        Column {
            name: "name",
            pg_type: PgType::Varchar,
        },
    ],
};

/// Every statement this file declares, for `tests/schema_check.rs`.
pub static ALL_QUERIES: &[&Query] = &[
    &MODULE_LIST,
    &MODULE_DETAIL,
    &MODULE_ARCHIVED_LIST,
    &MODULE_ROW,
    &MODULE_BY_PK,
    &MODULE_PROJECT,
    &MODULE_PROJECT_BY_ID,
    &MODULE_BY_NAME,
    &MODULE_NAME_TAKEN,
    &MODULE_EXTERNAL_ID,
    &MODULE_MIN_SORT_ORDER,
    &MODULE_INSERT,
    &MODULE_UPDATE,
    &MODULE_MEMBER_IDS,
    &MODULE_MEMBERS_SOFT_DELETE,
    &MODULE_MEMBER_INSERT,
    &MODULE_PROJECT_MEMBERS,
    &MODULE_ISSUE_IDS,
    &MODULE_SOFT_DELETE,
    &MODULE_ISSUES_SOFT_DELETE,
    &MODULE_FAVORITE_SOFT_DELETE_WS,
    &MODULE_ARCHIVE_LOOKUP,
    &MODULE_SET_ARCHIVED,
    &MODULE_CLEAR_ARCHIVED,
    &MODULE_PROJECT_ADMIN,
    &MODULE_ISSUE_ROW,
    &MODULE_ISSUE_NEW_IDS,
    &MODULE_ISSUE_INSERT,
    &MODULE_ISSUE_LIST_ROWS,
    &MODULE_ISSUE_DELETE_ROW,
];
