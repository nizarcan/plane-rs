// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `modules` group.
//!
//! Reproduces the ORM statements of
//!
//! * `plane/app/views/module/base.py` — `ModuleViewSet`, `ModuleLinkViewSet`,
//!   `ModuleUserPropertiesEndpoint`;
//! * `plane/app/views/module/archive.py` — `ModuleArchiveUnarchiveEndpoint`;
//! * `plane/app/views/workspace/module.py` — `WorkspaceModulesEndpoint`.
//!
//! The `ModuleViewSet` annotation set (`base.py:79-293`) is six
//! `Coalesce(Subquery(…), 0)` counts and six estimate sums, plus the favourite
//! `EXISTS` and the `ARRAY_AGG` of live members. Django expresses them as
//! correlated subqueries, which is what this catalog does too: the module row
//! is not grouped by its annotations, so `GROUP BY modules.id` is enough.
//!
//! The archived list has the quirk that its `member_ids` `ArrayAgg` filter does
//! **not** check `modulemember.deleted_at` (`archive.py:245-254`), unlike
//! `ModuleViewSet`'s — a soft-deleted module membership still appears there.

use plane_db::catalog::{ArrayElement, PgType};
use plane_db::{Column, Query};

/// `plane_query!` with an explicit `PgType` expression, because the shared
/// macro's `$ty:ident` cannot spell an array result column
/// (`PgType::ArrayOf(ArrayElement::Uuid)` for `member_ids`). The catalog and
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
    /// `ModuleViewSet.list` (`module/base.py:354-394`): `get_queryset()` with
    /// `archived_at IS NULL`, projected through `.values()` in the view's order.
    ///
    /// `$1` slug, `$2` project id, `$3` user id.
    MODULE_LIST = "modules_list",
    "SELECT DISTINCT modules.id AS id, modules.workspace_id AS workspace_id, \
     modules.project_id AS project_id, modules.name AS name, \
     modules.description AS description, modules.description_text AS description_text, \
     modules.description_html AS description_html, modules.start_date AS start_date, \
     modules.target_date AS target_date, modules.status AS status, \
     modules.lead_id AS lead_id, \
     COALESCE(ARRAY_AGG(DISTINCT module_members.member_id) FILTER (WHERE \
         module_members.member_id IS NOT NULL AND module_members.deleted_at IS NULL), \
         ARRAY[]::uuid[]) AS member_ids, \
     modules.view_props AS view_props, modules.sort_order AS sort_order, \
     modules.external_source AS external_source, modules.external_id AS external_id, \
     modules.logo_props AS logo_props, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'completed'), 0::float8) AS completed_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id), 0::float8) \
       AS total_estimate_points, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id), 0) AS total_issues, \
     EXISTS (SELECT 1 FROM user_favorites uf \
             INNER JOIN workspaces ufw ON uf.workspace_id = ufw.id \
             WHERE uf.deleted_at IS NULL AND uf.entity_identifier = modules.id \
             AND uf.entity_type = 'module' AND uf.project_id = $2 \
             AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'cancelled'), 0) AS cancelled_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'completed'), 0) AS completed_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'started'), 0) AS started_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'unstarted'), 0) AS unstarted_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'backlog'), 0) AS backlog_issues, \
     modules.created_at AS created_at, modules.updated_at AS updated_at \
     FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     LEFT OUTER JOIN module_members ON modules.id = module_members.module_id \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $2 \
     AND workspaces.slug = $1 AND modules.archived_at IS NULL \
     GROUP BY modules.id ORDER BY is_favorite DESC, modules.created_at DESC",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("description_text", PgType::Jsonb),
        ("description_html", PgType::Jsonb),
        ("start_date", PgType::Date),
        ("target_date", PgType::Date),
        ("status", PgType::Varchar),
        ("lead_id", PgType::Uuid),
        ("member_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("logo_props", PgType::Jsonb),
        ("completed_estimate_points", PgType::Float8),
        ("total_estimate_points", PgType::Float8),
        ("total_issues", PgType::Int8),
        ("is_favorite", PgType::Bool),
        ("cancelled_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("started_issues", PgType::Int8),
        ("unstarted_issues", PgType::Int8),
        ("backlog_issues", PgType::Int8),
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
    ]
}

catalog_query! {
    /// `ModuleViewSet.retrieve` (`module/base.py:396-650`): the list's
    /// annotations plus `sub_issues`, the four remaining estimate sums the
    /// `ModuleDetailSerializer` reads, and `logo_props`. `$4` is `pk`.
    MODULE_RETRIEVE = "modules_retrieve",
    "SELECT DISTINCT modules.id AS id, modules.workspace_id AS workspace_id, \
     modules.project_id AS project_id, modules.name AS name, \
     modules.description AS description, modules.description_text AS description_text, \
     modules.description_html AS description_html, modules.start_date AS start_date, \
     modules.target_date AS target_date, modules.status AS status, \
     modules.lead_id AS lead_id, \
     COALESCE(ARRAY_AGG(DISTINCT module_members.member_id) FILTER (WHERE \
         module_members.member_id IS NOT NULL AND module_members.deleted_at IS NULL), \
         ARRAY[]::uuid[]) AS member_ids, \
     modules.view_props AS view_props, modules.sort_order AS sort_order, \
     modules.external_source AS external_source, modules.external_id AS external_id, \
     modules.logo_props AS logo_props, \
     (SELECT COUNT(si.id) FROM issues si \
      LEFT OUTER JOIN states ss ON si.state_id = ss.id \
      INNER JOIN projects sp ON si.project_id = sp.id \
      INNER JOIN module_issues smi ON si.id = smi.issue_id \
      WHERE si.deleted_at IS NULL \
      AND NOT (ss.\"group\" = 'triage' AND ss.\"group\" IS NOT NULL) \
      AND si.archived_at IS NULL AND sp.archived_at IS NULL \
      AND NOT si.is_draft AND smi.module_id = $4 AND smi.deleted_at IS NULL \
      AND si.parent_id IS NOT NULL AND si.project_id = $2) AS sub_issues, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'backlog'), 0::float8) AS backlog_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'unstarted'), 0::float8) AS unstarted_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'started'), 0::float8) AS started_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'cancelled'), 0::float8) AS cancelled_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'completed'), 0::float8) AS completed_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id), 0::float8) \
       AS total_estimate_points, \
     EXISTS (SELECT 1 FROM user_favorites uf \
             INNER JOIN workspaces ufw ON uf.workspace_id = ufw.id \
             WHERE uf.deleted_at IS NULL AND uf.entity_identifier = modules.id \
             AND uf.entity_type = 'module' AND uf.project_id = $2 \
             AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id), 0) AS total_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'cancelled'), 0) AS cancelled_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'completed'), 0) AS completed_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'started'), 0) AS started_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'unstarted'), 0) AS unstarted_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'backlog'), 0) AS backlog_issues, \
     modules.created_at AS created_at, modules.updated_at AS updated_at, \
     modules.archived_at AS archived_at \
     FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     LEFT OUTER JOIN module_members ON modules.id = module_members.module_id \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $2 \
     AND workspaces.slug = $1 AND modules.archived_at IS NULL \
     AND modules.id = $4 \
     GROUP BY modules.id ORDER BY is_favorite DESC, modules.created_at DESC LIMIT 1",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("description_text", PgType::Jsonb),
        ("description_html", PgType::Jsonb),
        ("start_date", PgType::Date),
        ("target_date", PgType::Date),
        ("status", PgType::Varchar),
        ("lead_id", PgType::Uuid),
        ("member_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("logo_props", PgType::Jsonb),
        ("sub_issues", PgType::Int8),
        ("backlog_estimate_points", PgType::Float8),
        ("unstarted_estimate_points", PgType::Float8),
        ("started_estimate_points", PgType::Float8),
        ("cancelled_estimate_points", PgType::Float8),
        ("completed_estimate_points", PgType::Float8),
        ("total_estimate_points", PgType::Float8),
        ("is_favorite", PgType::Bool),
        ("total_issues", PgType::Int8),
        ("cancelled_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("started_issues", PgType::Int8),
        ("unstarted_issues", PgType::Int8),
        ("backlog_issues", PgType::Int8),
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("archived_at", PgType::Timestamptz),
    ]
}

catalog_query! {
    /// `ModuleArchiveUnarchiveEndpoint.get`'s list branch
    /// (`module/archive.py:258-293`): the archived modules of a project,
    /// projected without `logo_props` and ending with `archived_at`.
    ///
    /// `member_ids` here filters only `members__id__isnull=False`, so a
    /// soft-deleted `module_members` row still appears (`archive.py:245-254`).
    MODULE_ARCHIVED_LIST = "modules_archived_list",
    "SELECT DISTINCT modules.id AS id, modules.workspace_id AS workspace_id, \
     modules.project_id AS project_id, modules.name AS name, \
     modules.description AS description, modules.description_text AS description_text, \
     modules.description_html AS description_html, modules.start_date AS start_date, \
     modules.target_date AS target_date, modules.status AS status, \
     modules.lead_id AS lead_id, \
     COALESCE(ARRAY_AGG(DISTINCT module_members.member_id) FILTER (WHERE \
         module_members.member_id IS NOT NULL), ARRAY[]::uuid[]) AS member_ids, \
     modules.view_props AS view_props, modules.sort_order AS sort_order, \
     modules.external_source AS external_source, modules.external_id AS external_id, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id), 0) AS total_issues, \
     EXISTS (SELECT 1 FROM user_favorites uf \
             INNER JOIN workspaces ufw ON uf.workspace_id = ufw.id \
             WHERE uf.deleted_at IS NULL AND uf.entity_identifier = modules.id \
             AND uf.entity_type = 'module' AND uf.project_id = $2 \
             AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'cancelled'), 0) AS cancelled_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'completed'), 0) AS completed_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'started'), 0) AS started_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'unstarted'), 0) AS unstarted_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'backlog'), 0) AS backlog_issues, \
     modules.created_at AS created_at, modules.updated_at AS updated_at, \
     modules.archived_at AS archived_at \
     FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     LEFT OUTER JOIN module_members ON modules.id = module_members.module_id \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $2 \
     AND workspaces.slug = $1 AND modules.archived_at IS NOT NULL \
     GROUP BY modules.id ORDER BY is_favorite DESC, modules.created_at DESC",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("description_text", PgType::Jsonb),
        ("description_html", PgType::Jsonb),
        ("start_date", PgType::Date),
        ("target_date", PgType::Date),
        ("status", PgType::Varchar),
        ("lead_id", PgType::Uuid),
        ("member_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("total_issues", PgType::Int8),
        ("is_favorite", PgType::Bool),
        ("cancelled_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("started_issues", PgType::Int8),
        ("unstarted_issues", PgType::Int8),
        ("backlog_issues", PgType::Int8),
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("archived_at", PgType::Timestamptz),
    ]
}

catalog_query! {
    /// `ModuleArchiveUnarchiveEndpoint.get`'s single-module branch
    /// (`module/archive.py:295-542`): the archived module with every field
    /// `ModuleDetailSerializer` reads (including the six estimate sums and
    /// `sub_issues`) and no `archived_at IS NULL` filter. `$4` is `pk`.
    ///
    /// The query reuses [`MODULE_RETRIEVE`]'s shape with the archived filter
    /// flipped, so `logo_props` returns after `external_id` exactly as the
    /// serializer's field order expects it.
    MODULE_ARCHIVED_RETRIEVE = "modules_archived_retrieve",
    "SELECT DISTINCT modules.id AS id, modules.workspace_id AS workspace_id, \
     modules.project_id AS project_id, modules.name AS name, \
     modules.description AS description, modules.description_text AS description_text, \
     modules.description_html AS description_html, modules.start_date AS start_date, \
     modules.target_date AS target_date, modules.status AS status, \
     modules.lead_id AS lead_id, \
     COALESCE(ARRAY_AGG(DISTINCT module_members.member_id) FILTER (WHERE \
         module_members.member_id IS NOT NULL), ARRAY[]::uuid[]) AS member_ids, \
     modules.view_props AS view_props, modules.sort_order AS sort_order, \
     modules.external_source AS external_source, modules.external_id AS external_id, \
     modules.logo_props AS logo_props, \
     (SELECT COUNT(si.id) FROM issues si \
      LEFT OUTER JOIN states ss ON si.state_id = ss.id \
      INNER JOIN projects sp ON si.project_id = sp.id \
      INNER JOIN module_issues smi ON si.id = smi.issue_id \
      WHERE si.deleted_at IS NULL \
      AND NOT (ss.\"group\" = 'triage' AND ss.\"group\" IS NOT NULL) \
      AND si.archived_at IS NULL AND sp.archived_at IS NULL \
      AND NOT si.is_draft AND smi.module_id = $4 AND smi.deleted_at IS NULL \
      AND si.parent_id IS NOT NULL AND si.project_id = $2) AS sub_issues, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'backlog'), 0::float8) AS backlog_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'unstarted'), 0::float8) AS unstarted_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'started'), 0::float8) AS started_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'cancelled'), 0::float8) AS cancelled_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id \
       AND es.\"group\" = 'completed'), 0::float8) AS completed_estimate_points, \
     COALESCE((SELECT SUM(estimate_points.value::double precision) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN estimate_points ON ei.estimate_point_id = estimate_points.id \
       INNER JOIN estimates ON estimate_points.estimate_id = estimates.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND estimates.type = 'points' \
       AND emi.deleted_at IS NULL AND emi.module_id = modules.id), 0::float8) \
       AS total_estimate_points, \
     EXISTS (SELECT 1 FROM user_favorites uf \
             INNER JOIN workspaces ufw ON uf.workspace_id = ufw.id \
             WHERE uf.deleted_at IS NULL AND uf.entity_identifier = modules.id \
             AND uf.entity_type = 'module' AND uf.project_id = $2 \
             AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       LEFT OUTER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id), 0) AS total_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'cancelled'), 0) AS cancelled_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'completed'), 0) AS completed_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'started'), 0) AS started_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'unstarted'), 0) AS unstarted_issues, \
     COALESCE((SELECT COUNT(ei.id) FROM issues ei \
       INNER JOIN states es ON ei.state_id = es.id \
       INNER JOIN projects ep ON ei.project_id = ep.id \
       INNER JOIN module_issues emi ON ei.id = emi.issue_id \
       WHERE ei.deleted_at IS NULL \
       AND NOT (es.\"group\" = 'triage' AND es.\"group\" IS NOT NULL) \
       AND ei.archived_at IS NULL AND ep.archived_at IS NULL \
       AND NOT ei.is_draft AND emi.deleted_at IS NULL \
       AND emi.module_id = modules.id AND es.\"group\" = 'backlog'), 0) AS backlog_issues, \
     modules.created_at AS created_at, modules.updated_at AS updated_at, \
     modules.archived_at AS archived_at \
     FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     LEFT OUTER JOIN module_members ON modules.id = module_members.module_id \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $2 \
     AND workspaces.slug = $1 AND modules.archived_at IS NOT NULL \
     AND modules.id = $4 \
     GROUP BY modules.id ORDER BY is_favorite DESC, modules.created_at DESC LIMIT 1",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("description_text", PgType::Jsonb),
        ("description_html", PgType::Jsonb),
        ("start_date", PgType::Date),
        ("target_date", PgType::Date),
        ("status", PgType::Varchar),
        ("lead_id", PgType::Uuid),
        ("member_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("logo_props", PgType::Jsonb),
        ("sub_issues", PgType::Int8),
        ("backlog_estimate_points", PgType::Float8),
        ("unstarted_estimate_points", PgType::Float8),
        ("started_estimate_points", PgType::Float8),
        ("cancelled_estimate_points", PgType::Float8),
        ("completed_estimate_points", PgType::Float8),
        ("total_estimate_points", PgType::Float8),
        ("is_favorite", PgType::Bool),
        ("total_issues", PgType::Int8),
        ("cancelled_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("started_issues", PgType::Int8),
        ("unstarted_issues", PgType::Int8),
        ("backlog_issues", PgType::Int8),
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("archived_at", PgType::Timestamptz),
    ]
}

catalog_query! {
    /// `ModuleLinkViewSet.get_queryset` (`module/base.py:781-795`): the module's
    /// links visible to an active project member, ordered by `-created_at`.
    ///
    /// `$1` slug, `$2` project id, `$3` module id, `$4` user id.
    MODULE_LINKS_LIST = "modules_links_list",
    "SELECT DISTINCT module_links.created_at AS created_at, \
     module_links.updated_at AS updated_at, module_links.created_by_id AS created_by_id, \
     module_links.updated_by_id AS updated_by_id, module_links.deleted_at AS deleted_at, \
     module_links.id AS id, module_links.project_id AS project_id, \
     module_links.workspace_id AS workspace_id, module_links.title AS title, \
     module_links.url AS url, module_links.module_id AS module_id, \
     module_links.metadata AS metadata \
     FROM module_links \
     INNER JOIN workspaces ON module_links.workspace_id = workspaces.id \
     INNER JOIN projects ON module_links.project_id = projects.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     WHERE module_links.deleted_at IS NULL AND workspaces.slug = $1 \
     AND module_links.project_id = $2 AND module_links.module_id = $3 \
     AND project_members.member_id = $4 AND project_members.is_active \
     AND projects.archived_at IS NULL \
     ORDER BY module_links.created_at DESC",
    [
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("created_by_id", PgType::Uuid),
        ("updated_by_id", PgType::Uuid),
        ("deleted_at", PgType::Timestamptz),
        ("id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("title", PgType::Varchar),
        ("url", PgType::Varchar),
        ("module_id", PgType::Uuid),
        ("metadata", PgType::Jsonb),
    ]
}

catalog_query! {
    /// `ModuleLinkViewSet.get_queryset().get(pk=pk)` — the retrieve route
    /// (`module/base.py:781-795`). A miss is DRF's `Http404` → 404
    /// `{"detail": "Not found."}`. `$5` is `pk`.
    MODULE_LINK_BY_PK = "modules_link_by_pk",
    "SELECT DISTINCT module_links.created_at AS created_at, \
     module_links.updated_at AS updated_at, module_links.created_by_id AS created_by_id, \
     module_links.updated_by_id AS updated_by_id, module_links.deleted_at AS deleted_at, \
     module_links.id AS id, module_links.project_id AS project_id, \
     module_links.workspace_id AS workspace_id, module_links.title AS title, \
     module_links.url AS url, module_links.module_id AS module_id, \
     module_links.metadata AS metadata \
     FROM module_links \
     INNER JOIN workspaces ON module_links.workspace_id = workspaces.id \
     INNER JOIN projects ON module_links.project_id = projects.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     WHERE module_links.deleted_at IS NULL AND workspaces.slug = $1 \
     AND module_links.project_id = $2 AND module_links.module_id = $3 \
     AND project_members.member_id = $4 AND project_members.is_active \
     AND projects.archived_at IS NULL AND module_links.id = $5 \
     ORDER BY module_links.created_at DESC LIMIT 1",
    [
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("created_by_id", PgType::Uuid),
        ("updated_by_id", PgType::Uuid),
        ("deleted_at", PgType::Timestamptz),
        ("id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("title", PgType::Varchar),
        ("url", PgType::Varchar),
        ("module_id", PgType::Uuid),
        ("metadata", PgType::Jsonb),
    ]
}

catalog_query! {
    /// `ModuleViewSet.get_queryset`'s `link_module` prefetch
    /// (`module/base.py:219-224`), ordered by `ModuleLink.Meta.ordering`
    /// (`-created_at`). `$1` module id, `$2` the module ids array (one module
    /// here, but the prefetch shape is `module_id = ANY(...)`).
    MODULE_LINKS_BY_MODULE = "modules_links_by_module",
    "SELECT module_links.created_at AS created_at, \
     module_links.updated_at AS updated_at, module_links.created_by_id AS created_by_id, \
     module_links.updated_by_id AS updated_by_id, module_links.deleted_at AS deleted_at, \
     module_links.id AS id, module_links.project_id AS project_id, \
     module_links.workspace_id AS workspace_id, module_links.title AS title, \
     module_links.url AS url, module_links.module_id AS module_id, \
     module_links.metadata AS metadata \
     FROM module_links \
     WHERE module_links.deleted_at IS NULL AND module_links.module_id = $1 \
     ORDER BY module_links.created_at DESC",
    [
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("created_by_id", PgType::Uuid),
        ("updated_by_id", PgType::Uuid),
        ("deleted_at", PgType::Timestamptz),
        ("id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("title", PgType::Varchar),
        ("url", PgType::Varchar),
        ("module_id", PgType::Uuid),
        ("metadata", PgType::Jsonb),
    ]
}

catalog_query! {
    /// `ModuleUserProperties.objects.get_or_create(...)`
    /// (`module/base.py:858-865`).
    MODULE_USER_PROPERTIES_BY_KEY = "modules_user_properties_by_key",
    "SELECT module_user_properties.created_at, module_user_properties.updated_at, \
     module_user_properties.created_by_id, module_user_properties.updated_by_id, \
     module_user_properties.deleted_at, module_user_properties.id, \
     module_user_properties.project_id, module_user_properties.workspace_id, \
     module_user_properties.module_id, module_user_properties.user_id, \
     module_user_properties.filters, module_user_properties.display_filters, \
     module_user_properties.display_properties, module_user_properties.rich_filters \
     FROM module_user_properties \
     INNER JOIN workspaces ON module_user_properties.workspace_id = workspaces.id \
     WHERE module_user_properties.deleted_at IS NULL \
     AND module_user_properties.user_id = $1 \
     AND module_user_properties.project_id = $2 \
     AND module_user_properties.module_id = $3 AND workspaces.slug = $4 \
     ORDER BY module_user_properties.created_at DESC LIMIT 1",
    [
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("created_by_id", PgType::Uuid),
        ("updated_by_id", PgType::Uuid),
        ("deleted_at", PgType::Timestamptz),
        ("id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("module_id", PgType::Uuid),
        ("user_id", PgType::Uuid),
        ("filters", PgType::Jsonb),
        ("display_filters", PgType::Jsonb),
        ("display_properties", PgType::Jsonb),
        ("rich_filters", PgType::Jsonb),
    ]
}

catalog_query! {
    /// The insert half of `get_or_create`: the `module.py:14-55` defaults and
    /// `BaseModel.save`'s audit columns (`created_by` is the request user,
    /// `updated_by` stays `NULL` on create).
    MODULE_USER_PROPERTIES_INSERT = "modules_user_properties_insert",
    "INSERT INTO module_user_properties \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, module_id, user_id, filters, display_filters, \
      display_properties, rich_filters) \
     SELECT $1, $1, $2, NULL, NULL, $3, $4, projects.workspace_id, $5, $2, \
      '{\"priority\": null, \"state\": null, \"state_group\": null, \"assignees\": null, \"created_by\": null, \"labels\": null, \"start_date\": null, \"target_date\": null, \"subscriber\": null}'::jsonb, \
      '{\"group_by\": null, \"order_by\": \"-created_at\", \"type\": null, \"sub_issue\": true, \"show_empty_groups\": true, \"layout\": \"list\", \"calendar_date_range\": \"\"}'::jsonb, \
      '{\"assignee\": true, \"attachment_count\": true, \"created_on\": true, \"due_date\": true, \"estimate\": true, \"key\": true, \"labels\": true, \"link\": true, \"priority\": true, \"start_date\": true, \"state\": true, \"sub_issue_count\": true, \"updated_on\": true}'::jsonb, \
      '{}'::jsonb \
     FROM projects WHERE projects.id = $4 \
     RETURNING created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, module_id, user_id, filters, display_filters, \
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
        ("module_id", PgType::Uuid),
        ("user_id", PgType::Uuid),
        ("filters", PgType::Jsonb),
        ("display_filters", PgType::Jsonb),
        ("display_properties", PgType::Jsonb),
        ("rich_filters", PgType::Jsonb),
    ]
}

catalog_query! {
    /// `Project.objects.filter(workspace__slug, pk=project_id,
    /// estimate__isnull=False, estimate__type="points").exists()` — the
    /// `estimate_type` guard of the module detail (`module/base.py:418-423`,
    /// `archive.py:311-316`). `$1` project id, `$2` slug.
    MODULE_ESTIMATE_TYPE_EXISTS = "modules_estimate_type_exists",
    "SELECT EXISTS (SELECT 1 FROM projects \
     INNER JOIN estimates ON projects.estimate_id = estimates.id \
     INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
     WHERE projects.deleted_at IS NULL AND projects.estimate_id IS NOT NULL \
     AND estimates.type = 'points' AND projects.id = $1 \
     AND workspaces.slug = $2) AS present",
    [("present", PgType::Bool)]
}

catalog_query! {
    /// `ModuleViewSet.retrieve`'s issue-count assignee distribution
    /// (`module/base.py:539-590`): grouped by first/last name, `display_name`,
    /// `assignee_id` and `avatar_url`, ordered by `first_name, last_name`.
    MODULE_ASSIGNEES_ISSUES = "modules_assignees_issues",
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
     INNER JOIN module_issues ON issues.id = module_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     LEFT OUTER JOIN users ON issue_assignees.assignee_id = users.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND module_issues.deleted_at IS NULL \
     AND module_issues.module_id = $1 AND issues.project_id = $2 \
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
    /// `ModuleViewSet.retrieve`'s issue-count label distribution
    /// (`module/base.py:592-625`).
    MODULE_LABELS_ISSUES = "modules_labels_issues",
    "SELECT labels.name AS label_name, labels.color AS color, \
     issue_labels.label_id AS label_id, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND NOT issues.is_draft) AS total_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_issues, \
     COUNT(issues.id) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_issues \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN module_issues ON issues.id = module_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_labels ON issues.id = issue_labels.issue_id \
     LEFT OUTER JOIN labels ON issue_labels.label_id = labels.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND module_issues.deleted_at IS NULL \
     AND module_issues.module_id = $1 AND issues.project_id = $2 \
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
    /// `ModuleViewSet.retrieve`'s estimate assignee distribution
    /// (`module/base.py:431-491`): names plus `SUM(estimate_points.value)`.
    MODULE_ASSIGNEES_POINTS = "modules_assignees_points",
    "SELECT users.first_name AS first_name, users.last_name AS last_name, \
     issue_assignees.assignee_id AS assignee_id, \
     CASE WHEN users.avatar_asset_id IS NOT NULL \
          THEN '/api/assets/v2/static/' || users.avatar_asset_id::varchar || '/' \
          WHEN users.avatar_asset_id IS NULL THEN users.avatar ELSE NULL END AS avatar_url, \
     users.display_name AS display_name, \
     SUM(estimate_points.value::double precision) AS total_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_estimates \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN module_issues ON issues.id = module_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     LEFT OUTER JOIN users ON issue_assignees.assignee_id = users.id \
     LEFT OUTER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND module_issues.deleted_at IS NULL \
     AND module_issues.module_id = $1 AND issues.project_id = $2 \
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
        ("total_estimates", PgType::Float8),
        ("completed_estimates", PgType::Float8),
        ("pending_estimates", PgType::Float8),
    ]
}

catalog_query! {
    /// `ModuleViewSet.retrieve`'s estimate label distribution
    /// (`module/base.py:493-526`). Unlike the other three distributions, this
    /// one has **no** `issue_module__deleted_at__isnull=True` predicate — a
    /// soft-deleted module link still contributes its estimate.
    MODULE_LABELS_POINTS = "modules_labels_points",
    "SELECT labels.name AS label_name, labels.color AS color, \
     issue_labels.label_id AS label_id, \
     SUM(estimate_points.value::double precision) AS total_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NOT NULL AND NOT issues.is_draft) AS completed_estimates, \
     SUM(estimate_points.value::double precision) FILTER (WHERE issues.archived_at IS NULL AND issues.completed_at IS NULL AND NOT issues.is_draft) AS pending_estimates \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN module_issues ON issues.id = module_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_labels ON issues.id = issue_labels.issue_id \
     LEFT OUTER JOIN labels ON issue_labels.label_id = labels.id \
     LEFT OUTER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND module_issues.module_id = $1 \
     AND issues.project_id = $2 AND workspaces.slug = $3 \
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
    /// `burndown_plot`'s `total_estimate_points` for a module
    /// (`analytics_plot.py:145-155`).
    MODULE_BURNDOWN_TOTAL_ESTIMATES = "modules_burndown_total_estimates",
    "SELECT estimate_points.value::double precision AS value FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     INNER JOIN module_issues ON issues.id = module_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND issues.estimate_point_id IS NOT NULL \
     AND module_issues.deleted_at IS NULL AND module_issues.module_id = $1 \
     AND issues.project_id = $2 AND workspaces.slug = $3 \
     ORDER BY issues.created_at DESC",
    [("value", PgType::Float8)]
}

catalog_query! {
    /// `burndown_plot(plot_type="points")` for a module
    /// (`analytics_plot.py:207-220`); `$4` is the active timezone name.
    MODULE_BURNDOWN_COMPLETED_POINTS = "modules_burndown_completed_points",
    "SELECT (issues.completed_at AT TIME ZONE $4)::date AS date, \
     estimate_points.value::double precision AS value \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN estimate_points ON issues.estimate_point_id = estimate_points.id \
     INNER JOIN module_issues ON issues.id = module_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND issues.estimate_point_id IS NOT NULL \
     AND module_issues.deleted_at IS NULL AND module_issues.module_id = $1 \
     AND issues.project_id = $2 AND workspaces.slug = $3 \
     ORDER BY date ASC",
    [("date", PgType::Date), ("value", PgType::Float8)]
}

catalog_query! {
    /// `burndown_plot(plot_type="issues")` for a module
    /// (`analytics_plot.py:221-234`).
    MODULE_BURNDOWN_COMPLETED_ISSUES = "modules_burndown_completed_issues",
    "SELECT (issues.completed_at AT TIME ZONE $4)::date AS date, \
     COUNT(issues.id) AS total_completed \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN module_issues ON issues.id = module_issues.issue_id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     WHERE issues.deleted_at IS NULL \
     AND NOT (states.\"group\" = 'triage' AND states.\"group\" IS NOT NULL) \
     AND issues.archived_at IS NULL AND projects.archived_at IS NULL \
     AND NOT issues.is_draft AND module_issues.deleted_at IS NULL \
     AND module_issues.module_id = $1 AND issues.project_id = $2 \
     AND workspaces.slug = $3 \
     GROUP BY 1 ORDER BY 1 ASC",
    [("date", PgType::Date), ("total_completed", PgType::Int8)]
}

catalog_query! {
    /// `WorkspaceModulesEndpoint.get` (`workspace/module.py:25-129`): every
    /// non-archived module of the workspace in an active project the user is a
    /// member of, with the six `distinct=True` counts and `member_ids`,
    /// ordered by `-created_at` and serialized by `ModuleSerializer`.
    WORKSPACE_MODULES = "modules_workspace",
    "SELECT DISTINCT modules.id AS id, modules.workspace_id AS workspace_id, \
     modules.project_id AS project_id, modules.name AS name, \
     modules.description AS description, modules.description_text AS description_text, \
     modules.description_html AS description_html, modules.start_date AS start_date, \
     modules.target_date AS target_date, modules.status AS status, \
     modules.lead_id AS lead_id, \
     COALESCE(ARRAY_AGG(DISTINCT module_members.member_id) FILTER (WHERE \
         module_members.member_id IS NOT NULL AND module_members.deleted_at IS NULL), \
         ARRAY[]::uuid[]) AS member_ids, \
     modules.view_props AS view_props, modules.sort_order AS sort_order, \
     modules.external_source AS external_source, modules.external_id AS external_id, \
     modules.logo_props AS logo_props, \
     COALESCE((SELECT COUNT(mi.id) FROM module_issues mi \
       INNER JOIN issues i ON mi.issue_id = i.id \
       WHERE mi.deleted_at IS NULL AND mi.module_id = modules.id \
       AND i.archived_at IS NULL AND NOT i.is_draft), 0) AS total_issues, \
     COALESCE((SELECT COUNT(mi.id) FROM module_issues mi \
       INNER JOIN issues i ON mi.issue_id = i.id \
       INNER JOIN states st ON i.state_id = st.id \
       WHERE mi.deleted_at IS NULL AND mi.module_id = modules.id \
       AND i.archived_at IS NULL AND NOT i.is_draft \
       AND st.\"group\" = 'cancelled'), 0) AS cancelled_issues, \
     COALESCE((SELECT COUNT(mi.id) FROM module_issues mi \
       INNER JOIN issues i ON mi.issue_id = i.id \
       INNER JOIN states st ON i.state_id = st.id \
       WHERE mi.deleted_at IS NULL AND mi.module_id = modules.id \
       AND i.archived_at IS NULL AND NOT i.is_draft \
       AND st.\"group\" = 'completed'), 0) AS completed_issues, \
     COALESCE((SELECT COUNT(mi.id) FROM module_issues mi \
       INNER JOIN issues i ON mi.issue_id = i.id \
       INNER JOIN states st ON i.state_id = st.id \
       WHERE mi.deleted_at IS NULL AND mi.module_id = modules.id \
       AND i.archived_at IS NULL AND NOT i.is_draft \
       AND st.\"group\" = 'started'), 0) AS started_issues, \
     COALESCE((SELECT COUNT(mi.id) FROM module_issues mi \
       INNER JOIN issues i ON mi.issue_id = i.id \
       INNER JOIN states st ON i.state_id = st.id \
       WHERE mi.deleted_at IS NULL AND mi.module_id = modules.id \
       AND i.archived_at IS NULL AND NOT i.is_draft \
       AND st.\"group\" = 'unstarted'), 0) AS unstarted_issues, \
     COALESCE((SELECT COUNT(mi.id) FROM module_issues mi \
       INNER JOIN issues i ON mi.issue_id = i.id \
       INNER JOIN states st ON i.state_id = st.id \
       WHERE mi.deleted_at IS NULL AND mi.module_id = modules.id \
       AND i.archived_at IS NULL AND NOT i.is_draft \
       AND st.\"group\" = 'backlog'), 0) AS backlog_issues, \
     modules.created_at AS created_at, modules.updated_at AS updated_at, \
     modules.archived_at AS archived_at \
     FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     INNER JOIN projects ON modules.project_id = projects.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     LEFT OUTER JOIN module_members ON modules.id = module_members.module_id \
     WHERE modules.deleted_at IS NULL AND workspaces.slug = $1 \
     AND project_members.member_id = $2 AND project_members.is_active \
     AND projects.archived_at IS NULL AND modules.archived_at IS NULL \
     GROUP BY modules.id ORDER BY modules.created_at DESC",
    [
        ("id", PgType::Uuid),
        ("workspace_id", PgType::Uuid),
        ("project_id", PgType::Uuid),
        ("name", PgType::Varchar),
        ("description", PgType::Text),
        ("description_text", PgType::Jsonb),
        ("description_html", PgType::Jsonb),
        ("start_date", PgType::Date),
        ("target_date", PgType::Date),
        ("status", PgType::Varchar),
        ("lead_id", PgType::Uuid),
        ("member_ids", PgType::ArrayOf(ArrayElement::Uuid)),
        ("view_props", PgType::Jsonb),
        ("sort_order", PgType::Float8),
        ("external_source", PgType::Varchar),
        ("external_id", PgType::Varchar),
        ("logo_props", PgType::Jsonb),
        ("total_issues", PgType::Int8),
        ("cancelled_issues", PgType::Int8),
        ("completed_issues", PgType::Int8),
        ("started_issues", PgType::Int8),
        ("unstarted_issues", PgType::Int8),
        ("backlog_issues", PgType::Int8),
        ("created_at", PgType::Timestamptz),
        ("updated_at", PgType::Timestamptz),
        ("archived_at", PgType::Timestamptz),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &MODULE_LIST,
    &MODULE_RETRIEVE,
    &MODULE_ARCHIVED_LIST,
    &MODULE_ARCHIVED_RETRIEVE,
    &MODULE_LINKS_LIST,
    &MODULE_LINK_BY_PK,
    &MODULE_LINKS_BY_MODULE,
    &MODULE_USER_PROPERTIES_BY_KEY,
    &MODULE_USER_PROPERTIES_INSERT,
    &MODULE_ESTIMATE_TYPE_EXISTS,
    &MODULE_ASSIGNEES_ISSUES,
    &MODULE_LABELS_ISSUES,
    &MODULE_ASSIGNEES_POINTS,
    &MODULE_LABELS_POINTS,
    &MODULE_BURNDOWN_TOTAL_ESTIMATES,
    &MODULE_BURNDOWN_COMPLETED_POINTS,
    &MODULE_BURNDOWN_COMPLETED_ISSUES,
    &WORKSPACE_MODULES,
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
