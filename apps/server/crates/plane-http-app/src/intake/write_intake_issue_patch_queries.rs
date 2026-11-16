// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of `IntakeIssueViewSet.partial_update`
//! (`plane/app/views/intake/base.py:335-505`, `plans/INTAKE.md` set C).
//!
//! The link lookup is P0's [`super::write_queries::INTAKE_ISSUE_FOR_ISSUE`];
//! the response re-fetch is the read route's
//! [`super::queries::INTAKE_ISSUE_DETAIL`] because the PATCH's queryset is
//! byte-identical to `retrieve`'s (`:479-503` vs `:511-535`). What is left is
//! the intake id lookup ([`super::queries::INTAKE_ONLY`]), the nested issue
//! pre-image, the link's own issue, the accept transition's default state, the
//! `IntakeIssue` UPDATE and two membership reads that already exist in
//! `projects/queries.rs`.
//!
//! `tests/schema_check.rs` prepares every entry against a migrated database.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// The `issue` pre-image of `partial_update`
    /// (`plane/app/views/intake/base.py:386-403`):
    ///
    /// ```python
    /// Issue.objects.annotate(
    ///     label_ids=…, assignee_ids=…
    /// ).get(pk=intake_issue.issue_id, workspace__slug=slug, project_id=project_id)
    /// ```
    ///
    /// Three differences from the issue PATCH's pre-image
    /// (`issues/list_retrieve.rs::patch_detail`), all of them observable:
    ///
    /// * `Issue.objects`, the plain soft-deletion manager: a soft-deleted issue
    ///   is a `DoesNotExist`/404, and there is no triage/archived/draft/
    ///   archived-project filter;
    /// * only `label_ids`/`assignee_ids` are annotated, so the other five
    ///   annotation-backed fields of `IssueDetailSerializer` are absent and
    ///   `SkipField` drops them from `current_instance`;
    /// * the two aggregates use the through-row `deleted_at` filters below, not
    ///   the `project_members.is_active` join of the create re-fetch.
    ///
    /// The two aggregate expressions are the ones `retrieve`'s
    /// `INTAKE_ISSUE_DETAIL` uses for the same Django annotation
    /// (`intake/queries.rs:147-152`): `Q(label_issue__deleted_at__isnull=True)`
    /// names the through row reached through the label, which for the rows the
    /// read routes already pin to Django is the same predicate as the joined
    /// `issue_labels.deleted_at`.
    ///
    /// The select list is `Issue::COLUMNS` (28 columns, table order) followed
    /// by the two arrays, so `Issue::from_row(row, 0)` decodes the model and
    /// the arrays sit at 28 and 29.
    ///
    /// `get()` evaluates every matching row to raise
    /// `MultipleObjectsReturned`; `LIMIT 2` keeps that distinction.
    ///
    /// Bind order: `$1` issue id (`pk`, the link's `issue_id`), `$2` project
    /// id, `$3` workspace slug.
    INTAKE_ISSUE_PATCH_ISSUE = "intake_issue_patch_issue",
    "SELECT issues.\"created_at\", issues.\"updated_at\", issues.\"created_by_id\", \
     issues.\"updated_by_id\", issues.\"deleted_at\", issues.\"id\", \
     issues.\"project_id\", issues.\"workspace_id\", issues.\"parent_id\", \
     issues.\"state_id\", issues.\"point\", issues.\"estimate_point_id\", \
     issues.\"name\", issues.\"description_json\", issues.\"description_html\", \
     issues.\"description_stripped\", issues.\"description_binary\", \
     issues.\"priority\", issues.\"start_date\", issues.\"target_date\", \
     issues.\"sequence_id\", issues.\"sort_order\", issues.\"completed_at\", \
     issues.\"archived_at\", issues.\"is_draft\", issues.\"external_source\", \
     issues.\"external_id\", issues.\"type_id\", \
     COALESCE(ARRAY_AGG(DISTINCT \"issue_labels\".\"label_id\") \
       FILTER (WHERE (NOT (\"issue_labels\".\"label_id\" IS NULL) \
         AND \"issue_labels\".\"deleted_at\" IS NULL)), '{}'::uuid[]) AS \"label_ids\", \
     COALESCE(ARRAY_AGG(DISTINCT \"issue_assignees\".\"assignee_id\") \
       FILTER (WHERE (NOT (\"issue_assignees\".\"assignee_id\" IS NULL) \
         AND \"issue_assignees\".\"deleted_at\" IS NULL)), '{}'::uuid[]) AS \"assignee_ids\" \
     FROM \"issues\" \
     INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") \
     LEFT OUTER JOIN \"issue_labels\" ON (\"issues\".\"id\" = \"issue_labels\".\"issue_id\") \
     LEFT OUTER JOIN \"issue_assignees\" ON (\"issues\".\"id\" = \"issue_assignees\".\"issue_id\") \
     WHERE \"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1 \
       AND \"issues\".\"project_id\" = $2 AND \"workspaces\".\"slug\" = $3 \
     GROUP BY \"issues\".\"id\" LIMIT 2",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("parent_id", Uuid),
        ("state_id", Uuid),
        ("point", Int4),
        ("estimate_point_id", Uuid),
        ("name", Varchar),
        ("description_json", Jsonb),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_binary", Bytea),
        ("priority", Varchar),
        ("start_date", Date),
        ("target_date", Date),
        ("sequence_id", Int4),
        ("sort_order", Float8),
        ("completed_at", Timestamptz),
        ("archived_at", Date),
        ("is_draft", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("type_id", Uuid),
        ("label_ids", ArrayOf(Uuid)),
        ("assignee_ids", ArrayOf(Uuid)),
    ]
}

plane_query! {
    /// `intake_issue.issue`, the FK dereference the intake serializer performs
    /// (`plane/app/serializers/intake.py:59,81`):
    ///
    /// * `IntakeIssueSerializer.to_representation` renders the nested
    ///   `IssueIntakeSerializer` (`intake.py:29`), whose seven fields are the
    ///   columns selected here;
    /// * `validate` reads `issue.state`, `issue.state.group` and the accept
    ///   transition re-reads `instance.issue`.
    ///
    /// Django runs this through the forward-FK descriptor, i.e. the related
    /// model's **base** manager, so there is no `deleted_at` filter: a
    /// soft-deleted issue still renders and still moves out of triage.
    ///
    /// The `state` dereference is a second query in Python; the `LEFT OUTER
    /// JOIN states` here returns the same `group` (and `NULL` when
    /// `state_id IS NULL`, where Python's `issue.state` is `None`).
    ///
    /// Bind: `$1` issue id.
    INTAKE_ISSUE_PATCH_LINK_ISSUE = "intake_issue_patch_link_issue",
    "SELECT issues.\"created_at\", issues.\"updated_at\", issues.\"created_by_id\", \
     issues.\"updated_by_id\", issues.\"deleted_at\", issues.\"id\", \
     issues.\"project_id\", issues.\"workspace_id\", issues.\"parent_id\", \
     issues.\"state_id\", issues.\"point\", issues.\"estimate_point_id\", \
     issues.\"name\", issues.\"description_json\", issues.\"description_html\", \
     issues.\"description_stripped\", issues.\"description_binary\", \
     issues.\"priority\", issues.\"start_date\", issues.\"target_date\", \
     issues.\"sequence_id\", issues.\"sort_order\", issues.\"completed_at\", \
     issues.\"archived_at\", issues.\"is_draft\", issues.\"external_source\", \
     issues.\"external_id\", issues.\"type_id\", states.\"group\" \
     FROM \"issues\" \
     LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") \
     WHERE \"issues\".\"id\" = $1 LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("parent_id", Uuid),
        ("state_id", Uuid),
        ("point", Int4),
        ("estimate_point_id", Uuid),
        ("name", Varchar),
        ("description_json", Jsonb),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_binary", Bytea),
        ("priority", Varchar),
        ("start_date", Date),
        ("target_date", Date),
        ("sequence_id", Int4),
        ("sort_order", Float8),
        ("completed_at", Timestamptz),
        ("archived_at", Date),
        ("is_draft", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("type_id", Uuid),
        ("group", Varchar),
    ]
}

plane_query! {
    /// `State.objects.filter(workspace=…, project=…, default=True).first()` —
    /// both the `validate` guard (`plane/app/serializers/intake.py:64-66`) and
    /// the accept transition (`:84-86`).
    ///
    /// `State.objects` is `StateManager`: `deleted_at IS NULL` and
    /// `group <> 'triage'` (`plane/db/models/state.py:60-64`). There is no
    /// `is_triage` predicate here — that one belongs to H10a's
    /// `_ensure_default_state`, not to this filter. `Meta.ordering =
    /// ("sequence",)` makes `.first()` `ORDER BY sequence ASC LIMIT 1`.
    ///
    /// The two kwargs are the `workspace`/`project` FK instances, so they bind
    /// as the link row's `workspace_id`/`project_id` — no join.
    ///
    /// Bind order: `$1` workspace id, `$2` project id.
    INTAKE_ISSUE_PATCH_DEFAULT_STATE = "intake_issue_patch_default_state",
    "SELECT s.\"id\" FROM \"states\" s \
     WHERE s.\"deleted_at\" IS NULL AND NOT (s.\"group\" = 'triage') \
       AND s.\"workspace_id\" = $1 AND s.\"project_id\" = $2 AND s.\"default\" \
     ORDER BY s.\"sequence\" ASC LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `IntakeIssue.save()`'s UPDATE (`plane/db/models/intake.py:100-110`):
    /// `super().save()` writes the whole row by pk.
    ///
    /// Only the columns this route can change are written. `Model.save()` with
    /// no `update_fields` rewrites every column, but the ones omitted here keep
    /// their stored value: the serializer's seven fields plus H01 are
    /// `status`, `snoozed_till`, `duplicate_to_id`, `source`, `created_by_id`,
    /// `updated_by_id` and `updated_at`. `deleted_at` stays NULL (the manager
    /// the row was read through), and `ProjectBaseModel.save`'s
    /// `self.workspace = self.project.workspace` re-writes the same
    /// `workspace_id` the row already carries.
    ///
    /// `created_at` is `auto_now_add`, so it is never touched on an update.
    ///
    /// The values are the post-H16 `snoozed_till` and the audit columns H01
    /// set; `updated_at` is the ORM's `auto_now`.
    ///
    /// Bind order: `$1` id, `$2` updated_at, `$3` created_by_id, `$4`
    /// updated_by_id, `$5` status, `$6` snoozed_till, `$7` duplicate_to_id,
    /// `$8` source.
    INTAKE_ISSUE_PATCH_UPDATE = "intake_issue_patch_update",
    "UPDATE \"intake_issues\" SET \"updated_at\" = $2, \"created_by_id\" = $3, \
     \"updated_by_id\" = $4, \"status\" = $5, \"snoozed_till\" = $6, \
     \"duplicate_to_id\" = $7, \"source\" = $8 \
     WHERE \"intake_issues\".\"id\" = $1",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &INTAKE_ISSUE_PATCH_ISSUE,
    &INTAKE_ISSUE_PATCH_LINK_ISSUE,
    &INTAKE_ISSUE_PATCH_DEFAULT_STATE,
    &INTAKE_ISSUE_PATCH_UPDATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_intake_issue_patch_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "intake/write_intake_issue_patch_queries.rs declares {declared} statements but \
             ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
