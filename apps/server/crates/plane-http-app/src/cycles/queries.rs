// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `cycles` group.
//!
//! Every statement here is a fixed shape whose only variation is its bound
//! parameters, so it is cached per connection (`DECISIONS.md` D1,
//! `HANDLERS.md` §2). The statements reproduce the ORM queries of
//!
//! * `plane/app/views/cycle/base.py` — `CycleViewSet`, `CycleUserPropertiesEndpoint`,
//!   `CycleProgressEndpoint`, `CycleAnalyticsEndpoint`;
//! * `plane/app/views/cycle/archive.py` — `CycleArchiveUnarchiveEndpoint`;
//! * `plane/app/views/workspace/cycle.py` — `WorkspaceCyclesEndpoint`.
//!
//! The three `Cycle` statements select the same annotation set Django's
//! `get_queryset()` builds (`base.py:70-182`): the favourite `EXISTS`, the four
//! `COUNT(DISTINCT cycle_issues.issue_id) FILTER (…)` groups and the
//! `ARRAY_AGG` of live assignees. `COUNT(… FILTER (…))` returns `0` for an
//! empty group, `ARRAY_AGG` is `COALESCE`d to `{}` — both as Django emits them.
//!
//! `GROUP BY cycles.id` replaces Django's `GROUP BY <every selected ordinal>`:
//! the primary key is a functional dependency, so the result is the same row
//! set, and the trailing `created_at` column exists only because `SELECT
//! DISTINCT` requires every `ORDER BY` expression in its select list (Django
//! appends it the same way, as an extra unprojected column).

use plane_db::catalog::{ArrayElement, PgType};
use plane_db::{Column, Query};

/// `plane_query!` with an explicit `PgType` expression, because the shared
/// macro's `$ty:ident` cannot spell an array result column
/// (`PgType::ArrayOf(ArrayElement::Uuid)` for `assignee_ids`). The catalog and
/// its `schema_check` are otherwise identical.
macro_rules! catalog_query {
    (
        $(#[$meta:meta])*
        $ident:ident = $name:literal,
        $sql:literal,
        [ $( ($column:literal, $ty:expr) ),* $(,)? ]
        $(,)?
    ) => {
        $(#[$meta])*
        pub static $ident: Query = Query {
            name: $name,
            sql: $sql,
            columns: &[
                $(
                    Column {
                        name: $column,
                        pg_type: $ty,
                    },
                )*
            ],
        };
    };
}

catalog_query! {
    /// `Project.objects.get(id=project_id)` — only the timezone is read
    /// (`cycle/base.py:79-86`). The manager predicate applies, so a
    /// soft-deleted project is a 404 `{"error": "The required object does not
    /// exist."}` exactly as the Python `DoesNotExist` renders.
    CYCLE_PROJECT_TIMEZONE = "cycles_project_timezone",
    "SELECT projects.timezone FROM projects \
     WHERE projects.id = $1 AND projects.deleted_at IS NULL LIMIT 1",
    [("timezone", PgType::Varchar)]
}

catalog_query! {
    /// `CycleViewSet.list` (`cycle/base.py:185-269`): the annotated queryset
    /// with `archived_at IS NULL`, projected through `.values()` in the order
    /// the Python passes, ordered by `-is_favorite, -created_at`.
    ///
    /// `$1` slug, `$2` project id, `$3` user id, `$4` the CASE instant
    /// (`timezone.now()`; the Python round-trips it through the project
    /// timezone but that is the same instant).
    CYCLE_LIST = "cycles_list",
    "SELECT DISTINCT cycles.id AS id, cycles.workspace_id AS workspace_id, \
     cycles.project_id AS project_id, cycles.name AS name, \
     cycles.description AS description, cycles.start_date AS start_date, \
     cycles.end_date AS end_date, cycles.owned_by_id AS owned_by_id, \
     cycles.view_props AS view_props, cycles.sort_order AS sort_order, \
     cycles.external_source AS external_source, cycles.external_id AS external_id, \
     cycles.progress_snapshot AS progress_snapshot, cycles.logo_props AS logo_props, \
     EXISTS (SELECT 1 FROM user_favorites uf \
             INNER JOIN workspaces ufw ON uf.workspace_id = ufw.id \
             WHERE uf.deleted_at IS NULL AND uf.entity_identifier = cycles.id \
             AND uf.entity_type = 'cycle' AND uf.project_id = $2 \
             AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'completed') AS completed_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" IN ('cancelled')) AS cancelled_issues, \
     COALESCE(ARRAY_AGG(DISTINCT issue_assignees.assignee_id) FILTER (WHERE \
         issue_assignees.assignee_id IS NOT NULL \
         AND issue_assignees.deleted_at IS NULL), ARRAY[]::uuid[]) AS assignee_ids, \
     CASE WHEN cycles.start_date <= $4 AND cycles.end_date >= $4 THEN 'CURRENT' \
          WHEN cycles.start_date > $4 THEN 'UPCOMING' \
          WHEN cycles.end_date < $4 THEN 'COMPLETED' \
          WHEN cycles.start_date IS NULL AND cycles.end_date IS NULL THEN 'DRAFT' \
          ELSE 'DRAFT' END AS status, \
     cycles.version AS version, cycles.created_by_id AS created_by, \
     cycles.created_at AS created_at \
     FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN projects ON cycles.project_id = projects.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     LEFT OUTER JOIN cycle_issues ON cycles.id = cycle_issues.cycle_id \
     LEFT OUTER JOIN issues ON cycle_issues.issue_id = issues.id \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
     AND cycles.project_id = $2 AND project_members.is_active \
     AND project_members.member_id = $3 AND projects.archived_at IS NULL \
     AND cycles.archived_at IS NULL \
     GROUP BY cycles.id ORDER BY is_favorite DESC, cycles.created_at DESC",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("start_date", PgType::Timestamptz),
        ("end_date", PgType::Timestamptz),
        ("owned_by_id", PgType::Uuid),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("progress_snapshot", PgType::Jsonb),
        ("logo_props", PgType::Jsonb),
        ("is_favorite", PgType::Bool),
        ("total_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("cancelled_issues", PgType::Int8),
        ("assignee_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("status", PgType::Text),
        ("version", PgType::Int4),
        ("created_by", PgType::Uuid),
        ("created_at", PgType::Timestamptz),
    ]
}

catalog_query! {
    /// `CycleViewSet.list`'s `cycle_view=current` first attempt
    /// (`cycle/base.py:205-238`): the same projection with
    /// `start_date <= $5 AND end_date >= $5`. When it returns no row the view
    /// runs [`CYCLE_LIST`] again (the Python falls through), which the handler
    /// reproduces.
    CYCLE_LIST_CURRENT = "cycles_list_current",
    "SELECT DISTINCT cycles.id AS id, cycles.workspace_id AS workspace_id, \
     cycles.project_id AS project_id, cycles.name AS name, \
     cycles.description AS description, cycles.start_date AS start_date, \
     cycles.end_date AS end_date, cycles.owned_by_id AS owned_by_id, \
     cycles.view_props AS view_props, cycles.sort_order AS sort_order, \
     cycles.external_source AS external_source, cycles.external_id AS external_id, \
     cycles.progress_snapshot AS progress_snapshot, cycles.logo_props AS logo_props, \
     EXISTS (SELECT 1 FROM user_favorites uf \
             INNER JOIN workspaces ufw ON uf.workspace_id = ufw.id \
             WHERE uf.deleted_at IS NULL AND uf.entity_identifier = cycles.id \
             AND uf.entity_type = 'cycle' AND uf.project_id = $2 \
             AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'completed') AS completed_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" IN ('cancelled')) AS cancelled_issues, \
     COALESCE(ARRAY_AGG(DISTINCT issue_assignees.assignee_id) FILTER (WHERE \
         issue_assignees.assignee_id IS NOT NULL \
         AND issue_assignees.deleted_at IS NULL), ARRAY[]::uuid[]) AS assignee_ids, \
     CASE WHEN cycles.start_date <= $4 AND cycles.end_date >= $4 THEN 'CURRENT' \
          WHEN cycles.start_date > $4 THEN 'UPCOMING' \
          WHEN cycles.end_date < $4 THEN 'COMPLETED' \
          WHEN cycles.start_date IS NULL AND cycles.end_date IS NULL THEN 'DRAFT' \
          ELSE 'DRAFT' END AS status, \
     cycles.version AS version, cycles.created_by_id AS created_by, \
     cycles.created_at AS created_at \
     FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN projects ON cycles.project_id = projects.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     LEFT OUTER JOIN cycle_issues ON cycles.id = cycle_issues.cycle_id \
     LEFT OUTER JOIN issues ON cycle_issues.issue_id = issues.id \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
     AND cycles.project_id = $2 AND project_members.is_active \
     AND project_members.member_id = $3 AND projects.archived_at IS NULL \
     AND cycles.archived_at IS NULL AND cycles.start_date <= $5 \
     AND cycles.end_date >= $5 \
     GROUP BY cycles.id ORDER BY is_favorite DESC, cycles.created_at DESC",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("start_date", PgType::Timestamptz),
        ("end_date", PgType::Timestamptz),
        ("owned_by_id", PgType::Uuid),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("progress_snapshot", PgType::Jsonb),
        ("logo_props", PgType::Jsonb),
        ("is_favorite", PgType::Bool),
        ("total_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("cancelled_issues", PgType::Int8),
        ("assignee_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("status", PgType::Text),
        ("version", PgType::Int4),
        ("created_by", PgType::Uuid),
        ("created_at", PgType::Timestamptz),
    ]
}

catalog_query! {
    /// `CycleViewSet.retrieve` (`cycle/base.py:412-476`): [`CYCLE_LIST`]'s
    /// projection with `sub_issues` injected after `progress_snapshot` and
    /// `pk = $5`. `sub_issues` is the uncorrelated scalar subquery
    /// `Issue.issue_objects.filter(parent__isnull=False, issue_cycle__cycle_id=pk)`
    /// counted (`base.py:419-428`).
    CYCLE_RETRIEVE = "cycles_retrieve",
    "SELECT DISTINCT cycles.id AS id, cycles.workspace_id AS workspace_id, \
     cycles.project_id AS project_id, cycles.name AS name, \
     cycles.description AS description, cycles.start_date AS start_date, \
     cycles.end_date AS end_date, cycles.owned_by_id AS owned_by_id, \
     cycles.view_props AS view_props, cycles.sort_order AS sort_order, \
     cycles.external_source AS external_source, cycles.external_id AS external_id, \
     cycles.progress_snapshot AS progress_snapshot, \
     (SELECT COUNT(sub_issues.id) FROM issues sub_issues \
      LEFT OUTER JOIN states sub_states ON sub_issues.state_id = sub_states.id \
      INNER JOIN projects sub_projects ON sub_issues.project_id = sub_projects.id \
      INNER JOIN cycle_issues sub_cycle_issues ON sub_issues.id = sub_cycle_issues.issue_id \
      WHERE sub_issues.deleted_at IS NULL \
      AND NOT (sub_states.\"group\" = 'triage' AND sub_states.\"group\" IS NOT NULL) \
      AND sub_issues.archived_at IS NULL AND sub_projects.archived_at IS NULL \
      AND NOT sub_issues.is_draft AND sub_cycle_issues.cycle_id = $5 \
      AND sub_cycle_issues.deleted_at IS NULL \
      AND sub_issues.parent_id IS NOT NULL \
      AND sub_issues.project_id = $2) AS sub_issues, \
     cycles.logo_props AS logo_props, cycles.version AS version, \
     EXISTS (SELECT 1 FROM user_favorites uf \
             INNER JOIN workspaces ufw ON uf.workspace_id = ufw.id \
             WHERE uf.deleted_at IS NULL AND uf.entity_identifier = cycles.id \
             AND uf.entity_type = 'cycle' AND uf.project_id = $2 \
             AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'completed') AS completed_issues, \
     COALESCE(ARRAY_AGG(DISTINCT issue_assignees.assignee_id) FILTER (WHERE \
         issue_assignees.assignee_id IS NOT NULL \
         AND issue_assignees.deleted_at IS NULL), ARRAY[]::uuid[]) AS assignee_ids, \
     CASE WHEN cycles.start_date <= $4 AND cycles.end_date >= $4 THEN 'CURRENT' \
          WHEN cycles.start_date > $4 THEN 'UPCOMING' \
          WHEN cycles.end_date < $4 THEN 'COMPLETED' \
          WHEN cycles.start_date IS NULL AND cycles.end_date IS NULL THEN 'DRAFT' \
          ELSE 'DRAFT' END AS status, \
     cycles.created_by_id AS created_by, cycles.created_at AS created_at \
     FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN projects ON cycles.project_id = projects.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     LEFT OUTER JOIN cycle_issues ON cycles.id = cycle_issues.cycle_id \
     LEFT OUTER JOIN issues ON cycle_issues.issue_id = issues.id \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
     AND cycles.project_id = $2 AND project_members.is_active \
     AND project_members.member_id = $3 AND projects.archived_at IS NULL \
     AND cycles.id = $5 AND cycles.archived_at IS NULL \
     GROUP BY cycles.id ORDER BY is_favorite DESC, cycles.name ASC LIMIT 1",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("start_date", PgType::Timestamptz),
        ("end_date", PgType::Timestamptz),
        ("owned_by_id", PgType::Uuid),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("progress_snapshot", PgType::Jsonb),
        ("sub_issues", PgType::Int8),
        ("logo_props", PgType::Jsonb),
        ("version", PgType::Int4),
        ("is_favorite", PgType::Bool),
        ("total_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("assignee_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("status", PgType::Text),
        ("created_by", PgType::Uuid),
        ("created_at", PgType::Timestamptz),
    ]
}

catalog_query! {
    /// `CycleArchiveUnarchiveEndpoint.get_queryset` + the list branch
    /// (`cycle/archive.py:41-304`): the archived cycles of a project with the
    /// extra `started`/`unstarted`/`backlog` count annotations, projected in
    /// the order the view's `.values()` passes and ordered by
    /// `-is_favorite, -created_at`.
    ///
    /// `$1` slug, `$2` project id, `$3` user id, `$4` the CASE instant
    /// (`timezone.now()`, called separately at `:211-218`).
    CYCLE_ARCHIVED_LIST = "cycles_archived_list",
    "SELECT DISTINCT cycles.id AS id, cycles.workspace_id AS workspace_id, \
     cycles.project_id AS project_id, cycles.name AS name, \
     cycles.description AS description, cycles.start_date AS start_date, \
     cycles.end_date AS end_date, cycles.owned_by_id AS owned_by_id, \
     cycles.view_props AS view_props, cycles.sort_order AS sort_order, \
     cycles.external_source AS external_source, cycles.external_id AS external_id, \
     cycles.progress_snapshot AS progress_snapshot, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     EXISTS (SELECT 1 FROM user_favorites uf \
             INNER JOIN workspaces ufw ON uf.workspace_id = ufw.id \
             WHERE uf.deleted_at IS NULL AND uf.entity_identifier = cycles.id \
             AND uf.entity_type = 'cycle' AND uf.project_id = $2 \
             AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'cancelled') AS cancelled_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'completed') AS completed_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'started') AS started_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'unstarted') AS unstarted_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'backlog') AS backlog_issues, \
     COALESCE(ARRAY_AGG(DISTINCT issue_assignees.assignee_id) FILTER (WHERE \
         issue_assignees.assignee_id IS NOT NULL \
         AND issue_assignees.deleted_at IS NULL), ARRAY[]::uuid[]) AS assignee_ids, \
     CASE WHEN cycles.start_date <= $4 AND cycles.end_date >= $4 THEN 'CURRENT' \
          WHEN cycles.start_date > $4 THEN 'UPCOMING' \
          WHEN cycles.end_date < $4 THEN 'COMPLETED' \
          WHEN cycles.start_date IS NULL AND cycles.end_date IS NULL THEN 'DRAFT' \
          ELSE 'DRAFT' END AS status, \
     cycles.archived_at AS archived_at, cycles.created_at AS created_at \
     FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN projects ON cycles.project_id = projects.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     LEFT OUTER JOIN cycle_issues ON cycles.id = cycle_issues.cycle_id \
     LEFT OUTER JOIN issues ON cycle_issues.issue_id = issues.id \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
     AND cycles.project_id = $2 AND project_members.is_active \
     AND project_members.member_id = $3 AND projects.archived_at IS NULL \
     AND cycles.archived_at IS NOT NULL \
     GROUP BY cycles.id ORDER BY is_favorite DESC, cycles.created_at DESC",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("start_date", PgType::Timestamptz),
        ("end_date", PgType::Timestamptz),
        ("owned_by_id", PgType::Uuid),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("progress_snapshot", PgType::Jsonb),
        ("total_issues", PgType::Int8),
        ("is_favorite", PgType::Bool),
        ("cancelled_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("started_issues", PgType::Int8),
        ("unstarted_issues", PgType::Int8),
        ("backlog_issues", PgType::Int8),
        ("assignee_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("status", PgType::Text),
        ("archived_at", PgType::Timestamptz),
        ("created_at", PgType::Timestamptz),
    ]
}

catalog_query! {
    /// `CycleArchiveUnarchiveEndpoint.get`'s single-cycle branch
    /// (`cycle/archive.py:306-584`): the archived list's annotations plus
    /// `sub_issues`, `logo_props` and the two estimate sums the view projects
    /// (`completed_estimate_points`, `total_estimate_points`). `pk = $5`.
    ///
    /// The two estimate subqueries are `Issue.issue_objects.filter(
    /// estimate_point__estimate__type="points", state__group=…,
    /// issue_cycle__cycle_id=OuterRef("pk"))` summed over
    /// `estimate_points.value::float8` (`archive.py:49-113`); only the two the
    /// `.values()` call selects are emitted, as Postgres prunes the rest.
    CYCLE_ARCHIVED_RETRIEVE = "cycles_archived_retrieve",
    "SELECT DISTINCT cycles.id AS id, cycles.workspace_id AS workspace_id, \
     cycles.project_id AS project_id, cycles.name AS name, \
     cycles.description AS description, cycles.start_date AS start_date, \
     cycles.end_date AS end_date, cycles.owned_by_id AS owned_by_id, \
     cycles.view_props AS view_props, cycles.sort_order AS sort_order, \
     cycles.external_source AS external_source, cycles.external_id AS external_id, \
     cycles.progress_snapshot AS progress_snapshot, \
     (SELECT COUNT(sub_issues.id) FROM issues sub_issues \
      LEFT OUTER JOIN states sub_states ON sub_issues.state_id = sub_states.id \
      INNER JOIN projects sub_projects ON sub_issues.project_id = sub_projects.id \
      INNER JOIN cycle_issues sub_cycle_issues ON sub_issues.id = sub_cycle_issues.issue_id \
      WHERE sub_issues.deleted_at IS NULL \
      AND NOT (sub_states.\"group\" = 'triage' AND sub_states.\"group\" IS NOT NULL) \
      AND sub_issues.archived_at IS NULL AND sub_projects.archived_at IS NULL \
      AND NOT sub_issues.is_draft AND sub_cycle_issues.cycle_id = $5 \
      AND sub_cycle_issues.deleted_at IS NULL \
      AND sub_issues.parent_id IS NOT NULL \
      AND sub_issues.project_id = $2) AS sub_issues, \
     cycles.logo_props AS logo_props, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN cycle_issues eci ON ei.id = eci.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND eci.deleted_at IS NULL AND eci.cycle_id = cycles.id \
       AND es.\"group\" = 'completed' \
       GROUP BY eci.cycle_id LIMIT 1), 0::float8) AS completed_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN cycle_issues eci ON ei.id = eci.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND eci.deleted_at IS NULL AND eci.cycle_id = cycles.id \
       GROUP BY eci.cycle_id LIMIT 1), 0::float8) AS total_estimate_points, \
     EXISTS (SELECT 1 FROM user_favorites uf \
             INNER JOIN workspaces ufw ON uf.workspace_id = ufw.id \
             WHERE uf.deleted_at IS NULL AND uf.entity_identifier = cycles.id \
             AND uf.entity_type = 'cycle' AND uf.project_id = $2 \
             AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'cancelled') AS cancelled_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'completed') AS completed_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'started') AS started_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'unstarted') AS unstarted_issues, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'backlog') AS backlog_issues, \
     COALESCE(ARRAY_AGG(DISTINCT issue_assignees.assignee_id) FILTER (WHERE \
         issue_assignees.assignee_id IS NOT NULL \
         AND issue_assignees.deleted_at IS NULL), ARRAY[]::uuid[]) AS assignee_ids, \
     CASE WHEN cycles.start_date <= $4 AND cycles.end_date >= $4 THEN 'CURRENT' \
          WHEN cycles.start_date > $4 THEN 'UPCOMING' \
          WHEN cycles.end_date < $4 THEN 'COMPLETED' \
          WHEN cycles.start_date IS NULL AND cycles.end_date IS NULL THEN 'DRAFT' \
          ELSE 'DRAFT' END AS status, \
     cycles.created_by_id AS created_by, cycles.archived_at AS archived_at, \
     cycles.created_at AS created_at \
     FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN projects ON cycles.project_id = projects.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     LEFT OUTER JOIN cycle_issues ON cycles.id = cycle_issues.cycle_id \
     LEFT OUTER JOIN issues ON cycle_issues.issue_id = issues.id \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
     AND cycles.project_id = $2 AND project_members.is_active \
     AND project_members.member_id = $3 AND projects.archived_at IS NULL \
     AND cycles.id = $5 AND cycles.archived_at IS NOT NULL \
     GROUP BY cycles.id LIMIT 1",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("start_date", PgType::Timestamptz),
        ("end_date", PgType::Timestamptz),
        ("owned_by_id", PgType::Uuid),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("progress_snapshot", PgType::Jsonb),
        ("sub_issues", PgType::Int8),
        ("logo_props", PgType::Jsonb),
        ("completed_estimate_points", PgType::Float8),
        ("total_estimate_points", PgType::Float8),
        ("is_favorite", PgType::Bool),
        ("total_issues", PgType::Int8),
        ("cancelled_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("started_issues", PgType::Int8),
        ("unstarted_issues", PgType::Int8),
        ("backlog_issues", PgType::Int8),
        ("assignee_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("status", PgType::Text),
        ("created_by", PgType::Uuid),
        ("archived_at", PgType::Timestamptz),
        ("created_at", PgType::Timestamptz),
    ]
}

catalog_query! {
    /// `CycleProgressEndpoint.get`'s `cycle = Cycle.objects.filter(...).first()`
    /// (`cycle/base.py:666`). Only `progress_snapshot` decides the branch taken.
    CYCLE_PROGRESS_ROW = "cycles_progress_row",
    "SELECT cycles.progress_snapshot AS progress_snapshot FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
     AND cycles.id = $2 AND cycles.project_id = $3 \
     ORDER BY cycles.created_at DESC LIMIT 1",
    [("progress_snapshot", PgType::Jsonb)]
}

catalog_query! {
    /// `CycleProgressEndpoint`'s six `Sum(Case(When(state__group=…)))`
    /// aggregates (`cycle/base.py:669-716`). One row always, `NULL` sums when
    /// the cycle has no estimated issues; the total is `COALESCE`d to
    /// `0::float8` by the Python's `default=Value(0)`.
    CYCLE_PROGRESS_ESTIMATES = "cycles_progress_estimates",
    "SELECT \
     SUM(CASE WHEN states.\"group\" = 'backlog' THEN estimate_points.value::double precision ELSE 0.0 END) AS backlog_estimate_point, \
     SUM(CASE WHEN states.\"group\" = 'unstarted' THEN estimate_points.value::double precision ELSE 0.0 END) AS unstarted_estimate_point, \
     SUM(CASE WHEN states.\"group\" = 'started' THEN estimate_points.value::double precision ELSE 0.0 END) AS started_estimate_point, \
     SUM(CASE WHEN states.\"group\" = 'cancelled' THEN estimate_points.value::double precision ELSE 0.0 END) AS cancelled_estimate_point, \
     SUM(CASE WHEN states.\"group\" = 'completed' THEN estimate_points.value::double precision ELSE 0.0 END) AS completed_estimate_points, \
     COALESCE(SUM(estimate_points.value::double precision), 0::float8) AS total_estimate_points \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND estimates.type = 'points' \
     AND cycle_issues.cycle_id = $1 AND cycle_issues.deleted_at IS NULL \
     AND workspaces.slug = $2 AND issues.project_id = $3",
    [
        ("backlog_estimate_point", PgType::Float8),
        ("unstarted_estimate_point", PgType::Float8),
        ("started_estimate_point", PgType::Float8),
        ("cancelled_estimate_point", PgType::Float8),
        ("completed_estimate_points", PgType::Float8),
        ("total_estimate_points", PgType::Float8),
    ]
}

catalog_query! {
    /// The six `.count()` calls of `CycleProgressEndpoint`'s `else` branch
    /// (`cycle/base.py:725-770`), in one statement: one `COUNT(*) FILTER` per
    /// state group plus the total. Every issue joins `cycle_issues` at most
    /// once (`cycle_issue_when_deleted_at_null`), so the counts equal the
    /// Python's `COUNT(*)` over the same filters.
    CYCLE_PROGRESS_COUNTS = "cycles_progress_counts",
    "SELECT \
     COUNT(*) FILTER (WHERE states.\"group\" = 'backlog') AS backlog_issues, \
     COUNT(*) FILTER (WHERE states.\"group\" = 'unstarted') AS unstarted_issues, \
     COUNT(*) FILTER (WHERE states.\"group\" = 'started') AS started_issues, \
     COUNT(*) FILTER (WHERE states.\"group\" = 'cancelled') AS cancelled_issues, \
     COUNT(*) FILTER (WHERE states.\"group\" = 'completed') AS completed_issues, \
     COUNT(*) AS total_issues \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND workspaces.slug = $2 \
     AND issues.project_id = $3",
    [
        ("backlog_issues", PgType::Int8),
        ("unstarted_issues", PgType::Int8),
        ("started_issues", PgType::Int8),
        ("cancelled_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("total_issues", PgType::Int8),
    ]
}

catalog_query! {
    /// `CycleUserProperties.objects.get_or_create(user, project_id, cycle_id,
    /// workspace__slug=slug)` (`cycle/base.py:651-660`).
    CYCLE_USER_PROPERTIES_BY_KEY = "cycles_user_properties_by_key",
    "SELECT cycle_user_properties.created_at, cycle_user_properties.updated_at, \
     cycle_user_properties.created_by_id, cycle_user_properties.updated_by_id, \
     cycle_user_properties.deleted_at, cycle_user_properties.id, \
     cycle_user_properties.project_id, cycle_user_properties.workspace_id, \
     cycle_user_properties.cycle_id, cycle_user_properties.user_id, \
     cycle_user_properties.filters, cycle_user_properties.display_filters, \
     cycle_user_properties.display_properties, cycle_user_properties.rich_filters \
     FROM cycle_user_properties \
     INNER JOIN workspaces ON cycle_user_properties.workspace_id = workspaces.id \
     WHERE cycle_user_properties.deleted_at IS NULL \
     AND cycle_user_properties.user_id = $1 \
     AND cycle_user_properties.project_id = $2 \
     AND cycle_user_properties.cycle_id = $3 AND workspaces.slug = $4 \
     ORDER BY created_at DESC LIMIT 1",
    [
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("created_by_id", PgType::Uuid),
        ("updated_by_id", PgType::Uuid),
        ("deleted_at", PgType::Timestamptz),
        ("id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("cycle_id", PgType::Uuid),
        ("user_id", PgType::Uuid),
        ("filters", PgType::Jsonb),
        ("display_filters", PgType::Jsonb),
        ("display_properties", PgType::Jsonb),
        ("rich_filters", PgType::Jsonb),
    ]
}

catalog_query! {
    /// The insert half of `get_or_create` (`django/db/models/query.py`),
    /// including the model defaults of `cycle.py:14-68` and
    /// `BaseModel.save`'s audit columns: `created_by` is the request user,
    /// `updated_by` stays `NULL` on create (`plane/db/models/base.py:23-44`).
    /// The workspace comes from the project (`ProjectBaseModel.save`,
    /// `plane/db/models/project.py:237-239`).
    CYCLE_USER_PROPERTIES_INSERT = "cycles_user_properties_insert",
    "INSERT INTO cycle_user_properties \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, cycle_id, user_id, filters, display_filters, \
      display_properties, rich_filters) \
     SELECT $1, $1, $2, NULL, NULL, $3, $4, projects.workspace_id, $5, $2, \
      '{\"priority\": null, \"state\": null, \"state_group\": null, \"assignees\": null, \"created_by\": null, \"labels\": null, \"start_date\": null, \"target_date\": null, \"subscriber\": null}'::jsonb, \
      '{\"group_by\": null, \"order_by\": \"-created_at\", \"type\": null, \"sub_issue\": true, \"show_empty_groups\": true, \"layout\": \"list\", \"calendar_date_range\": \"\"}'::jsonb, \
      '{\"assignee\": true, \"attachment_count\": true, \"created_on\": true, \"due_date\": true, \"estimate\": true, \"key\": true, \"labels\": true, \"link\": true, \"priority\": true, \"start_date\": true, \"state\": true, \"sub_issue_count\": true, \"updated_on\": true}'::jsonb, \
      '{}'::jsonb \
     FROM projects WHERE projects.id = $4 \
     RETURNING created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, cycle_id, user_id, filters, display_filters, \
      display_properties, rich_filters",
    [
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("created_by_id", PgType::Uuid),
        ("updated_by_id", PgType::Uuid),
        ("deleted_at", PgType::Timestamptz),
        ("id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("cycle_id", PgType::Uuid),
        ("user_id", PgType::Uuid),
        ("filters", PgType::Jsonb),
        ("display_filters", PgType::Jsonb),
        ("display_properties", PgType::Jsonb),
        ("rich_filters", PgType::Jsonb),
    ]
}

catalog_query! {
    /// `CycleAnalyticsEndpoint.get`'s `cycle = (Cycle.objects.filter(...)
    /// .annotate(total_issues=Count(...)).first())` (`cycle/base.py:795-810`):
    /// the cycle row plus the `total_issues` annotation the issues burndown
    /// starts from. A miss is an `AttributeError` in the Python (500), which the
    /// handler reproduces.
    CYCLE_ANALYTICS_ROW = "cycles_analytics_row",
    "SELECT cycles.id AS id, cycles.start_date AS start_date, \
     cycles.end_date AS end_date, cycles.progress_snapshot AS progress_snapshot, \
     COUNT(DISTINCT cycle_issues.issue_id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft) AS total_issues \
     FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     LEFT OUTER JOIN cycle_issues ON cycles.id = cycle_issues.cycle_id \
     LEFT OUTER JOIN issues ON cycle_issues.issue_id = issues.id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
     AND cycles.id = $2 AND cycles.project_id = $3 \
     GROUP BY cycles.id ORDER BY cycles.id ASC LIMIT 1",
    [
        ("id", PgType::Uuid),
        ("start_date", PgType::Timestamptz),
        ("end_date", PgType::Timestamptz),
        ("progress_snapshot", PgType::Jsonb),
        ("total_issues", PgType::Int8),
    ]
}

catalog_query! {
    /// `Project.objects.filter(workspace__slug, pk=project_id,
    /// estimate__isnull=False, estimate__type="points").exists()` — the
    /// `estimate_type` guard of `CycleAnalyticsEndpoint` (`base.py:837-842`),
    /// the archive retrieve (`archive.py:358-363`) and `burndown_plot`
    /// (`plane/utils/analytics_plot.py:127-132`).
    CYCLE_ESTIMATE_TYPE_EXISTS = "cycles_estimate_type_exists",
    "SELECT EXISTS (SELECT 1 FROM projects \
     INNER JOIN estimates ON projects.estimate_id = estimates.id \
     INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
     WHERE projects.deleted_at IS NULL AND projects.estimate_id IS NOT NULL \
     AND estimates.type = 'points' AND projects.id = $1 \
     AND workspaces.slug = $2) AS present",
    [("present", PgType::Bool)]
}

catalog_query! {
    /// `CycleAnalyticsEndpoint`'s issue-count assignee distribution
    /// (`cycle/base.py:945-1003`): grouped by `display_name`, `assignee_id`,
    /// `avatar_url`; `COUNT(assignee_id)` per completed/pending filter, ordered
    /// by `display_name`.
    ///
    /// The label and assignee joins are `LEFT OUTER` because the Python
    /// `values()` over a relation without a filter that requires a match keeps
    /// rows with no relation (the "unassigned" / "no label" group).
    CYCLE_ANALYTICS_ASSIGNEES_ISSUES = "cycles_analytics_assignees_issues",
    "SELECT users.display_name AS display_name, issue_assignees.assignee_id AS assignee_id, \
     CASE WHEN users.avatar_asset_id IS NOT NULL \
          THEN '/api/assets/v2/static/' || users.avatar_asset_id::varchar || '/' \
          WHEN users.avatar_asset_id IS NULL THEN users.avatar ELSE NULL END AS avatar_url, \
     COUNT(issue_assignees.assignee_id) FILTER (WHERE issues.archived_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(issue_assignees.assignee_id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_issues, \
     COUNT(issue_assignees.assignee_id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_issues \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     LEFT OUTER JOIN users ON issue_assignees.assignee_id = users.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY users.display_name, issue_assignees.assignee_id, users.avatar_asset_id, users.avatar \
     ORDER BY users.display_name ASC",
    [
        ("display_name", PgType::Varchar),
        ("assignee_id", PgType::Uuid),
        ("avatar_url", PgType::Text),
        ("total_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("pending_issues", PgType::Int8),
    ]
}

catalog_query! {
    /// `CycleAnalyticsEndpoint`'s label distribution for `type=issues`
    /// (`cycle/base.py:1005-1038`): `COUNT(label_id)` per label, ordered by
    /// `label_name`. The `labels` join has no `deleted_at` predicate — Django
    /// adds none for a `F("labels__…")` annotation (`grouper.py` has no
    /// manager filter on relations either).
    CYCLE_ANALYTICS_LABELS_ISSUES = "cycles_analytics_labels_issues",
    "SELECT labels.name AS label_name, labels.color AS color, \
     issue_labels.label_id AS label_id, \
     COUNT(issue_labels.label_id) FILTER (WHERE issues.archived_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(issue_labels.label_id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_issues, \
     COUNT(issue_labels.label_id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_issues \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_labels ON issues.id = issue_labels.issue_id \
     LEFT OUTER JOIN labels ON issue_labels.label_id = labels.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY labels.name, labels.color, issue_labels.label_id \
     ORDER BY labels.name ASC",
    [
        ("label_name", PgType::Varchar),
        ("color", PgType::Varchar),
        ("label_id", PgType::Uuid),
        ("total_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("pending_issues", PgType::Int8),
    ]
}

catalog_query! {
    /// `CycleAnalyticsEndpoint`'s estimate assignee distribution
    /// (`cycle/base.py:848-901`): the same rows as
    /// [`CYCLE_ANALYTICS_ASSIGNEES_ISSUES`] with `SUM(estimate_points.value)`
    /// instead of counts. `estimate_points` is `LEFT OUTER`, so an issue
    /// without an estimate contributes a `NULL` sum (Django emits `LEFT OUTER`
    /// for an aggregate over a nullable FK).
    CYCLE_ANALYTICS_ASSIGNEES_POINTS = "cycles_analytics_assignees_points",
    "SELECT users.display_name AS display_name, issue_assignees.assignee_id AS assignee_id, \
     CASE WHEN users.avatar_asset_id IS NOT NULL \
          THEN '/api/assets/v2/static/' || users.avatar_asset_id::varchar || '/' \
          WHEN users.avatar_asset_id IS NULL THEN users.avatar ELSE NULL END AS avatar_url, \
     SUM(estimate_points.value::double precision) AS total_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_estimates \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     LEFT OUTER JOIN users ON issue_assignees.assignee_id = users.id \
     LEFT OUTER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY users.display_name, issue_assignees.assignee_id, users.avatar_asset_id, users.avatar \
     ORDER BY users.display_name ASC",
    [
        ("display_name", PgType::Varchar),
        ("assignee_id", PgType::Uuid),
        ("avatar_url", PgType::Text),
        ("total_estimates", PgType::Float8),
        ("completed_estimates", PgType::Float8),
        ("pending_estimates", PgType::Float8),
    ]
}

catalog_query! {
    /// `CycleAnalyticsEndpoint`'s estimate label distribution
    /// (`cycle/base.py:903-936`).
    CYCLE_ANALYTICS_LABELS_POINTS = "cycles_analytics_labels_points",
    "SELECT labels.name AS label_name, labels.color AS color, \
     issue_labels.label_id AS label_id, \
     SUM(estimate_points.value::double precision) AS total_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_estimates \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_labels ON issues.id = issue_labels.issue_id \
     LEFT OUTER JOIN labels ON issue_labels.label_id = labels.id \
     LEFT OUTER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY labels.name, labels.color, issue_labels.label_id \
     ORDER BY labels.name ASC",
    [
        ("label_name", PgType::Varchar),
        ("color", PgType::Varchar),
        ("label_id", PgType::Uuid),
        ("total_estimates", PgType::Float8),
        ("completed_estimates", PgType::Float8),
        ("pending_estimates", PgType::Float8),
    ]
}

catalog_query! {
    /// `burndown_plot`'s `total_estimate_points` (`analytics_plot.py:134-143`):
    /// every `estimate_points.value` of the cycle's issues with a non-null
    /// estimate point, in the manager's `-created_at` order (the order is
    /// irrelevant to the sum but part of the statement).
    CYCLE_BURNDOWN_TOTAL_ESTIMATES = "cycles_burndown_total_estimates",
    "SELECT estimate_points.value::double precision AS value FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND issues.estimate_point_id IS NOT NULL \
     AND cycle_issues.deleted_at IS NULL AND cycle_issues.cycle_id = $1 \
     AND issues.project_id = $2 AND workspaces.slug = $3 \
     ORDER BY issues.created_at DESC",
    [("value", PgType::Float8)]
}

catalog_query! {
    /// `burndown_plot(plot_type="points")`'s completed distribution
    /// (`analytics_plot.py:170-182`): `TruncDate("completed_at")` in the active
    /// (user profile) timezone and the estimate value, ordered by date.
    /// `$4` is the zone name Django passes to `AT TIME ZONE`.
    CYCLE_BURNDOWN_COMPLETED_POINTS = "cycles_burndown_completed_points",
    "SELECT (issues.completed_at AT TIME ZONE $4)::date AS date, \
     estimate_points.value::double precision AS value \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND issues.estimate_point_id IS NOT NULL \
     AND cycle_issues.deleted_at IS NULL AND cycle_issues.cycle_id = $1 \
     AND issues.project_id = $2 AND workspaces.slug = $3 \
     ORDER BY date ASC",
    [("date", PgType::Date), ("value", PgType::Float8)]
}

catalog_query! {
    /// `burndown_plot(plot_type="issues")`'s completed distribution
    /// (`analytics_plot.py:184-196`): `COUNT(id)` per `TruncDate("completed_at")`.
    CYCLE_BURNDOWN_COMPLETED_ISSUES = "cycles_burndown_completed_issues",
    "SELECT (issues.completed_at AT TIME ZONE $4)::date AS date, \
     COUNT(issues.id) AS total_completed \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.deleted_at IS NULL \
     AND cycle_issues.cycle_id = $1 AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY 1 ORDER BY 1 ASC",
    [("date", PgType::Date), ("total_completed", PgType::Int8)]
}

catalog_query! {
    /// `CycleArchiveUnarchiveEndpoint.get`'s issue-count assignee distribution
    /// (`archive.py:471-531`): `Count("id")` per `first_name, last_name`
    /// group, ordered by `first_name, last_name`.
    CYCLE_ARCHIVE_ASSIGNEES_ISSUES = "cycles_archive_assignees_issues",
    "SELECT users.first_name AS first_name, users.last_name AS last_name, \
     issue_assignees.assignee_id AS assignee_id, \
     CASE WHEN users.avatar_asset_id IS NOT NULL \
          THEN '/api/assets/v2/static/' || users.avatar_asset_id::varchar || '/' \
          WHEN users.avatar_asset_id IS NULL THEN users.avatar ELSE NULL END AS avatar_url, \
     users.display_name AS display_name, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_issues \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     LEFT OUTER JOIN users ON issue_assignees.assignee_id = users.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY users.first_name, users.last_name, issue_assignees.assignee_id, \
      users.avatar_asset_id, users.avatar, users.display_name \
     ORDER BY users.first_name ASC, users.last_name ASC",
    [
        ("first_name", PgType::Varchar),
        ("last_name", PgType::Varchar),
        ("assignee_id", PgType::Uuid),
        ("avatar_url", PgType::Text),
        ("display_name", PgType::Varchar),
        ("total_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("pending_issues", PgType::Int8),
    ]
}

catalog_query! {
    /// `CycleArchiveUnarchiveEndpoint.get`'s issue-count label distribution
    /// (`archive.py:534-567`).
    CYCLE_ARCHIVE_LABELS_ISSUES = "cycles_archive_labels_issues",
    "SELECT labels.name AS label_name, labels.color AS color, \
     issue_labels.label_id AS label_id, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_issues \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_labels ON issues.id = issue_labels.issue_id \
     LEFT OUTER JOIN labels ON issue_labels.label_id = labels.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY labels.name, labels.color, issue_labels.label_id \
     ORDER BY labels.name ASC",
    [
        ("label_name", PgType::Varchar),
        ("color", PgType::Varchar),
        ("label_id", PgType::Uuid),
        ("total_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("pending_issues", PgType::Int8),
    ]
}

catalog_query! {
    /// `CycleArchiveUnarchiveEndpoint.get`'s estimate assignee distribution
    /// (`archive.py:367-419`).
    CYCLE_ARCHIVE_ASSIGNEES_POINTS = "cycles_archive_assignees_points",
    "SELECT users.display_name AS display_name, issue_assignees.assignee_id AS assignee_id, \
     CASE WHEN users.avatar_asset_id IS NOT NULL \
          THEN '/api/assets/v2/static/' || users.avatar_asset_id::varchar || '/' \
          WHEN users.avatar_asset_id IS NULL THEN users.avatar ELSE NULL END AS avatar_url, \
     SUM(estimate_points.value::double precision) AS total_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_estimates \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     LEFT OUTER JOIN users ON issue_assignees.assignee_id = users.id \
     LEFT OUTER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY users.display_name, issue_assignees.assignee_id, users.avatar_asset_id, users.avatar \
     ORDER BY users.display_name ASC",
    [
        ("display_name", PgType::Varchar),
        ("assignee_id", PgType::Uuid),
        ("avatar_url", PgType::Text),
        ("total_estimates", PgType::Float8),
        ("completed_estimates", PgType::Float8),
        ("pending_estimates", PgType::Float8),
    ]
}

catalog_query! {
    /// `CycleArchiveUnarchiveEndpoint.get`'s estimate label distribution
    /// (`archive.py:421-454`).
    CYCLE_ARCHIVE_LABELS_POINTS = "cycles_archive_labels_points",
    "SELECT labels.name AS label_name, labels.color AS color, \
     issue_labels.label_id AS label_id, \
     SUM(estimate_points.value::double precision) AS total_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_estimates \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN cycle_issues ON issues.id = cycle_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_labels ON issues.id = issue_labels.issue_id \
     LEFT OUTER JOIN labels ON issue_labels.label_id = labels.id \
     LEFT OUTER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND cycle_issues.cycle_id = $1 \
     AND cycle_issues.deleted_at IS NULL AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY labels.name, labels.color, issue_labels.label_id \
     ORDER BY labels.name ASC",
    [
        ("label_name", PgType::Varchar),
        ("color", PgType::Varchar),
        ("label_id", PgType::Uuid),
        ("total_estimates", PgType::Float8),
        ("completed_estimates", PgType::Float8),
        ("pending_estimates", PgType::Float8),
    ]
}

catalog_query! {
    /// `WorkspaceCyclesEndpoint.get` (`workspace/cycle.py:22-109`): every
    /// non-archived cycle of the workspace in a project the user is an active
    /// member of and that is not archived, with the six count annotations,
    /// serialized through `CycleSerializer(many=True)`.
    ///
    /// The counts are `Count("issue_cycle", …)`/`Count("issue_cycle__issue__state__group", …)`
    /// without `distinct=True`: the join is one row per live cycle link, so the
    /// values equal the number of issues; `COUNT(column)` returns 0 for a group
    /// with no member, and `SELECT DISTINCT` collapses the duplicate rows the
    /// `project_members` join introduces.
    WORKSPACE_CYCLES = "cycles_workspace",
    "SELECT DISTINCT cycles.id AS id, cycles.workspace_id AS workspace_id, \
     cycles.project_id AS project_id, cycles.name AS name, \
     cycles.description AS description, cycles.start_date AS start_date, \
     cycles.end_date AS end_date, cycles.owned_by_id AS owned_by_id, \
     cycles.view_props AS view_props, cycles.sort_order AS sort_order, \
     cycles.external_source AS external_source, cycles.external_id AS external_id, \
     cycles.progress_snapshot AS progress_snapshot, cycles.logo_props AS logo_props, \
     COUNT(cycle_issues.id) FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(states.\"group\") FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'cancelled') AS cancelled_issues, \
     COUNT(states.\"group\") FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'completed') AS completed_issues, \
     COUNT(states.\"group\") FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'started') AS started_issues, \
     COUNT(states.\"group\") FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'unstarted') AS unstarted_issues, \
     COUNT(states.\"group\") FILTER (WHERE \
         cycle_issues.deleted_at IS NULL AND issues.archived_at IS NULL \
         AND issues.deleted_at IS NULL AND NOT issues.is_draft \
         AND states.\"group\" = 'backlog') AS backlog_issues, \
     cycles.created_at AS created_at \
     FROM cycles \
     INNER JOIN workspaces ON cycles.workspace_id = workspaces.id \
     INNER JOIN projects ON cycles.project_id = projects.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     LEFT OUTER JOIN cycle_issues ON cycles.id = cycle_issues.cycle_id \
     LEFT OUTER JOIN issues ON cycle_issues.issue_id = issues.id \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     WHERE cycles.deleted_at IS NULL AND workspaces.slug = $1 \
     AND project_members.member_id = $2 AND project_members.is_active \
     AND projects.archived_at IS NULL AND cycles.archived_at IS NULL \
     GROUP BY cycles.id ORDER BY cycles.created_at DESC",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("start_date", PgType::Timestamptz),
        ("end_date", PgType::Timestamptz),
        ("owned_by_id", PgType::Uuid),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("progress_snapshot", PgType::Jsonb),
        ("logo_props", PgType::Jsonb),
        ("total_issues", PgType::Int8),
        ("cancelled_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("started_issues", PgType::Int8),
        ("unstarted_issues", PgType::Int8),
        ("backlog_issues", PgType::Int8),
        ("created_at", PgType::Timestamptz),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &CYCLE_PROJECT_TIMEZONE,
    &CYCLE_LIST,
    &CYCLE_LIST_CURRENT,
    &CYCLE_RETRIEVE,
    &CYCLE_ARCHIVED_LIST,
    &CYCLE_ARCHIVED_RETRIEVE,
    &CYCLE_PROGRESS_ROW,
    &CYCLE_PROGRESS_ESTIMATES,
    &CYCLE_PROGRESS_COUNTS,
    &CYCLE_USER_PROPERTIES_BY_KEY,
    &CYCLE_USER_PROPERTIES_INSERT,
    &CYCLE_ANALYTICS_ROW,
    &CYCLE_ESTIMATE_TYPE_EXISTS,
    &CYCLE_ANALYTICS_ASSIGNEES_ISSUES,
    &CYCLE_ANALYTICS_LABELS_ISSUES,
    &CYCLE_ANALYTICS_ASSIGNEES_POINTS,
    &CYCLE_ANALYTICS_LABELS_POINTS,
    &CYCLE_BURNDOWN_TOTAL_ESTIMATES,
    &CYCLE_BURNDOWN_COMPLETED_POINTS,
    &CYCLE_BURNDOWN_COMPLETED_ISSUES,
    &CYCLE_ARCHIVE_ASSIGNEES_ISSUES,
    &CYCLE_ARCHIVE_LABELS_ISSUES,
    &CYCLE_ARCHIVE_ASSIGNEES_POINTS,
    &CYCLE_ARCHIVE_LABELS_POINTS,
    &WORKSPACE_CYCLES,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("queries.rs");
        let declared = source.matches(concat!("catalog_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
            assert!(!query.columns.is_empty());
        }
    }
}
