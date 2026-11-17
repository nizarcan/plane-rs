// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of set A of the estimates writes (routes 46/47).
//!
//! [`super::write_queries`] holds the statements the four write verbs share
//! (the project→workspace lookup, 49's point lookup and 49's issue scan). This
//! module holds the three statements 46/47 add, so the next set appends to
//! `write_queries.rs` without touching this file.
//!
//! Reused from the read catalog rather than duplicated:
//!
//! * `Estimate.objects.get(pk=…, workspace__slug=…, project_id=…)` (47 PATCH
//!   and DELETE) is [`super::queries::ESTIMATE_BY_ID`].
//!
//! 47 PATCH's point lookup is [`ESTIMATE_POINTS_BY_IDS`] rather than
//! [`super::queries::ESTIMATE_POINTS_BY_PROJECT`]: the view filters by
//! `pk__in=`, so the dedicated statement keeps the same row set (and the same
//! `value` ordering) instead of reading every point of the estimate.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Estimate.objects.create(...)` (`plane/app/views/estimate/base.py:69-74`)
    /// → `Model.save()`'s INSERT.
    ///
    /// `deleted_at` is the model's `None` default. Bind order: the two
    /// timestamps, the audit columns, the id, the two FK ids and the four
    /// writable columns, in `local_concrete_fields` order
    /// (`django/db/models/base.py:_save_table`).
    ESTIMATE_INSERT = "estimates_write_estimate_insert",
    "INSERT INTO estimates \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, name, description, type, last_used) \
     VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11)",
    []
}

plane_query! {
    /// 47 PATCH's `estimate.save()` (`plane/app/views/estimate/base.py:121`):
    /// a full-row UPDATE (`non_pks_non_generated`, no `update_fields`).
    ///
    /// Bind order: every column but the id, then the id.
    ESTIMATE_UPDATE = "estimates_write_estimate_update",
    "UPDATE estimates SET created_at = $1, updated_at = $2, created_by_id = $3, \
       updated_by_id = $4, deleted_at = $5, project_id = $6, workspace_id = $7, \
       name = $8, description = $9, type = $10, last_used = $11 \
     WHERE id = $12",
    []
}

plane_query! {
    /// `EstimatePoint.objects.bulk_create([...], batch_size=10,
    /// ignore_conflicts=True)` (`plane/app/views/estimate/base.py:82-98`).
    ///
    /// Django builds `INSERT … VALUES (…), …` rows from the 12 concrete
    /// columns, with `deleted_at` NULL. This is one statement over parallel
    /// arrays instead of one per batch: the rows are identical and the whole
    /// insert is inside one transaction either way
    /// (`QuerySet.bulk_create`'s `transaction.atomic`, `query.py:756`).
    ///
    /// Bind order: `created_at`, `updated_at`, `created_by_id`,
    /// `updated_by_id`, `id`, `project_id`, `workspace_id`, `estimate_id`,
    /// `key`, `description`, `value` — one array each.
    ESTIMATE_POINT_BULK_INSERT = "estimates_write_point_bulk_insert",
    "INSERT INTO estimate_points \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, estimate_id, key, description, value) \
     SELECT unnest($1::timestamptz[]), unnest($2::timestamptz[]), \
       unnest($3::uuid[]), unnest($4::uuid[]), NULL, unnest($5::uuid[]), \
       unnest($6::uuid[]), unnest($7::uuid[]), unnest($8::uuid[]), \
       unnest($9::int4[]), unnest($10::text[]), unnest($11::varchar[]) \
     ON CONFLICT DO NOTHING",
    []
}

plane_query! {
    /// One arm of 47 PATCH's
    /// `EstimatePoint.objects.bulk_update(updated, ["key", "value"],
    /// batch_size=10)` (`plane/app/views/estimate/base.py:141`).
    ///
    /// Django writes a single `CASE` statement per batch inside
    /// `transaction.atomic(savepoint=False)` (`django/db/models/query.py:907-924`).
    /// Only `key` and `value` are in the SET list — `updated_at` and
    /// `updated_by_id` are untouched — and the handler runs the same rows in
    /// one transaction, so the final rows are identical.
    ///
    /// Bind order: `key`, `value`, `id`.
    ESTIMATE_POINT_UPDATE = "estimates_write_point_update",
    "UPDATE estimate_points SET key = $1, value = $2 WHERE id = $3",
    []
}

plane_query! {
    /// 47 PATCH's `EstimatePoint.objects.filter(pk__in=…,
    /// workspace__slug=…, project_id=…, estimate_id=…)`
    /// (`plane/app/views/estimate/base.py:125-130`).
    ///
    /// Django compiles `pk__in=[…]` to `id IN (…, …)`; `= ANY($4)` is the same
    /// predicate with one cacheable statement. `Meta.ordering = ("value",)` is
    /// the iteration order the view's loop sees.
    ///
    /// Bind order: `$1` estimate id, `$2` project id, `$3` workspace slug,
    /// `$4` the parsed body ids.
    ESTIMATE_POINTS_BY_IDS = "estimates_write_points_by_ids",
    "SELECT ep.\"created_at\", ep.\"updated_at\", ep.\"created_by_id\", \
     ep.\"updated_by_id\", ep.\"deleted_at\", ep.\"id\", ep.\"project_id\", \
     ep.\"workspace_id\", ep.\"estimate_id\", ep.\"key\", ep.\"description\", \
     ep.\"value\" \
     FROM estimate_points ep \
     INNER JOIN workspaces w ON (ep.workspace_id = w.id) \
     WHERE ep.deleted_at IS NULL AND ep.estimate_id = $1 \
       AND ep.project_id = $2 AND w.slug = $3 AND ep.id = ANY($4) \
     ORDER BY ep.\"value\" ASC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("estimate_id", Uuid),
        ("key", Int4),
        ("description", Text),
        ("value", Varchar),
    ]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &ESTIMATE_INSERT,
    &ESTIMATE_UPDATE,
    &ESTIMATE_POINT_BULK_INSERT,
    &ESTIMATE_POINT_UPDATE,
    &ESTIMATE_POINTS_BY_IDS,
];
