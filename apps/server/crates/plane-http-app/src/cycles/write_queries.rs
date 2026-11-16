// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the cycle-core writes (routes 31, 32, 35, 36, 37,
//! 39, 40, 41, 42). The read statements live in [`super::queries`].

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Project.objects.get(id=project_id)` for the create's timezone
    /// conversions and `ProjectBaseModel.save`'s workspace
    /// (`plane/app/views/cycle/base.py:276-280`,
    /// `plane/utils/timezone_converter.py:56`).
    ///
    /// Bind order: `$1` project id.
    CYCLE_PROJECT = "cycles_write_project",
    "SELECT p.id, p.workspace_id, p.timezone FROM projects p \
     WHERE p.deleted_at IS NULL AND p.id = $1 LIMIT 1",
    [("id", Uuid), ("workspace_id", Uuid), ("timezone", Varchar)]
}

plane_query! {
    /// `CycleViewSet.get_queryset()`'s annotated row filtered by pk — the
    /// create/update response's `.values()` and the PATCH `current_instance`'s
    /// instance. Unlike `CYCLE_LIST` (`cycles/queries.rs`) it does **not**
    /// filter `cycles.archived_at`: `create`/`partial_update` use
    /// `get_queryset()` without the list's archived filter, and the archive
    /// and completed-cycle guards are explicit in the view.
    ///
    /// Bind order: `$1` slug, `$2` project id, `$3` user id, `$4` the CASE
    /// instant, `$5` cycle id.
    CYCLE_WRITE_ROW = "cycles_write_row",
    "SELECT DISTINCT cycles.id AS id, cycles.workspace_id AS workspace_id, \
     cycles.project_id AS project_id, cycles.name AS name, \
     cycles.description AS description, cycles.start_date AS start_date, \
     cycles.end_date AS end_date, cycles.owned_by_id AS owned_by_id, \
     cycles.view_props AS view_props, cycles.sort_order AS sort_order, \
     cycles.external_source AS external_source, cycles.external_id AS external_id, \
     cycles.progress_snapshot AS progress_snapshot, cycles.logo_props AS logo_props, \
     cycles.version AS version, cycles.archived_at AS archived_at, \
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
     cycles.created_by_id AS created_by \
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
     AND cycles.id = $5 \
     GROUP BY cycles.id",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("start_date", Timestamptz),
        ("end_date", Timestamptz),
        ("owned_by_id", Uuid),
        ("view_props", Jsonb),
        ("sort_order", Float8),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("progress_snapshot", Jsonb),
        ("logo_props", Jsonb),
        ("version", Int4),
        ("archived_at", Timestamptz),
        ("is_favorite", Bool),
        ("total_issues", Int8),
        ("completed_issues", Int8),
        ("cancelled_issues", Int8),
        ("assignee_ids", ArrayOf(Uuid)),
        ("status", Text),
        ("created_by", Uuid),
    ]
}

plane_query! {
    /// `Cycle.objects.create` / `save()` (H22 + `ProjectBaseModel.save` +
    /// H01). Defaults: `description=''`, `view_props={}`, `progress_snapshot={}`,
    /// `logo_props={}`, `timezone='UTC'`, `version=1`, `archived_at NULL`,
    /// external columns NULL.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, project, workspace, name, description, start_date, end_date,
    /// owned_by, view_props, sort_order, external_source, external_id,
    /// progress_snapshot, logo_props, timezone, version.
    CYCLE_INSERT = "cycles_write_insert",
    "INSERT INTO cycles \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, name, description, start_date, end_date, \
      owned_by_id, view_props, sort_order, external_source, external_id, \
      progress_snapshot, archived_at, logo_props, timezone, version) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
       $15, $16, $17, $18, NULL, $19, $20, $21)",
    []
}

plane_query! {
    /// The stored row a PATCH loads to run `ModelSerializer.update` +
    /// `instance.save()` (`cycle/base.py:438-440`). The annotated
    /// `CYCLE_WRITE_ROW` carries what the response needs; this carries every
    /// column the full-row save writes back.
    ///
    /// Bind order: `$1` cycle id.
    CYCLE_MODEL_ROW = "cycles_write_model_row",
    "SELECT c.created_at, c.updated_at, c.created_by_id, c.updated_by_id, \
     c.deleted_at, c.id, c.project_id, c.workspace_id, c.name, c.description, \
     c.start_date, c.end_date, c.owned_by_id, c.view_props, c.sort_order, \
     c.external_source, c.external_id, c.progress_snapshot, c.archived_at, \
     c.logo_props, c.timezone, c.version \
     FROM cycles c WHERE c.deleted_at IS NULL AND c.id = $1 LIMIT 1",
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
        ("start_date", Timestamptz),
        ("end_date", Timestamptz),
        ("owned_by_id", Uuid),
        ("view_props", Jsonb),
        ("sort_order", Float8),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("progress_snapshot", Jsonb),
        ("archived_at", Timestamptz),
        ("logo_props", Jsonb),
        ("timezone", Varchar),
        ("version", Int4),
    ]
}

plane_query! {
    /// `Cycle.save()` on an existing row: a full-row UPDATE (no
    /// `update_fields`), after H01 set `updated_by`/`updated_at`.
    ///
    /// Bind order: every column but the id, then the id.
    CYCLE_UPDATE = "cycles_write_update",
    "UPDATE cycles SET created_at = $1, updated_at = $2, created_by_id = $3, \
       updated_by_id = $4, deleted_at = $5, id = $6, project_id = $7, \
       workspace_id = $8, name = $9, description = $10, start_date = $11, \
       end_date = $12, owned_by_id = $13, view_props = $14, sort_order = $15, \
       external_source = $16, external_id = $17, progress_snapshot = $18, \
       archived_at = $19, logo_props = $20, timezone = $21, version = $22 \
     WHERE id = $23",
    []
}

plane_query! {
    /// `Cycle.objects.get(workspace__slug=slug, project_id=project_id, pk=pk)`
    /// for `destroy` (`base.py:480`).
    ///
    /// Bind order: `$1` slug, `$2` project id, `$3` cycle id.
    CYCLE_DESTROY_LOOKUP = "cycles_write_destroy_lookup",
    "SELECT c.id, c.name FROM cycles c \
     INNER JOIN workspaces w ON (c.workspace_id = w.id) \
     WHERE c.deleted_at IS NULL AND w.slug = $1 AND c.project_id = $2 \
       AND c.id = $3 LIMIT 1",
    [("id", Uuid), ("name", Varchar)]
}

plane_query! {
    /// `cycle.delete()`'s `self.deleted_at = timezone.now(); self.save()`
    /// (`plane/db/mixins.py:65-77`): the attribute and `auto_now` are two
    /// distinct `timezone.now()` calls, and H01 sets `updated_by`.
    ///
    /// Bind order: `$1` deleted_at, `$2` updated_at, `$3` updated_by,
    /// `$4` cycle id.
    CYCLE_SOFT_DELETE = "cycles_write_soft_delete",
    "UPDATE cycles SET deleted_at = $1, updated_at = $2, updated_by_id = $3 \
     WHERE id = $4",
    []
}

plane_query! {
    /// `cycle.archived_at = timezone.now(); cycle.save()` for the archive POST
    /// (`archive.py:596-597`). The full save writes the same other values; the
    /// two timestamps are distinct `timezone.now()` calls.
    ///
    /// Bind order: `$1` archived_at, `$2` updated_at, `$3` updated_by,
    /// `$4` cycle id.
    CYCLE_SET_ARCHIVED = "cycles_write_set_archived",
    "UPDATE cycles SET archived_at = $1, updated_at = $2, updated_by_id = $3 \
     WHERE id = $4",
    []
}

plane_query! {
    /// `cycle.archived_at = None; cycle.save()` for the archive DELETE
    /// (`archive.py:608-610`).
    ///
    /// Bind order: `$1` updated_at, `$2` updated_by, `$3` cycle id.
    CYCLE_CLEAR_ARCHIVED = "cycles_write_clear_archived",
    "UPDATE cycles SET archived_at = NULL, updated_at = $1, updated_by_id = $2 \
     WHERE id = $3",
    []
}

plane_query! {
    /// `CycleIssue.objects.filter(cycle_id=pk).values_list("issue",
    /// flat=True)` for the destroy activity (`base.py:482`), in the model's
    /// `-created_at` order.
    ///
    /// Bind order: `$1` cycle id.
    CYCLE_ISSUE_IDS = "cycles_write_issue_ids",
    "SELECT ci.issue_id FROM cycle_issues ci \
     WHERE ci.deleted_at IS NULL AND ci.cycle_id = $1 \
     ORDER BY ci.created_at DESC",
    [("issue_id", Uuid)]
}

plane_query! {
    /// `Cycle.objects.get(pk=cycle_id, project_id=project_id,
    /// workspace__slug=slug)` for archive/unarchive (`archive.py:588,608`).
    ///
    /// Bind order: `$1` cycle id, `$2` project id, `$3` slug.
    CYCLE_ARCHIVE_LOOKUP = "cycles_write_archive_lookup",
    "SELECT c.id, c.end_date, c.archived_at FROM cycles c \
     INNER JOIN workspaces w ON (c.workspace_id = w.id) \
     WHERE c.deleted_at IS NULL AND c.id = $1 AND c.project_id = $2 \
       AND w.slug = $3 LIMIT 1",
    [("id", Uuid), ("end_date", Timestamptz), ("archived_at", Timestamptz)]
}

plane_query! {
    /// `CycleDateCheckEndpoint.post`'s overlap query (`base.py:539-546`).
    ///
    /// Bind order: `$1` slug, `$2` project id, `$3` start, `$4` end,
    /// `$5` excluded cycle id (`NULL` = no exclusion).
    CYCLE_DATE_OVERLAP = "cycles_write_date_overlap",
    "SELECT 1 AS one FROM cycles c \
     INNER JOIN workspaces w ON (c.workspace_id = w.id) \
     WHERE c.deleted_at IS NULL AND w.slug = $1 AND c.project_id = $2 \
       AND ((c.start_date <= $3 AND c.end_date >= $3) \
         OR (c.start_date <= $4 AND c.end_date >= $4) \
         OR (c.start_date >= $3 AND c.end_date <= $4)) \
       AND ($5::uuid IS NULL OR c.id <> $5) LIMIT 1",
    [("one", Int4)]
}

plane_query! {
    /// `get_favorite_entities("cycle", user, project).get(pk=cycle,
    /// workspace__slug=slug)` (`plane/app/serializers/favorite.py:70-99`):
    /// an active membership of the unarchived project.
    ///
    /// Bind order: `$1` project id, `$2` cycle id, `$3` slug, `$4` user id.
    CYCLE_FAVORITE_TARGET = "cycles_write_favorite_target",
    "SELECT c.id, c.workspace_id FROM cycles c \
     INNER JOIN workspaces w ON (c.workspace_id = w.id) \
     WHERE c.deleted_at IS NULL AND c.project_id = $1 AND c.id = $2 \
       AND w.slug = $3 \
       AND EXISTS (SELECT 1 FROM project_members pm \
                   INNER JOIN projects p2 ON (p2.id = pm.project_id) \
                   WHERE pm.project_id = $1 AND pm.member_id = $4 \
                     AND pm.is_active AND pm.deleted_at IS NULL \
                     AND p2.archived_at IS NULL) \
     LIMIT 1",
    [("id", Uuid), ("workspace_id", Uuid)]
}

plane_query! {
    /// `UserFavorite.objects.create(...)` (route 36). The sequence comes from
    /// H25; `name` is NULL, `is_folder` false, `parent_id` NULL.
    ///
    /// Bind order: created/updated, created_by, id, workspace, project, user,
    /// entity_identifier, sequence.
    USER_FAVORITE_INSERT = "cycles_write_favorite_insert",
    "INSERT INTO user_favorites \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, project_id, user_id, entity_type, entity_identifier, name, \
      is_folder, sequence, parent_id) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, 'cycle', $8, NULL, \
       FALSE, $9, NULL)",
    []
}

plane_query! {
    /// `UserFavorite.objects.get(project=…, entity_type="cycle",
    /// user=request.user, workspace__slug=…, entity_identifier=cycle_id)` —
    /// the favourite destroy lookup (`base.py:588-594`).
    ///
    /// Bind order: `$1` project id, `$2` user id, `$3` slug, `$4` cycle id.
    USER_FAVORITE_LOOKUP = "cycles_write_favorite_lookup",
    "SELECT uf.id FROM user_favorites uf \
     INNER JOIN workspaces w ON (uf.workspace_id = w.id) \
     WHERE uf.deleted_at IS NULL AND uf.project_id = $1 \
       AND uf.entity_type = 'cycle' AND uf.user_id = $2 AND w.slug = $3 \
       AND uf.entity_identifier = $4 LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `UserFavorite.objects.filter(user=request.user, entity_type="cycle",
    /// entity_identifier=pk, project_id=project_id).delete()` in
    /// `CycleViewSet.destroy` (`base.py:487-492`) — H04, live rows only, the
    /// requesting user only (no workspace filter).
    ///
    /// Bind order: `$1` deleted_at, `$2` user id, `$3` cycle id,
    /// `$4` project id.
    USER_FAVORITE_SOFT_DELETE_USER = "cycles_write_favorite_soft_delete_user",
    "UPDATE user_favorites SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND user_id = $2 AND entity_type = 'cycle' \
       AND entity_identifier = $3 AND project_id = $4",
    []
}

plane_query! {
    /// `UserFavorite.objects.filter(entity_type="cycle",
    /// entity_identifier=cycle_id, project_id, workspace__slug).delete()` in
    /// the archive POST (`archive.py:599-604`) — H04 for **every user**.
    ///
    /// Bind order: `$1` deleted_at, `$2` cycle id, `$3` project id,
    /// `$4` slug.
    USER_FAVORITE_SOFT_DELETE_ALL = "cycles_write_favorite_soft_delete_all",
    "UPDATE user_favorites SET deleted_at = $1 \
     WHERE deleted_at IS NULL AND entity_type = 'cycle' \
       AND entity_identifier = $2 AND project_id = $3 \
       AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $4)",
    []
}

plane_query! {
    /// `UserRecentVisit.objects.filter(project_id, workspace__slug,
    /// entity_identifier, entity_name="cycle").delete(soft=False)` in
    /// `CycleViewSet.destroy` (`base.py:494-499`): a hard DELETE for every
    /// user. `UserRecentVisit` is a `SoftDeleteModel`, so `objects` is the
    /// `SoftDeletionManager`: its queryset already carries
    /// `deleted_at IS NULL` (`plane/db/mixins.py:57-58`) and `delete(soft=False)`
    /// keeps that predicate — an already-soft-deleted visit survives.
    ///
    /// Bind order: `$1` project id, `$2` cycle id, `$3` slug.
    USER_RECENT_VISIT_HARD_DELETE = "cycles_write_recent_visit_delete",
    "DELETE FROM user_recent_visits \
     WHERE deleted_at IS NULL AND project_id = $1 AND entity_identifier = $2 \
       AND entity_name = 'cycle' \
       AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $3)",
    []
}

plane_query! {
    /// `cycle_properties.save()` for the user-properties PATCH
    /// (`base.py:632-649`): a full-row UPDATE, after H01.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, project, workspace, cycle, user, filters, display_filters,
    /// display_properties, rich_filters, id.
    CYCLE_USER_PROPERTIES_UPDATE = "cycles_write_user_properties_update",
    "UPDATE cycle_user_properties SET created_at = $1, updated_at = $2, \
       created_by_id = $3, updated_by_id = $4, deleted_at = $5, id = $6, \
       project_id = $7, workspace_id = $8, cycle_id = $9, user_id = $10, \
       filters = $11, display_filters = $12, display_properties = $13, \
       rich_filters = $14 \
     WHERE id = $15",
    []
}

/// Every catalogued statement of this file, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &CYCLE_PROJECT,
    &CYCLE_WRITE_ROW,
    &CYCLE_MODEL_ROW,
    &CYCLE_INSERT,
    &CYCLE_UPDATE,
    &CYCLE_DESTROY_LOOKUP,
    &CYCLE_SOFT_DELETE,
    &CYCLE_SET_ARCHIVED,
    &CYCLE_CLEAR_ARCHIVED,
    &CYCLE_ISSUE_IDS,
    &CYCLE_ARCHIVE_LOOKUP,
    &CYCLE_DATE_OVERLAP,
    &CYCLE_FAVORITE_TARGET,
    &USER_FAVORITE_INSERT,
    &USER_FAVORITE_LOOKUP,
    &USER_FAVORITE_SOFT_DELETE_USER,
    &USER_FAVORITE_SOFT_DELETE_ALL,
    &USER_RECENT_VISIT_HARD_DELETE,
    &CYCLE_USER_PROPERTIES_UPDATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
