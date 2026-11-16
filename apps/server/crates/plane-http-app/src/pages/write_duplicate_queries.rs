// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of `PageDuplicateEndpoint.post` (route 133,
//! `plane/app/views/page/base.py:575-636`). The read statements of the group
//! live in [`super::queries`].
//!
//! Every name is prefixed `PAGE_DUPLICATE_` / `pages_duplicate_` because set A
//! also writes `pages` and `project_pages`, and the catalog name is global
//! (`tests/schema_check.rs::query_names_are_unique_across_the_crate`).

use plane_db::plane_query;

plane_query! {
    /// `Page.objects.get(pk=page_id, workspace__slug=slug,
    /// projects__id=project_id, project_pages__deleted_at__isnull=True)`
    /// (`page/base.py:579-584`).
    ///
    /// Django uses **one** `project_pages` join for both M2M predicates: the
    /// `projects__id` term trims the `projects` join down to the through
    /// table's `project_id` column, and the reverse-FK predicate reuses that
    /// join, so the pair restricts the same link row. `.get()` drops the model
    /// ordering and caps the result at 21 rows
    /// (`django/db/models/query.py:621-630`); more than one row is
    /// `MultipleObjectsReturned`, which the family handler answers as a 500
    /// (`plane/app/views/base.py:235-239`) — unreachable in practice because the
    /// partial unique constraint allows one live link per (project, page).
    ///
    /// Bind order: `$1` page id, `$2` project id, `$3` workspace slug.
    PAGE_DUPLICATE_LOOKUP = "pages_duplicate_lookup",
    "SELECT p.name AS name, p.workspace_id AS workspace_id, \
     p.description_json AS description_json, \
     p.description_html AS description_html, p.owned_by_id AS owned_by_id, \
     p.access AS access, p.color AS color, p.parent_id AS parent_id, \
     p.archived_at AS archived_at, p.is_locked AS is_locked, \
     p.view_props AS view_props, p.logo_props AS logo_props, \
     p.is_global AS is_global, p.moved_to_page AS moved_to_page, \
     p.moved_to_project AS moved_to_project, p.sort_order AS sort_order, \
     p.external_id AS external_id, p.external_source AS external_source \
     FROM pages p \
     INNER JOIN project_pages pp ON (p.id = pp.page_id) \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     WHERE p.deleted_at IS NULL AND p.id = $1 AND pp.deleted_at IS NULL \
     AND pp.project_id = $2 AND w.slug = $3 \
     LIMIT 21",
    [
        ("name", Text),
        ("workspace_id", Uuid),
        ("description_json", Jsonb),
        ("description_html", Text),
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
    /// `page.save()` after `page.pk = None` (`page/base.py:593-599`): a full
    /// INSERT of every column, in `Page._meta.local_concrete_fields` order.
    ///
    /// Django's `_save_table` fills the pk from `UUIDField(default=uuid4)`
    /// before deciding insert-vs-update, so `pk_set` is true and it first runs a
    /// no-op UPDATE against the new id, then (0 rows updated) the INSERT
    /// (`django/db/models/base.py`, `_save_table`). The UPDATE cannot match and
    /// is not observable, so it is not reproduced.
    ///
    /// `auto_now_add`/`auto_now` call `timezone.now()` once each on the INSERT
    /// (`django/db/models/fields/__init__.py:864-877`), `description_stripped`
    /// is H26's recomputation and `description_binary` is the column the view
    /// clears (`page/base.py:595`).
    ///
    /// Bind order follows the column list.
    PAGE_DUPLICATE_PAGE_INSERT = "pages_duplicate_page_insert",
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
    /// `ProjectPage.objects.filter(page_id=page_id).values_list("project_id",
    /// flat=True)` (`page/base.py:591`).
    ///
    /// The default manager keeps live links only (`plane/db/mixins.py:55-57`)
    /// and `Meta.ordering = ("-created_at",)` supplies the `ORDER BY`
    /// (`plane/db/models/page.py:152`). The queryset is lazy in Python: the
    /// `for` loop evaluates it after `page.save()`.
    ///
    /// Bind order: `$1` page id.
    PAGE_DUPLICATE_LINK_IDS = "pages_duplicate_link_ids",
    "SELECT pp.project_id AS project_id FROM project_pages pp \
     WHERE pp.deleted_at IS NULL AND pp.page_id = $1 \
     ORDER BY pp.created_at DESC",
    [("project_id", Uuid)]
}

plane_query! {
    /// `ProjectPage.objects.create(workspace_id=page.workspace_id,
    /// project_id=project_id, page_id=page.id, created_by_id=page.created_by_id,
    /// updated_by_id=page.updated_by_id)` (`page/base.py:602-608`).
    ///
    /// H01's create branch replaces both audit ids: `created_by` is the request
    /// user (crum) and `updated_by` is explicitly NULL, so the `updated_by_id`
    /// the view passes never reaches the row (`plane/db/models/base.py:36-39`).
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by id,
    /// `$4` id, `$5` project id, `$6` page id, `$7` workspace id.
    PAGE_DUPLICATE_LINK_INSERT = "pages_duplicate_link_insert",
    "INSERT INTO project_pages \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, page_id, workspace_id) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7)",
    []
}

plane_query! {
    /// The response re-fetch (`page/base.py:625-634`):
    /// `Page.objects.filter(pk=page.id).annotate(project_ids=Coalesce(
    /// ArrayAgg("projects__id", distinct=True, filter=~Q(projects__id=True)),
    /// Value([], …))).first()`.
    ///
    /// The annotated queryset is considered unordered (the model's default
    /// `-created_at` is not part of `GROUP BY`), so `.first()` orders by pk
    /// (`django/db/models/query.py`, `first()`/`ordered`). The `projects` join is
    /// the through table with **no** `deleted_at` predicate — Django does not
    /// apply the through model's manager to an M2M aggregate — and the FILTER's
    /// `project_id IS NOT NULL` guard is what Django appends for a negated
    /// condition over a nullable join. `~Q(projects__id=True)` coerces `True`
    /// through `UUIDField.to_python(int)`, i.e. uuid
    /// `00000000-0000-0000-0000-000000000001`.
    ///
    /// Bind order: `$1` page id.
    PAGE_DUPLICATE_REREAD = "pages_duplicate_reread",
    "SELECT p.id AS id, p.name AS name, p.owned_by_id AS owned_by_id, \
     p.access AS access, p.color AS color, p.parent_id AS parent_id, \
     p.is_locked AS is_locked, p.archived_at AS archived_at, \
     p.workspace_id AS workspace_id, p.created_at AS created_at, \
     p.updated_at AS updated_at, p.created_by_id AS created_by_id, \
     p.updated_by_id AS updated_by_id, p.view_props AS view_props, \
     p.logo_props AS logo_props, p.description_html AS description_html, \
     COALESCE(ARRAY_AGG(DISTINCT pp.project_id) \
       FILTER (WHERE NOT (pp.project_id = \
         '00000000-0000-0000-0000-000000000001'::uuid \
         AND pp.project_id IS NOT NULL)), '{}'::uuid[]) AS project_ids \
     FROM pages p LEFT OUTER JOIN project_pages pp ON (p.id = pp.page_id) \
     WHERE p.deleted_at IS NULL AND p.id = $1 \
     GROUP BY p.id ORDER BY p.id ASC LIMIT 1",
    [
        ("id", Uuid),
        ("name", Text),
        ("owned_by_id", Uuid),
        ("access", Int2),
        ("color", Varchar),
        ("parent_id", Uuid),
        ("is_locked", Bool),
        ("archived_at", Date),
        ("workspace_id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("view_props", Jsonb),
        ("logo_props", Jsonb),
        ("description_html", Text),
        ("project_ids", ArrayOf(Uuid)),
    ]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&plane_db::Query] = &[
    &PAGE_DUPLICATE_LOOKUP,
    &PAGE_DUPLICATE_PAGE_INSERT,
    &PAGE_DUPLICATE_LINK_IDS,
    &PAGE_DUPLICATE_LINK_INSERT,
    &PAGE_DUPLICATE_REREAD,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_duplicate_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
