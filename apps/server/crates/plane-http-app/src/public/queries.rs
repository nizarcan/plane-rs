// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL of the `public` group's shared board resolution.
//!
//! Every public view starts by resolving the deploy board for its `anchor`
//! (`PUBLIC_INSTANCES.md` §2a), and the seven statements here are the three
//! lookups and the three write-scope predicates plus their two variants, each
//! copied from the ORM SQL `apps/api/plane/space/` emits:
//!
//! | Statement | Python |
//! | --- | --- |
//! | `PUBLISHED_BOARD_BY_ANCHOR` | `.filter(anchor=anchor, entity_name="project").first()` (`space/views/issue.py:113`) |
//! | `BOARD_BY_ANCHOR` | `.filter(anchor=anchor).first()` (`space/views/asset.py:48`, `space/views/project.py:69`) |
//! | `ISSUE_IN_BOARD_SCOPE` | `_issue_in_board_scope` (`space/views/issue.py:76-87`) |
//! | `COMMENT_IN_BOARD_SCOPE` | `_comment_in_board_scope` (`space/views/issue.py:90-97`) |
//! | `OWN_BOARD_COMMENT` | `_is_own_board_comment` (`space/views/asset.py:27-35`) |
//!
//! The two `.get()` lookups (`intake.py:58`, `meta.py:21`, `project.py:23`,
//! `issue.py:690`, `asset.py:48`) are the same two statements: `anchor` is
//! `unique` (`plane/db/models/deploy_board.py:19`), so `.get()` cannot match
//! more than one row and differs from `.first()` only in what a miss raises
//! (a `DoesNotExist` the view's `handle_exception` turns into a 404, see
//! [`super::board::require_published_board`]).
//!
//! Three facts come from the models, not the views:
//!
//! * `DeployBoard.Meta.ordering = ("-created_at",)` (`deploy_board.py:33`), so
//!   `.first()` is `ORDER BY created_at DESC LIMIT 1`;
//! * `DeployBoard.objects` is `SoftDeletionManager` (`plane/db/mixins.py:57`),
//!   so every lookup carries `deleted_at IS NULL` — a soft-deleted board is a
//!   404, not a hit;
//! * `Issue.issue_objects` is `IssueManager` (`plane/db/models/issue.py:92-101`),
//!   whose four exclusions [`ISSUE_IN_BOARD_SCOPE`] reproduces in full: the
//!   public board writes only to what the public board shows.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `DeployBoard.objects.filter(anchor=anchor, entity_name="project").first()`
    /// (`plane/space/views/issue.py:113`, `asset.py:207,233,258`).
    ///
    /// Bind order: `$1` anchor.
    PUBLISHED_BOARD_BY_ANCHOR = "public_published_board_by_anchor",
    "SELECT b.\"created_at\", b.\"updated_at\", b.\"created_by_id\", \
     b.\"updated_by_id\", b.\"deleted_at\", b.\"id\", b.\"workspace_id\", \
     b.\"project_id\", b.\"entity_identifier\", b.\"entity_name\", \
     b.\"anchor\", b.\"is_comments_enabled\", b.\"is_reactions_enabled\", \
     b.\"intake_id\", b.\"is_votes_enabled\", b.\"view_props\", \
     b.\"is_activity_enabled\", b.\"is_disabled\" \
     FROM deploy_boards b \
     WHERE b.\"deleted_at\" IS NULL AND b.\"anchor\" = $1 \
       AND b.\"entity_name\" = 'project' \
     ORDER BY b.\"created_at\" DESC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("entity_identifier", Uuid),
        ("entity_name", Varchar),
        ("anchor", Varchar),
        ("is_comments_enabled", Bool),
        ("is_reactions_enabled", Bool),
        ("intake_id", Uuid),
        ("is_votes_enabled", Bool),
        ("view_props", Jsonb),
        ("is_activity_enabled", Bool),
        ("is_disabled", Bool),
    ]
}

plane_query! {
    /// `DeployBoard.objects.filter(anchor=anchor).first()` — no `entity_name`
    /// filter (`plane/space/views/asset.py:48`, `project.py:69`). A board for a
    /// page still answers, which is how the asset route reaches a page board's
    /// workspace.
    ///
    /// Bind order: `$1` anchor.
    BOARD_BY_ANCHOR = "public_board_by_anchor",
    "SELECT b.\"created_at\", b.\"updated_at\", b.\"created_by_id\", \
     b.\"updated_by_id\", b.\"deleted_at\", b.\"id\", b.\"workspace_id\", \
     b.\"project_id\", b.\"entity_identifier\", b.\"entity_name\", \
     b.\"anchor\", b.\"is_comments_enabled\", b.\"is_reactions_enabled\", \
     b.\"intake_id\", b.\"is_votes_enabled\", b.\"view_props\", \
     b.\"is_activity_enabled\", b.\"is_disabled\" \
     FROM deploy_boards b \
     WHERE b.\"deleted_at\" IS NULL AND b.\"anchor\" = $1 \
     ORDER BY b.\"created_at\" DESC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("entity_identifier", Uuid),
        ("entity_name", Varchar),
        ("anchor", Varchar),
        ("is_comments_enabled", Bool),
        ("is_reactions_enabled", Bool),
        ("intake_id", Uuid),
        ("is_votes_enabled", Bool),
        ("view_props", Jsonb),
        ("is_activity_enabled", Bool),
        ("is_disabled", Bool),
    ]
}

plane_query! {
    /// `_issue_in_board_scope(issue_id, board)`
    /// (`plane/space/views/issue.py:76-87`):
    ///
    /// ```python
    /// Issue.issue_objects.filter(
    ///     id=issue_id,
    ///     project_id=project_deploy_board.project_id,
    ///     workspace_id=project_deploy_board.workspace_id,
    /// ).exists()
    /// ```
    ///
    /// `IssueManager`'s four exclusions are inlined verbatim from
    /// [`plane_db::models::issue::Issue::ISSUE_OBJECTS`] under the alias `i`
    /// (the unit test below proves the text is the generated predicate).
    ///
    /// Bind order: `$1` issue id, `$2` project id, `$3` workspace id. A board
    /// without a project binds `NULL`: `project_id = NULL` is `NULL` (not true)
    /// exactly as Django's `project_id IS NULL` is false for a column that is
    /// `NOT NULL`.
    ISSUE_IN_BOARD_SCOPE = "public_issue_in_board_scope",
    "SELECT EXISTS (\
       SELECT 1 FROM issues i \
       WHERE i.\"id\" = $1 AND i.\"project_id\" = $2 AND i.\"workspace_id\" = $3 \
         AND (i.\"deleted_at\" IS NULL) \
         AND (NOT (EXISTS (SELECT 1 FROM \"states\" WHERE \"states\".\"id\" = i.\"state_id\" \
              AND \"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL))) \
         AND (NOT ((i.\"archived_at\" IS NOT NULL))) \
         AND (NOT (EXISTS (SELECT 1 FROM \"projects\" WHERE \"projects\".\"id\" = i.\"project_id\" \
              AND \"projects\".\"archived_at\" IS NOT NULL))) \
         AND (NOT ((i.\"is_draft\"))) \
     ) AS \"exists\"",
    [("exists", Bool)]
}

plane_query! {
    /// `_comment_in_board_scope(comment_id, board)`
    /// (`plane/space/views/issue.py:90-97`): an `EXTERNAL` comment in the
    /// board's project and workspace. `IssueComment.objects` is
    /// `SoftDeletionManager`, hence `deleted_at IS NULL`.
    ///
    /// Bind order: `$1` comment id, `$2` project id, `$3` workspace id.
    COMMENT_IN_BOARD_SCOPE = "public_comment_in_board_scope",
    "SELECT EXISTS (\
       SELECT 1 FROM issue_comments \
       WHERE \"id\" = $1 AND \"project_id\" = $2 AND \"workspace_id\" = $3 \
         AND \"access\" = 'EXTERNAL' AND \"deleted_at\" IS NULL\
     ) AS \"exists\"",
    [("exists", Bool)]
}

plane_query! {
    /// `_is_own_board_comment(comment_id, board, user)`
    /// (`plane/space/views/asset.py:27-35`): the same scope as
    /// [`COMMENT_IN_BOARD_SCOPE] plus `actor`.
    ///
    /// Bind order: `$1` comment id, `$2` project id, `$3` workspace id, `$4`
    /// actor id.
    OWN_BOARD_COMMENT = "public_own_board_comment",
    "SELECT EXISTS (\
       SELECT 1 FROM issue_comments \
       WHERE \"id\" = $1 AND \"project_id\" = $2 AND \"workspace_id\" = $3 \
         AND \"access\" = 'EXTERNAL' AND \"actor_id\" = $4 \
         AND \"deleted_at\" IS NULL\
     ) AS \"exists\"",
    [("exists", Bool)]
}

/// Every statement in this catalog, for `tests/schema_check.rs`.
pub const ALL_QUERIES: &[&Query] = &[
    &PUBLISHED_BOARD_BY_ANCHOR,
    &BOARD_BY_ANCHOR,
    &ISSUE_IN_BOARD_SCOPE,
    &COMMENT_IN_BOARD_SCOPE,
    &OWN_BOARD_COMMENT,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_names_are_unique() {
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
        }
    }

    /// Both board lookups select `DeployBoard::COLUMNS` under the alias `b`, so
    /// `DeployBoard::from_row(row, 0)` decodes them.
    #[test]
    fn the_board_lookups_select_the_model_columns() {
        let expected: Vec<String> = plane_db::models::deploy_board::DeployBoard::COLUMNS
            .split(", ")
            .map(|column| {
                let column = column.trim();
                let start = column.find('"').expect("quoted column") + 1;
                let end = column.rfind('"').expect("quoted column");
                format!("b.\"{}\"", &column[start..end])
            })
            .collect();
        let expected = format!("SELECT {} FROM", expected.join(", "));
        for query in [&PUBLISHED_BOARD_BY_ANCHOR, &BOARD_BY_ANCHOR] {
            assert!(
                query.sql.starts_with(&expected),
                "{} does not select DeployBoard::COLUMNS under `b`:\n{}",
                query.name,
                query.sql
            );
        }
    }

    /// `IssueManager`'s filters are the generated model's predicate, not a
    /// hand-written approximation: a change to `Issue::ISSUE_OBJECTS` must fail
    /// here instead of silently widening what a public board may write to.
    #[test]
    fn the_issue_scope_is_the_issue_manager() {
        let predicate =
            plane_db::models::with_alias(plane_db::models::issue::Issue::ISSUE_OBJECTS, "i");
        assert!(
            ISSUE_IN_BOARD_SCOPE.sql.contains(&predicate),
            "ISSUE_IN_BOARD_SCOPE does not inline Issue::ISSUE_OBJECTS:\n{}\n\n{}",
            ISSUE_IN_BOARD_SCOPE.sql,
            predicate
        );
    }
}
