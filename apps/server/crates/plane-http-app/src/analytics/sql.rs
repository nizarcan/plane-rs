// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `Issue` querysets the analytics views build, as composable SQL.
//!
//! Every endpoint in this group starts from one of two managers and adds
//! lookups the way the Python does — `filter(**filters)` where `filters` is
//! `issue_filters(query_params, "GET")` plus a handful of membership lookups:
//!
//! ```python
//! Issue.issue_objects.filter(workspace__slug=slug, **filters)          # analytics
//! Issue.issue_objects.filter(workspace__slug=slug, **base_filters)     # advance
//! Issue.objects.filter(...)                                           # label details
//! ```
//!
//! The joins those lookups add decide duplicate rows and therefore every
//! aggregate, so this module composes them from `plane-query`'s
//! [`LegacyFilters`] (the same parser the issue list uses) plus the relations
//! `plane-query` does not model (`estimate_points`, the `users` join behind
//! `created_by`/`assignees`).
//!
//! Statements are built dynamically and sent as **unnamed** statements
//! (`DECISIONS.md` D1 amendment): the filter set is request-controlled.
//!
//! ## What Django emits, and why each piece is here
//!
//! `Issue.issue_objects` is `IssueManager` (`plane/db/models/issue.py:92-101`):
//!
//! ```sql
//! issues.deleted_at IS NULL
//! AND NOT (states.group = 'triage' AND states.group IS NOT NULL)
//! AND NOT (issues.archived_at IS NOT NULL)
//! AND NOT (projects.archived_at IS NOT NULL)
//! AND NOT (issues.is_draft)
//! ```
//!
//! with `states` a `LEFT OUTER JOIN` and `projects` an `INNER JOIN`
//! (`plane_query::issue::model::manager_predicate`, which the issue list also
//! uses). `Issue.objects` is the plain `SoftDeletionManager`
//! (`plane/db/mixins.py:57`): only `deleted_at IS NULL`, no joins.

use chrono_tz::Tz;
use plane_query::issue::filter::LegacyFilters;
use plane_query::issue::model::{Relation, manager_predicate};
use plane_query::sql::{Frag, Join, JoinKind, Sql, Value};
use uuid::Uuid;

/// One `FROM <table>` query under construction.
#[derive(Debug, Clone)]
pub(crate) struct QueryBase {
    from: &'static str,
    joins: Vec<Join>,
    predicates: Vec<Frag>,
}

impl QueryBase {
    /// `FROM <table>` with no joins.
    pub(crate) fn new(from: &'static str) -> Self {
        Self {
            from,
            joins: Vec::new(),
            predicates: Vec::new(),
        }
    }

    /// `Issue.issue_objects`: the manager predicate and the joins it needs.
    pub(crate) fn issue_objects() -> Self {
        let mut base = Self::new("issues");
        base.require_relation(Relation::States, JoinKind::Left);
        base.require_relation(Relation::Projects, JoinKind::Inner);
        base.and(manager_predicate());
        base
    }

    /// `Issue.objects`: `deleted_at IS NULL` only.
    pub(crate) fn issue_objects_off() -> Self {
        let mut base = Self::new("issues");
        base.and(Frag::is_null("issues.deleted_at"));
        base
    }

    /// Require a join; requiring the same alias twice keeps one entry and lets
    /// `INNER` win, which is Django's join reuse (`build_filter` reuses the
    /// existing join and promotes it when a filter needs a match).
    pub(crate) fn require(&mut self, join: Join) {
        if let Some(slot) = self.joins.iter_mut().find(|item| item.alias == join.alias) {
            if join.kind == JoinKind::Inner {
                slot.kind = JoinKind::Inner;
            }
            return;
        }
        self.joins.push(join);
    }

    /// Require a join spelled out explicitly (a table `plane-query` does not
    /// model), deduplicating by alias like [`Self::require`].
    pub(crate) fn join(
        &mut self,
        kind: JoinKind,
        table: &'static str,
        alias: &'static str,
        on: Frag,
    ) {
        self.require(Join {
            kind,
            table,
            alias,
            on,
        });
    }

    /// Require one of `plane-query`'s relations, joining its dependency first.
    pub(crate) fn require_relation(&mut self, relation: Relation, kind: JoinKind) {
        if let Some(dependency) = relation.depends_on() {
            self.require_relation(dependency, JoinKind::Left);
        }
        self.require(Join {
            kind,
            table: relation.table(),
            alias: relation.alias(),
            on: relation.on(),
        });
    }

    pub(crate) fn and(&mut self, predicate: Frag) {
        if predicate.is_empty() {
            return;
        }
        self.predicates.push(predicate);
    }

    /// `Workspace.objects.filter(slug=…)`'s join and predicate.
    pub(crate) fn workspace_slug(mut self, slug: &str) -> Self {
        self.require_relation(Relation::Workspaces, JoinKind::Inner);
        self.and(Frag::compare(
            "workspaces.slug",
            "=",
            Value::Text(slug.to_owned()),
        ));
        self
    }

    /// `project__project_projectmember__member=user` and friends.
    ///
    /// `deleted_at` is part of the lookup only when the Python asks for it:
    /// `analytics` does (`project__project_projectmember__deleted_at__isnull`),
    /// `get_analytics_filters`' `base_filters` does not.
    pub(crate) fn project_members(mut self, member_id: Uuid, with_deleted_at: bool) -> Self {
        // `plane-query`'s `Relation` has no `project_members`: the issue list
        // never joins it. Django's join for
        // `project__project_projectmember__member` is `INNER JOIN project_members
        // ON projects.id = project_members.project_id`, and the lookup goes
        // through the project, so `projects` is joined too — including from
        // `Issue.objects`, where the manager predicate does not add it.
        self.require_relation(Relation::Projects, JoinKind::Inner);
        self.require(Join {
            kind: JoinKind::Inner,
            table: "project_members",
            alias: "project_members",
            on: Frag::raw("projects.id = project_members.project_id"),
        });
        if with_deleted_at {
            self.and(Frag::is_null("project_members.deleted_at"));
        }
        self.and(Frag::raw("project_members.is_active"));
        self.and(Frag::compare(
            "project_members.member_id",
            "=",
            Value::Uuid(member_id),
        ));
        self
    }

    /// `issue_filters(query_params, "GET")` as Django applies it.
    pub(crate) fn legacy(mut self, filters: &LegacyFilters, timezone: Tz) -> Self {
        let mut builder = plane_query::issue::filter::FilterBuilder::new();
        filters.apply(&mut builder);
        filters.apply_dates(&mut builder, timezone);
        for (relation, kind) in builder.joins.entries() {
            self.require_relation(relation, kind);
        }
        self.and(builder.predicate);
        self
    }

    /// The `WHERE` clause: every predicate, `AND`-ed in insertion order.
    pub(crate) fn predicate(&self) -> Frag {
        let mut out = Frag::empty();
        for predicate in &self.predicates {
            out = out.and(predicate.clone());
        }
        out
    }

    /// `SELECT <projection> FROM issues <joins> WHERE <predicates><tail>`.
    ///
    /// `tail` carries `GROUP BY`/`ORDER BY`/`LIMIT`, which never bind
    /// parameters here.
    pub(crate) fn statement(&self, projection: Frag, tail: &str) -> Sql {
        let mut frag = Frag::raw("SELECT ");
        frag.push(projection);
        frag.push_raw(" FROM ");
        frag.push_raw(self.from);
        for join in &self.joins {
            frag.push_raw(match join.kind {
                JoinKind::Inner => " INNER JOIN ",
                JoinKind::Left => " LEFT OUTER JOIN ",
            });
            frag.push_raw(join.table);
            frag.push_raw(" ");
            frag.push_raw(join.alias);
            frag.push_raw(" ON ");
            frag.push(join.on.clone());
        }
        frag.push_raw(" WHERE ");
        frag.push(self.predicate());
        frag.push_raw(tail);
        frag.into_sql()
    }
}

impl QueryBase {
    /// `filter(state__group__in=[...])` — the caller has already required the
    /// `states` join at the kind Django emits for it.
    pub(crate) fn state_groups(&mut self, groups: &[&str]) {
        self.and(Frag::in_texts(
            "states.group",
            &groups
                .iter()
                .map(|group| (*group).to_owned())
                .collect::<Vec<_>>(),
        ));
    }
}

/// Prepare and run a composed statement.
///
/// `plane-query`'s [`Sql::prepare`] is an unnamed statement: the text depends
/// on the request (`DECISIONS.md` D1 amendment).
pub(crate) async fn fetch(
    client: &deadpool_postgres::Client,
    sql: &Sql,
) -> Result<Vec<tokio_postgres::Row>, plane_drf::error::ViewError> {
    sql.query(client)
        .await
        .map_err(plane_drf::error::ViewError::from)
}

/// `AT TIME ZONE '<name>'` as a *literal*, the way Django inlines the active
/// timezone (`… AT TIME ZONE 'Asia/Kolkata'`).
///
/// Binding the name (`AT TIME ZONE $1`) changes the SQL text of every
/// occurrence, so two expressions that are otherwise identical no longer
/// compare equal: `GROUP BY 2` then fails with "column must appear in the
/// GROUP BY clause" for the copy numbered `$1`. The name comes from
/// `chrono_tz`'s fixed table and is quoted here anyway.
pub(crate) fn timezone_literal(timezone: Tz) -> String {
    format!("AT TIME ZONE '{}'", timezone.name().replace('\'', "''"))
}

/// `SELECT COUNT(*)` over `base`.
pub(crate) fn count(base: &QueryBase) -> Sql {
    base.statement(Frag::raw("COUNT(*)"), "")
}

/// The `estimate_points` join `SUM(Cast("estimate_point__value", FloatField()))`
/// adds (`plane/utils/analytics_plot.py:111`).
pub(crate) fn require_estimate_points(base: &mut QueryBase) {
    base.require(Join {
        kind: JoinKind::Left,
        table: "estimate_points",
        alias: "estimate_points",
        on: Frag::raw("issues.estimate_point_id = estimate_points.id"),
    });
}

/// The `cycles` join `values("issue_cycle__cycle__name")` adds.
pub(crate) fn require_cycles(base: &mut QueryBase) {
    base.require(Join {
        kind: JoinKind::Inner,
        table: "cycles",
        alias: "cycles",
        on: Frag::raw("cycle_issues.cycle_id = cycles.id"),
    });
}

/// The `modules` join `values("issue_module__module__name")` adds.
pub(crate) fn require_modules(base: &mut QueryBase) {
    base.require(Join {
        kind: JoinKind::Inner,
        table: "modules",
        alias: "modules",
        on: Frag::raw("module_issues.module_id = modules.id"),
    });
}

/// The `labels` join `values("labels__name")` adds.
pub(crate) fn require_labels(base: &mut QueryBase) {
    base.require(Join {
        kind: JoinKind::Inner,
        table: "labels",
        alias: "labels",
        on: Frag::raw("issue_labels.label_id = labels.id"),
    });
}

/// The `users` join behind the assignees (`issue_assignees.assignee_id`).
pub(crate) fn require_assignee_users(base: &mut QueryBase, kind: JoinKind) {
    base.require(Join {
        kind,
        table: "users",
        alias: "assignee_user",
        on: Frag::raw("issue_assignees.assignee_id = assignee_user.id"),
    });
}

/// The `users` join behind `created_by`.
pub(crate) fn require_creator_users(base: &mut QueryBase, kind: JoinKind) {
    base.require(Join {
        kind,
        table: "users",
        alias: "creator_user",
        on: Frag::raw("issues.created_by_id = creator_user.id"),
    });
}

/// `"/api/assets/v2/static/" || avatar_asset || "/"`, else `avatar`.
///
/// The `Case`/`Concat`/`Cast` of `plane/app/views/analytic/base.py:109-127`
/// (and its five copies). Rendered exactly as Django does — a `CASE` whose
/// first branch concatenates the static path with the asset id — because the
/// body carries the resulting string, not the parts.
pub(crate) fn avatar_url(alias: &str) -> String {
    format!(
        "CASE WHEN {alias}.avatar_asset_id IS NOT NULL \
         THEN (COALESCE('/api/assets/v2/static/', '') || COALESCE((COALESCE(({alias}.avatar_asset_id)::varchar, '') || COALESCE('/', '')), '')) \
         WHEN {alias}.avatar_asset_id IS NULL THEN {alias}.avatar ELSE NULL END"
    )
}
