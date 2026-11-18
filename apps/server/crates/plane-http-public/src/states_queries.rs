// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the v1 state routes (312/313).
//!
//! The create/update/delete statements are the app surface's
//! (`plane_http_app::states::queries`): both views run the same model hooks
//! (H19, H01, H03) on the same table. What is new here is the v1 patched
//! row lookup, the `external_id` guards the v1 view makes and the count for
//! the list's cursor envelope.
//!
//! `State.objects` is `StateManager` (`plane/db/models/state.py:62-68`), whose
//! queryset excludes `group = 'triage'` on top of `deleted_at IS NULL`; every
//! statement below carries both predicates.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `State.objects.get(workspace__slug=slug, project_id=project_id,
    /// pk=state_id)` — `StateDetailAPIEndpoint.patch`
    /// (`plane/api/views/state.py:289`).
    ///
    /// Unlike the read queryset this has **no** membership, `archived_at` or
    /// `is_triage` predicate: the project-admin permission is the only
    /// authorisation, and a triage state is still hidden by the manager.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` state id.
    STATE_BY_ID = "v1_state_by_id",
    "SELECT s.\"created_at\", s.\"updated_at\", s.\"created_by_id\", \
     s.\"updated_by_id\", s.\"deleted_at\", s.\"id\", s.\"project_id\", \
     s.\"workspace_id\", s.\"name\", s.\"description\", s.\"color\", s.\"slug\", \
     s.\"sequence\", s.\"group\", s.\"is_triage\", s.\"default\", \
     s.\"external_source\", s.\"external_id\" \
     FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND w.slug = $1 AND s.project_id = $2 AND s.id = $3 \
     LIMIT 21",
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
    /// `State.objects.filter(project_id=…, workspace__slug=…, external_source=…,
    /// external_id=…).exists()` — the create's 409 guard
    /// (`plane/api/views/state.py:98-104`).
    ///
    /// Bind order: `$1` project id, `$2` workspace slug, `$3` external source,
    /// `$4` external id.
    STATE_EXTERNAL_EXISTS = "v1_state_external_exists",
    "SELECT 1::int4 AS present FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND s.project_id = $1 AND w.slug = $2 \
       AND s.external_source = $3 AND s.external_id = $4 LIMIT 1",
    [("present", Int4)]
}

plane_query! {
    /// The row the create's 409 body reports (`plane/api/views/state.py:105-116`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` external id,
    /// `$4` external source.
    STATE_EXTERNAL_FIRST = "v1_state_external_first",
    "SELECT s.\"created_at\", s.\"updated_at\", s.\"created_by_id\", \
     s.\"updated_by_id\", s.\"deleted_at\", s.\"id\", s.\"project_id\", \
     s.\"workspace_id\", s.\"name\", s.\"description\", s.\"color\", s.\"slug\", \
     s.\"sequence\", s.\"group\", s.\"is_triage\", s.\"default\", \
     s.\"external_source\", s.\"external_id\" \
     FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND w.slug = $1 AND s.project_id = $2 \
       AND s.external_id = $3 AND s.external_source = $4 \
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
    /// The patch's 409 guard (`plane/api/views/state.py:295-301`): the create
    /// filter, with no `exclude` — the comparison against `state.external_id`
    /// is the exclusion.
    ///
    /// Bind order: `$1` project id, `$2` workspace slug, `$3` external source,
    /// `$4` external id.
    STATE_EXTERNAL_EXISTS_PATCH = "v1_state_external_exists_patch",
    "SELECT 1::int4 AS present FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND s.project_id = $1 AND w.slug = $2 \
       AND s.external_source = $3 AND s.external_id = $4 LIMIT 1",
    [("present", Int4)]
}

plane_query! {
    /// `State.objects.filter(workspace__slug, project_id, name=<raw>).first()`
    /// — the `except IntegrityError` lookup (`plane/api/views/state.py:123-127`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` name.
    STATE_BY_NAME_FIRST = "v1_state_by_name_first",
    "SELECT s.\"created_at\", s.\"updated_at\", s.\"created_by_id\", \
     s.\"updated_by_id\", s.\"deleted_at\", s.\"id\", s.\"project_id\", \
     s.\"workspace_id\", s.\"name\", s.\"description\", s.\"color\", s.\"slug\", \
     s.\"sequence\", s.\"group\", s.\"is_triage\", s.\"default\", \
     s.\"external_source\", s.\"external_id\" \
     FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND w.slug = $1 AND s.project_id = $2 AND s.name = $3 \
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
    /// `State.objects.filter(project_id=…).exclude(pk=…).update(default=False)`
    /// — `StateSerializer.save` after a create/patch that set `default`
    /// (`plane/api/serializers/state.py:28-33`).
    ///
    /// A queryset `update()`: one column, `updated_at` untouched; the manager
    /// hides soft-deleted and triage-group rows.
    ///
    /// Bind order: `$1` project id, `$2` the saved state's id.
    STATE_CLEAR_OTHER_DEFAULTS = "v1_state_clear_other_defaults",
    "UPDATE states SET \"default\" = FALSE \
     WHERE project_id = $1 AND deleted_at IS NULL \
       AND NOT (\"group\" = 'triage') AND NOT (id = $2)",
    []
}

plane_query! {
    /// The list's `SELECT COUNT(*)` (`plane/utils/paginator.py:110-132`): the
    /// distinct queryset the page renders.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id.
    STATES_COUNT_BY_PROJECT = "v1_states_count_by_project",
    "SELECT COUNT(*) AS count FROM (SELECT DISTINCT s.id \
     FROM states s \
     INNER JOIN workspaces w ON (s.workspace_id = w.id) \
     INNER JOIN projects p ON (s.project_id = p.id) \
     INNER JOIN project_members pm ON (p.id = pm.project_id) \
     WHERE s.deleted_at IS NULL AND NOT (s.\"group\" = 'triage') \
       AND w.slug = $1 AND s.project_id = $2 AND p.archived_at IS NULL \
       AND pm.is_active AND pm.member_id = $3 AND NOT s.is_triage) counted",
    [("count", Int8)]
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &STATE_BY_ID,
    &STATE_EXTERNAL_EXISTS,
    &STATE_EXTERNAL_FIRST,
    &STATE_EXTERNAL_EXISTS_PATCH,
    &STATE_BY_NAME_FIRST,
    &STATE_CLEAR_OTHER_DEFAULTS,
    &STATES_COUNT_BY_PROJECT,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("states_queries.rs");
        let declared = source.matches(concat!("plane", "_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "states_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
        }
    }

    /// Every lookup is a `SELECT` over the model's columns; `schema_check`
    /// prepares each statement against the migrated database and checks the
    /// column names and types, which is the authoritative guard.
    #[test]
    fn the_lookups_are_selects() {
        assert!(STATE_BY_ID.sql.starts_with("SELECT "));
    }
}
