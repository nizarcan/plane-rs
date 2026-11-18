// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the v1 project routes (307–311).
//!
//! Python: `plane/api/views/project.py:81-821`. The list and the detail read
//! run the same annotated queryset; the detail adds `pk = $3`. Both order by a
//! caller-chosen allowlisted column, so the base statement is catalogued
//! without its `ORDER BY`/`LIMIT` (the handler appends them, like the app
//! surface's `projects` group does for its dynamic ordering).
//!
//! The eight annotations are the ones `ProjectSerializer`
//! (`plane/api/serializers/project.py:210-240`) declares. They are correlated
//! subqueries, exactly as Django compiles them:
//!
//! * `total_members`/`total_cycles`/`total_modules` are scalar aggregate
//!   subqueries. Django **drops the `GROUP BY`** that the inner
//!   `.values("count")` would otherwise carry (checked against a live Django
//!   process: `(SELECT Count(U0."id") FROM … WHERE …)`), so the value is `0`
//!   for a project with no rows, never `NULL`;
//! * `member_role` and `sort_order` are single-value subqueries over the
//!   caller's live `project_members` row;
//! * `is_member` and `is_deployed` are `EXISTS`.
//!
//! `cover_image_url` is a model property (`project.py:128-139`), not an
//! annotation; the handler resolves it through one batched asset query.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `ProjectListCreateAPIEndpoint.get_queryset()`
    /// (`plane/api/views/project.py:90-148`) plus the serializer's
    /// `sort_order` annotation (`:177-201`).
    ///
    /// Bind order: `$1` the workspace slug, `$2` the caller's user id.
    /// `?order_by=` is appended by the handler from the allowlist
    /// (`plane/utils/order_queryset.py:57-63`), then `LIMIT`/`OFFSET`.
    PROJECT_LIST_BASE = "v1_projects_list_base",
    "SELECT DISTINCT p.created_at, p.updated_at, p.created_by_id, p.updated_by_id, \
     p.deleted_at, p.id, p.name, p.description, p.description_text, \
     p.description_html, p.network, p.workspace_id, p.identifier, \
     p.default_assignee_id, p.project_lead_id, p.emoji, p.icon_prop, \
     p.module_view, p.cycle_view, p.issue_views_view, p.page_view, \
     p.intake_view, p.is_time_tracking_enabled, p.is_issue_type_enabled, \
     p.guest_view_all_features, p.cover_image, p.cover_image_asset_id, \
     p.estimate_id, p.archive_in, p.close_in, p.logo_props, \
     p.default_state_id, p.archived_at, p.timezone, p.external_source, \
     p.external_id, \
     EXISTS (SELECT 1 FROM project_members em \
             INNER JOIN workspaces ew ON ew.id = em.workspace_id \
             WHERE em.member_id = $2 AND em.project_id = p.id \
               AND ew.slug = $1 AND em.is_active AND em.deleted_at IS NULL) \
       AS is_member, \
     (SELECT count(em2.id) FROM project_members em2 \
        INNER JOIN users eu2 ON eu2.id = em2.member_id \
        WHERE em2.project_id = p.id AND eu2.is_bot = false AND em2.is_active \
          AND em2.deleted_at IS NULL) AS total_members, \
     (SELECT count(c.id) FROM cycles c \
        WHERE c.project_id = p.id AND c.deleted_at IS NULL) AS total_cycles, \
     (SELECT count(m.id) FROM modules m \
        WHERE m.project_id = p.id AND m.deleted_at IS NULL) AS total_modules, \
     (SELECT em3.role FROM project_members em3 \
        WHERE em3.project_id = p.id AND em3.member_id = $2 \
          AND em3.is_active AND em3.deleted_at IS NULL) AS member_role, \
     EXISTS (SELECT 1 FROM deploy_boards ed \
             INNER JOIN workspaces ew2 ON ew2.id = ed.workspace_id \
             WHERE ed.project_id = p.id AND ew2.slug = $1 \
               AND ed.deleted_at IS NULL) AS is_deployed, \
     (SELECT em4.sort_order FROM project_members em4 \
        INNER JOIN workspaces ew3 ON ew3.id = em4.workspace_id \
        WHERE em4.member_id = $2 AND em4.project_id = p.id \
          AND ew3.slug = $1 AND em4.is_active \
          AND em4.deleted_at IS NULL) AS sort_order \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     LEFT OUTER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 \
       AND ((pm.member_id = $2 AND pm.is_active AND pm.deleted_at IS NULL) \
            OR p.network = 2)",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("description_text", Jsonb),
        ("description_html", Jsonb),
        ("network", Int2),
        ("workspace_id", Uuid),
        ("identifier", Varchar),
        ("default_assignee_id", Uuid),
        ("project_lead_id", Uuid),
        ("emoji", Varchar),
        ("icon_prop", Jsonb),
        ("module_view", Bool),
        ("cycle_view", Bool),
        ("issue_views_view", Bool),
        ("page_view", Bool),
        ("intake_view", Bool),
        ("is_time_tracking_enabled", Bool),
        ("is_issue_type_enabled", Bool),
        ("guest_view_all_features", Bool),
        ("cover_image", Text),
        ("cover_image_asset_id", Uuid),
        ("estimate_id", Uuid),
        ("archive_in", Int4),
        ("close_in", Int4),
        ("logo_props", Jsonb),
        ("default_state_id", Uuid),
        ("archived_at", Timestamptz),
        ("timezone", Varchar),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("is_member", Bool),
        ("total_members", Int8),
        ("total_cycles", Int8),
        ("total_modules", Int8),
        ("member_role", Int2),
        ("is_deployed", Bool),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// [`PROJECT_LIST_BASE`] with `pk = $3` — `ProjectDetailAPIEndpoint`'s
    /// `get_queryset().get(workspace__slug=slug, pk=pk)`
    /// (`plane/api/views/project.py:444-527`).
    PROJECT_DETAIL_ONE = "v1_projects_detail_one",
    "SELECT DISTINCT p.created_at, p.updated_at, p.created_by_id, p.updated_by_id, \
     p.deleted_at, p.id, p.name, p.description, p.description_text, \
     p.description_html, p.network, p.workspace_id, p.identifier, \
     p.default_assignee_id, p.project_lead_id, p.emoji, p.icon_prop, \
     p.module_view, p.cycle_view, p.issue_views_view, p.page_view, \
     p.intake_view, p.is_time_tracking_enabled, p.is_issue_type_enabled, \
     p.guest_view_all_features, p.cover_image, p.cover_image_asset_id, \
     p.estimate_id, p.archive_in, p.close_in, p.logo_props, \
     p.default_state_id, p.archived_at, p.timezone, p.external_source, \
     p.external_id, \
     EXISTS (SELECT 1 FROM project_members em \
             INNER JOIN workspaces ew ON ew.id = em.workspace_id \
             WHERE em.member_id = $2 AND em.project_id = p.id \
               AND ew.slug = $1 AND em.is_active AND em.deleted_at IS NULL) \
       AS is_member, \
     (SELECT count(em2.id) FROM project_members em2 \
        INNER JOIN users eu2 ON eu2.id = em2.member_id \
        WHERE em2.project_id = p.id AND eu2.is_bot = false AND em2.is_active \
          AND em2.deleted_at IS NULL) AS total_members, \
     (SELECT count(c.id) FROM cycles c \
        WHERE c.project_id = p.id AND c.deleted_at IS NULL) AS total_cycles, \
     (SELECT count(m.id) FROM modules m \
        WHERE m.project_id = p.id AND m.deleted_at IS NULL) AS total_modules, \
     (SELECT em3.role FROM project_members em3 \
        WHERE em3.project_id = p.id AND em3.member_id = $2 \
          AND em3.is_active AND em3.deleted_at IS NULL) AS member_role, \
     EXISTS (SELECT 1 FROM deploy_boards ed \
             INNER JOIN workspaces ew2 ON ew2.id = ed.workspace_id \
             WHERE ed.project_id = p.id AND ew2.slug = $1 \
               AND ed.deleted_at IS NULL) AS is_deployed, \
     (SELECT em4.sort_order FROM project_members em4 \
        INNER JOIN workspaces ew3 ON ew3.id = em4.workspace_id \
        WHERE em4.member_id = $2 AND em4.project_id = p.id \
          AND ew3.slug = $1 AND em4.is_active \
          AND em4.deleted_at IS NULL) AS sort_order \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     LEFT OUTER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 \
       AND ((pm.member_id = $2 AND pm.is_active AND pm.deleted_at IS NULL) \
            OR p.network = 2) \
       AND p.id = $3",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("description_text", Jsonb),
        ("description_html", Jsonb),
        ("network", Int2),
        ("workspace_id", Uuid),
        ("identifier", Varchar),
        ("default_assignee_id", Uuid),
        ("project_lead_id", Uuid),
        ("emoji", Varchar),
        ("icon_prop", Jsonb),
        ("module_view", Bool),
        ("cycle_view", Bool),
        ("issue_views_view", Bool),
        ("page_view", Bool),
        ("intake_view", Bool),
        ("is_time_tracking_enabled", Bool),
        ("is_issue_type_enabled", Bool),
        ("guest_view_all_features", Bool),
        ("cover_image", Text),
        ("cover_image_asset_id", Uuid),
        ("estimate_id", Uuid),
        ("archive_in", Int4),
        ("close_in", Int4),
        ("logo_props", Jsonb),
        ("default_state_id", Uuid),
        ("archived_at", Timestamptz),
        ("timezone", Varchar),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("is_member", Bool),
        ("total_members", Int8),
        ("total_cycles", Int8),
        ("total_modules", Int8),
        ("member_role", Int2),
        ("is_deployed", Bool),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `projects.count()` over [`PROJECT_LIST_BASE`] with the same predicates
    /// (`plane/utils/paginator.py:169`). `DISTINCT` on the pk is the same row
    /// set as Django's distinct over every selected column, because each
    /// annotation is a function of the project row alone.
    PROJECT_LIST_COUNT = "v1_projects_list_count",
    "SELECT COUNT(*) FROM ( \
       SELECT DISTINCT p.id FROM projects p \
       INNER JOIN workspaces w ON (p.workspace_id = w.id) \
       LEFT OUTER JOIN project_members pm ON (p.id = pm.project_id) \
       WHERE p.deleted_at IS NULL AND w.slug = $1 \
         AND ((pm.member_id = $2 AND pm.is_active AND pm.deleted_at IS NULL) \
              OR p.network = 2) \
     ) AS \"__count\"",
    [("count", Int8)]
}

plane_query! {
    /// `ProjectListLiteAPIEndpoint.get_queryset()`
    /// (`plane/api/views/project.py:354-367`): the same membership/public
    /// filter, `.distinct()`, then `archived_at__isnull=True` unless
    /// `?include_archived=true` (`:416-418`). The filtered variant is a second
    /// statement so neither plan carries an unused parameter.
    ///
    /// `ProjectLiteSerializer`'s `cover_image_url` is a model property, so it
    /// is not part of the statement.
    PROJECT_LITE_PAGE = "v1_projects_lite_page",
    "SELECT DISTINCT p.id, p.identifier, p.name, p.cover_image, p.icon_prop, \
     p.emoji, p.description, p.cover_image_asset_id, p.archived_at, \
     p.created_at, p.updated_at \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     LEFT OUTER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 \
       AND ((pm.member_id = $2 AND pm.is_active AND pm.deleted_at IS NULL) \
            OR p.network = 2)",
    [
        ("id", Uuid),
        ("identifier", Varchar),
        ("name", Varchar),
        ("cover_image", Text),
        ("icon_prop", Jsonb),
        ("emoji", Varchar),
        ("description", Text),
        ("cover_image_asset_id", Uuid),
        ("archived_at", Timestamptz),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
    ]
}

plane_query! {
    /// [`PROJECT_LITE_PAGE`] with `archived_at IS NULL`
    /// (`plane/api/views/project.py:418`).
    PROJECT_LITE_PAGE_LIVE = "v1_projects_lite_page_live",
    "SELECT DISTINCT p.id, p.identifier, p.name, p.cover_image, p.icon_prop, \
     p.emoji, p.description, p.cover_image_asset_id, p.archived_at, \
     p.created_at, p.updated_at \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     LEFT OUTER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 \
       AND ((pm.member_id = $2 AND pm.is_active AND pm.deleted_at IS NULL) \
            OR p.network = 2) \
       AND p.archived_at IS NULL",
    [
        ("id", Uuid),
        ("identifier", Varchar),
        ("name", Varchar),
        ("cover_image", Text),
        ("icon_prop", Jsonb),
        ("emoji", Varchar),
        ("description", Text),
        ("cover_image_asset_id", Uuid),
        ("archived_at", Timestamptz),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
    ]
}

plane_query! {
    /// [`PROJECT_LITE_PAGE`]'s count.
    PROJECT_LITE_COUNT = "v1_projects_lite_count",
    "SELECT COUNT(*) FROM ( \
       SELECT DISTINCT p.id FROM projects p \
       INNER JOIN workspaces w ON (p.workspace_id = w.id) \
       LEFT OUTER JOIN project_members pm ON (p.id = pm.project_id) \
       WHERE p.deleted_at IS NULL AND w.slug = $1 \
         AND ((pm.member_id = $2 AND pm.is_active AND pm.deleted_at IS NULL) \
              OR p.network = 2) \
     ) AS \"__count\"",
    [("count", Int8)]
}

plane_query! {
    /// [`PROJECT_LITE_PAGE_LIVE`]'s count.
    PROJECT_LITE_COUNT_LIVE = "v1_projects_lite_count_live",
    "SELECT COUNT(*) FROM ( \
       SELECT DISTINCT p.id FROM projects p \
       INNER JOIN workspaces w ON (p.workspace_id = w.id) \
       LEFT OUTER JOIN project_members pm ON (p.id = pm.project_id) \
       WHERE p.deleted_at IS NULL AND w.slug = $1 \
         AND ((pm.member_id = $2 AND pm.is_active AND pm.deleted_at IS NULL) \
              OR p.network = 2) \
         AND p.archived_at IS NULL \
     ) AS \"__count\"",
    [("count", Int8)]
}

plane_query! {
    /// The nested `UserLiteSerializer` rows for `?expand=`
    /// (`plane/api/serializers/base.py:96-115` → the forward FK descriptors,
    /// which use the base manager, so no `deleted_at` filter exists on `users`).
    USERS_LITE_BY_IDS = "v1_projects_users_lite_by_ids",
    "SELECT u.id, u.first_name, u.last_name, u.email, u.avatar, \
     u.avatar_asset_id, u.display_name \
     FROM users u WHERE u.id = ANY($1)",
    [
        ("id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("email", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("display_name", Varchar),
    ]
}

plane_query! {
    /// The `WorkspaceLiteSerializer` for `?expand=workspace`
    /// (`Project.workspace`'s forward descriptor; the workspace's own
    /// `deleted_at` is not consulted).
    WORKSPACE_LITE = "v1_projects_workspace_lite",
    "SELECT w.name, w.slug, w.id FROM workspaces w WHERE w.slug = $1 LIMIT 1",
    [("name", Varchar), ("slug", Varchar), ("id", Uuid)]
}

plane_query! {
    /// `Project.objects.filter(pk=project_id, workspace__slug=slug).first()`
    /// (`plane/api/views/project.py:729`). `.first()` orders by the model's
    /// default `-created_at` and takes one row.
    SUMMARY_PROJECT = "v1_projects_summary_project",
    "SELECT p.id, p.name, p.identifier \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     WHERE p.deleted_at IS NULL AND p.id = $1 AND w.slug = $2 \
     ORDER BY p.created_at DESC LIMIT 1",
    [("id", Uuid), ("name", Varchar), ("identifier", Varchar)]
}

plane_query! {
    /// `_get_all_summary_counts` (`plane/api/views/project.py:749-821`) with
    /// every allowed field's subquery in one row, in
    /// `ALLOWED_PROJECT_SUMMARY_FIELDS` order.
    ///
    /// `issues` uses `Issue.objects`, whose manager excludes triage states,
    /// archived issues, archived projects and drafts
    /// (`plane/db/models/issue.py:92-101`), and the view excludes triage again.
    SUMMARY_COUNTS = "v1_projects_summary_counts",
    "SELECT \
       (SELECT count(*) FROM project_members pm \
         WHERE pm.deleted_at IS NULL AND pm.project_id = $1 AND pm.is_active) AS members, \
       (SELECT count(*) FROM states s \
         WHERE s.deleted_at IS NULL AND s.project_id = $1 \
           AND NOT (s.\"group\" = 'triage')) AS states, \
       (SELECT count(*) FROM labels l \
         WHERE l.deleted_at IS NULL AND l.project_id = $1) AS labels, \
       (SELECT count(*) FROM cycles c \
         WHERE c.deleted_at IS NULL AND c.project_id = $1) AS cycles, \
       (SELECT count(*) FROM modules m \
         WHERE m.deleted_at IS NULL AND m.project_id = $1) AS modules, \
       (SELECT count(*) FROM issues i \
         WHERE i.deleted_at IS NULL AND i.project_id = $1 \
           AND i.state_id IN (SELECT ts.id FROM states ts \
                              WHERE ts.deleted_at IS NULL \
                                AND NOT (ts.\"group\" = 'triage'))) AS issues, \
       (SELECT count(*) FROM intake_issues ii \
         WHERE ii.deleted_at IS NULL AND ii.project_id = $1) AS intakes, \
       (SELECT count(*) FROM project_pages pp \
         WHERE pp.deleted_at IS NULL AND pp.project_id = $1) AS pages",
    [
        ("members", Int8),
        ("states", Int8),
        ("labels", Int8),
        ("cycles", Int8),
        ("modules", Int8),
        ("issues", Int8),
        ("intakes", Int8),
        ("pages", Int8),
    ]
}

plane_query! {
    /// `Project.objects.get(pk=project_id, workspace__slug=slug)`
    /// (`plane/api/views/project.py:678,702`) — the plain row, not the
    /// annotated queryset.
    PROJECT_BY_ID_IN_WORKSPACE = "v1_projects_by_id_in_workspace",
    "SELECT p.created_at, p.updated_at, p.created_by_id, p.updated_by_id, \
     p.deleted_at, p.id, p.name, p.description, p.description_text, \
     p.description_html, p.network, p.workspace_id, p.identifier, \
     p.default_assignee_id, p.project_lead_id, p.emoji, p.icon_prop, \
     p.module_view, p.cycle_view, p.issue_views_view, p.page_view, \
     p.intake_view, p.is_time_tracking_enabled, p.is_issue_type_enabled, \
     p.guest_view_all_features, p.cover_image, p.cover_image_asset_id, \
     p.estimate_id, p.archive_in, p.close_in, p.logo_props, \
     p.default_state_id, p.archived_at, p.timezone, p.external_source, \
     p.external_id \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     WHERE p.deleted_at IS NULL AND p.id = $1 AND w.slug = $2 \
     LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("description_text", Jsonb),
        ("description_html", Jsonb),
        ("network", Int2),
        ("workspace_id", Uuid),
        ("identifier", Varchar),
        ("default_assignee_id", Uuid),
        ("project_lead_id", Uuid),
        ("emoji", Varchar),
        ("icon_prop", Jsonb),
        ("module_view", Bool),
        ("cycle_view", Bool),
        ("issue_views_view", Bool),
        ("page_view", Bool),
        ("intake_view", Bool),
        ("is_time_tracking_enabled", Bool),
        ("is_issue_type_enabled", Bool),
        ("guest_view_all_features", Bool),
        ("cover_image", Text),
        ("cover_image_asset_id", Uuid),
        ("estimate_id", Uuid),
        ("archive_in", Int4),
        ("close_in", Int4),
        ("logo_props", Jsonb),
        ("default_state_id", Uuid),
        ("archived_at", Timestamptz),
        ("timezone", Varchar),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

plane_query! {
    /// `project.archived_at = timezone.now(); project.save()`
    /// (`plane/api/views/project.py:678-680`). `Project.save` has no override
    /// beyond `BaseModel.save` (H01's update branch stamps `updated_by`).
    PROJECT_ARCHIVE = "v1_projects_archive",
    "UPDATE projects SET archived_at = $1, updated_at = $1, updated_by_id = $2 \
     WHERE id = $3",
    []
}

plane_query! {
    /// `project.archived_at = None; project.save()`
    /// (`plane/api/views/project.py:702-704`).
    PROJECT_UNARCHIVE = "v1_projects_unarchive",
    "UPDATE projects SET archived_at = NULL, updated_at = $1, \
     updated_by_id = $2 \
     WHERE id = $3",
    []
}

plane_query! {
    /// `UserFavorite.objects.filter(workspace__slug=slug, project=project_id)
    ///  .delete()` (`plane/api/views/project.py:681`) — a queryset delete, so
    /// it is a soft delete of the matching rows (`SoftDeletionQuerySet.delete`
    /// at `plane/db/mixins.py:45-50`).
    FAVORITES_SOFT_DELETE = "v1_projects_favorites_soft_delete",
    "UPDATE user_favorites uf SET deleted_at = $1 \
     FROM workspaces w \
     WHERE uf.workspace_id = w.id AND w.slug = $2 AND uf.project_id = $3 \
       AND uf.deleted_at IS NULL",
    []
}

// ---------------------------------------------------------------------------
// Writes (307 POST, 309 PATCH/DELETE)
// ---------------------------------------------------------------------------

plane_query! {
    /// `Project.objects.get(pk=pk)` in `ProjectDetailAPIEndpoint.patch`
    /// (`api/views/project.py:558`) — **no workspace filter** and the
    /// soft-delete manager only.
    ///
    /// Bind order: `$1` the project id.
    PROJECT_BY_PK_ANY = "v1_projects_by_pk_any",
    "SELECT p.created_at, p.updated_at, p.created_by_id, p.updated_by_id, \
     p.deleted_at, p.id, p.name, p.description, p.description_text, \
     p.description_html, p.network, p.workspace_id, p.identifier, \
     p.default_assignee_id, p.project_lead_id, p.emoji, p.icon_prop, \
     p.module_view, p.cycle_view, p.issue_views_view, p.page_view, \
     p.intake_view, p.is_time_tracking_enabled, p.is_issue_type_enabled, \
     p.guest_view_all_features, p.cover_image, p.cover_image_asset_id, \
     p.estimate_id, p.archive_in, p.close_in, p.logo_props, \
     p.default_state_id, p.archived_at, p.timezone, p.external_source, \
     p.external_id \
     FROM projects p WHERE p.deleted_at IS NULL AND p.id = $1 LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("description_text", Jsonb),
        ("description_html", Jsonb),
        ("network", Int2),
        ("workspace_id", Uuid),
        ("identifier", Varchar),
        ("default_assignee_id", Uuid),
        ("project_lead_id", Uuid),
        ("emoji", Varchar),
        ("icon_prop", Jsonb),
        ("module_view", Bool),
        ("cycle_view", Bool),
        ("issue_views_view", Bool),
        ("page_view", Bool),
        ("intake_view", Bool),
        ("is_time_tracking_enabled", Bool),
        ("is_issue_type_enabled", Bool),
        ("guest_view_all_features", Bool),
        ("cover_image", Text),
        ("cover_image_asset_id", Uuid),
        ("estimate_id", Uuid),
        ("archive_in", Int4),
        ("close_in", Int4),
        ("logo_props", Jsonb),
        ("default_state_id", Uuid),
        ("archived_at", Timestamptz),
        ("timezone", Varchar),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

plane_db::plane_query! {
    /// `UserFavorite.objects.filter(entity_type="project",
    /// entity_identifier=pk, project_id=pk).delete()`
    /// (`api/views/project.py:635`) — H04, a bare `deleted_at` update.
    ///
    /// Bind order: `$1` deleted_at, `$2` the project id (both
    /// `entity_identifier` and `project_id`).
    PROJECT_FAVORITES_BY_PROJECT = "v1_projects_favorites_by_project",
    "UPDATE user_favorites SET deleted_at = $1 \
     WHERE user_favorites.deleted_at IS NULL \
       AND user_favorites.entity_type = 'project' \
       AND user_favorites.entity_identifier = $2 \
       AND user_favorites.project_id = $2",
    []
}

/// `Project` states that `ProjectCreateSerializer` bulk-inserts:
/// `DEFAULT_STATES` (`plane/db/models/state.py:24-61`).
pub struct DefaultState {
    pub name: &'static str,
    pub color: &'static str,
    pub sequence: f64,
    pub group: &'static str,
    pub default: bool,
}

/// The six default states, in `DEFAULT_STATES` order.
pub const DEFAULT_STATES: [DefaultState; 6] = [
    DefaultState {
        name: "Backlog",
        color: "#60646C",
        sequence: 15000.0,
        group: "backlog",
        default: true,
    },
    DefaultState {
        name: "Todo",
        color: "#60646C",
        sequence: 25000.0,
        group: "unstarted",
        default: false,
    },
    DefaultState {
        name: "In Progress",
        color: "#F59E0B",
        sequence: 35000.0,
        group: "started",
        default: false,
    },
    DefaultState {
        name: "Done",
        color: "#46A758",
        sequence: 45000.0,
        group: "completed",
        default: false,
    },
    DefaultState {
        name: "Cancelled",
        color: "#9AA4BC",
        sequence: 55000.0,
        group: "cancelled",
        default: false,
    },
    DefaultState {
        name: "Triage",
        color: "#4E5355",
        sequence: 65000.0,
        group: "triage",
        default: false,
    },
];

plane_db::plane_query! {
    /// `PrimaryKeyRelatedField(queryset=User.objects)` for `project_lead` and
    /// `default_assignee` (`write_pk_project_create_serializer_*`).
    ///
    /// Bind order: `$1` the pk.
    PROJECT_USER_BY_ID = "v1_projects_user_by_id",
    "SELECT \"users\".\"id\" AS \"pk\" FROM \"users\" \
     WHERE \"users\".\"id\" = $1",
    [("pk", Uuid)]
}

plane_db::plane_query! {
    /// `PrimaryKeyRelatedField(queryset=State.objects)` for `default_state`
    /// (`write_pk_project_update_serializer_default_state`): `StateManager`
    /// hides soft-deleted rows and triage states.
    ///
    /// Bind order: `$1` the pk.
    PROJECT_STATE_BY_ID = "v1_projects_state_by_id",
    "SELECT \"states\".\"id\" AS \"pk\" FROM \"states\" \
     WHERE (\"states\".\"deleted_at\" IS NULL \
            AND NOT (\"states\".\"group\" = 'triage') \
            AND \"states\".\"id\" = $1)",
    [("pk", Uuid)]
}

plane_db::plane_query! {
    /// `PrimaryKeyRelatedField(queryset=Estimate.objects)` for `estimate`
    /// (`write_pk_project_update_serializer_estimate`).
    ///
    /// Bind order: `$1` the pk.
    PROJECT_ESTIMATE_BY_ID = "v1_projects_estimate_by_id",
    "SELECT \"estimates\".\"id\" AS \"pk\" FROM \"estimates\" \
     WHERE (\"estimates\".\"deleted_at\" IS NULL \
            AND \"estimates\".\"id\" = $1)",
    [("pk", Uuid)]
}

/// Every catalogued statement of this route family, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_LIST_BASE,
    &PROJECT_DETAIL_ONE,
    &PROJECT_LIST_COUNT,
    &PROJECT_LITE_PAGE,
    &PROJECT_LITE_PAGE_LIVE,
    &PROJECT_LITE_COUNT,
    &PROJECT_LITE_COUNT_LIVE,
    &USERS_LITE_BY_IDS,
    &WORKSPACE_LITE,
    &SUMMARY_PROJECT,
    &SUMMARY_COUNTS,
    &PROJECT_BY_ID_IN_WORKSPACE,
    &PROJECT_ARCHIVE,
    &PROJECT_UNARCHIVE,
    &PROJECT_BY_PK_ANY,
    &PROJECT_FAVORITES_BY_PROJECT,
    &PROJECT_USER_BY_ID,
    &PROJECT_STATE_BY_ID,
    &PROJECT_ESTIMATE_BY_ID,
    &FAVORITES_SOFT_DELETE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("projects_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
