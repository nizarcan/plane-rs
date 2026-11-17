// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the estimates writes (routes 46–49).
//!
//! The read statements live in [`super::queries`]. This module holds the
//! statements the four write verbs share; a set that owns one verb appends its
//! INSERT/UPDATE/DELETE statements here and adds them to [`ALL_QUERIES`], which
//! `tests/schema_check.rs` prepares against a migrated database.
//!
//! Reused from the read catalog rather than duplicated:
//!
//! * `Estimate.objects.get(pk=…, workspace__slug=…, project_id=…)` (47 PATCH
//!   and DELETE) is [`super::queries::ESTIMATE_BY_ID`];
//! * `estimate.points` / `EstimatePoint.objects.filter(estimate_id=…,
//!   project_id=…, workspace__slug=…)` (46, 47's response, 49's list) is
//!   [`super::queries::ESTIMATE_POINTS_OF_ESTIMATE`] and
//!   [`super::queries::ESTIMATE_POINTS_BY_PROJECT`].

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `ProjectBaseModel.save`'s `self.workspace = self.project.workspace`
    /// (`plane/db/models/project.py:237-239`) — H01's second SELECT.
    ///
    /// `Estimate.objects.create` / `EstimatePoint.objects.create` never pass
    /// `workspace`, so `save()` reads it through the forward FK. Django reads
    /// the whole `projects` row through `_base_manager` (no `deleted_at`
    /// predicate) and then the `workspaces` row; only the id is observable, so
    /// this is one column instead of two full rows.
    ///
    /// Bind: `$1` project id.
    ESTIMATE_WRITE_PROJECT = "estimates_write_project",
    "SELECT p.\"workspace_id\" FROM \"projects\" p WHERE p.\"id\" = $1 LIMIT 1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `EstimatePoint.objects.filter(pk=…, estimate_id=…, project_id=…,
    /// workspace__slug=…).first()` — 49 DELETE's `old_estimate_point`
    /// (`plane/app/views/estimate/base.py:242-247`).
    ///
    /// `.first()` on a queryset that keeps `Meta.ordering = ("value",)` is
    /// `ORDER BY value ASC LIMIT 1`; the columns and the join are
    /// [`super::queries::ESTIMATE_POINTS_BY_PROJECT`]'s plus the pk.
    ///
    /// Bind order: `$1` point id, `$2` estimate id, `$3` project id, `$4`
    /// workspace slug.
    ESTIMATE_WRITE_POINT = "estimates_write_point",
    "SELECT ep.\"created_at\", ep.\"updated_at\", ep.\"created_by_id\", \
     ep.\"updated_by_id\", ep.\"deleted_at\", ep.\"id\", ep.\"project_id\", \
     ep.\"workspace_id\", ep.\"estimate_id\", ep.\"key\", ep.\"description\", \
     ep.\"value\" \
     FROM estimate_points ep \
     INNER JOIN workspaces w ON (ep.workspace_id = w.id) \
     WHERE ep.deleted_at IS NULL AND ep.id = $1 AND ep.estimate_id = $2 \
       AND ep.project_id = $3 AND w.slug = $4 \
     ORDER BY ep.\"value\" ASC LIMIT 1",
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

plane_query! {
    /// 49 DELETE's issue scan (`plane/app/views/estimate/base.py:204-208`,
    /// `:223-227`).
    ///
    /// `Issue.objects` is `SoftDeletionManager` (`plane/db/mixins.py:56-66`),
    /// **not** `IssueManager`: its only predicate is `deleted_at IS NULL`. The
    /// triage, archived, archived-project and draft exclusions belong to
    /// `Issue.issue_objects` (`plane/db/models/issue.py:92-101`, `:172`) and
    /// are not part of this queryset. `Meta.ordering = ("-created_at",)`
    /// (`:174-178`) makes the fan-out order deterministic.
    ///
    /// Only the two columns the view reads are selected: `issue.id` and
    /// `issue.estimate_point_id` both go into the activity task's arguments
    /// (`:210-220`).
    ///
    /// Bind order: `$1` estimate point id, `$2` project id, `$3` workspace
    /// slug.
    ESTIMATE_WRITE_POINT_ISSUES = "estimates_write_point_issues",
    "SELECT i.\"id\", i.\"estimate_point_id\" \
     FROM issues i \
     INNER JOIN workspaces w ON (i.workspace_id = w.id) \
     WHERE i.deleted_at IS NULL AND i.estimate_point_id = $1 \
       AND i.project_id = $2 AND w.slug = $3 \
     ORDER BY i.\"created_at\" DESC",
    [("id", Uuid), ("estimate_point_id", Uuid)]
}

/// Every catalogued statement of this module, in declaration order.
///
/// Sets A (46, 47) and B (48, 49) append their INSERT/UPDATE/DELETE
/// statements here.
pub static ALL_QUERIES: &[&Query] = &[
    &ESTIMATE_WRITE_PROJECT,
    &ESTIMATE_WRITE_POINT,
    &ESTIMATE_WRITE_POINT_ISSUES,
];
