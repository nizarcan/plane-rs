// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The generated `PrimaryKeyRelatedField` lookups, checked against Django.
//!
//! DRF validates a related pk with `queryset.get(pk=data)`
//! (`rest_framework/relations.py:252-265`), where the queryset is the field's
//! own — so a manager that hides triage, archived or draft rows must hide them
//! here too. `tools/inventory/serializers.py` compiles each field's queryset
//! into a statement; this test pins a representative set of those statements to
//! the SQL Django 5.2.17 / DRF 3.18.3 produced on 2026-11-17, so a generator
//! change that alters a predicate fails loudly instead of silently widening
//! what the API accepts.
//!
//! The expected strings were recorded with the repository's own virtualenv:
//!
//! ```python
//! # apps/api/.venv/bin/python
//! qs = IssueCreateSerializer().fields["state_id"].queryset
//! lookup = qs.filter(pk=uuid.UUID("00000000-0000-0000-0000-000000000000"))
//! sql, params = lookup.values_list("pk", flat=True).order_by().query.sql_with_params()
//! # The pk parameter is rendered as `$1`; the queryset's own constants
//! # (`'triage'`) are inlined the way Postgres quotes a literal.
//! ```
//!
//! A list field's child compiles `pk__in` with one element, which Django
//! renders as `IN ($1)`; the helper runs that statement once per element.

use plane_db::Query;
use plane_http_app::write::generated::{
    draft_issue_create_serializer, intake_issue_serializer, issue_create_serializer,
    page_serializer, user_favorite_serializer,
};

/// `(statement, recorded SQL)`, in the order Django's compiler emitted it.
fn recorded() -> Vec<(&'static Query, &'static str)> {
    vec![
        // `State.all_state_objects.all()` — the declared field's queryset. It
        // is a plain manager, so a soft-deleted or triage state passes the
        // field and the serializer's `validate()` rejects it instead.
        (
            &issue_create_serializer::PK_STATE_ID,
            "SELECT \"states\".\"id\" AS \"pk\" FROM \"states\" WHERE \"states\".\"id\" = $1",
        ),
        // `Issue.objects.all()` — the soft-deletion manager only: an archived,
        // draft or triage parent is accepted under `parent_id`.
        (
            &issue_create_serializer::PK_PARENT_ID,
            "SELECT \"issues\".\"id\" AS \"pk\" FROM \"issues\" \
             WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1)",
        ),
        // `Label.objects.all()` through a `ListField` child: one `IN ($1)` per
        // element, so a soft-deleted label is an indexed `does_not_exist`.
        (
            &issue_create_serializer::PK_LABEL_IDS,
            "SELECT \"labels\".\"id\" AS \"pk\" FROM \"labels\" \
             WHERE (\"labels\".\"deleted_at\" IS NULL AND \"labels\".\"id\" IN ($1))",
        ),
        // The model `parent` field uses `Issue.issue_objects`
        // (`IssueManager`): triage, archived, draft and archived-project rows
        // are all `does_not_exist` here.
        (
            &issue_create_serializer::PK_PARENT,
            "SELECT \"issues\".\"id\" AS \"pk\" FROM \"issues\" \
             LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") \
             INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") \
             WHERE (\"issues\".\"deleted_at\" IS NULL \
             AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) \
             AND NOT (\"issues\".\"archived_at\" IS NOT NULL) \
             AND NOT (\"projects\".\"archived_at\" IS NOT NULL) \
             AND NOT (\"issues\".\"is_draft\") \
             AND \"issues\".\"id\" = $1)",
        ),
        // The model `state` field uses `State.objects` (`StateManager`):
        // soft-deleted and triage states are `does_not_exist`.
        (
            &issue_create_serializer::PK_STATE,
            "SELECT \"states\".\"id\" AS \"pk\" FROM \"states\" \
             WHERE (\"states\".\"deleted_at\" IS NULL \
             AND NOT (\"states\".\"group\" = 'triage') AND \"states\".\"id\" = $1)",
        ),
        (
            &issue_create_serializer::PK_ESTIMATE_POINT,
            "SELECT \"estimate_points\".\"id\" AS \"pk\" FROM \"estimate_points\" \
             WHERE (\"estimate_points\".\"deleted_at\" IS NULL \
             AND \"estimate_points\".\"id\" = $1)",
        ),
        (
            &issue_create_serializer::PK_TYPE,
            "SELECT \"issue_types\".\"id\" AS \"pk\" FROM \"issue_types\" \
             WHERE (\"issue_types\".\"deleted_at\" IS NULL AND \"issue_types\".\"id\" = $1)",
        ),
        // `DraftIssueCreateSerializer.state_id` declares `State.objects.all()`,
        // unlike `IssueCreateSerializer.state_id` above.
        (
            &draft_issue_create_serializer::PK_STATE_ID,
            "SELECT \"states\".\"id\" AS \"pk\" FROM \"states\" \
             WHERE (\"states\".\"deleted_at\" IS NULL \
             AND NOT (\"states\".\"group\" = 'triage') AND \"states\".\"id\" = $1)",
        ),
        // `IntakeIssueSerializer.duplicate_to` is a model FK, so it reads
        // `Issue._default_manager` = `issue_objects`.
        (
            &intake_issue_serializer::PK_DUPLICATE_TO,
            "SELECT \"issues\".\"id\" AS \"pk\" FROM \"issues\" \
             LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") \
             INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") \
             WHERE (\"issues\".\"deleted_at\" IS NULL \
             AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) \
             AND NOT (\"issues\".\"archived_at\" IS NOT NULL) \
             AND NOT (\"projects\".\"archived_at\" IS NOT NULL) \
             AND NOT (\"issues\".\"is_draft\") \
             AND \"issues\".\"id\" = $1)",
        ),
        // `UserFavorite.parent` is `UserFavorite.objects`, a soft-deletion
        // manager (the favourites domain's live-pk case).
        (
            &user_favorite_serializer::PK_PARENT,
            "SELECT \"user_favorites\".\"id\" AS \"pk\" FROM \"user_favorites\" \
             WHERE (\"user_favorites\".\"deleted_at\" IS NULL \
             AND \"user_favorites\".\"id\" = $1)",
        ),
        // `PageSerializer.labels` is a `ListField` of `PrimaryKeyRelatedField`
        // children over `Label.objects`.
        (
            &page_serializer::PK_LABELS,
            "SELECT \"labels\".\"id\" AS \"pk\" FROM \"labels\" \
             WHERE (\"labels\".\"deleted_at\" IS NULL AND \"labels\".\"id\" IN ($1))",
        ),
    ]
}

#[test]
fn generated_pk_sql_matches_django() {
    for (statement, expected) in recorded() {
        assert_eq!(
            statement.sql, expected,
            "{}: the generated lookup drifted from Django's compiled SQL",
            statement.name
        );
        assert_eq!(
            statement.columns.len(),
            1,
            "{}: a pk lookup selects exactly one column",
            statement.name
        );
        assert_eq!(statement.columns[0].name, "pk");
        assert_eq!(statement.columns[0].pg_type.name(), "uuid");
    }
}

/// The predicates that make the manager-backed fields stricter than a plain
/// `id = $1` are the whole point of the change; keep them visible in one place.
#[test]
fn manager_predicates_are_present() {
    let parent = issue_create_serializer::PK_PARENT.sql;
    for predicate in [
        "\"states\".\"group\" = 'triage'",
        "\"issues\".\"archived_at\" IS NOT NULL",
        "\"projects\".\"archived_at\" IS NOT NULL",
        "\"issues\".\"is_draft\"",
        "\"issues\".\"deleted_at\" IS NULL",
    ] {
        assert!(
            parent.contains(predicate),
            "Issue.issue_objects lookup is missing {predicate}: {parent}"
        );
    }
    let state = issue_create_serializer::PK_STATE.sql;
    assert!(state.contains("\"states\".\"group\" = 'triage'"));
    // `state_id` must not: its queryset is `State.all_state_objects`.
    assert!(
        !issue_create_serializer::PK_STATE_ID
            .sql
            .contains("'triage'"),
        "State.all_state_objects must not filter the triage group"
    );
}
