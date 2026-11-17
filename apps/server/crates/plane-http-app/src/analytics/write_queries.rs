// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `analytic-view` writes (routes 1 and 2).
//!
//! The read statements live in [`super::queries`]. This module holds the three
//! the write verbs need; [`ALL_QUERIES`] is prepared by
//! `crates/plane-http-app/tests/schema_check.rs` against a Django-migrated
//! database.
//!
//! Delete is H03 (`write::soft_delete`, `plane_db::hooks::soft_delete`), which
//! needs no catalogued statement here: it formats one `UPDATE` from constants.
//!
//! `AnalyticView` has no `save()` override (`MODEL_HOOKS.md:1347-1349`), so
//! every write is a plain Django model save: an INSERT on create, a
//! **full-row** UPDATE on PATCH and on the delete's `self.save()`.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `AnalyticView.objects.create(**validated_data)` (`plane/app/serializers/
    /// analytic.py:16-22`) with `workspace_id` added by `perform_create`
    /// (`plane/app/views/analytic/base.py:188-190`).
    ///
    /// The column order is the model's field order (`plane/db/models/
    /// analytic.py:11-25` with the `AuditModel` parents). `query` and
    /// `query_dict` are the serializer's values; every other column is either
    /// a validated body value, a model default or an H01 timestamp.
    ///
    /// Bind order: created_at, updated_at, created_by_id, updated_by_id,
    /// deleted_at, id, workspace_id, name, description, query, query_dict.
    ANALYTIC_VIEW_INSERT = "analytics_write_analytic_view_insert",
    "INSERT INTO analytic_views \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      workspace_id, name, description, query, query_dict) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    []
}

plane_query! {
    /// `ModelSerializer.update` → `instance.save()` on an existing row
    /// (`rest_framework/serializers.py:1034-1084`): a **full** save, so Django
    /// writes every concrete column, with `updated_at` from `auto_now` and
    /// `updated_by_id` from H01. `query` is always the `{}` the serializer's
    /// `update()` recomputes (`plane/app/serializers/analytic.py:25-31`).
    ///
    /// `id` is the primary key, so it is only in the predicate; the SET order
    /// is the model's field order minus the pk.
    ///
    /// Bind order: created_at, updated_at, created_by_id, updated_by_id,
    /// deleted_at, workspace_id, name, description, query, query_dict, id.
    ANALYTIC_VIEW_UPDATE = "analytics_write_analytic_view_update",
    "UPDATE analytic_views SET created_at = $1, updated_at = $2, \
      created_by_id = $3, updated_by_id = $4, deleted_at = $5, \
      workspace_id = $6, name = $7, description = $8, query = $9, \
      query_dict = $10 \
     WHERE id = $11",
    []
}

plane_query! {
    /// PATCH/DELETE's `get_object()` (DRF's `GenericAPIView.get_object`,
    /// `rest_framework/generics.py:85-103`) over `AnalyticViewViewset`'s
    /// queryset (`plane/app/views/analytic/base.py:192-193`):
    /// `AnalyticView.objects.filter(workspace__slug=slug)` plus `pk=` —
    /// the default manager's `deleted_at IS NULL`, the unfiltered join to
    /// `workspaces`, `Meta.ordering = ("-created_at",)` and `LIMIT 21`.
    ///
    /// It is the same shape as [`super::queries::ANALYTIC_VIEW_IN_WORKSPACE`],
    /// kept here so this set's catalog is self-contained; a miss is DRF's
    /// `Http404` → `404 {"detail": "No AnalyticView matches the given
    /// query."}`.
    ///
    /// Bind order: `$1` the workspace slug, `$2` the view id.
    ANALYTIC_VIEW_BY_PK = "analytics_write_analytic_view_by_pk",
    "SELECT analytic_views.\"created_at\", analytic_views.\"updated_at\", \
     analytic_views.\"created_by_id\", analytic_views.\"updated_by_id\", \
     analytic_views.\"deleted_at\", analytic_views.\"id\", \
     analytic_views.\"workspace_id\", analytic_views.\"name\", \
     analytic_views.\"description\", analytic_views.\"query\", \
     analytic_views.\"query_dict\" \
     FROM analytic_views \
     INNER JOIN workspaces ON analytic_views.workspace_id = workspaces.id \
     WHERE analytic_views.deleted_at IS NULL AND workspaces.slug = $1 \
       AND analytic_views.id = $2 \
     ORDER BY analytic_views.created_at DESC LIMIT 21",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("query", Jsonb),
        ("query_dict", Jsonb),
    ]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &ANALYTIC_VIEW_INSERT,
    &ANALYTIC_VIEW_UPDATE,
    &ANALYTIC_VIEW_BY_PK,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
