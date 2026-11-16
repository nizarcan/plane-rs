// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the module-core writes (routes 103, 104, 112,
//! 113, 114, 115). The read statements live in [`super::queries`].
//!
//! Every statement reproduces the ORM SQL of `plane/app/views/module/base.py`
//! and `archive.py`, so the differential harness sees the same rows Django
//! writes. The soft-delete statements are the H04 shape — a bare `deleted_at`
//! update with no `updated_at`, because `SoftDeletionQuerySet.delete()`
//! (`plane/db/mixins.py:49-53`) is a queryset `.update()`.

use plane_db::catalog::{ArrayElement, PgType};
use plane_db::{Column, Query, plane_query};

/// `plane_query!` with an explicit `PgType` expression, because the shared
/// macro's `$ty:ident` cannot spell an array result column
/// (`PgType::ArrayOf(ArrayElement::Uuid)` for `member_ids`). Same shape as the
/// read catalog's macro in [`super::queries`].
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
    /// `Project.objects.get(workspace__slug=slug, pk=project_id)` for
    /// `ModuleViewSet.create` (`module/base.py:297`). The default manager
    /// filters soft-deleted projects; the join carries the slug filter.
    ///
    /// Bind order: `$1` project id, `$2` slug.
    MODULE_PROJECT = "modules_write_project",
    "SELECT p.id, p.workspace_id FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     WHERE p.deleted_at IS NULL AND p.id = $1 AND w.slug = $2 LIMIT 1",
    [("id", PgType::Uuid), ("workspace_id", PgType::Uuid)]
}

catalog_query! {
    /// `ModuleViewSet.get_queryset().filter(pk=pk)` — the create/PATCH
    /// response's `.values()` row (`module/base.py:303-338`, `:674-706`) and
    /// the PATCH `current_instance`'s instance (`:669`). Unlike
    /// `MODULE_LIST` (`modules/queries.rs`) it does **not** filter
    /// `archived_at`: `create`/`partial_update` use `get_queryset()` without
    /// the list's archived filter, and the archived guard is explicit.
    ///
    /// Bind order: `$1` slug, `$2` project id, `$3` user id, `$4` module id.
    MODULE_WRITE_ROW = "modules_write_row",
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
     modules.created_at AS created_at, modules.updated_at AS updated_at, \
     modules.archived_at AS archived_at \
     FROM modules \
     INNER JOIN workspaces ON modules.workspace_id = workspaces.id \
     LEFT OUTER JOIN module_members ON modules.id = module_members.module_id \
     WHERE modules.deleted_at IS NULL AND modules.project_id = $2 \
     AND workspaces.slug = $1 AND modules.id = $4 \
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
        ("archived_at", PgType::Timestamptz),
    ]
}

plane_query! {
    /// `Module.objects.create(**validated_data, project=project)` with H21 +
    /// `ProjectBaseModel.save` (H07) + H01. Defaults the body does not carry:
    /// `description=''`, `status='planned'`, `view_props={}`, `logo_props={}`,
    /// `sort_order` from H21, `archived_at NULL`, the nullable columns NULL.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, project, workspace, name, description, description_text,
    /// description_html, start_date, target_date, status, lead, view_props,
    /// sort_order, external_source, external_id, archived_at, logo_props.
    MODULE_INSERT = "modules_write_insert",
    "INSERT INTO modules \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, name, description, description_text, \
      description_html, start_date, target_date, status, lead_id, view_props, \
      sort_order, external_source, external_id, archived_at, logo_props) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
       $15, $16, $17, $18, $19, $20, $21, $22)",
    []
}

plane_query! {
    /// The stored row a PATCH loads to run `ModelSerializer.update` +
    /// `instance.save()` (`module/base.py:670-673`). The annotated
    /// `MODULE_WRITE_ROW` carries what the response needs; this carries every
    /// column the full-row save writes back.
    ///
    /// Bind order: `$1` module id.
    MODULE_MODEL_ROW = "modules_write_model_row",
    "SELECT m.created_at, m.updated_at, m.created_by_id, m.updated_by_id, \
     m.deleted_at, m.id, m.project_id, m.workspace_id, m.name, m.description, \
     m.description_text, m.description_html, m.start_date, m.target_date, \
     m.status, m.lead_id, m.view_props, m.sort_order, m.external_source, \
     m.external_id, m.archived_at, m.logo_props \
     FROM modules m WHERE m.deleted_at IS NULL AND m.id = $1 LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("description_text", Jsonb),
        ("description_html", Jsonb),
        ("start_date", Date),
        ("target_date", Date),
        ("status", Varchar),
        ("lead_id", Uuid),
        ("view_props", Jsonb),
        ("sort_order", Float8),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("archived_at", Timestamptz),
        ("logo_props", Jsonb),
    ]
}

plane_query! {
    /// `Module.save()` on an existing row: a full-row UPDATE (no
    /// `update_fields`), after H01 set `updated_by`/`updated_at`.
    ///
    /// Bind order: every column but the id, then the id.
    MODULE_UPDATE = "modules_write_update",
    "UPDATE modules SET created_at = $1, updated_at = $2, created_by_id = $3, \
       updated_by_id = $4, deleted_at = $5, id = $6, project_id = $7, \
       workspace_id = $8, name = $9, description = $10, description_text = $11, \
       description_html = $12, start_date = $13, target_date = $14, \
       status = $15, lead_id = $16, view_props = $17, sort_order = $18, \
       external_source = $19, external_id = $20, archived_at = $21, \
       logo_props = $22 \
     WHERE id = $23",
    []
}

plane_query! {
    /// `Module.objects.filter(name=name, project=project).exists()` — the
    /// create's duplicate-name guard (`module/base.py` in
    /// `serializers/module.py:71`).
    ///
    /// Bind order: `$1` name, `$2` project id.
    MODULE_NAME_EXISTS = "modules_write_name_exists",
    "SELECT 1 AS one FROM modules m \
     WHERE m.deleted_at IS NULL AND m.name = $1 AND m.project_id = $2 LIMIT 1",
    [("one", Int4)]
}

plane_query! {
    /// `Module.objects.filter(name=name, project=instance.project)
    /// .exclude(id=instance.id).exists()` — the update's duplicate-name guard
    /// (`serializers/module.py:99`).
    ///
    /// Bind order: `$1` name, `$2` project id, `$3` excluded module id.
    MODULE_NAME_EXISTS_OTHER = "modules_write_name_exists_other",
    "SELECT 1 AS one FROM modules m \
     WHERE m.deleted_at IS NULL AND m.name = $1 AND m.project_id = $2 \
       AND m.id <> $3 LIMIT 1",
    [("one", Int4)]
}

plane_query! {
    /// `ModuleMember.objects.bulk_create([...], batch_size=10,
    /// ignore_conflicts=True)` (`serializers/module.py:76-90`, `:104-118`).
    /// Every row carries its own generated id and timestamps, `deleted_at`
    /// NULL, and the module's audit ids; `ON CONFLICT DO NOTHING` is
    /// `ignore_conflicts=True`.
    ///
    /// Bind order: `$1` created_by, `$2` updated_by, `$3` project,
    /// `$4` workspace, `$5` module, `$6` created_at array, `$7` updated_at
    /// array, `$8` id array, `$9` member id array.
    MODULE_MEMBER_INSERT = "modules_write_member_insert",
    "INSERT INTO module_members \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, module_id, member_id) \
     SELECT r.created_at, r.updated_at, $1, $2, NULL, r.id, $3, $4, $5, \
       r.member_id \
     FROM unnest($6::timestamptz[], $7::timestamptz[], $8::uuid[], $9::uuid[]) \
       AS r(created_at, updated_at, id, member_id) \
     ON CONFLICT DO NOTHING",
    []
}

plane_query! {
    /// `ModuleMember.objects.filter(module=instance).delete()` in
    /// `ModuleWriteSerializer.update` (`serializers/module.py:103`) — H04: a
    /// bare `deleted_at` update, no `updated_at`, no cascade task.
    ///
    /// Bind order: `$1` deleted_at, `$2` module id.
    MODULE_MEMBER_SOFT_DELETE = "modules_write_member_soft_delete",
    "UPDATE module_members SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND module_id = $2",
    []
}

plane_query! {
    /// `Module.objects.get(workspace__slug=slug, project_id=project_id,
    /// pk=pk)` for `destroy` (`module/base.py:726`).
    ///
    /// Bind order: `$1` slug, `$2` project id, `$3` module id.
    MODULE_DESTROY_LOOKUP = "modules_write_destroy_lookup",
    "SELECT m.id, m.name FROM modules m \
     INNER JOIN workspaces w ON (m.workspace_id = w.id) \
     WHERE m.deleted_at IS NULL AND w.slug = $1 AND m.project_id = $2 \
       AND m.id = $3 LIMIT 1",
    [("id", Uuid), ("name", Varchar)]
}

plane_query! {
    /// `ModuleIssue.objects.filter(module_id=pk).values_list("issue",
    /// flat=True)` for the destroy activity (`module/base.py:728`), in the
    /// model's `-created_at` order.
    ///
    /// Bind order: `$1` module id.
    MODULE_ISSUE_IDS = "modules_write_issue_ids",
    "SELECT mi.issue_id FROM module_issues mi \
     WHERE mi.deleted_at IS NULL AND mi.module_id = $1 \
     ORDER BY mi.created_at DESC",
    [("issue_id", Uuid)]
}

plane_query! {
    /// `module.delete()`'s `self.deleted_at = timezone.now(); self.save()`
    /// (`plane/db/mixins.py:65-77`): the attribute and `auto_now` are two
    /// distinct `timezone.now()` calls, and H01 sets `updated_by`.
    ///
    /// Bind order: `$1` deleted_at, `$2` updated_at, `$3` updated_by,
    /// `$4` module id.
    MODULE_SOFT_DELETE = "modules_write_soft_delete",
    "UPDATE modules SET deleted_at = $1, updated_at = $2, updated_by_id = $3 \
     WHERE id = $4",
    []
}

plane_query! {
    /// `ModuleIssue.objects.filter(module=pk, project_id=project_id).delete()`
    /// in `ModuleViewSet.destroy` (`module/base.py:745`) — H04, no
    /// `updated_at`, live rows only.
    ///
    /// Bind order: `$1` deleted_at, `$2` module id, `$3` project id.
    MODULE_ISSUE_SOFT_DELETE = "modules_write_issue_soft_delete",
    "UPDATE module_issues SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND module_id = $2 AND project_id = $3",
    []
}

plane_query! {
    /// `UserFavorite.objects.filter(user=request.user, entity_type="module",
    /// entity_identifier=pk, project_id=project_id).delete()` in
    /// `ModuleViewSet.destroy` (`module/base.py:747-752`) — H04, the
    /// requesting user only (no workspace filter).
    ///
    /// Bind order: `$1` deleted_at, `$2` user id, `$3` module id,
    /// `$4` project id.
    USER_FAVORITE_SOFT_DELETE_USER = "modules_write_favorite_soft_delete_user",
    "UPDATE user_favorites SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND user_id = $2 AND entity_type = 'module' \
       AND entity_identifier = $3 AND project_id = $4",
    []
}

plane_query! {
    /// `UserRecentVisit.objects.filter(project_id, workspace__slug,
    /// entity_identifier, entity_name="module").delete(soft=False)` in
    /// `ModuleViewSet.destroy` (`module/base.py:754-759`): a hard DELETE for
    /// every user.
    ///
    /// Bind order: `$1` project id, `$2` module id, `$3` slug.
    USER_RECENT_VISIT_HARD_DELETE = "modules_write_recent_visit_delete",
    "DELETE FROM user_recent_visits \
     WHERE project_id = $1 AND entity_identifier = $2 AND entity_name = 'module' \
       AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $3)",
    []
}

plane_query! {
    /// `Module.objects.get(pk=module_id, project_id=project_id,
    /// workspace__slug=slug)` for archive/unarchive
    /// (`module/archive.py:545,562`).
    ///
    /// Bind order: `$1` module id, `$2` project id, `$3` slug.
    MODULE_ARCHIVE_LOOKUP = "modules_write_archive_lookup",
    "SELECT m.id, m.status, m.archived_at FROM modules m \
     INNER JOIN workspaces w ON (m.workspace_id = w.id) \
     WHERE m.deleted_at IS NULL AND m.id = $1 AND m.project_id = $2 \
       AND w.slug = $3 LIMIT 1",
    [
        ("id", Uuid),
        ("status", Varchar),
        ("archived_at", Timestamptz),
    ]
}

plane_query! {
    /// `module.archived_at = timezone.now(); module.save()` for the archive
    /// POST (`module/archive.py:551-552`). The full save writes the same other
    /// values; the two timestamps are distinct `timezone.now()` calls.
    ///
    /// Bind order: `$1` archived_at, `$2` updated_at, `$3` updated_by,
    /// `$4` module id.
    MODULE_SET_ARCHIVED = "modules_write_set_archived",
    "UPDATE modules SET archived_at = $1, updated_at = $2, updated_by_id = $3 \
     WHERE id = $4",
    []
}

plane_query! {
    /// `module.archived_at = None; module.save()` for the archive DELETE
    /// (`module/archive.py:563-564`).
    ///
    /// Bind order: `$1` updated_at, `$2` updated_by, `$3` module id.
    MODULE_CLEAR_ARCHIVED = "modules_write_clear_archived",
    "UPDATE modules SET archived_at = NULL, updated_at = $1, \
       updated_by_id = $2 \
     WHERE id = $3",
    []
}

plane_query! {
    /// `UserFavorite.objects.filter(entity_type="module",
    /// entity_identifier=module_id, project_id=project_id,
    /// workspace__slug=slug).delete()` in the archive POST
    /// (`module/archive.py:553-558`) — H04 for **every user**.
    ///
    /// Bind order: `$1` deleted_at, `$2` module id, `$3` project id,
    /// `$4` slug.
    USER_FAVORITE_SOFT_DELETE_ALL = "modules_write_favorite_soft_delete_all",
    "UPDATE user_favorites SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND entity_type = 'module' \
       AND entity_identifier = $2 AND project_id = $3 \
       AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $4)",
    []
}

plane_query! {
    /// `module_properties.save()` for the user-properties PATCH
    /// (`module/base.py:839-852`): a full-row UPDATE, after H01.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, project, workspace, module, user, filters, display_filters,
    /// display_properties, rich_filters, id.
    MODULE_USER_PROPERTIES_UPDATE = "modules_write_user_properties_update",
    "UPDATE module_user_properties SET created_at = $1, updated_at = $2, \
       created_by_id = $3, updated_by_id = $4, deleted_at = $5, id = $6, \
       project_id = $7, workspace_id = $8, module_id = $9, user_id = $10, \
       filters = $11, display_filters = $12, display_properties = $13, \
       rich_filters = $14 \
     WHERE id = $15",
    []
}

/// Every catalogued statement of this file, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &MODULE_PROJECT,
    &MODULE_WRITE_ROW,
    &MODULE_INSERT,
    &MODULE_MODEL_ROW,
    &MODULE_UPDATE,
    &MODULE_NAME_EXISTS,
    &MODULE_NAME_EXISTS_OTHER,
    &MODULE_MEMBER_INSERT,
    &MODULE_MEMBER_SOFT_DELETE,
    &MODULE_DESTROY_LOOKUP,
    &MODULE_ISSUE_IDS,
    &MODULE_SOFT_DELETE,
    &MODULE_ISSUE_SOFT_DELETE,
    &USER_FAVORITE_SOFT_DELETE_USER,
    &USER_RECENT_VISIT_HARD_DELETE,
    &MODULE_ARCHIVE_LOOKUP,
    &MODULE_SET_ARCHIVED,
    &MODULE_CLEAR_ARCHIVED,
    &USER_FAVORITE_SOFT_DELETE_ALL,
    &MODULE_USER_PROPERTIES_UPDATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count()
            + source.matches(concat!("catalog", "_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
