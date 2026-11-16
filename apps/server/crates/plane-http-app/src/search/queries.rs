// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `search` group.
//!
//! Only two statements have a fixed shape: the "does this issue exist, and what
//! is its parent" lookup `IssueSearchEndpoint`'s three filters share, and the
//! relation-pair list its `issue_relation` filter reads. Everything else in
//! this group depends on whether a `search`/`query` needle and a project scope
//! exist, so it is composed with `Frag` and sent as an unnamed statement
//! (`DECISIONS.md` D1's amendment) — see `crate::search::base` and
//! `crate::search::issue`.
//!
//! `plane_query!` requires a literal, so each statement repeats its SQL; the
//! catalog is what `tests/schema_check.rs` prepares against a Django-migrated
//! database.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Issue.issue_objects.filter(pk=issue_id).first()`
    /// (`plane/app/views/search/issue.py:43`, `:53`, `:72`).
    ///
    /// `issue_objects` (`plane/db/models/issue.py:92-101`) excludes triage
    /// states, archived issues, issues of archived projects and drafts; the
    /// lookup itself is **not** scoped to the URL workspace or project.
    ISSUE_PARENT_BY_PK = "search_issue_parent_by_pk",
    "SELECT i.id, i.parent_id \
     FROM issues i \
     LEFT OUTER JOIN states s ON i.state_id = s.id \
     INNER JOIN projects p ON i.project_id = p.id \
     WHERE i.deleted_at IS NULL \
       AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) \
       AND NOT (i.archived_at IS NOT NULL) \
       AND NOT (p.archived_at IS NOT NULL) \
       AND NOT (i.is_draft) \
       AND i.id = $1 \
     LIMIT 1",
    [("id", Uuid), ("parent_id", Uuid)]
}

plane_query! {
    /// `IssueRelation.objects.filter(Q(related_issue=issue) | Q(issue=issue))
    /// .values_list("issue_id", "related_issue_id").distinct()`
    /// (`plane/app/views/search/issue.py:54-58`).
    ///
    /// `IssueRelation` is soft-deletable, so its default manager's predicate is
    /// part of the statement.
    RELATED_ISSUES = "search_related_issues",
    "SELECT DISTINCT r.issue_id, r.related_issue_id \
     FROM issue_relations r \
     WHERE r.deleted_at IS NULL AND (r.related_issue_id = $1 OR r.issue_id = $1)",
    [("issue_id", Uuid), ("related_issue_id", Uuid)]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&ISSUE_PARENT_BY_PK, &RELATED_ISSUES];

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
            assert!(!query.columns.is_empty());
        }
    }
}
