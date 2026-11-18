// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the v1 sticky routes (346–349).
//!
//! Python: `plane/api/views/sticky.py:30-113`. The queryset is
//! `Sticky.objects.filter(workspace__slug=slug).filter(owner_id=user)
//! .distinct()` and `list` orders it `-created_at` — the model's own ordering,
//! unlike the app surface's `-sort_order` (`plane/app/views/workspace/
//! sticky.py:47-59`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `list`'s page (`plane/api/views/sticky.py:66-77`), with the sticky's own
    /// 17 columns in model order. `LIMIT $3` is `limit + 1`, `OFFSET $4` is
    /// `cursor.offset * limit`.
    STICKY_PAGE = "v1_stickies_page",
    "SELECT DISTINCT s.id, s.created_at, s.updated_at, s.deleted_at, s.name, \
     s.description, s.description_html, s.description_stripped, \
     s.description_binary, s.logo_props, s.color, s.background_color, \
     s.sort_order, s.created_by_id, s.updated_by_id, s.workspace_id, s.owner_id \
     FROM stickies s \
     INNER JOIN workspaces w ON s.workspace_id = w.id \
     WHERE s.deleted_at IS NULL AND w.slug = $1 AND s.owner_id = $2 \
     ORDER BY s.created_at DESC",
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
    /// (`plane/api/views/sticky.py:69-70`). The caller escapes `%`, `_` and
    /// `\` and wraps the needle in `%…%`, as Django does.
    STICKY_PAGE_FILTERED = "v1_stickies_page_filtered",
    "SELECT DISTINCT s.id, s.created_at, s.updated_at, s.deleted_at, s.name, \
     s.description, s.description_html, s.description_stripped, \
     s.description_binary, s.logo_props, s.color, s.background_color, \
     s.sort_order, s.created_by_id, s.updated_by_id, s.workspace_id, s.owner_id \
     FROM stickies s \
     INNER JOIN workspaces w ON s.workspace_id = w.id \
     WHERE s.deleted_at IS NULL AND w.slug = $1 AND s.owner_id = $2 \
       AND UPPER(s.description_stripped::text) LIKE UPPER($3) \
     ORDER BY s.created_at DESC",
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
    /// `queryset.count()` — `SELECT COUNT(*) FROM (SELECT DISTINCT stickies.id …)`.
    STICKY_COUNT = "v1_stickies_count",
    "SELECT COUNT(*) FROM ( \
       SELECT DISTINCT s.id FROM stickies s \
       INNER JOIN workspaces w ON s.workspace_id = w.id \
       WHERE s.deleted_at IS NULL AND w.slug = $1 AND s.owner_id = $2 \
     ) AS \"__count\"",
    [("count", Int8)]
}

plane_query! {
    /// [`STICKY_COUNT`] with the `query` filter.
    STICKY_COUNT_FILTERED = "v1_stickies_count_filtered",
    "SELECT COUNT(*) FROM ( \
       SELECT DISTINCT s.id FROM stickies s \
       INNER JOIN workspaces w ON s.workspace_id = w.id \
       WHERE s.deleted_at IS NULL AND w.slug = $1 AND s.owner_id = $2 \
         AND UPPER(s.description_stripped::text) LIKE UPPER($3) \
     ) AS \"__count\"",
    [("count", Int8)]
}

plane_query! {
    /// `retrieve`'s `get_object()`: `get_queryset().get(pk=pk)`
    /// (`plane/api/views/sticky.py:85-87`, `rest_framework/mixins.py:17-24`).
    STICKY_BY_PK = "v1_stickies_by_pk",
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
    /// `serializer.save()` on an existing sticky (`plane/api/views/sticky.py:100`)
    /// — a full-row `UPDATE` after H23 recomputed `description_stripped` and
    /// H01 stamped `updated_at`/`updated_by`.
    STICKY_UPDATE = "v1_stickies_update",
    "UPDATE stickies SET created_at = $1, updated_at = $2, created_by_id = $3, \
     updated_by_id = $4, deleted_at = $5, name = $6, description = $7, \
     description_html = $8, description_stripped = $9, description_binary = $10, \
     logo_props = $11, color = $12, background_color = $13, workspace_id = $14, \
     owner_id = $15, sort_order = $16 \
     WHERE id = $17",
    []
}

/// Every catalogued statement of this route family, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &STICKY_PAGE,
    &STICKY_PAGE_FILTERED,
    &STICKY_COUNT,
    &STICKY_COUNT_FILTERED,
    &STICKY_BY_PK,
    &STICKY_UPDATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("stickies_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
