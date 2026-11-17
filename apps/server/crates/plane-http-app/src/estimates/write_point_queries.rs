// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of set B of the estimates writes (routes 48/49).
//!
//! [`super::write_queries`] holds the statements the four write verbs share
//! (the project→workspace lookup, 49's point lookup and 49's issue scan), and
//! [`super::write_estimate_queries`] holds set A's three. This module holds the
//! four statements only the estimate-point verbs add, so each set owns its own
//! file.
//!
//! Reused from the other catalogs rather than duplicated:
//!
//! * 48's `Estimate.objects.filter(pk=…, workspace__slug=…, project_id=…)
//!   .first()` is [`super::queries::ESTIMATE_BY_ID`];
//! * 48's and 49 PATCH's `ProjectBaseModel.save`'s `self.project.workspace` is
//!   [`super::write_queries::ESTIMATE_WRITE_PROJECT`];
//! * 49 PATCH's `EstimatePoint.objects.get(…)` and 49 DELETE's
//!   `EstimatePoint.objects.filter(…).first()` are
//!   [`super::write_queries::ESTIMATE_WRITE_POINT`];
//! * 49 DELETE's issue scan is
//!   [`super::write_queries::ESTIMATE_WRITE_POINT_ISSUES`];
//! * 49 DELETE's `EstimatePoint.objects.filter(estimate_id=…, project_id=…,
//!   workspace__slug=…)` is [`super::queries::ESTIMATE_POINTS_BY_PROJECT`].

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `EstimatePoint.objects.create(…)` (`plane/app/views/estimate/base.py:175-177`)
    /// → `Model.save()`'s INSERT.
    ///
    /// The view passes only `estimate_id`, `project_id`, `key` and `value`;
    /// `description` is the model's `""` default and `deleted_at` its `NULL`
    /// default. Bind order: the two timestamps, the audit columns, the id, the
    /// three FK ids and the three writable columns, in
    /// `local_concrete_fields` order (`django/db/models/base.py:_save_table`).
    ESTIMATE_WRITE_POINT_INSERT = "estimates_write_point_insert",
    "INSERT INTO estimate_points \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, estimate_id, key, description, value) \
     VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, $11)",
    []
}

plane_query! {
    /// 49 PATCH's `serializer.save()` (`plane/app/views/estimate/base.py:193`):
    /// `ModelSerializer.update` assigns the validated fields and calls
    /// `instance.save()` with no `update_fields`, so every non-pk column is
    /// written (`non_pks_non_generated`).
    ///
    /// Bind order: every column but the id, then the id.
    ESTIMATE_WRITE_POINT_UPDATE = "estimates_write_point_full_update",
    "UPDATE estimate_points SET created_at = $1, updated_at = $2, \
       created_by_id = $3, updated_by_id = $4, deleted_at = $5, \
       project_id = $6, workspace_id = $7, estimate_id = $8, key = $9, \
       description = $10, value = $11 \
     WHERE id = $12",
    []
}

plane_query! {
    /// One arm of 49 DELETE's
    /// `EstimatePoint.objects.bulk_update(updated_estimate_points, ["key"],
    /// batch_size=10)` (`plane/app/views/estimate/base.py:261`).
    ///
    /// Django writes one `CASE` statement per batch inside
    /// `transaction.atomic(savepoint=False)` (`django/db/models/query.py:907-924`)
    /// and the SET list holds `key` alone — no `updated_at`, no `updated_by`.
    /// The handler runs one single-row UPDATE per point inside one transaction,
    /// which leaves the same rows.
    ///
    /// Bind order: `key`, `id`.
    ESTIMATE_WRITE_POINT_KEY_UPDATE = "estimates_write_point_key_update",
    "UPDATE estimate_points SET key = $1 WHERE id = $2",
    []
}

plane_query! {
    /// 49 DELETE's `issues.update(estimate_point_id=new_estimate_id)`
    /// (`plane/app/views/estimate/base.py:221`), run once per issue exactly as
    /// the Python loop does.
    ///
    /// `Issue.objects` is `SoftDeletionManager` (`plane/db/mixins.py:56-66`),
    /// so the only row predicate is `deleted_at IS NULL` plus the view's
    /// `project_id`, `estimate_point_id` and `workspace__slug`;
    /// `workspace__slug` compiles to the `IN (SELECT …)` subquery Django uses
    /// for a joined filter on an UPDATE. `QuerySet.update` bypasses
    /// `Issue.save`, so no `updated_at`, no sequence and no hook runs.
    ///
    /// Bind order: `$1` the raw body value (Django's `UUIDField.get_prep_value`
    /// runs before the statement; a value that is not a UUID is the family
    /// `ValidationError` 400), `$2` the point id being deleted, `$3` project
    /// id, `$4` workspace slug.
    ESTIMATE_WRITE_ISSUE_POINT_UPDATE = "estimates_write_issue_point_update",
    "UPDATE issues SET estimate_point_id = $1 \
     WHERE deleted_at IS NULL AND estimate_point_id = $2 AND project_id = $3 \
       AND workspace_id IN (SELECT w.id FROM workspaces w WHERE w.slug = $4)",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &ESTIMATE_WRITE_POINT_INSERT,
    &ESTIMATE_WRITE_POINT_UPDATE,
    &ESTIMATE_WRITE_POINT_KEY_UPDATE,
    &ESTIMATE_WRITE_ISSUE_POINT_UPDATE,
];
