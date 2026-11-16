// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the modules **set B** write handlers
//! (`plans/MODULES.md` §3): module issues (105/106/107), module links
//! (108/109) and module favourites (110/111).
//!
//! Every statement is declared with [`plane_query!`](plane_db::plane_query) so
//! `tests/schema_check.rs` checks its columns against a migrated database. The
//! column lists and filters are derived from the ORM SQL each Python view
//! emits; every statement names the line it reproduces.
//!
//! The managers decide most of the predicates:
//!
//! * `Module.objects` / `Project.objects` / `ModuleIssue.objects` /
//!   `ModuleLink.objects` / `UserFavorite.objects` are `SoftDeletionManager`s —
//!   `deleted_at IS NULL` only (`plane/db/mixins.py:53-56`);
//! * `Issue.issue_objects` is `IssueManager` — additionally excludes triage
//!   states, archived/draft issues and archived projects
//!   (`plane/db/models/issue.py:92-101`);
//! * `get_favorite_entities` (`plane/app/serializers/favorite.py:76-104`) is a
//!   `Module.objects` query plus an `EXISTS` on an active project membership of
//!   an unarchived project.
//!
//! `UserFavorite.save`'s H25 `MAX(sequence)` works the same way for a module
//! favourite as for a project/cycle one, so it reuses
//! [`crate::modules::base`]'s read catalog only for the module-link response.

use plane_db::plane_query;

// ---------------------------------------------------------------------------
// Module issues (routes 105, 106, 107)
// ---------------------------------------------------------------------------

plane_query! {
    /// `Project.objects.get(pk=project_id)` (`module/issue.py:226`, `:264`):
    /// the soft-delete manager, then `project.workspace_id` for the inserted
    /// rows.
    PROJECT_WORKSPACE = "modules_write_project_workspace",
    "SELECT p.workspace_id FROM projects p \
     WHERE p.deleted_at IS NULL AND p.id = $1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `Module.objects.get(pk=module_id, project_id=…, workspace__slug=…)`
    /// (`module/issue.py:228`, `:268`). `DoesNotExist` is the family 404.
    MODULE_IN_PROJECT = "modules_write_module_in_project",
    "SELECT EXISTS (SELECT 1 FROM modules m \
     INNER JOIN workspaces w ON w.id = m.workspace_id \
     WHERE m.deleted_at IS NULL AND m.id = $1 AND m.project_id = $2 \
       AND w.slug = $3) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `Issue.issue_objects.filter(workspace__slug, project_id, pk__in)`
    /// `.values_list("id", flat=True)` (`module/issue.py:230-236`): the
    /// `IssueManager` exclusions, ordered by the model's `-created_at` — this
    /// order is the activity order and the insert order.
    MODULE_ISSUE_ISSUE_IDS = "modules_write_module_issue_issue_ids",
    "SELECT i.id FROM issues i \
     LEFT OUTER JOIN states s ON s.id = i.state_id \
     INNER JOIN projects p ON p.id = i.project_id \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND (s.\"group\" IS NULL OR s.\"group\" <> 'triage') \
       AND i.archived_at IS NULL AND p.archived_at IS NULL AND NOT i.is_draft \
       AND w.slug = $1 AND i.project_id = $2 AND i.id = ANY($3) \
     ORDER BY i.created_at DESC",
    [("id", Uuid)]
}

plane_query! {
    /// `Module.objects.filter(pk__in=modules, project_id=…,
    /// workspace__slug=…).values_list("id", flat=True)`
    /// (`module/issue.py:271-273`): the same manager filter, ordered by
    /// `-created_at`; the view stringifies each id for `requested_data`.
    MODULE_ISSUE_MODULE_IDS = "modules_write_module_issue_module_ids",
    "SELECT m.id FROM modules m \
     INNER JOIN workspaces w ON w.id = m.workspace_id \
     WHERE m.deleted_at IS NULL AND m.id = ANY($1) AND m.project_id = $2 \
       AND w.slug = $3 \
     ORDER BY m.created_at DESC",
    [("id", Uuid)]
}

plane_query! {
    /// `ModuleIssue.objects.filter(workspace__slug, project_id, module_id,
    /// issue_id).first()` with the module name for the activity's
    /// `current_instance` (`module/issue.py:302-312`, `:336-348`).
    ///
    /// The task is published even when no row matches (the name is `None`
    /// then), so the statement returns the row *if* there is one, as
    /// `first()`'s `ORDER BY -created_at LIMIT 1`.
    MODULE_ISSUE_FIRST = "modules_write_module_issue_first",
    "SELECT mi.id, m.name FROM module_issues mi \
     INNER JOIN modules m ON m.id = mi.module_id \
     INNER JOIN workspaces w ON w.id = mi.workspace_id \
     WHERE mi.deleted_at IS NULL AND w.slug = $1 AND mi.project_id = $2 \
       AND mi.module_id = $3 AND mi.issue_id = $4 \
     ORDER BY mi.created_at DESC LIMIT 1",
    [
        ("id", Uuid),
        ("name", Varchar),
    ]
}

plane_query! {
    /// `ModuleIssue.objects.filter(...).delete()` — H04, the queryset form
    /// (`module/issue.py:315`, `:352`): one bare `deleted_at` update with no
    /// `updated_at`, no audit column and no cascade task.
    ///
    /// Bind order: `$1` deleted_at, `$2` slug, `$3` project id, `$4` module
    /// id, `$5` issue id.
    SOFT_DELETE_MODULE_ISSUE = "modules_write_soft_delete_module_issue",
    "UPDATE module_issues SET deleted_at = $1 \
     WHERE deleted_at IS NULL \
       AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $2) \
       AND project_id = $3 AND module_id = $4 AND issue_id = $5",
    []
}

// ---------------------------------------------------------------------------
// Module links (routes 108, 109)
// ---------------------------------------------------------------------------

plane_query! {
    /// `ModuleLink.objects.create(...)` (`module/base.py:763-780`) plus
    /// `ProjectBaseModel.save` (`plane/db/models/project.py:237-239`): the
    /// workspace comes from the project, `created_by` is the caller and
    /// `updated_by` is NULL.
    ///
    /// Bind order: `$1`/`$2` created/updated at, `$3` created_by, `$4`
    /// deleted_at, `$5` id, `$6` project, `$7` workspace, `$8` title, `$9`
    /// url, `$10` metadata, `$11` module.
    MODULE_LINK_INSERT = "modules_write_link_insert",
    "INSERT INTO module_links \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, title, url, metadata, module_id) \
     VALUES ($1, $2, $3, NULL, $4, $5, $6, $7, $8, $9, $10, $11)",
    []
}

plane_query! {
    /// `ModuleLinkSerializer.update`'s duplicate check
    /// (`plane/app/serializers/module.py:196-201`):
    /// `ModuleLink.objects.filter(url=…, module_id=instance.module_id)
    /// .exclude(pk=instance.id).exists()`.
    ///
    /// Bind order: `$1` url, `$2` module id, `$3` excluded pk.
    MODULE_LINK_URL_TAKEN_ELSEWHERE = "modules_write_link_url_taken_elsewhere",
    "SELECT EXISTS (SELECT 1 FROM module_links l \
     WHERE l.deleted_at IS NULL AND l.url = $1 AND l.module_id = $2 \
       AND l.id <> $3) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `serializer.save()` → `instance.save()` on a module link
    /// (`module/base.py:763-780`): every writable field is set from
    /// `validated_data` (the caller keeps the row's value for absent keys) and
    /// `BaseModel.save` stamps `updated_by` and `auto_now`.
    ///
    /// Bind order: `$1` title, `$2` url, `$3` metadata, `$4` deleted_at, `$5`
    /// updated_at, `$6` updated_by, `$7` pk.
    MODULE_LINK_UPDATE = "modules_write_link_update",
    "UPDATE module_links SET title = $1, url = $2, metadata = $3, \
     deleted_at = $4, updated_at = $5, updated_by_id = $6 \
     WHERE id = $7",
    []
}

plane_query! {
    /// `ModuleLinkSerializer.create`'s duplicate check
    /// (`plane/app/serializers/module.py:190-191`):
    /// `ModuleLink.objects.filter(url=…, module_id=validated_data.get(
    /// "module_id"))`. `module` is read-only on the serializer, so
    /// `validated_data` never carries it and the lookup is
    /// `module_id IS NULL` — it can never match. Ported as-is: duplicate URLs
    /// in the same module are two rows.
    ///
    /// Bind order: `$1` url, `$2` module id (always NULL from the caller).
    MODULE_LINK_URL_TAKEN_ON_CREATE = "modules_write_link_url_taken_on_create",
    "SELECT EXISTS (SELECT 1 FROM module_links l \
     WHERE l.deleted_at IS NULL AND l.url = $1 AND l.module_id = $2) AS exists",
    [("exists", Bool)]
}

// ---------------------------------------------------------------------------
// Module favourites (routes 110, 111)
// ---------------------------------------------------------------------------

plane_query! {
    /// `get_favorite_entities("module", user, project).get(pk=…,
    /// workspace__slug=…)` (`plane/app/serializers/favorite.py:76-104` plus the
    /// view's filters, `module/base.py:822-824`): a live module in the project
    /// and slug, visible to an active member of the unarchived project.
    ///
    /// The membership `Exists` joins `projects` for `archived_at` only —
    /// related-table joins do not apply the related manager, so there is no
    /// `projects.deleted_at` predicate (checked against the emitted SQL).
    ///
    /// Bind order: `$1` project id, `$2` module id, `$3` slug, `$4` user id.
    MODULE_FAVORITE_GATE = "modules_write_favorite_gate",
    "SELECT EXISTS (SELECT 1 FROM modules m \
     INNER JOIN workspaces w ON w.id = m.workspace_id \
     WHERE m.deleted_at IS NULL AND m.project_id = $1 AND m.id = $2 \
       AND w.slug = $3 \
       AND EXISTS (SELECT 1 FROM project_members pm \
                   INNER JOIN projects p ON p.id = pm.project_id \
                   WHERE pm.project_id = $1 AND pm.member_id = $4 \
                     AND pm.is_active AND pm.deleted_at IS NULL \
                     AND p.archived_at IS NULL)) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `self.project.workspace` in `WorkspaceBaseModel.save`
    /// (`plane/db/models/workspace.py:192-195`) — the FK descriptor reads the
    /// **base** manager (`django/db/models/fields/related_descriptors.py`,
    /// `get_object`), so there is no `deleted_at` filter here.
    PROJECT_WORKSPACE_ANY = "modules_write_project_workspace_any",
    "SELECT p.workspace_id FROM projects p WHERE p.id = $1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `UserFavorite.objects.create(...)` for a module
    /// (`module/base.py:825-830`). The sequence comes from H25; `name` is
    /// NULL, `is_folder` false, `parent_id` NULL.
    ///
    /// Bind order: `$1`/`$2` created/updated at, `$3` created_by, `$4` id,
    /// `$5` workspace, `$6` project, `$7` user, `$8` entity_identifier, `$9`
    /// sequence.
    USER_FAVORITE_INSERT = "modules_write_favorite_insert",
    "INSERT INTO user_favorites \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, project_id, user_id, entity_type, entity_identifier, name, \
      is_folder, sequence, parent_id) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, 'module', $8, NULL, \
       FALSE, $9, NULL)",
    []
}

plane_query! {
    /// `UserFavorite.objects.get(project_id=…, user=…, workspace__slug=…,
    /// entity_type="module", entity_identifier=module_id)`
    /// (`module/base.py:836-841`) — the favourite destroy lookup.
    ///
    /// Bind order: `$1` project id, `$2` user id, `$3` slug, `$4` module id.
    USER_FAVORITE_LOOKUP = "modules_write_favorite_lookup",
    "SELECT uf.id FROM user_favorites uf \
     INNER JOIN workspaces w ON w.id = uf.workspace_id \
     WHERE uf.deleted_at IS NULL AND uf.project_id = $1 \
       AND uf.entity_type = 'module' AND uf.user_id = $2 AND w.slug = $3 \
       AND uf.entity_identifier = $4 LIMIT 1",
    [("id", Uuid)]
}

/// The catalog entry `tests/schema_check.rs` verifies against a migrated
/// database.
pub static ALL_QUERIES: &[&plane_db::Query] = &[
    &PROJECT_WORKSPACE,
    &MODULE_IN_PROJECT,
    &MODULE_ISSUE_ISSUE_IDS,
    &MODULE_ISSUE_MODULE_IDS,
    &MODULE_ISSUE_FIRST,
    &SOFT_DELETE_MODULE_ISSUE,
    &MODULE_LINK_INSERT,
    &MODULE_LINK_URL_TAKEN_ELSEWHERE,
    &MODULE_LINK_UPDATE,
    &MODULE_LINK_URL_TAKEN_ON_CREATE,
    &MODULE_FAVORITE_GATE,
    &PROJECT_WORKSPACE_ANY,
    &USER_FAVORITE_INSERT,
    &USER_FAVORITE_LOOKUP,
];
