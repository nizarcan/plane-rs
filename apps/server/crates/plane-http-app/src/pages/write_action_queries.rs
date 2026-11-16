// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the page write actions (routes 128, 129, 130).
//! The read statements live in [`super::queries`].
//!
//! Three statements, all taken from `plane/app/views/page/base.py`:
//!
//! * [`PAGE_ACTION_LOOKUP`] — `PageViewSet.lock`/`.unlock`/`.access`'s
//!   `Page.objects.get(pk=…, workspace__slug=…, projects__id=…,
//!   project_pages__deleted_at__isnull=True)` (`base.py:241-246`, `:253-258`,
//!   `:265-270`);
//! * [`PAGE_DESCRIPTION_LOOKUP`] — `PagesDescriptionViewSet.partial_update`'s
//!   variant with the `Q(owned_by=user) | Q(access=0)` filter (`base.py:518-523`);
//! * [`PAGE_FULL_SAVE`] — the `UPDATE` a full `page.save()` emits.
//!
//! ## One `project_pages` join, not two
//!
//! `projects__id=project_id` (the m2m's through table) and
//! `project_pages__deleted_at__isnull=True` (the reverse FK) look like two
//! relations, but Django reuses the one `project_pages` join for both: the
//! printed SQL of the two filters is a single `INNER JOIN "project_pages"` with
//! `project_pages.deleted_at IS NULL AND project_pages.project_id = …` in the
//! `WHERE` (verified against Django 5.2.17 with `str(queryset.query)`). That
//! matters for `.get()`: a second alias would multiply the rows and turn a page
//! with two live links into `MultipleObjectsReturned`. The statement is issued
//! with `LIMIT 21` and the caller requires exactly one row, which is `.get()`'s
//! own `len(results) != 1` check.
//!
//! ## The full save
//!
//! `Page.save()` writes every concrete column, `created_at` and `id`-less
//! included — Django's `SQLUpdateCompiler` lists all fields of the model in
//! declaration order. The captured statement is
//! `SET created_at, updated_at, created_by_id, updated_by_id, deleted_at,
//! workspace_id, name, description_json, description_binary, description_html,
//! description_stripped, owned_by_id, access, color, parent_id, archived_at,
//! is_locked, view_props, logo_props, is_global, moved_to_page,
//! moved_to_project, sort_order, external_id, external_source WHERE id = $1`.

use plane_db::{Query, plane_query};

plane_query! {
    /// `PageViewSet.lock`/`.unlock`/`.access`'s lookup (`base.py:241-246`,
    /// `:253-258`, `:265-270`), projected to every column a full save writes.
    ///
    /// The joins are the ORM's: `project_pages` (the m2m through) then
    /// `workspaces`; `Meta.ordering = ("-created_at",)` supplies the `ORDER BY`,
    /// which is irrelevant for a unique pk but is part of the statement.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` page id.
    PAGE_ACTION_LOOKUP = "pages_action_lookup",
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
     p.moved_to_page AS moved_to_page, p.moved_to_project AS moved_to_project, \
     p.sort_order AS sort_order, p.external_id AS external_id, \
     p.external_source AS external_source \
     FROM pages p \
     INNER JOIN project_pages pp ON p.id = pp.page_id \
     INNER JOIN workspaces w ON p.workspace_id = w.id \
     WHERE p.deleted_at IS NULL AND w.slug = $1 AND pp.deleted_at IS NULL \
     AND pp.project_id = $2 AND p.id = $3 \
     ORDER BY p.created_at DESC LIMIT 21",
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
    /// `PagesDescriptionViewSet.partial_update`'s lookup (`base.py:518-523`):
    /// [`PAGE_ACTION_LOOKUP`] plus `(owned_by = $4 OR access = 0)`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` page id,
    /// `$4` user id.
    PAGE_DESCRIPTION_LOOKUP = "pages_description_lookup",
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
     p.moved_to_page AS moved_to_page, p.moved_to_project AS moved_to_project, \
     p.sort_order AS sort_order, p.external_id AS external_id, \
     p.external_source AS external_source \
     FROM pages p \
     INNER JOIN project_pages pp ON p.id = pp.page_id \
     INNER JOIN workspaces w ON p.workspace_id = w.id \
     WHERE p.deleted_at IS NULL AND (p.owned_by_id = $4 OR p.access = 0) \
     AND w.slug = $1 AND pp.deleted_at IS NULL \
     AND pp.project_id = $2 AND p.id = $3 \
     ORDER BY p.created_at DESC LIMIT 21",
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
    /// `page.save()` on an existing row: a full-row `UPDATE` in the model's
    /// column order (see the module docs).
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, deleted_at,
    /// workspace, name, description_json, description_binary, description_html,
    /// description_stripped, owned_by, access, color, parent, archived_at,
    /// is_locked, view_props, logo_props, is_global, moved_to_page,
    /// moved_to_project, sort_order, external_id, external_source, id.
    PAGE_FULL_SAVE = "pages_full_save",
    "UPDATE pages SET created_at = $1, updated_at = $2, created_by_id = $3, \
     updated_by_id = $4, deleted_at = $5, workspace_id = $6, name = $7, \
     description_json = $8, description_binary = $9, description_html = $10, \
     description_stripped = $11, owned_by_id = $12, access = $13, color = $14, \
     parent_id = $15, archived_at = $16, is_locked = $17, view_props = $18, \
     logo_props = $19, is_global = $20, moved_to_page = $21, \
     moved_to_project = $22, sort_order = $23, external_id = $24, \
     external_source = $25 \
     WHERE id = $26",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PAGE_ACTION_LOOKUP,
    &PAGE_DESCRIPTION_LOOKUP,
    &PAGE_FULL_SAVE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_action_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_action_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
    }
}
