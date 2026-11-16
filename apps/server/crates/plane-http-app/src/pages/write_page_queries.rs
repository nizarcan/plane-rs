// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the page-core writes (routes 124, 125, 127).
//! The read statements live in [`super::queries`].
//!
//! Every statement reproduces the ORM SQL of `plane/app/views/page/base.py`, so
//! the differential harness sees the rows Django writes. The shapes worth
//! naming:
//!
//! * the row lookup of `partial_update`/`archive`/`unarchive`/`destroy`
//!   (`base.py:172-177`, `:304-309`, `:335-340`, `:364-369`) is one
//!   `Page.objects.get(...)`: the default manager's `pages.deleted_at IS NULL`,
//!   an `INNER JOIN project_pages` carrying both `projects__id` and
//!   `project_pages__deleted_at__isnull=True` (Django compiles `projects__id`
//!   to the through table, not to `projects`), the workspace slug and
//!   `Meta.ordering` — `.get()` uses `LIMIT 21`;
//! * the PATCH save is a **full-row** update (no `update_fields`), so every
//!   non-pk column is written, including the recomputed `description_stripped`
//!   and `updated_by_id`;
//! * the delete (`base.py:399`) is H03's `delete(soft=True)`, whose `.save()`
//!   is also full-row; because every column except the three it changes is
//!   written back unchanged, the statement below sets only those three
//!   (`description_stripped` is re-derived by H26 from the stored html);
//! * the children detach (`base.py:392-397`) is a queryset `.update(parent=None)`
//!   so Django emits `UPDATE … WHERE id IN (SELECT …)` with a join;
//! * the archive/unarchive recursion is the view's own raw SQL
//!   (`base.py:61-75`), copied verbatim: it ignores `deleted_at` and does not
//!   touch `updated_at`;
//! * `unarchive`'s parent detach is `save(update_fields=["parent"])`
//!   (`base.py:355-357`): exactly one column, no `updated_at` bump
//!   (`django/db/models/base.py:1102-1107`) and no `deleted_at` predicate (the
//!   save keys on the pk only).
//!
//! The `PageLabel` batch insert is not catalogued: its text depends on the
//! batch size, and it is built in [`super::write_page`] exactly like
//! `plane_db::hooks::issue_m2m`'s through-row inserts.

use plane_db::{Query, plane_query};

plane_query! {
    /// `Project.objects.get(pk=project_id)` inside `PageSerializer.create`
    /// (`plane/app/serializers/page.py:70`).
    ///
    /// **Unscoped by the URL workspace**: the only predicate is the default
    /// manager's `deleted_at IS NULL`, so a project in another workspace is
    /// found and the page is created in that workspace.
    ///
    /// Bind order: `$1` project id.
    PAGE_PROJECT_BY_PK = "pages_write_project_by_pk",
    "SELECT p.workspace_id FROM projects p \
     WHERE p.deleted_at IS NULL AND p.id = $1 LIMIT 1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `Page.objects.create(**validated_data, description_json=…,
    /// description_binary=…, description_html=…, owned_by_id=…,
    /// workspace_id=project.workspace_id)` (`serializers/page.py:73-80`), in
    /// the model's column order.
    ///
    /// `description_binary` is always NULL here: the view passes the raw body
    /// value through unvalidated (`page/base.py:152`) and psycopg's `bytea`
    /// dumper raises `TypeError: bytes or buffer expected` for a string, which
    /// the family handler renders as a 500 before the INSERT runs.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// id, workspace, name, description_json, description_binary,
    /// description_html, description_stripped, owned_by, access, color,
    /// parent, archived_at, is_locked, view_props, logo_props, is_global,
    /// moved_to_page, moved_to_project, sort_order, external_id,
    /// external_source.
    PAGE_INSERT = "pages_write_insert",
    "INSERT INTO pages \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, name, description_json, description_binary, \
      description_html, description_stripped, owned_by_id, access, color, \
      parent_id, archived_at, is_locked, view_props, logo_props, is_global, \
      moved_to_page, moved_to_project, sort_order, external_id, \
      external_source) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
       $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26)",
    []
}

plane_query! {
    /// `ProjectPage.objects.create(workspace_id=…, project_id=…, page_id=…,
    /// created_by_id=page.created_by_id, updated_by_id=page.updated_by_id)`
    /// (`serializers/page.py:83-89`). The audit ids are the **page's**, i.e.
    /// the request user and NULL; the row's own `save()` re-derives them from
    /// the current user, which is the same pair.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, id,
    /// workspace, project, page.
    PAGE_PROJECT_PAGE_INSERT = "pages_write_project_page_insert",
    "INSERT INTO project_pages \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, project_id, page_id) \
     VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8)",
    []
}

plane_query! {
    /// `Page.objects.get(pk=page_id, workspace__slug=slug,
    /// projects__id=project_id, project_pages__deleted_at__isnull=True)` —
    /// the lookup of `partial_update` (`base.py:172-177`), `archive`
    /// (`:304-309`), `unarchive` (`:335-340`) and `destroy` (`:364-369`).
    ///
    /// `projects__id` compiles to the through table
    /// (`project_pages.project_id`), so one join serves both the m2m filter and
    /// the explicit `project_pages__deleted_at` one. `LIMIT 21` is Django's
    /// `.get()`.
    ///
    /// Bind order: `$1` page id, `$2` project id, `$3` workspace slug.
    PAGE_ROW_BY_PROJECT = "pages_write_row_by_project",
    "SELECT p.created_at AS created_at, p.updated_at AS updated_at, \
     p.created_by_id AS created_by_id, p.updated_by_id AS updated_by_id, \
     p.deleted_at AS deleted_at, p.id AS id, p.workspace_id AS workspace_id, \
     p.name AS name, p.description_json AS description_json, \
     p.description_binary AS description_binary, \
     p.description_html AS description_html, \
     p.description_stripped AS description_stripped, \
     p.owned_by_id AS owned_by_id, p.access AS access, p.color AS color, \
     p.parent_id AS parent_id, p.archived_at AS archived_at, \
     p.is_locked AS is_locked, p.view_props AS view_props, \
     p.logo_props AS logo_props, p.is_global AS is_global, \
     p.moved_to_page AS moved_to_page, \
     p.moved_to_project AS moved_to_project, p.sort_order AS sort_order, \
     p.external_id AS external_id, p.external_source AS external_source \
     FROM pages p \
     INNER JOIN project_pages ON p.id = project_pages.page_id \
     INNER JOIN workspaces w ON p.workspace_id = w.id \
     WHERE p.deleted_at IS NULL AND p.id = $1 \
     AND project_pages.deleted_at IS NULL AND project_pages.project_id = $2 \
     AND w.slug = $3 \
     ORDER BY p.created_at DESC \
     LIMIT 21",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("name", Text),
        ("description_json", Jsonb),
        ("description_binary", Bytea),
        ("description_html", Text),
        ("description_stripped", Text),
        ("owned_by_id", Uuid),
        ("access", Int2),
        ("color", Varchar),
        ("parent_id", Uuid),
        ("archived_at", Date),
        ("is_locked", Bool),
        ("view_props", Jsonb),
        ("logo_props", Jsonb),
        ("is_global", Bool),
        ("moved_to_page", Uuid),
        ("moved_to_project", Uuid),
        ("sort_order", Float8),
        ("external_id", Varchar),
        ("external_source", Varchar),
    ]
}

plane_query! {
    /// `instance.save()` on the PATCH row: a full-row update with no
    /// `update_fields`, the pk excluded (Django's `non_pks`). H26's
    /// `description_stripped`, H01's `updated_by_id` and `auto_now`'s
    /// `updated_at` are already in the row the handler binds.
    ///
    /// `description_binary` is bound as the stored bytes: no serializer field
    /// writes it on these routes.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// workspace, name, description_json, description_binary,
    /// description_html, description_stripped, owned_by, access, color,
    /// parent, archived_at, is_locked, view_props, logo_props, is_global,
    /// moved_to_page, moved_to_project, sort_order, external_id,
    /// external_source, id.
    PAGE_UPDATE = "pages_write_update",
    "UPDATE pages SET created_at = $1, updated_at = $2, created_by_id = $3, \
      updated_by_id = $4, deleted_at = $5, workspace_id = $6, name = $7, \
      description_json = $8, description_binary = $9, description_html = $10, \
      description_stripped = $11, owned_by_id = $12, access = $13, \
      color = $14, parent_id = $15, archived_at = $16, is_locked = $17, \
      view_props = $18, logo_props = $19, is_global = $20, \
      moved_to_page = $21, moved_to_project = $22, sort_order = $23, \
      external_id = $24, external_source = $25 \
     WHERE id = $26",
    []
}

plane_query! {
    /// `page.delete()` — H03's `self.deleted_at = timezone.now();
    /// self.save()` (`plane/db/mixins.py:72-78`).
    ///
    /// The save is full, but every column it writes other than these three
    /// holds its stored value, so only the changed ones are bound:
    /// `description_stripped` (H26 re-runs on the full save),
    /// `deleted_at`/`updated_at` (the same `now`) and `updated_by_id` (H01).
    /// The statement keys on the pk only — an instance save, not a queryset
    /// delete.
    ///
    /// Bind order: `$1` description_stripped, `$2` deleted_at, `$3`
    /// updated_at, `$4` updated_by, `$5` page id.
    PAGE_SOFT_DELETE = "pages_write_soft_delete",
    "UPDATE pages SET description_stripped = $1, deleted_at = $2, \
      updated_at = $3, updated_by_id = $4 \
     WHERE id = $5",
    []
}

plane_query! {
    /// `Page.objects.filter(parent_id=page_id, projects__id=project_id,
    /// workspace__slug=slug, project_pages__deleted_at__isnull=True)
    /// .update(parent=None)` (`base.py:392-397`).
    ///
    /// Django cannot join in an `UPDATE`, so it emits the pk subquery shown
    /// here (compiled from the real queryset); the tables below are its alias
    /// numbering. The default manager's `pages.deleted_at IS NULL` applies to
    /// the outer model.
    ///
    /// Bind order: `$1` page id, `$2` project id, `$3` workspace slug.
    PAGE_CHILDREN_DETACH = "pages_write_children_detach",
    "UPDATE pages SET parent_id = NULL WHERE pages.id IN (\
       SELECT U0.id FROM pages U0 \
       INNER JOIN project_pages U2 ON U0.id = U2.page_id \
       INNER JOIN workspaces U4 ON U0.workspace_id = U4.id \
       WHERE U0.deleted_at IS NULL AND U0.parent_id = $1 \
       AND U2.deleted_at IS NULL AND U2.project_id = $2 AND U4.slug = $3)",
    []
}

plane_query! {
    /// `PageLabel.objects.filter(page=instance).delete()` in
    /// `PageSerializer.update` (`serializers/page.py:111`) — H04: a bare
    /// `deleted_at` update with no `updated_at`, no audit columns and no
    /// cascade task, restricted to live rows by the manager
    /// (`plane/db/mixins.py:56-58`).
    ///
    /// Bind order: `$1` deleted_at, `$2` page id.
    PAGE_LABEL_SOFT_DELETE = "pages_write_label_soft_delete",
    "UPDATE page_labels SET deleted_at = $1 \
     WHERE page_id = $2 AND deleted_at IS NULL",
    []
}

plane_query! {
    /// `UserFavorite.objects.filter(entity_type="page",
    /// entity_identifier=page_id, project_id=project_id,
    /// workspace__slug=slug).delete()` — H04 for **every** user, in both
    /// `archive` (`base.py:323-328`) and `destroy` (`:401-406`), which pass
    /// the same four predicates. Django emits the pk subquery because of the
    /// workspace join.
    ///
    /// Bind order: `$1` deleted_at, `$2` page id, `$3` project id, `$4` slug.
    PAGE_FAVORITE_SOFT_DELETE = "pages_write_favorite_soft_delete",
    "UPDATE user_favorites SET deleted_at = $1 WHERE user_favorites.id IN (\
       SELECT U0.id FROM user_favorites U0 \
       INNER JOIN workspaces U2 ON U0.workspace_id = U2.id \
       WHERE U0.deleted_at IS NULL AND U0.entity_identifier = $2 \
       AND U0.entity_type = 'page' AND U0.project_id = $3 \
       AND U2.slug = $4)",
    []
}

plane_query! {
    /// `UserRecentVisit.objects.filter(project_id=project_id,
    /// workspace__slug=slug, entity_identifier=page_id, entity_name="page")
    /// .delete(soft=False)` (`base.py:408-413`): a hard DELETE that still runs
    /// on the default manager, so `deleted_at IS NULL` is part of the SQL
    /// (`plane/db/mixins.py:56-58`, `models/recent_visit.py:22`).
    ///
    /// Bind order: `$1` page id, `$2` project id, `$3` slug.
    PAGE_RECENT_VISIT_HARD_DELETE = "pages_write_recent_visit_hard_delete",
    "DELETE FROM user_recent_visits WHERE user_recent_visits.id IN (\
       SELECT U0.id FROM user_recent_visits U0 \
       INNER JOIN workspaces U2 ON U0.workspace_id = U2.id \
       WHERE U0.deleted_at IS NULL AND U0.entity_identifier = $1 \
       AND U0.entity_name = 'page' AND U0.project_id = $2 \
       AND U2.slug = $3)",
    []
}

plane_query! {
    /// `unarchive_archive_page_and_descendants(page_id, archived_at)`
    /// (`base.py:61-75`), verbatim: a recursive CTE that ignores
    /// `pages.deleted_at`, sets `archived_at` for the page and every
    /// descendant and does not touch `updated_at` or any audit column.
    /// `archive` passes `datetime.now()`; `unarchive` passes `None`, which is
    /// the same statement with a NULL parameter.
    ///
    /// Bind order: `$1` page id, `$2` archived_at (a `date`; Django's naive
    /// local `datetime.now()` is narrowed by `DateField.to_python`).
    PAGE_ARCHIVE_CTE = "pages_write_archive_cte",
    "WITH RECURSIVE descendants AS ( \
         SELECT id FROM pages WHERE id = $1 \
         UNION ALL \
         SELECT pages.id FROM pages, descendants \
         WHERE pages.parent_id = descendants.id \
     ) \
     UPDATE pages SET archived_at = $2 WHERE id IN (SELECT id FROM descendants)",
    []
}

plane_query! {
    /// `page.parent.archived_at` in `unarchive` (`base.py:355`). The forward FK
    /// descriptor reads through `_base_manager`, i.e. with **no**
    /// `deleted_at IS NULL` predicate, so a soft-deleted parent is still
    /// visible here.
    ///
    /// Bind order: `$1` parent page id.
    PAGE_PARENT_ARCHIVED = "pages_write_parent_archived",
    "SELECT p.archived_at AS archived_at FROM pages p \
     WHERE p.id = $1 LIMIT 21",
    [("archived_at", Date)]
}

plane_query! {
    /// `page.parent = None; page.save(update_fields=["parent"])`
    /// (`base.py:356-357`). `update_fields` narrows the save to exactly that
    /// column (`django/db/models/base.py:1102-1107`), so no `updated_at`, no
    /// `updated_by_id` and no H26 write; the statement keys on the pk only.
    ///
    /// Bind order: `$1` page id.
    PAGE_PARENT_DETACH = "pages_write_parent_detach",
    "UPDATE pages SET parent_id = NULL WHERE id = $1",
    []
}

plane_query! {
    /// `ProjectMember.objects.filter(project_id=project_id,
    /// member=request.user, is_active=True, role__lte=15).exists()` — the
    /// archive/unarchive owner-or-admin guard (`base.py:312-317`,
    /// `:343-348`). Note: **no workspace slug**, unlike `destroy`'s probe.
    ///
    /// Bind order: `$1` project id, `$2` user id.
    PAGE_MEMBER_ROLE_LTE15 = "pages_write_member_role_lte15",
    "SELECT 1 AS \"exists\" FROM project_members \
     WHERE deleted_at IS NULL AND is_active AND project_id = $1 \
     AND member_id = $2 AND role <= 15 LIMIT 1",
    [("exists", Int4)]
}

plane_query! {
    /// `ProjectMember.objects.filter(workspace__slug=slug, member=request.user,
    /// role=20, project_id=project_id, is_active=True).exists()` — the
    /// `destroy` admin-or-owner guard (`base.py:377-385`).
    ///
    /// Bind order: `$1` slug, `$2` project id, `$3` user id.
    PAGE_PROJECT_ADMIN_EXISTS = "pages_write_project_admin_exists",
    "SELECT 1 AS \"exists\" FROM project_members pm \
     INNER JOIN workspaces w ON pm.workspace_id = w.id \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $3 \
     AND pm.project_id = $2 AND pm.role = 20 AND w.slug = $1 LIMIT 1",
    [("exists", Int4)]
}

/// Every catalogued statement of this file, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PAGE_PROJECT_BY_PK,
    &PAGE_INSERT,
    &PAGE_PROJECT_PAGE_INSERT,
    &PAGE_ROW_BY_PROJECT,
    &PAGE_UPDATE,
    &PAGE_SOFT_DELETE,
    &PAGE_CHILDREN_DETACH,
    &PAGE_LABEL_SOFT_DELETE,
    &PAGE_FAVORITE_SOFT_DELETE,
    &PAGE_RECENT_VISIT_HARD_DELETE,
    &PAGE_ARCHIVE_CTE,
    &PAGE_PARENT_ARCHIVED,
    &PAGE_PARENT_DETACH,
    &PAGE_MEMBER_ROLE_LTE15,
    &PAGE_PROJECT_ADMIN_EXISTS,
];

#[cfg(test)]
mod tests {
    use super::*;
    use plane_db::models::page::Page;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_page_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }

    /// The lookup's select list is `Page::COLUMNS` under the alias `p`, with
    /// each column's `AS` alias; if the generated model gains a column this
    /// fails instead of silently decoding a different column.
    #[test]
    fn the_row_lookup_selects_the_model_columns() {
        let expected: Vec<String> = Page::COLUMNS
            .split(", ")
            .map(|column| {
                let column = column.trim();
                let start = column.find('"').expect("quoted column") + 1;
                let end = column.rfind('"').expect("quoted column");
                let name = &column[start..end];
                format!("p.{name} AS {name}")
            })
            .collect();
        let prefix = format!("SELECT {}", expected.join(", "));
        assert!(
            PAGE_ROW_BY_PROJECT.sql.starts_with(&prefix),
            "PAGE_ROW_BY_PROJECT does not select Page::COLUMNS:\n{}",
            PAGE_ROW_BY_PROJECT.sql
        );
        assert_eq!(PAGE_ROW_BY_PROJECT.columns.len(), Page::COLUMN_COUNT);
    }
}
