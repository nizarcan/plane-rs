// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Static statements of the v1 cycle routes (`plane/api/views/cycle.py`).
//!
//! The read statements reproduce the ORM shape of
//! `CycleListCreateAPIEndpoint.get_queryset` (`cycle.py:91-169`): a `cycles`
//! row joined to `project_members` for the membership filter, six
//! `COUNT(… ) FILTER (…)` annotations over a `LEFT OUTER JOIN` of
//! `cycle_issues → issues → states` and `ORDER BY cycles.created_at DESC`.
//!
//! The join does **not** filter `issues.deleted_at` — Django only applies a
//! manager's predicate to the queryset's own model, never to a join — so a
//! soft-deleted issue still counts towards `total_issues`. The counts use
//! `COUNT(cycle_issues.id)` / `COUNT(states."group")` exactly like the ORM
//! (the second skips rows without a state, the first does not).
//!
//! The archived list (`cycle.py:660-762`) adds the three estimate sums.
//! `total_estimates` has **no** `FILTER`, so it also sums soft-deleted
//! cycle-issue rows and archived/draft issues.
//!
//! Writes use `Cycle.objects` (soft delete only) and never `archived_at`.

use plane_db::Query;
use plane_db::catalog::{Column, PgType};

/// The model columns every read statement selects, in `CycleSerializer`'s
/// `_readable_fields` order plus the six counts.
static CYCLE_READ_COLUMNS: [Column; 28] = [
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
        name: "start_date",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "end_date",
        pg_type: PgType::Timestamptz,
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
        name: "progress_snapshot",
        pg_type: PgType::Jsonb,
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
        name: "timezone",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "version",
        pg_type: PgType::Int4,
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
        name: "owned_by_id",
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
];

/// The archived list selects the same columns plus the three estimate sums.
static CYCLE_ARCHIVED_COLUMNS: [Column; 31] = [
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
        name: "start_date",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "end_date",
        pg_type: PgType::Timestamptz,
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
        name: "progress_snapshot",
        pg_type: PgType::Jsonb,
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
        name: "timezone",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "version",
        pg_type: PgType::Int4,
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
        name: "owned_by_id",
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
        name: "total_estimates",
        pg_type: PgType::Int8,
    },
    Column {
        name: "completed_estimates",
        pg_type: PgType::Int8,
    },
    Column {
        name: "started_estimates",
        pg_type: PgType::Int8,
    },
];

/// The model columns of a bare `Cycle.objects` row, in serializer order.
static CYCLE_ROW_COLUMNS: [Column; 22] = [
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
        name: "start_date",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "end_date",
        pg_type: PgType::Timestamptz,
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
        name: "progress_snapshot",
        pg_type: PgType::Jsonb,
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
        name: "timezone",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "version",
        pg_type: PgType::Int4,
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
        name: "owned_by_id",
        pg_type: PgType::Uuid,
    },
];

/// A statement whose result columns are one of the shared column lists above.
macro_rules! cycle_query {
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

/// The `SELECT` list of a read statement: the 22 model columns then the six
/// counts, in serializer order.
macro_rules! cycle_select {
    () => {
        "SELECT cycles.id, cycles.created_at, cycles.updated_at, cycles.deleted_at, \
         cycles.name, cycles.description, cycles.start_date, cycles.end_date, \
         cycles.view_props, cycles.sort_order, cycles.external_source, \
         cycles.external_id, cycles.progress_snapshot, cycles.archived_at, \
         cycles.logo_props, cycles.timezone, cycles.version, cycles.created_by_id, \
         cycles.updated_by_id, cycles.project_id, cycles.workspace_id, \
         cycles.owned_by_id, \
         COUNT(cycle_issues.id) FILTER (WHERE cycle_issues.deleted_at IS NULL \
             AND issues.archived_at IS NULL AND NOT issues.is_draft) \
             AS total_issues, \
         COUNT(states.\"group\") FILTER (WHERE cycle_issues.deleted_at IS NULL \
             AND issues.archived_at IS NULL AND NOT issues.is_draft \
             AND states.\"group\" = 'cancelled') AS cancelled_issues, \
         COUNT(states.\"group\") FILTER (WHERE cycle_issues.deleted_at IS NULL \
             AND issues.archived_at IS NULL AND NOT issues.is_draft \
             AND states.\"group\" = 'completed') AS completed_issues, \
         COUNT(states.\"group\") FILTER (WHERE cycle_issues.deleted_at IS NULL \
             AND issues.archived_at IS NULL AND NOT issues.is_draft \
             AND states.\"group\" = 'started') AS started_issues, \
         COUNT(states.\"group\") FILTER (WHERE cycle_issues.deleted_at IS NULL \
             AND issues.archived_at IS NULL AND NOT issues.is_draft \
             AND states.\"group\" = 'unstarted') AS unstarted_issues, \
         COUNT(states.\"group\") FILTER (WHERE cycle_issues.deleted_at IS NULL \
             AND issues.archived_at IS NULL AND NOT issues.is_draft \
             AND states.\"group\" = 'backlog') AS backlog_issues"
    };
}

/// The joins and the membership filter every list/detail statement shares.
macro_rules! cycle_from {
    () => {
        "FROM cycles \
         INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
         INNER JOIN project_members ON cycles.project_id = project_members.project_id \
         LEFT OUTER JOIN cycle_issues ON cycles.id = cycle_issues.cycle_id \
         LEFT OUTER JOIN issues ON cycle_issues.issue_id = issues.id \
         LEFT OUTER JOIN states ON issues.state_id = states.id \
         WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
           AND cycles.project_id = $2 AND project_members.is_active \
           AND project_members.member_id = $3 \
           AND cycles.archived_at IS NULL"
    };
}

cycle_query! {
    /// `cycle_view=all` (`cycle.py:273-283`).
    CYCLE_LIST = "v1_cycles_list",
    concat!(cycle_select!(), " ", cycle_from!(),
            " GROUP BY cycles.id ORDER BY cycles.created_at DESC"),
    CYCLE_READ_COLUMNS
}

cycle_query! {
    /// `cycle_view=upcoming` (`cycle.py:215-227`).
    CYCLE_LIST_UPCOMING = "v1_cycles_list_upcoming",
    concat!(cycle_select!(), " ", cycle_from!(),
            " AND cycles.start_date > $4 GROUP BY cycles.id \
             ORDER BY cycles.created_at DESC"),
    CYCLE_READ_COLUMNS
}

cycle_query! {
    /// `cycle_view=completed` (`cycle.py:230-242`).
    CYCLE_LIST_COMPLETED = "v1_cycles_list_completed",
    concat!(cycle_select!(), " ", cycle_from!(),
            " AND cycles.end_date < $4 GROUP BY cycles.id \
             ORDER BY cycles.created_at DESC"),
    CYCLE_READ_COLUMNS
}

cycle_query! {
    /// `cycle_view=draft` (`cycle.py:245-257`).
    CYCLE_LIST_DRAFT = "v1_cycles_list_draft",
    concat!(cycle_select!(), " ", cycle_from!(),
            " AND cycles.end_date IS NULL AND cycles.start_date IS NULL \
             GROUP BY cycles.id ORDER BY cycles.created_at DESC"),
    CYCLE_READ_COLUMNS
}

cycle_query! {
    /// `cycle_view=incomplete` (`cycle.py:260-272`):
    /// `end_date >= now OR end_date IS NULL`.
    CYCLE_LIST_INCOMPLETE = "v1_cycles_list_incomplete",
    concat!(cycle_select!(), " ", cycle_from!(),
            " AND (cycles.end_date >= $4 OR cycles.end_date IS NULL) \
             GROUP BY cycles.id ORDER BY cycles.created_at DESC"),
    CYCLE_READ_COLUMNS
}

cycle_query! {
    /// `cycle_view=current` (`cycle.py:203-212`), the plain-array branch.
    CYCLE_LIST_CURRENT = "v1_cycles_list_current",
    concat!(cycle_select!(), " ", cycle_from!(),
            " AND cycles.start_date <= $4 AND cycles.end_date >= $4 \
             GROUP BY cycles.id ORDER BY cycles.created_at DESC"),
    CYCLE_READ_COLUMNS
}

cycle_query! {
    /// `CycleDetailAPIEndpoint.get` (`cycle.py:518`): `$4` the cycle id.
    CYCLE_DETAIL = "v1_cycles_detail",
    concat!(cycle_select!(), " ", cycle_from!(),
            " AND cycles.id = $4 GROUP BY cycles.id \
             ORDER BY cycles.created_at DESC"),
    CYCLE_READ_COLUMNS
}

cycle_query! {
    /// `CycleArchiveUnarchiveAPIEndpoint.get` (`cycle.py:660-762`):
    /// `archived_at IS NOT NULL` plus the three estimate sums. `$1` slug,
    /// `$2` project id, `$3` user id.
    CYCLE_ARCHIVED_LIST = "v1_cycles_archived_list",
    concat!(cycle_select!(),
            ", SUM(estimate_points.key) AS total_estimates, \
             SUM(estimate_points.key) FILTER (WHERE \
                 cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
                 AND NOT issues.is_draft AND states.\"group\" = 'completed') \
                 AS completed_estimates, \
             SUM(estimate_points.key) FILTER (WHERE \
                 cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
                 AND NOT issues.is_draft AND states.\"group\" = 'started') \
                 AS started_estimates \
             FROM cycles \
             INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
             INNER JOIN project_members \
                 ON cycles.project_id = project_members.project_id \
             LEFT OUTER JOIN cycle_issues ON cycles.id = cycle_issues.cycle_id \
             LEFT OUTER JOIN issues ON cycle_issues.issue_id = issues.id \
             LEFT OUTER JOIN states ON issues.state_id = states.id \
             LEFT OUTER JOIN estimate_points \
                 ON issues.estimate_point_id = estimate_points.id \
             WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
               AND cycles.project_id = $2 AND project_members.is_active \
               AND project_members.member_id = $3 \
               AND cycles.archived_at IS NOT NULL \
             GROUP BY cycles.id ORDER BY cycles.created_at DESC"),
    CYCLE_ARCHIVED_COLUMNS
}

cycle_query! {
    /// `Cycle.objects.get(workspace__slug, project_id, pk)` (`cycle.py:549`,
    /// `:623`, `:807`, `:838`): the bare row, without annotations. `$1` slug,
    /// `$2` project id, `$3` cycle id.
    CYCLE_ROW = "v1_cycles_row",
    "SELECT cycles.id, cycles.created_at, cycles.updated_at, cycles.deleted_at, \
         cycles.name, cycles.description, cycles.start_date, cycles.end_date, \
         cycles.view_props, cycles.sort_order, cycles.external_source, \
         cycles.external_id, cycles.progress_snapshot, cycles.archived_at, \
         cycles.logo_props, cycles.timezone, cycles.version, cycles.created_by_id, \
         cycles.updated_by_id, cycles.project_id, cycles.workspace_id, \
         cycles.owned_by_id \
     FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
       AND cycles.project_id = $2 AND cycles.id = $3",
    CYCLE_ROW_COLUMNS
}

cycle_query! {
    /// `Cycle.objects.get(pk=…)` after a write (`cycle.py:349`, `:604`).
    CYCLE_BY_PK = "v1_cycles_by_pk",
    "SELECT cycles.id, cycles.created_at, cycles.updated_at, cycles.deleted_at, \
         cycles.name, cycles.description, cycles.start_date, cycles.end_date, \
         cycles.view_props, cycles.sort_order, cycles.external_source, \
         cycles.external_id, cycles.progress_snapshot, cycles.archived_at, \
         cycles.logo_props, cycles.timezone, cycles.version, cycles.created_by_id, \
         cycles.updated_by_id, cycles.project_id, cycles.workspace_id, \
         cycles.owned_by_id \
     FROM cycles WHERE cycles.id = $1",
    CYCLE_ROW_COLUMNS
}

plane_db::plane_query! {
    /// `Project.objects.get(id=project_id)` in `CycleCreateSerializer.validate`
    /// (`serializers/cycle.py:71-75`), and the `CycleListCreateAPIEndpoint.get`
    /// context lookup (`cycle.py:198`) — the default manager, so a
    /// soft-deleted project is a 404.
    CYCLE_PROJECT = "v1_cycles_project",
    "SELECT projects.id, projects.cycle_view, projects.timezone, \
         projects.workspace_id \
     FROM projects WHERE projects.id = $1 AND projects.deleted_at IS NULL",
    [("id", Uuid), ("cycle_view", Bool), ("timezone", Varchar), ("workspace_id", Uuid)]
}

plane_db::plane_query! {
    /// `Cycle.objects.filter(project_id=…, workspace__slug=…,
    /// external_source=…, external_id=…).first()` — the duplicate check
    /// (`cycle.py:317-329`, `:578-583`), newest first.
    CYCLE_EXTERNAL_ID = "v1_cycles_external_id",
    "SELECT cycles.id FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     WHERE cycles.deleted_at IS NULL AND cycles.project_id = $1 \
       AND workspaces.slug = $2 AND cycles.external_source = $3 \
       AND cycles.external_id = $4 \
     ORDER BY cycles.created_at DESC LIMIT 1",
    [("id", Uuid)]
}

plane_db::plane_query! {
    /// `Cycle.objects.filter(project=project).aggregate(Min("sort_order"))`
    /// (`db/models/cycle.py:90-96`), soft delete only.
    CYCLE_MIN_SORT_ORDER = "v1_cycles_min_sort_order",
    "SELECT MIN(cycles.sort_order) FROM cycles \
     WHERE cycles.deleted_at IS NULL AND cycles.project_id = $1",
    [("min", Float8)]
}

plane_db::plane_query! {
    /// `Cycle.objects.create(**validated_data, project_id=…)`
    /// (`serializers/cycle.py:83-121` + `db/models/cycle.py:88-97`).
    ///
    /// Bind order: id, created_at, updated_at, created_by, project, workspace,
    /// name, description, start_date, end_date, owned_by, sort_order,
    /// external_source, external_id, timezone.
    CYCLE_INSERT = "v1_cycles_insert",
    "INSERT INTO cycles (id, created_at, updated_at, created_by_id, \
         project_id, workspace_id, name, description, start_date, end_date, \
         owned_by_id, sort_order, external_source, external_id, timezone) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    []
}

plane_db::plane_query! {
    /// `instance.save()` on PATCH: the serializer's validated fields plus
    /// `updated_at`/`updated_by` (`BaseModel.save`, `db/models/base.py:24-45`).
    ///
    /// Bind order: name, description, start_date, end_date, owned_by,
    /// external_source, external_id, timezone, updated_at, updated_by, id.
    CYCLE_UPDATE = "v1_cycles_update",
    "UPDATE cycles SET name = $1, description = $2, start_date = $3, \
         end_date = $4, owned_by_id = $5, external_source = $6, \
         external_id = $7, timezone = $8, updated_at = $9, updated_by_id = $10 \
     WHERE cycles.id = $11",
    []
}

plane_db::plane_query! {
    /// `CycleIssue.objects.filter(cycle_id=pk).values_list("issue",
    /// flat=True)` (`cycle.py:630`), in the through model's `-created_at`
    /// order.
    CYCLE_ISSUE_IDS = "v1_cycles_issue_ids",
    "SELECT cycle_issues.issue_id FROM cycle_issues \
     WHERE cycle_issues.cycle_id = $1 \
     ORDER BY cycle_issues.created_at DESC",
    [("issue_id", Uuid)]
}

plane_db::plane_query! {
    /// `cycle.delete()` (`db/mixins.py:72-80`): the soft delete writes
    /// `deleted_at` and re-saves the row (`auto_now` + `updated_by`).
    ///
    /// Bind order: deleted_at, updated_at, updated_by, id.
    CYCLE_SOFT_DELETE = "v1_cycles_soft_delete",
    "UPDATE cycles SET deleted_at = $1, updated_at = $2, updated_by_id = $3 \
     WHERE cycles.id = $4",
    []
}

plane_db::plane_query! {
    /// `UserFavorite.objects.filter(entity_type="cycle", entity_identifier=pk,
    /// project_id=…).delete()` — `SoftDeletionQuerySet.delete` (`cycle.py:650`,
    /// `:815-820`). Bind order: deleted_at, entity id, project id.
    CYCLE_FAVORITE_SOFT_DELETE_ALL = "v1_cycles_favorite_soft_delete_all",
    "UPDATE user_favorites SET deleted_at = $1 \
     WHERE user_favorites.deleted_at IS NULL \
       AND user_favorites.entity_type = 'cycle' \
       AND user_favorites.entity_identifier = $2 \
       AND user_favorites.project_id = $3",
    []
}

plane_db::plane_query! {
    /// The archive POST's favourite cleanup
    /// (`cycle.py:815-820`), which also joins `workspace__slug`.
    ///
    /// Bind order: deleted_at, entity id, project id, slug.
    CYCLE_FAVORITE_SOFT_DELETE_WS = "v1_cycles_favorite_soft_delete_ws",
    "UPDATE user_favorites SET deleted_at = $1 \
     FROM workspaces \
     WHERE workspaces.id = user_favorites.workspace_id \
       AND user_favorites.deleted_at IS NULL \
       AND user_favorites.entity_type = 'cycle' \
       AND user_favorites.entity_identifier = $2 \
       AND user_favorites.project_id = $3 AND workspaces.slug = $4",
    []
}

plane_db::plane_query! {
    /// The archive POST's row lookup (`cycle.py:807`) and the unarchive
    /// DELETE's (`cycle.py:838`).
    CYCLE_ARCHIVE_LOOKUP = "v1_cycles_archive_lookup",
    "SELECT cycles.end_date, cycles.archived_at FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     WHERE cycles.deleted_at IS NULL AND cycles.id = $1 \
       AND cycles.project_id = $2 AND workspaces.slug = $3",
    [("end_date", Timestamptz), ("archived_at", Timestamptz)]
}

plane_db::plane_query! {
    /// `cycle.archived_at = timezone.now(); cycle.save()` (`cycle.py:813-814`).
    CYCLE_SET_ARCHIVED = "v1_cycles_set_archived",
    "UPDATE cycles SET archived_at = $1, updated_at = $2, updated_by_id = $3 \
     WHERE cycles.id = $4",
    []
}

plane_db::plane_query! {
    /// `cycle.archived_at = None; cycle.save()` (`cycle.py:839-840`).
    CYCLE_CLEAR_ARCHIVED = "v1_cycles_clear_archived",
    "UPDATE cycles SET archived_at = NULL, updated_at = $1, updated_by_id = $2 \
     WHERE cycles.id = $3",
    []
}

plane_db::plane_query! {
    /// `ProjectAdminPermission.has_permission`'s project-admin branch
    /// (`plane/utils/permissions/project.py:131-138`).
    CYCLE_PROJECT_ADMIN = "v1_cycles_project_admin",
    "SELECT 1 FROM project_members \
     INNER JOIN workspaces ON project_members.workspace_id = workspaces.id \
     WHERE workspaces.slug = $1 AND project_members.member_id = $2 \
       AND project_members.project_id = $3 AND project_members.is_active \
       AND project_members.deleted_at IS NULL AND project_members.role = 20 \
     LIMIT 1",
    [("?column?", Int4)]
}

plane_db::plane_query! {
    /// The workspace-admin half of `ProjectAdminPermission`
    /// (`plane/utils/permissions/project.py:140-148`).
    CYCLE_WORKSPACE_ADMIN = "v1_cycles_workspace_admin",
    "SELECT 1 FROM workspace_members \
     INNER JOIN workspaces ON workspace_members.workspace_id = workspaces.id \
     WHERE workspaces.slug = $1 AND workspace_members.member_id = $2 \
       AND workspace_members.is_active \
       AND workspace_members.deleted_at IS NULL \
       AND workspace_members.role = 20 LIMIT 1",
    [("?column?", Int4)]
}

/// Every statement this file declares, for `tests/schema_check.rs`.
pub static ALL_QUERIES: &[&Query] = &[
    &CYCLE_LIST,
    &CYCLE_LIST_UPCOMING,
    &CYCLE_LIST_COMPLETED,
    &CYCLE_LIST_DRAFT,
    &CYCLE_LIST_INCOMPLETE,
    &CYCLE_LIST_CURRENT,
    &CYCLE_DETAIL,
    &CYCLE_ARCHIVED_LIST,
    &CYCLE_ROW,
    &CYCLE_BY_PK,
    &CYCLE_PROJECT,
    &CYCLE_EXTERNAL_ID,
    &CYCLE_MIN_SORT_ORDER,
    &CYCLE_INSERT,
    &CYCLE_UPDATE,
    &CYCLE_ISSUE_IDS,
    &CYCLE_SOFT_DELETE,
    &CYCLE_FAVORITE_SOFT_DELETE_ALL,
    &CYCLE_FAVORITE_SOFT_DELETE_WS,
    &CYCLE_ARCHIVE_LOOKUP,
    &CYCLE_SET_ARCHIVED,
    &CYCLE_CLEAR_ARCHIVED,
    &CYCLE_PROJECT_ADMIN,
    &CYCLE_WORKSPACE_ADMIN,
];
