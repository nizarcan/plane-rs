// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `stickies` group.
//!
//! `WorkspaceStickyViewSet.list` (`plane/app/views/workspace/sticky.py:47-59`)
//! runs three statements: the page (`queryset[offset:stop]`), the count of that
//! same slice (`page_results.count()`) and the total (`queryset.count()`).
//! [`STICKY_PAGE`]/[`STICKY_PAGE_FILTERED`] and
//! [`STICKY_COUNT`]/[`STICKY_COUNT_FILTERED`] are the first and last; the
//! middle one is not issued because it counts the same slice the page
//! statement already returned (`STICKY_PAGE_LIMIT - 1` rows is enough for the
//! `> limit` test, exactly what `page_results.count() > limit` computes) — one
//! fewer round trip than Django.
//!
//! Two statements per shape instead of one with a nullable `$3` filter
//! parameter: a prepared statement with an unused parameter cannot be planned,
//! and `$3::text IS NULL` would change the plan for the unfiltered case.
//!
//! `get_queryset()` is
//! `Sticky.objects.filter(workspace__slug=slug).filter(owner_id=user).select_related("workspace", "owner").distinct()`
//! (`plane/app/views/workspace/sticky.py:23-31`), ordered by `-sort_order` in
//! `list()`. The `workspaces` join is `workspace__slug`; it deliberately does
//! not consult the workspace's `deleted_at` (Django's `__slug` join does not),
//! and the two `select_related` targets contribute no columns beyond
//! `workspace_id`/`owner_id`, so no extra join is needed. `DISTINCT` is kept
//! because the queryset has it.

use plane_db::Query;
use plane_db::plane_query;

// The column list is `StickySerializer`'s declaration order
// (`plane-serializers::generated::app_workspace::StickySerializer`): the
// serializer emits the model's pk, then declared fields (none here), then the
// model fields in model order — which is not the table's column order.

plane_query! {
    /// `Sticky.objects.filter(...).order_by("-sort_order")[offset:offset+limit+1]`.
    ///
    /// `LIMIT $3` is `limit + 1` (`paginate`'s `stop = offset + limit + 1`, so
    /// the extra row is how `next_page_results` is decided), `OFFSET $4` is
    /// `cursor.offset * limit`.
    STICKY_PAGE = "stickies_page",
    "SELECT DISTINCT s.id, s.created_at, s.updated_at, s.deleted_at, s.name, \
     s.description, s.description_html, s.description_stripped, \
     s.description_binary, s.logo_props, s.color, s.background_color, \
     s.sort_order, s.created_by_id, s.updated_by_id, s.workspace_id, s.owner_id \
     FROM stickies s \
     INNER JOIN workspaces w ON s.workspace_id = w.id \
     WHERE s.deleted_at IS NULL AND w.slug = $1 AND s.owner_id = $2 \
     ORDER BY s.sort_order DESC LIMIT $3 OFFSET $4",
    [
        ("id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("name", Text),
        ("description", Jsonb),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_binary", Bytea),
        ("logo_props", Jsonb),
        ("color", Varchar),
        ("background_color", Varchar),
        ("sort_order", Float8),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("workspace_id", Uuid),
        ("owner_id", Uuid),
    ]
}

plane_query! {
    /// [`STICKY_PAGE`] plus `description_stripped__icontains=query`
    /// (`sticky.py:52-53`). Django renders the lookup as
    /// `UPPER(...::text) LIKE UPPER(%query%)` and escapes `%`, `_` and `\` in
    /// the needle before interpolating it — see `crate::stickies::like` for the
    /// escaping the caller applies.
    STICKY_PAGE_FILTERED = "stickies_page_filtered",
    "SELECT DISTINCT s.id, s.created_at, s.updated_at, s.deleted_at, s.name, \
     s.description, s.description_html, s.description_stripped, \
     s.description_binary, s.logo_props, s.color, s.background_color, \
     s.sort_order, s.created_by_id, s.updated_by_id, s.workspace_id, s.owner_id \
     FROM stickies s \
     INNER JOIN workspaces w ON s.workspace_id = w.id \
     WHERE s.deleted_at IS NULL AND w.slug = $1 AND s.owner_id = $2 \
       AND UPPER(s.description_stripped::text) LIKE UPPER($5) \
     ORDER BY s.sort_order DESC LIMIT $3 OFFSET $4",
    [
        ("id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("name", Text),
        ("description", Jsonb),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_binary", Bytea),
        ("logo_props", Jsonb),
        ("color", Varchar),
        ("background_color", Varchar),
        ("sort_order", Float8),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("workspace_id", Uuid),
        ("owner_id", Uuid),
    ]
}

plane_query! {
    /// `queryset.count()` — `SELECT COUNT(*) FROM (SELECT DISTINCT stickies.id …)`
    /// without the slice. The `DISTINCT` and the join are kept so the count is
    /// the length of the same row set the page statement walks.
    STICKY_COUNT = "stickies_count",
    "SELECT COUNT(*) FROM ( \
       SELECT DISTINCT s.id FROM stickies s \
       INNER JOIN workspaces w ON s.workspace_id = w.id \
       WHERE s.deleted_at IS NULL AND w.slug = $1 AND s.owner_id = $2 \
     ) AS \"__count\"",
    [("count", Int8)]
}

plane_query! {
    /// [`STICKY_COUNT`] with the `query` filter of [`STICKY_PAGE_FILTERED`].
    STICKY_COUNT_FILTERED = "stickies_count_filtered",
    "SELECT COUNT(*) FROM ( \
       SELECT DISTINCT s.id FROM stickies s \
       INNER JOIN workspaces w ON s.workspace_id = w.id \
       WHERE s.deleted_at IS NULL AND w.slug = $1 AND s.owner_id = $2 \
         AND UPPER(s.description_stripped::text) LIKE UPPER($3) \
     ) AS \"__count\"",
    [("count", Int8)]
}

plane_query! {
    /// `get_object()`: `Sticky.objects.filter(workspace__slug=slug,
    /// owner=user, pk=pk)` with the model's `select_related("workspace",
    /// "owner")` (`plane/app/views/workspace/sticky.py:23-31`,
    /// `rest_framework/mixins.py:17-24`). The row must be live, in the URL
    /// workspace and owned by the caller; a miss is `Http404` → 404
    /// `{"detail": "Not found."}`.
    STICKY_BY_PK = "stickies_by_pk",
    "SELECT DISTINCT s.id, s.created_at, s.updated_at, s.deleted_at, s.name, \
     s.description, s.description_html, s.description_stripped, s.description_binary, \
     s.logo_props, s.color, s.background_color, s.sort_order, s.created_by_id, \
     s.updated_by_id, s.workspace_id, s.owner_id \
     FROM stickies s \
     INNER JOIN workspaces w ON s.workspace_id = w.id \
     WHERE s.deleted_at IS NULL AND w.slug = $1 AND s.owner_id = $2 AND s.id = $3 \
     LIMIT 1",
    [
        ("id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("name", Text),
        ("description", Jsonb),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_binary", Bytea),
        ("logo_props", Jsonb),
        ("color", Varchar),
        ("background_color", Varchar),
        ("sort_order", Float8),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("workspace_id", Uuid),
        ("owner_id", Uuid),
    ]
}

plane_query! {
    /// `Sticky.objects.create(...)` → `Sticky.save()` → `BaseModel.save()`
    /// (`plane/db/models/sticky.py:38-54`, `plane/db/models/base.py:23-45`).
    ///
    /// Every column is written: `id` from `uuid.uuid4`, the two timestamps from
    /// the field's `auto_now_add`/`auto_now`, the audit columns from H01, the
    /// model defaults for absent body fields, and H23's
    /// `description_stripped`/`sort_order` values. No `RETURNING`: the response
    /// is rendered from the same in-memory values, as `serializer.data` is.
    STICKY_INSERT = "stickies_insert",
    "INSERT INTO stickies (created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, id, name, description, description_html, description_stripped, \
     description_binary, logo_props, color, background_color, workspace_id, \
     owner_id, sort_order) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
    []
}

plane_query! {
    /// `Sticky.save()` on an existing row: a full-row `UPDATE` (`Model.save()`
    /// with no `update_fields`), after H23 recomputed `description_stripped`
    /// and H01 set `updated_by`/`updated_at`.
    STICKY_UPDATE = "stickies_update",
    "UPDATE stickies SET created_at = $1, updated_at = $2, created_by_id = $3, \
     updated_by_id = $4, deleted_at = $5, name = $6, description = $7, \
     description_html = $8, description_stripped = $9, description_binary = $10, \
     logo_props = $11, color = $12, background_color = $13, workspace_id = $14, \
     owner_id = $15, sort_order = $16 \
     WHERE id = $17",
    []
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &STICKY_PAGE,
    &STICKY_PAGE_FILTERED,
    &STICKY_COUNT,
    &STICKY_COUNT_FILTERED,
    &STICKY_BY_PK,
    &STICKY_INSERT,
    &STICKY_UPDATE,
];

// The shared column list is documented above; the statements repeat it because
// `plane_query!` takes a string literal.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
            // A write statement has no result columns; a read must declare
            // them, because that is what the row decoder reads.
            if query.name.starts_with("stickies_page")
                || query.name.starts_with("stickies_count")
                || query.name == "stickies_by_pk"
            {
                assert!(!query.columns.is_empty(), "{} has no columns", query.name);
            }
        }
    }
}
