// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the project satellite writes.
//!
//! One statement per `QuerySet`/`save()` the Python views run:
//!
//! * `ProjectArchiveUnarchiveEndpoint` (`plane/app/views/project/base.py:428-445`):
//!   the project row, the full-row `UPDATE` `project.save()` emits, and the
//!   favourites sweep (H04: one bare `UPDATE … deleted_at`, no `updated_at`);
//! * `ProjectUserViewsEndpoint` (`:475-496`): the membership row and its
//!   four-column update;
//! * `ProjectFavoritesViewSet.create`/`destroy` (`:499-537`): the
//!   `get_favorite_entities` visibility gate, H25's `MAX(sequence)` lives in
//!   `plane_db::hooks::favorite`, the `INSERT` and the hard `DELETE`;
//! * `ProjectUserDisplayPropertyEndpoint.patch`
//!   (`plane/app/views/issue/base.py:745-766`) and the row it creates first;
//! * `ProjectMemberPreferenceEndpoint.patch`
//!   (`plane/app/views/project/member.py:416-432`);
//! * `DeployBoardViewSet.create`/`partial_update`/`destroy`
//!   (`plane/app/views/project/base.py:539-616`): the `get_or_create` lookup,
//!   the full-row `save()`, the project/workspace lite join and the two
//!   validation `EXISTS` probes.
//!
//! `Project.save()` is called without `update_fields`, so it writes **every**
//! column — including the ones the view did not touch (`plane/db/models/
//! project.py:193-227` → `Model.save()`). The same is true of the deploy
//! board: `DeployBoard` has no `save()` override, so `model.save()` writes all
//! of its columns.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Project.objects.get(pk=project_id, workspace__slug=slug)`
    /// (`plane/app/views/project/base.py:430`, `:441`).
    ///
    /// Bind order: `$1` project id, `$2` workspace slug.
    PROJECT_BY_ID_IN_WORKSPACE = "projects_write_project_by_id_in_workspace",
    "SELECT p.\"created_at\", p.\"updated_at\", p.\"created_by_id\", \
     p.\"updated_by_id\", p.\"deleted_at\", p.\"id\", p.\"name\", \
     p.\"description\", p.\"description_text\", p.\"description_html\", \
     p.\"network\", p.\"workspace_id\", p.\"identifier\", \
     p.\"default_assignee_id\", p.\"project_lead_id\", p.\"emoji\", \
     p.\"icon_prop\", p.\"module_view\", p.\"cycle_view\", \
     p.\"issue_views_view\", p.\"page_view\", p.\"intake_view\", \
     p.\"is_time_tracking_enabled\", p.\"is_issue_type_enabled\", \
     p.\"guest_view_all_features\", p.\"cover_image\", \
     p.\"cover_image_asset_id\", p.\"estimate_id\", p.\"archive_in\", \
     p.\"close_in\", p.\"logo_props\", p.\"default_state_id\", \
     p.\"archived_at\", p.\"timezone\", p.\"external_source\", \
     p.\"external_id\" \
     FROM projects p \
     INNER JOIN workspaces w ON (p.\"workspace_id\" = w.\"id\") \
     WHERE p.\"deleted_at\" IS NULL AND p.\"id\" = $1 AND w.\"slug\" = $2 \
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
    /// `project.save()` with no `update_fields`
    /// (`plane/app/views/project/base.py:432`, `:443`): every column, because
    /// `Model.save()` writes the whole row.
    ///
    /// Bind order: the 35 writable columns in [`PROJECT_BY_ID_IN_WORKSPACE`]'s
    /// order (everything except `id`, which is `$36`), ending with the row id.
    PROJECT_UPDATE = "projects_write_project_update",
    "UPDATE projects SET \
     \"created_at\" = $1, \"updated_at\" = $2, \"created_by_id\" = $3, \
     \"updated_by_id\" = $4, \"deleted_at\" = $5, \"name\" = $7, \
     \"description\" = $8, \"description_text\" = $9, \"description_html\" = $10, \
     \"network\" = $11, \"workspace_id\" = $12, \"identifier\" = $13, \
     \"default_assignee_id\" = $14, \"project_lead_id\" = $15, \"emoji\" = $16, \
     \"icon_prop\" = $17, \"module_view\" = $18, \"cycle_view\" = $19, \
     \"issue_views_view\" = $20, \"page_view\" = $21, \"intake_view\" = $22, \
     \"is_time_tracking_enabled\" = $23, \"is_issue_type_enabled\" = $24, \
     \"guest_view_all_features\" = $25, \"cover_image\" = $26, \
     \"cover_image_asset_id\" = $27, \"estimate_id\" = $28, \"archive_in\" = $29, \
     \"close_in\" = $30, \"logo_props\" = $31, \"default_state_id\" = $32, \
     \"archived_at\" = $33, \"timezone\" = $34, \"external_source\" = $35, \
     \"external_id\" = $36 \
     WHERE \"id\" = $6",
    []
}

plane_query! {
    /// `UserFavorite.objects.filter(workspace__slug=slug, project=project_id)
    /// .delete()` (`plane/app/views/project/base.py:433`) — H04:
    /// `SoftDeletionQuerySet.delete` is `self.update(deleted_at=now)`
    /// (`plane/db/mixins.py:47-52`), so `updated_at` and `updated_by` are left
    /// alone and no cascade task is published.
    ///
    /// Bind order: `$1` project id, `$2` the new `deleted_at`, `$3` workspace
    /// slug.
    PROJECT_FAVORITES_SOFT_DELETE = "projects_write_project_favorites_soft_delete",
    "UPDATE user_favorites SET \"deleted_at\" = $2 \
     WHERE \"deleted_at\" IS NULL AND \"project_id\" = $1 \
       AND \"workspace_id\" IN (SELECT \"id\" FROM workspaces WHERE \"slug\" = $3)",
    []
}

plane_query! {
    /// `ProjectMember.objects.filter(member=user, project=project,
    /// is_active=True).first()` (`plane/app/views/project/base.py:480`).
    ///
    /// Bind order: `$1` member id, `$2` project id.
    PROJECT_MEMBER_FOR_VIEW_PROPS = "projects_write_project_member_for_view_props",
    "SELECT m.\"id\", m.\"view_props\", m.\"default_props\", m.\"preferences\", \
     m.\"sort_order\" \
     FROM project_members m \
     WHERE m.\"deleted_at\" IS NULL AND m.\"member_id\" = $1 \
       AND m.\"project_id\" = $2 AND m.\"is_active\" \
     ORDER BY m.\"created_at\" DESC \
     LIMIT 1",
    [
        ("id", Uuid),
        ("view_props", Jsonb),
        ("default_props", Jsonb),
        ("preferences", Jsonb),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `project_member.save()` (`plane/app/views/project/base.py:495`) for the
    /// four columns the view assigned. `ProjectMember.save`'s create branch is
    /// H32 and does not run on an update; `BaseModel.save` stamps `updated_by`
    /// and `auto_now` stamps `updated_at`, and `WorkspaceBaseModel.save` sets
    /// `workspace = project.workspace` (already equal).
    ///
    /// Bind order: `$1` view_props, `$2` default_props, `$3` preferences,
    /// `$4` sort_order, `$5` updated_at, `$6` updated_by, `$7` row id.
    PROJECT_MEMBER_SAVE_VIEW_PROPS = "projects_write_project_member_save_view_props",
    "UPDATE project_members SET \"view_props\" = $1, \"default_props\" = $2, \
     \"preferences\" = $3, \"sort_order\" = $4, \"updated_at\" = $5, \
     \"updated_by_id\" = $6 \
     WHERE \"id\" = $7",
    []
}

plane_query! {
    /// `get_favorite_entities("project", user_id, project_id)
    /// .get(pk=project_id, workspace__slug=slug)`
    /// (`plane/app/views/project/base.py:517-518`, serializer
    /// `plane/app/serializers/favorite.py:86-93`):
    ///
    /// * the user must be an active `ProjectMember` of the project, and the
    ///   project must not be archived (`project__archived_at__isnull=True`);
    /// * the project itself must be live and in the URL's workspace.
    ///
    /// Bind order: `$1` project id, `$2` user id, `$3` workspace slug.
    PROJECT_FAVORITE_GATE = "projects_write_project_favorite_gate",
    "SELECT p.\"workspace_id\" FROM projects p \
     INNER JOIN workspaces w ON (p.\"workspace_id\" = w.\"id\") \
     WHERE p.\"deleted_at\" IS NULL AND p.\"id\" = $1 AND w.\"slug\" = $3 \
       AND EXISTS ( \
         SELECT 1 FROM project_members pm \
         INNER JOIN projects mp ON (pm.\"project_id\" = mp.\"id\") \
         WHERE pm.\"deleted_at\" IS NULL AND pm.\"project_id\" = $1 \
           AND pm.\"member_id\" = $2 AND pm.\"is_active\" \
           AND mp.\"archived_at\" IS NULL \
       ) \
     LIMIT 1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `UserFavorite.objects.create(user=…, entity_type="project",
    /// entity_identifier=project, project_id=project)`
    /// (`plane/app/views/project/base.py:519-524`).
    ///
    /// `WorkspaceBaseModel.save` derives `workspace` from the project
    /// (`plane/db/models/workspace.py:192-195`); `created_by` is the request
    /// user and `updated_by` stays NULL (H01); `sequence` is H25's
    /// `plane_db::hooks::favorite::before_insert` value.
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by_id,
    /// `$4` id, `$5` workspace_id, `$6` project_id, `$7` user_id,
    /// `$8` entity_identifier, `$9` sequence.
    USER_FAVORITE_INSERT = "projects_write_user_favorite_insert",
    "INSERT INTO user_favorites \
     (\"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \
      \"deleted_at\", \"id\", \"workspace_id\", \"project_id\", \"user_id\", \
      \"entity_type\", \"entity_identifier\", \"name\", \"is_folder\", \
      \"sequence\", \"parent_id\") \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, 'project', $8, NULL, \
             FALSE, $9, NULL)",
    []
}

plane_query! {
    /// `UserFavorite.objects.get(entity_identifier=project_id,
    /// entity_type="project", project=project_id, user=request.user,
    /// workspace__slug=slug)` (`plane/app/views/project/base.py:529-535`).
    ///
    /// Bind order: `$1` project id, `$2` user id, `$3` workspace slug.
    USER_FAVORITE_FOR_PROJECT = "projects_write_user_favorite_for_project",
    "SELECT f.\"id\" FROM user_favorites f \
     INNER JOIN workspaces w ON (f.\"workspace_id\" = w.\"id\") \
     WHERE f.\"deleted_at\" IS NULL AND f.\"entity_identifier\" = $1 \
       AND f.\"entity_type\" = 'project' AND f.\"project_id\" = $1 \
       AND f.\"user_id\" = $2 AND w.\"slug\" = $3 \
     LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `project_favorite.delete(soft=False)` → `Model.delete()`
    /// (`plane/app/views/project/base.py:536`, `plane/db/mixins.py:65-77`): a
    /// hard `DELETE`, not a soft delete, so no cascade task is published.
    ///
    /// Bind order: `$1` row id.
    USER_FAVORITE_DELETE = "projects_write_user_favorite_delete",
    "DELETE FROM user_favorites WHERE \"id\" = $1",
    []
}

plane_query! {
    /// `ProjectUserProperty.objects.get(user=request.user, project_id=…)`
    /// (`plane/app/views/issue/base.py:749-752`).
    ///
    /// Bind order: `$1` user id, `$2` project id.
    PROJECT_USER_PROPERTY_BY_USER = "projects_write_project_user_property_by_user",
    "SELECT q.\"created_at\", q.\"updated_at\", q.\"created_by_id\", \
     q.\"updated_by_id\", q.\"deleted_at\", q.\"id\", q.\"project_id\", \
     q.\"workspace_id\", q.\"user_id\", q.\"filters\", q.\"display_filters\", \
     q.\"display_properties\", q.\"rich_filters\", q.\"preferences\", \
     q.\"sort_order\" \
     FROM project_user_properties q \
     WHERE q.\"deleted_at\" IS NULL AND q.\"user_id\" = $1 \
       AND q.\"project_id\" = $2 \
     ORDER BY q.\"created_at\" DESC \
     LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("user_id", Uuid),
        ("filters", Jsonb),
        ("display_filters", Jsonb),
        ("display_properties", Jsonb),
        ("rich_filters", Jsonb),
        ("preferences", Jsonb),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `ProjectUserProperty.objects.create(user=request.user,
    /// project_id=project_id)` (`plane/app/views/issue/base.py:754-757`).
    ///
    /// `WorkspaceBaseModel.save` derives `workspace` from the project; the five
    /// JSON columns get the model's `default=` callables (H-note: they are
    /// passed in, so the caller owns them); audit columns are the request
    /// user (`BaseModel.save`).
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by_id,
    /// `$4` id, `$5` project_id, `$6` workspace_id, `$7` user_id,
    /// `$8` filters, `$9` display_filters, `$10` display_properties,
    /// `$11` rich_filters, `$12` preferences, `$13` sort_order.
    PROJECT_USER_PROPERTY_INSERT = "projects_write_project_user_property_insert",
    "INSERT INTO project_user_properties \
     (\"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \
      \"deleted_at\", \"id\", \"project_id\", \"workspace_id\", \"user_id\", \
      \"filters\", \"display_filters\", \"display_properties\", \"rich_filters\", \
      \"preferences\", \"sort_order\") \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    []
}

plane_query! {
    /// `serializer.save()` (`plane/app/views/issue/base.py:765`) — a
    /// `ModelSerializer.update` then `save()` with no `update_fields`, so every
    /// column is written (`auto_now` adds `updated_at`; `BaseModel.save` adds
    /// `updated_by`).
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by_id,
    /// `$4` updated_by_id, `$5` deleted_at, `$6` project_id, `$7` workspace_id,
    /// `$8` user_id, `$9` filters, `$10` display_filters,
    /// `$11` display_properties, `$12` rich_filters, `$13` preferences,
    /// `$14` sort_order, `$15` row id.
    PROJECT_USER_PROPERTY_UPDATE = "projects_write_project_user_property_update",
    "UPDATE project_user_properties SET \
     \"created_at\" = $1, \"updated_at\" = $2, \"created_by_id\" = $3, \
     \"updated_by_id\" = $4, \"deleted_at\" = $5, \"project_id\" = $6, \
     \"workspace_id\" = $7, \"user_id\" = $8, \"filters\" = $9, \
     \"display_filters\" = $10, \"display_properties\" = $11, \
     \"rich_filters\" = $12, \"preferences\" = $13, \"sort_order\" = $14 \
     WHERE \"id\" = $15",
    []
}

plane_query! {
    /// `ProjectMember.objects.get(project_id=…, member_id=…,
    /// workspace__slug=…)` (`plane/app/views/project/member.py:409-414`).
    ///
    /// Bind order: `$1` project id, `$2` member id, `$3` workspace slug.
    PROJECT_MEMBER_BY_MEMBER_ID = "projects_write_project_member_by_member_id",
    "SELECT m.\"created_at\", m.\"updated_at\", m.\"created_by_id\", \
     m.\"updated_by_id\", m.\"deleted_at\", m.\"id\", m.\"project_id\", \
     m.\"workspace_id\", m.\"member_id\", m.\"comment\", m.\"role\", \
     m.\"view_props\", m.\"default_props\", m.\"preferences\", \
     m.\"sort_order\", m.\"is_active\" \
     FROM project_members m \
     INNER JOIN workspaces w ON (m.\"workspace_id\" = w.\"id\") \
     WHERE m.\"deleted_at\" IS NULL AND m.\"project_id\" = $1 \
       AND m.\"member_id\" = $2 AND w.\"slug\" = $3 \
     LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("member_id", Uuid),
        ("comment", Text),
        ("role", Int2),
        ("view_props", Jsonb),
        ("default_props", Jsonb),
        ("preferences", Jsonb),
        ("sort_order", Float8),
        ("is_active", Bool),
    ]
}

plane_query! {
    /// `serializer.save()` for the merged preferences
    /// (`plane/app/views/project/member.py:429`): `ProjectMember.save`'s branch
    /// only runs on create (H32), so this is `updated_at`/`updated_by` plus the
    /// column.
    ///
    /// Bind order: `$1` preferences, `$2` updated_at, `$3` updated_by, `$4` id.
    PROJECT_MEMBER_SAVE_PREFERENCES = "projects_write_project_member_save_preferences",
    "UPDATE project_members SET \"preferences\" = $1, \"updated_at\" = $2, \
     \"updated_by_id\" = $3 \
     WHERE \"id\" = $4",
    []
}

plane_query! {
    /// `DeployBoard.objects.get_or_create(entity_name="project",
    /// entity_identifier=project_id, project_id=project_id)`
    /// (`plane/app/views/project/base.py:590-592`).
    ///
    /// Bind order: `$1` project id.
    DEPLOY_BOARD_BY_ENTITY = "projects_write_deploy_board_by_entity",
    "SELECT b.\"created_at\", b.\"updated_at\", b.\"created_by_id\", \
     b.\"updated_by_id\", b.\"deleted_at\", b.\"id\", b.\"workspace_id\", \
     b.\"project_id\", b.\"entity_identifier\", b.\"entity_name\", \
     b.\"anchor\", b.\"is_comments_enabled\", b.\"is_reactions_enabled\", \
     b.\"intake_id\", b.\"is_votes_enabled\", b.\"view_props\", \
     b.\"is_activity_enabled\", b.\"is_disabled\" \
     FROM deploy_boards b \
     WHERE b.\"deleted_at\" IS NULL AND b.\"entity_name\" = 'project' \
       AND b.\"entity_identifier\" = $1 AND b.\"project_id\" = $1 \
     ORDER BY b.\"created_at\" DESC \
     LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("entity_identifier", Uuid),
        ("entity_name", Varchar),
        ("anchor", Varchar),
        ("is_comments_enabled", Bool),
        ("is_reactions_enabled", Bool),
        ("intake_id", Uuid),
        ("is_votes_enabled", Bool),
        ("view_props", Jsonb),
        ("is_activity_enabled", Bool),
        ("is_disabled", Bool),
    ]
}

plane_query! {
    /// The `get_or_create` create branch (`plane/app/views/project/base.py:590-592`
    /// → `DeployBoard.objects.create`): model defaults for every column the view
    /// does not pass, `workspace` from the project (`WorkspaceBaseModel.save`),
    /// `anchor = uuid4().hex` (`plane/db/models/deploy_board.py:14-15`).
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by_id,
    /// `$4` id, `$5` workspace_id, `$6` project_id, `$7` entity_identifier,
    /// `$8` entity_name, `$9` anchor, `$10` intake_id, `$11` view_props,
    /// `$12` is_votes_enabled, `$13` is_comments_enabled,
    /// `$14` is_reactions_enabled.
    DEPLOY_BOARD_INSERT = "projects_write_deploy_board_insert",
    "INSERT INTO deploy_boards \
     (\"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \
      \"deleted_at\", \"id\", \"workspace_id\", \"project_id\", \
      \"entity_identifier\", \"entity_name\", \"anchor\", \
      \"is_comments_enabled\", \"is_reactions_enabled\", \"intake_id\", \
      \"is_votes_enabled\", \"view_props\", \"is_activity_enabled\", \
      \"is_disabled\") \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, $9, $10, $11, $12, \
             $13, $14, TRUE, FALSE)",
    []
}

plane_query! {
    /// `DeployBoard.objects.filter(workspace__slug=…, entity_name="project",
    /// entity_identifier=project_id, project_id=project_id)`
    /// (`plane/app/views/project/base.py:552-557`) plus `pk=`, the lookup
    /// `get_object()` runs for PATCH and DELETE.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` board id.
    DEPLOY_BOARD_BY_PK = "projects_write_deploy_board_by_pk",
    "SELECT b.\"created_at\", b.\"updated_at\", b.\"created_by_id\", \
     b.\"updated_by_id\", b.\"deleted_at\", b.\"id\", b.\"workspace_id\", \
     b.\"project_id\", b.\"entity_identifier\", b.\"entity_name\", \
     b.\"anchor\", b.\"is_comments_enabled\", b.\"is_reactions_enabled\", \
     b.\"intake_id\", b.\"is_votes_enabled\", b.\"view_props\", \
     b.\"is_activity_enabled\", b.\"is_disabled\" \
     FROM deploy_boards b \
     INNER JOIN workspaces w ON (b.\"workspace_id\" = w.\"id\") \
     WHERE b.\"deleted_at\" IS NULL AND w.\"slug\" = $1 \
       AND b.\"entity_name\" = 'project' AND b.\"entity_identifier\" = $2 \
       AND b.\"project_id\" = $2 AND b.\"id\" = $3 \
     LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("entity_identifier", Uuid),
        ("entity_name", Varchar),
        ("anchor", Varchar),
        ("is_comments_enabled", Bool),
        ("is_reactions_enabled", Bool),
        ("intake_id", Uuid),
        ("is_votes_enabled", Bool),
        ("view_props", Jsonb),
        ("is_activity_enabled", Bool),
        ("is_disabled", Bool),
    ]
}

plane_query! {
    /// `project_deploy_board.save()` — a `DeployBoard` has no `save()` override,
    /// so `Model.save()` writes every column.
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by_id,
    /// `$4` updated_by_id, `$5` deleted_at, `$6` workspace_id, `$7` project_id,
    /// `$8` entity_identifier, `$9` entity_name, `$10` anchor,
    /// `$11` is_comments_enabled, `$12` is_reactions_enabled, `$13` intake_id,
    /// `$14` is_votes_enabled, `$15` view_props, `$16` is_activity_enabled,
    /// `$17` is_disabled, `$18` row id.
    DEPLOY_BOARD_UPDATE = "projects_write_deploy_board_update",
    "UPDATE deploy_boards SET \
     \"created_at\" = $1, \"updated_at\" = $2, \"created_by_id\" = $3, \
     \"updated_by_id\" = $4, \"deleted_at\" = $5, \"workspace_id\" = $6, \
     \"project_id\" = $7, \"entity_identifier\" = $8, \"entity_name\" = $9, \
     \"anchor\" = $10, \"is_comments_enabled\" = $11, \
     \"is_reactions_enabled\" = $12, \"intake_id\" = $13, \
     \"is_votes_enabled\" = $14, \"view_props\" = $15, \
     \"is_activity_enabled\" = $16, \"is_disabled\" = $17 \
     WHERE \"id\" = $18",
    []
}

plane_query! {
    /// `Project.objects.filter(pk=project_id, workspace__slug=slug).exists()`
    /// (`plane/app/views/project/base.py:575-576`).
    ///
    /// Bind order: `$1` project id, `$2` workspace slug.
    PROJECT_EXISTS_IN_WORKSPACE = "projects_write_project_exists_in_workspace",
    "SELECT EXISTS ( \
       SELECT 1 FROM projects p \
       INNER JOIN workspaces w ON (p.\"workspace_id\" = w.\"id\") \
       WHERE p.\"deleted_at\" IS NULL AND p.\"id\" = $1 AND w.\"slug\" = $2 \
     )",
    [("exists", Bool)]
}

plane_query! {
    /// `Intake.objects.filter(pk=intake, project_id=project_id).exists()`
    /// (`plane/app/views/project/base.py:581`).
    ///
    /// Bind order: `$1` intake id, `$2` project id.
    INTAKE_EXISTS_IN_PROJECT = "projects_write_intake_exists_in_project",
    "SELECT EXISTS ( \
       SELECT 1 FROM intakes i \
       WHERE i.\"deleted_at\" IS NULL AND i.\"id\" = $1 \
         AND i.\"project_id\" = $2 \
     )",
    [("exists", Bool)]
}

plane_query! {
    /// The `project_details` half of `DeployBoardSerializer`
    /// (`plane/app/serializers/project.py:261-263`): `ProjectLiteSerializer`
    /// plus its two related assets' entity types (`cover_image_url` reads
    /// `project.cover_image_asset.asset_url` through `_base_manager`, so the
    /// join has no `deleted_at` predicate) and the workspace slug the URL needs.
    ///
    /// Bind order: `$1` project id.
    DEPLOY_BOARD_PROJECT_LITE = "projects_write_deploy_board_project_lite",
    "SELECT p.\"id\", p.\"identifier\", p.\"name\", p.\"cover_image\", \
     p.\"cover_image_asset_id\", p.\"logo_props\", p.\"description\", \
     w.\"slug\", cover.\"entity_type\", p.\"workspace_id\" \
     FROM projects p \
     INNER JOIN workspaces w ON (p.\"workspace_id\" = w.\"id\") \
     LEFT OUTER JOIN file_assets cover ON (cover.\"id\" = p.\"cover_image_asset_id\") \
     WHERE p.\"id\" = $1",
    [
        ("id", Uuid),
        ("identifier", Varchar),
        ("name", Varchar),
        ("cover_image", Text),
        ("cover_image_asset_id", Uuid),
        ("logo_props", Jsonb),
        ("description", Text),
        ("slug", Varchar),
        ("entity_type", Varchar),
        ("workspace_id", Uuid),
    ]
}

plane_query! {
    /// The `workspace_detail` half of `DeployBoardSerializer`:
    /// `WorkspaceLiteSerializer(board.workspace)` with `logo_url`.
    ///
    /// Bind order: `$1` workspace id.
    DEPLOY_BOARD_WORKSPACE_LITE = "projects_write_deploy_board_workspace_lite",
    "SELECT w.\"name\", w.\"slug\", w.\"id\", w.\"logo\", w.\"logo_asset_id\", \
     logo.\"entity_type\" \
     FROM workspaces w \
     LEFT OUTER JOIN file_assets logo ON (logo.\"id\" = w.\"logo_asset_id\") \
     WHERE w.\"id\" = $1",
    [
        ("name", Varchar),
        ("slug", Varchar),
        ("id", Uuid),
        ("logo", Text),
        ("logo_asset_id", Uuid),
        ("entity_type", Varchar),
    ]
}

plane_query! {
    /// `self.project.workspace` (`plane/db/models/workspace.py:192-195`) — the
    /// `WorkspaceBaseModel.save` lookup `ProjectUserProperty.objects.create`
    /// triggers (`plane/app/views/issue/base.py:754-757`).
    ///
    /// Bind order: `$1` project id, `$2` workspace slug.
    PROJECT_WORKSPACE_BY_ID = "projects_write_project_workspace_by_id",
    "SELECT p.\"workspace_id\" \
     FROM projects p \
     INNER JOIN workspaces w ON (p.\"workspace_id\" = w.\"id\") \
     WHERE p.\"deleted_at\" IS NULL AND p.\"id\" = $1 AND w.\"slug\" = $2 \
     LIMIT 1",
    [("workspace_id", Uuid)]
}

/// Every catalogued statement of this file, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_BY_ID_IN_WORKSPACE,
    &PROJECT_UPDATE,
    &PROJECT_FAVORITES_SOFT_DELETE,
    &PROJECT_MEMBER_FOR_VIEW_PROPS,
    &PROJECT_MEMBER_SAVE_VIEW_PROPS,
    &PROJECT_FAVORITE_GATE,
    &USER_FAVORITE_INSERT,
    &USER_FAVORITE_FOR_PROJECT,
    &USER_FAVORITE_DELETE,
    &PROJECT_USER_PROPERTY_BY_USER,
    &PROJECT_USER_PROPERTY_INSERT,
    &PROJECT_USER_PROPERTY_UPDATE,
    &PROJECT_MEMBER_BY_MEMBER_ID,
    &PROJECT_MEMBER_SAVE_PREFERENCES,
    &DEPLOY_BOARD_BY_ENTITY,
    &DEPLOY_BOARD_INSERT,
    &DEPLOY_BOARD_BY_PK,
    &DEPLOY_BOARD_UPDATE,
    &PROJECT_EXISTS_IN_WORKSPACE,
    &INTAKE_EXISTS_IN_PROJECT,
    &PROJECT_WORKSPACE_BY_ID,
    &DEPLOY_BOARD_PROJECT_LITE,
    &DEPLOY_BOARD_WORKSPACE_LITE,
];
