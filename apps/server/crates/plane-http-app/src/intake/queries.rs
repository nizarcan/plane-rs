// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `intake` group.
//!
//! Every statement here is the ORM SQL Django emits for the read routes of
//! `plane/app/views/intake/base.py` and `IntakeStateEndpoint`
//! (`plane/app/views/state/base.py:151-161`), with the joins and the
//! `GROUP BY` list copied from `CaptureQueriesContext` on the read fixture, so
//! a partially ordered result keeps the plan and the row order Django returns.
//!
//! Dynamic statements (`?status=`, every `issue_filters` lookup, the ordering
//! parameter) are composed with `plane-query` and sent unnamed; see
//! `src/intake/issue.rs`. `DECISIONS.md` D1 and its amendment explain why.
//!
//! Bind order is documented per statement; every one of them is prepared
//! exactly once (`prepare_cached`) because its text never changes.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `IntakeViewSet.get_queryset().first()` with `list`'s serializer fields
    /// (`plane/app/views/intake/base.py:62-77`).
    ///
    /// Two facts come from the ORM, not the view, and are reproduced:
    ///
    /// * `Intake.Meta.ordering = ("name",)` (`plane/db/models/intake.py:33`),
    ///   so `.first()` is `ORDER BY intakes.name ASC LIMIT 1`;
    /// * `pending_issue_count` is `Count("issue_intake", filter=Q(status=-2))`,
    ///   a `LEFT OUTER JOIN intake_issues` whose `deleted_at` is **not**
    ///   filtered — a soft-deleted pending intake issue is counted. The join is
    ///   also what puts the query in `GROUP BY intakes.id, projects.id,
    ///   workspaces.id`.
    ///
    /// The project columns are the subset `ProjectLiteSerializer` reads
    /// (`plane/app/serializers/project.py:113-125`); the cover asset row is
    /// loaded separately by `src/intake/asset.rs`, as Django's
    /// `Project.cover_image_url` property loads it lazily.
    ///
    /// Bind order: `$1` project id, `$2` workspace slug.
    INTAKE_BY_PROJECT = "intake_by_project",
    "SELECT i.\"created_at\", i.\"updated_at\", i.\"created_by_id\", \
     i.\"updated_by_id\", i.\"deleted_at\", i.\"id\", i.\"project_id\", \
     i.\"workspace_id\", i.\"name\", i.\"description\", i.\"is_default\", \
     i.\"view_props\", i.\"logo_props\", \
     COUNT(ii.\"id\") FILTER (WHERE ii.\"status\" = -2) AS \"pending_issue_count\", \
     p.\"id\" AS \"p_id\", p.\"identifier\" AS \"p_identifier\", \
     p.\"name\" AS \"p_name\", p.\"cover_image\" AS \"p_cover_image\", \
     p.\"cover_image_asset_id\" AS \"p_cover_image_asset_id\", \
     p.\"logo_props\" AS \"p_logo_props\", p.\"description\" AS \"p_description\" \
     FROM \"intakes\" i \
     INNER JOIN \"projects\" p ON (i.\"project_id\" = p.\"id\") \
     INNER JOIN \"workspaces\" w ON (i.\"workspace_id\" = w.\"id\") \
     LEFT OUTER JOIN \"intake_issues\" ii ON (i.\"id\" = ii.\"intake_id\") \
     WHERE i.\"deleted_at\" IS NULL AND i.\"project_id\" = $1 AND w.\"slug\" = $2 \
     GROUP BY i.\"id\", p.\"id\", w.\"id\" \
     ORDER BY i.\"name\" ASC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("is_default", Bool),
        ("view_props", Jsonb),
        ("logo_props", Jsonb),
        ("pending_issue_count", Int8),
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
    ]
}

plane_query! {
    /// The `Intake.objects.filter(...).first()` of the intake-issue routes
    /// (`plane/app/views/intake/base.py:180,340,509,556`).
    ///
    /// `Intake.Meta.ordering` makes it `ORDER BY intakes.name ASC LIMIT 1`; the
    /// routes only use the id (and `retrieve`/`partial_update` dereference it
    /// unconditionally, which is why a project without an intake is a 500
    /// there — see `src/intake/issue.rs`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    INTAKE_ONLY = "intake_only",
    "SELECT i.\"id\" FROM \"intakes\" i \
     INNER JOIN \"workspaces\" w ON (i.\"workspace_id\" = w.\"id\") \
     WHERE i.\"deleted_at\" IS NULL AND i.\"project_id\" = $2 AND w.\"slug\" = $1 \
     ORDER BY i.\"name\" ASC LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `Project.objects.get(pk=project_id)` plus the view's guest membership
    /// check, in one statement.
    ///
    /// `plane/app/views/intake/base.py:184,212-222` runs
    /// `Project.objects.get(pk=…)` and then
    /// `ProjectMember.objects.filter(workspace__slug=…, project_id=…,
    /// member=…, role=5, is_active=True).exists()`. The slug join is implied by
    /// the project id once `allow_permission` passed; it is kept here so the
    /// two statements cannot disagree.
    ///
    /// Bind order: `$1` project id, `$2` member id, `$3` workspace slug.
    PROJECT_WITH_GUEST = "intake_project_with_guest",
    "SELECT p.\"guest_view_all_features\", (pm.\"member_id\" IS NOT NULL) AS \"is_guest\" \
     FROM \"projects\" p \
     INNER JOIN \"workspaces\" w ON (p.\"workspace_id\" = w.\"id\") \
     LEFT OUTER JOIN \"project_members\" pm ON (pm.\"project_id\" = p.\"id\" \
       AND pm.\"member_id\" = $2 AND pm.\"role\" = 5 AND pm.\"is_active\" \
       AND pm.\"deleted_at\" IS NULL) \
     WHERE p.\"deleted_at\" IS NULL AND p.\"id\" = $1 AND w.\"slug\" = $3 \
     LIMIT 1",
    [("guest_view_all_features", Bool), ("is_guest", Bool)]
}

plane_query! {
    /// `IntakeIssueViewSet.retrieve`'s one-row query
    /// (`plane/app/views/intake/base.py:511-535`).
    ///
    /// Reproduced from the ORM: `select_related("issue")` is the `INNER JOIN
    /// issues`; the two `ArrayAgg` annotations are the `LEFT OUTER JOIN`s and
    /// the `FILTER` clauses; the aggregate puts the statement in `GROUP BY
    /// intake_issues.id, issues.id`.
    ///
    /// The two arrays are `FieldValue::value` on the Issue serializer because
    /// `IntakeIssueDetailSerializer.to_representation` assigns them when the
    /// attribute exists (`plane/app/serializers/intake.py:117-124`).
    ///
    /// Bind order: `$1` intake id, `$2` issue id, `$3` project id.
    INTAKE_ISSUE_DETAIL = "intake_issue_detail",
    "SELECT ii.\"created_at\", ii.\"updated_at\", ii.\"created_by_id\", \
     ii.\"updated_by_id\", ii.\"deleted_at\", ii.\"id\", ii.\"project_id\", \
     ii.\"workspace_id\", ii.\"intake_id\", ii.\"issue_id\", ii.\"status\", \
     ii.\"snoozed_till\", ii.\"duplicate_to_id\", ii.\"source\", \
     ii.\"source_email\", ii.\"external_source\", ii.\"external_id\", ii.\"extra\", \
     COALESCE(ARRAY_AGG(DISTINCT \"issue_labels\".\"label_id\") \
       FILTER (WHERE (NOT (\"issue_labels\".\"label_id\" IS NULL) \
         AND \"issue_labels\".\"deleted_at\" IS NULL)), '{}'::uuid[]) AS \"label_ids\", \
     COALESCE(ARRAY_AGG(DISTINCT \"issue_assignees\".\"assignee_id\") \
       FILTER (WHERE (NOT (\"issue_assignees\".\"assignee_id\" IS NULL) \
         AND \"issue_assignees\".\"deleted_at\" IS NULL)), '{}'::uuid[]) AS \"assignee_ids\", \
     issues.\"created_at\" AS \"i_created_at\", issues.\"updated_at\" AS \"i_updated_at\", \
     issues.\"created_by_id\" AS \"i_created_by_id\", \
     issues.\"updated_by_id\" AS \"i_updated_by_id\", \
     issues.\"deleted_at\" AS \"i_deleted_at\", issues.\"id\" AS \"i_id\", \
     issues.\"project_id\" AS \"i_project_id\", \
     issues.\"workspace_id\" AS \"i_workspace_id\", \
     issues.\"parent_id\" AS \"i_parent_id\", issues.\"state_id\" AS \"i_state_id\", \
     issues.\"point\" AS \"i_point\", \
     issues.\"estimate_point_id\" AS \"i_estimate_point_id\", \
     issues.\"name\" AS \"i_name\", \
     issues.\"description_json\" AS \"i_description_json\", \
     issues.\"description_html\" AS \"i_description_html\", \
     issues.\"description_stripped\" AS \"i_description_stripped\", \
     issues.\"description_binary\" AS \"i_description_binary\", \
     issues.\"priority\" AS \"i_priority\", \
     issues.\"start_date\" AS \"i_start_date\", \
     issues.\"target_date\" AS \"i_target_date\", \
     issues.\"sequence_id\" AS \"i_sequence_id\", \
     issues.\"sort_order\" AS \"i_sort_order\", \
     issues.\"completed_at\" AS \"i_completed_at\", \
     issues.\"archived_at\" AS \"i_archived_at\", \
     issues.\"is_draft\" AS \"i_is_draft\", \
     issues.\"external_source\" AS \"i_external_source\", \
     issues.\"external_id\" AS \"i_external_id\", \
     issues.\"type_id\" AS \"i_type_id\" \
     FROM \"intake_issues\" ii \
     INNER JOIN \"issues\" ON (ii.\"issue_id\" = issues.\"id\") \
     LEFT OUTER JOIN \"issue_labels\" ON (issues.\"id\" = \"issue_labels\".\"issue_id\") \
     LEFT OUTER JOIN \"issue_assignees\" ON (issues.\"id\" = \"issue_assignees\".\"issue_id\") \
     WHERE ii.\"deleted_at\" IS NULL AND ii.\"intake_id\" = $1 \
       AND ii.\"issue_id\" = $2 AND ii.\"project_id\" = $3 \
     GROUP BY ii.\"id\", issues.\"id\" LIMIT 2",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("intake_id", Uuid),
        ("issue_id", Uuid),
        ("status", Int4),
        ("snoozed_till", Timestamptz),
        ("duplicate_to_id", Uuid),
        ("source", Varchar),
        ("source_email", Text),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("extra", Jsonb),
        ("label_ids", ArrayOf(Uuid)),
        ("assignee_ids", ArrayOf(Uuid)),
        ("i_created_at", Timestamptz),
        ("i_updated_at", Timestamptz),
        ("i_created_by_id", Uuid),
        ("i_updated_by_id", Uuid),
        ("i_deleted_at", Timestamptz),
        ("i_id", Uuid),
        ("i_project_id", Uuid),
        ("i_workspace_id", Uuid),
        ("i_parent_id", Uuid),
        ("i_state_id", Uuid),
        ("i_point", Int4),
        ("i_estimate_point_id", Uuid),
        ("i_name", Varchar),
        ("i_description_json", Jsonb),
        ("i_description_html", Text),
        ("i_description_stripped", Text),
        ("i_description_binary", Bytea),
        ("i_priority", Varchar),
        ("i_start_date", Date),
        ("i_target_date", Date),
        ("i_sequence_id", Int4),
        ("i_sort_order", Float8),
        ("i_completed_at", Timestamptz),
        ("i_archived_at", Date),
        ("i_is_draft", Bool),
        ("i_external_source", Varchar),
        ("i_external_id", Varchar),
        ("i_type_id", Uuid),
    ]
}

plane_query! {
    /// `intake_issue.duplicate_to` — a plain FK dereference with Django's
    /// `_base_manager` (`related_descriptors.py:445-470`), so **no**
    /// `deleted_at` filter: a soft-deleted duplicate target still renders.
    ///
    /// The serializer over it is `IssueIntakeSerializer`
    /// (`plane/app/serializers/intake.py:102`), whose fields are the seven
    /// selected here; `label_ids` is not an attribute of a plain `Issue` and is
    /// therefore skipped (`rest_framework/fields.py:421-446`).
    ///
    /// Bind: `$1` issue id.
    ISSUE_INTAKE_BY_ID = "intake_issue_intake_by_id",
    "SELECT \"issues\".\"id\", \"issues\".\"name\", \"issues\".\"priority\", \
     \"issues\".\"sequence_id\", \"issues\".\"project_id\", \
     \"issues\".\"created_at\", \"issues\".\"created_by_id\" \
     FROM \"issues\" WHERE \"issues\".\"id\" = $1 LIMIT 1",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("priority", Varchar),
        ("sequence_id", Int4),
        ("project_id", Uuid),
        ("created_at", Timestamptz),
        ("created_by_id", Uuid),
    ]
}

plane_query! {
    /// `IntakeStateEndpoint.get` (`plane/app/views/state/base.py:151-161`):
    /// `State.triage_objects.filter(workspace__slug=…, project_id=…)`. The
    /// manager predicate is `deleted_at IS NULL AND group = 'triage'`
    /// (`plane/db/models/state.py:62-68`) and `Meta.ordering = ("sequence",)`.
    ///
    /// Bind order: `$1` project id, `$2` workspace slug.
    TRIAGE_STATE = "intake_triage_state",
    "SELECT s.\"created_at\", s.\"updated_at\", s.\"created_by_id\", \
     s.\"updated_by_id\", s.\"deleted_at\", s.\"id\", s.\"project_id\", \
     s.\"workspace_id\", s.\"name\", s.\"description\", s.\"color\", s.\"slug\", \
     s.\"sequence\", s.\"group\", s.\"is_triage\", s.\"default\", \
     s.\"external_source\", s.\"external_id\" \
     FROM \"states\" s \
     INNER JOIN \"workspaces\" w ON (s.\"workspace_id\" = w.\"id\") \
     WHERE s.\"deleted_at\" IS NULL AND s.\"group\" = 'triage' \
       AND s.\"project_id\" = $1 AND w.\"slug\" = $2 \
     ORDER BY s.\"sequence\" ASC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("color", Varchar),
        ("slug", Varchar),
        ("sequence", Float8),
        ("group", Varchar),
        ("is_triage", Bool),
        ("default", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
    ]
}

plane_query! {
    /// `Issue.objects.get(workspace__slug=…, project_id=…, pk=work_item_id)` in
    /// `IntakeWorkItemDescriptionVersionEndpoint.get`
    /// (`plane/app/views/intake/base.py:586`). The default manager only filters
    /// `deleted_at`; the guest check below it reads `created_by`.
    ///
    /// Bind order: `$1` issue id, `$2` project id, `$3` workspace slug.
    ISSUE_FOR_VERSIONS = "intake_issue_for_versions",
    "SELECT issues.\"id\", issues.\"created_by_id\" FROM \"issues\" \
     INNER JOIN \"workspaces\" w ON (issues.\"workspace_id\" = w.\"id\") \
     WHERE issues.\"deleted_at\" IS NULL AND issues.\"id\" = $1 \
       AND issues.\"project_id\" = $2 AND w.\"slug\" = $3 LIMIT 1",
    [("id", Uuid), ("created_by_id", Uuid)]
}

plane_query! {
    /// `IssueDescriptionVersion.objects.get(...)` for the detail route
    /// (`plane/app/views/intake/base.py:604-613`).
    ///
    /// Bind order: `$1` issue id, `$2` primary key, `$3` project id,
    /// `$4` workspace slug.
    DESCRIPTION_VERSION_DETAIL = "intake_description_version_detail",
    "SELECT v.\"created_at\", v.\"updated_at\", v.\"created_by_id\", \
     v.\"updated_by_id\", v.\"deleted_at\", v.\"id\", v.\"project_id\", \
     v.\"workspace_id\", v.\"issue_id\", v.\"description_binary\", \
     v.\"description_html\", v.\"description_stripped\", v.\"description_json\", \
     v.\"last_saved_at\", v.\"owned_by_id\" \
     FROM \"issue_description_versions\" v \
     INNER JOIN \"workspaces\" w ON (v.\"workspace_id\" = w.\"id\") \
     WHERE v.\"deleted_at\" IS NULL AND v.\"issue_id\" = $1 AND v.\"id\" = $2 \
       AND v.\"project_id\" = $3 AND w.\"slug\" = $4 LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("issue_id", Uuid),
        ("description_binary", Bytea),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_json", Jsonb),
        ("last_saved_at", Timestamptz),
        ("owned_by_id", Uuid),
    ]
}

plane_query! {
    /// `paginate(base_queryset=…)`'s `base_queryset.count()`
    /// (`plane/utils/global_paginator.py:41`), for the description-version
    /// list. The queryset joins `workspaces` for the slug filter
    /// (`plane/app/views/intake/base.py:630-632`).
    ///
    /// Bind order: `$1` issue id, `$2` project id, `$3` workspace slug.
    DESCRIPTION_VERSION_COUNT = "intake_description_version_count",
    "SELECT COUNT(*) FROM \"issue_description_versions\" v \
     INNER JOIN \"workspaces\" w ON (v.\"workspace_id\" = w.\"id\") \
     WHERE v.\"deleted_at\" IS NULL AND v.\"issue_id\" = $1 \
       AND v.\"project_id\" = $2 AND w.\"slug\" = $3",
    [("count", Int8)]
}

plane_query! {
    /// `queryset[start_index:end_index].values(*required_fields)`
    /// (`plane/app/views/intake/base.py:617-641`), the ten aliases Django
    /// names in the `values()` list (FKs become `<field>_id` but keep the name
    /// the view asked for).
    ///
    /// No `ORDER BY`: `IssueDescriptionVersion.Meta` has none, and Django's
    /// statement has none either.
    ///
    /// Bind order: `$1` issue id, `$2` project id, `$3` workspace slug,
    /// `$4` limit, `$5` offset.
    DESCRIPTION_VERSION_PAGE = "intake_description_version_page",
    "SELECT v.\"id\" AS \"id\", v.\"workspace_id\" AS \"workspace\", \
     v.\"project_id\" AS \"project\", v.\"issue_id\" AS \"issue\", \
     v.\"last_saved_at\" AS \"last_saved_at\", v.\"owned_by_id\" AS \"owned_by\", \
     v.\"created_at\" AS \"created_at\", v.\"updated_at\" AS \"updated_at\", \
     v.\"created_by_id\" AS \"created_by\", v.\"updated_by_id\" AS \"updated_by\" \
     FROM \"issue_description_versions\" v \
     INNER JOIN \"workspaces\" w ON (v.\"workspace_id\" = w.\"id\") \
     WHERE v.\"deleted_at\" IS NULL AND v.\"issue_id\" = $1 \
       AND v.\"project_id\" = $2 AND w.\"slug\" = $3 \
     LIMIT $4 OFFSET $5",
    [
        ("id", Uuid),
        ("workspace", Uuid),
        ("project", Uuid),
        ("issue", Uuid),
        ("last_saved_at", Timestamptz),
        ("owned_by", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by", Uuid),
        ("updated_by", Uuid),
    ]
}

plane_query! {
    /// The `FileAsset` rows behind the `*_url` properties this group renders
    /// (`Project.cover_image_url`, `plane/db/models/project.py:128-139`).
    ///
    /// No `deleted_at` filter: the property dereferences the FK through
    /// Django's base manager, which does not apply the soft-delete predicate.
    /// The workspace slug is joined for `FileAsset.asset_url`'s issue-asset
    /// branches (`plane/db/models/asset.py:83-104`); Django loads it lazily.
    ///
    /// Bind: `$1` asset ids.
    ASSETS_BY_IDS = "intake_assets_by_ids",
    "SELECT fa.\"id\", fa.\"entity_type\", fa.\"project_id\", fa.\"issue_id\", w.\"slug\" \
     FROM \"file_assets\" fa \
     LEFT OUTER JOIN \"workspaces\" w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"id\" = ANY($1)",
    [
        ("id", Uuid),
        ("entity_type", Varchar),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("slug", Varchar),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &INTAKE_BY_PROJECT,
    &INTAKE_ONLY,
    &PROJECT_WITH_GUEST,
    &INTAKE_ISSUE_DETAIL,
    &ISSUE_INTAKE_BY_ID,
    &TRIAGE_STATE,
    &ISSUE_FOR_VERSIONS,
    &DESCRIPTION_VERSION_DETAIL,
    &DESCRIPTION_VERSION_COUNT,
    &DESCRIPTION_VERSION_PAGE,
    &ASSETS_BY_IDS,
];
