// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the issue export (`POST …/export-issues/`, route
//! 233).
//!
//! `ExportIssuesEndpoint.post` (`plane/app/views/exporter/base.py:24-66`) runs
//! at most two statements: the project fallback when the body carries no
//! `project` value, and one `exporters` INSERT. Their read counterpart
//! (`ExportIssuesEndpoint.get`) lives in [`super::list_export`].
//!
//! The INSERT exists twice because Django hands the body's `project` value to
//! psycopg2 **unconverted** unless it is a list/tuple
//! (`django/contrib/postgres/fields/array.py:125-131`): a list is converted
//! element by element through `UUIDField`, while a bare `str` reaches Postgres
//! as text and is parsed as an array literal. [`EXPORTER_INSERT`] is the
//! converted path (bound as `uuid[]`) and [`EXPORTER_INSERT_PROJECT_TEXT`] the
//! raw one (`text` cast by the server), so a malformed string stays a
//! `DataError` — the family 500 — instead of becoming a 400.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `if not project_ids:` → `Project.objects.filter(...).values_list("id",
    /// flat=True)` (`plane/app/views/exporter/base.py:33-40`).
    ///
    /// The default manager's `deleted_at IS NULL` (`plane/db/mixins.py:57`), the
    /// unfiltered `workspaces` join the slug filter adds, an `INNER JOIN
    /// project_members` filtered on `member` and `is_active` — **not** on the
    /// membership's `deleted_at` (`base.py:36-38`) — and `Meta.ordering =
    /// ("-created_at",)` (`plane/db/models/project.py:167`).
    ///
    /// Bind order: `$1` the caller's user id, `$2` the workspace slug.
    EXPORT_PROJECT_IDS = "issues_export_project_ids",
    "SELECT projects.id FROM projects \
     INNER JOIN workspaces ON projects.workspace_id = workspaces.id \
     INNER JOIN project_members ON projects.id = project_members.project_id \
     WHERE projects.deleted_at IS NULL \
       AND project_members.member_id = $1 \
       AND project_members.is_active \
       AND projects.archived_at IS NULL \
       AND workspaces.slug = $2 \
     ORDER BY projects.created_at DESC",
    [("id", Uuid)]
}

plane_query! {
    /// `ExporterHistory.objects.create(workspace=…, project=…,
    /// initiated_by=request.user, provider=…, type="issue_exports")`
    /// (`plane/app/views/exporter/base.py:42-48`) for a list-shaped `project`.
    ///
    /// Every column is written, in the model's field order
    /// (`plane/db/models/exporter.py:24-57` with the `AuditModel` parents):
    /// `name`, `deleted_at`, `url` and `filters` are NULL, `updated_by` is NULL
    /// on create (H01), `type`/`status`/`reason`/`key`/`rich_filters` take the
    /// model defaults, and `token` is the `uuid4().hex` the view publishes as
    /// `token_id`.
    ///
    /// Bind order: created_at, updated_at, created_by_id, updated_by_id, id,
    /// workspace_id, project, provider, token, initiated_by_id.
    EXPORTER_INSERT = "issues_exporter_insert",
    "INSERT INTO exporters \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      name, type, workspace_id, project, provider, status, reason, key, url, \
      token, initiated_by_id, filters, rich_filters) \
     VALUES ($1, $2, $3, $4, NULL, $5, NULL, 'issue_exports', $6, $7, $8, \
      'queued', '', '', NULL, $9, $10, NULL, '{}'::jsonb)",
    []
}

plane_query! {
    /// The same INSERT for a `project` the body sent as a single value
    /// (`base.py:30`): Django passes it through untouched, so psycopg2 sends
    /// its text and Postgres parses it as a `uuid[]` array literal. `$7::text`
    /// keeps that parse on the server, which is what makes `"abc"` a
    /// `DataError` (500) and `"<uuid>"` one element.
    ///
    /// Bind order: as [`EXPORTER_INSERT`], with `$7` a string.
    EXPORTER_INSERT_PROJECT_TEXT = "issues_exporter_insert_project_text",
    "INSERT INTO exporters \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      name, type, workspace_id, project, provider, status, reason, key, url, \
      token, initiated_by_id, filters, rich_filters) \
     VALUES ($1, $2, $3, $4, NULL, $5, NULL, 'issue_exports', $6, \
      $7::text::uuid[], $8, 'queued', '', '', NULL, $9, $10, NULL, '{}'::jsonb)",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &EXPORT_PROJECT_IDS,
    &EXPORTER_INSERT,
    &EXPORTER_INSERT_PROJECT_TEXT,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_export_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_export_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
