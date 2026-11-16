// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/order_queryset.py`, ported.
//!
//! `order_issue_queryset` turns the `order_by` parameter into an annotated
//! queryset *and* returns a new parameter string. The returned string is what
//! the paginator uses, which is why the two disagree in one case: for
//! `order_by=priority` the function orders `priority_order ASC` but returns
//! `-priority_order`, and `OffsetPaginator` then re-applies it with
//! `desc(nulls_last=True)` — so the list endpoint returns *low* priorities
//! first. `IssueListEndpoint`, which uses `global_paginator.paginate` and never
//! re-applies the ordering, returns *urgent* first for the same parameter.
//! Both are kept.
//!
//! ## Ordering is not total
//!
//! Every branch appends `-created_at` as a tiebreak, so an ordering is total
//! only while `created_at` is distinct. `order_by=created_at` does *not*
//! append it (`order_queryset.py:201-203`), so ties there — and ties in
//! `created_at` itself — fall back to whatever Postgres produces, which is not
//! stable across plans. The fixture gives every issue a distinct `created_at`
//! so the goldens are reproducible; in production the same query can return
//! tied rows in a different order, exactly as Django does today. That is a
//! documented upstream property, not something the port may "fix": changing
//! the tiebreak would change the output.

use crate::issue::model::JoinSet;
use crate::issue::model::Relation;
use crate::sql::{Frag, Select, Value};

/// `PRIORITY_ORDER` (`order_queryset.py:8`).
pub const PRIORITY_ORDER: [&str; 5] = ["urgent", "high", "medium", "low", "none"];

/// `STATE_ORDER` (`order_queryset.py:9`).
pub const STATE_ORDER: [&str; 5] = ["backlog", "unstarted", "started", "completed", "cancelled"];

/// `ISSUE_ORDER_BY_ALLOWLIST` (`order_queryset.py:18`).
pub const ISSUE_ORDER_BY_ALLOWLIST: [&str; 14] = [
    "created_at",
    "updated_at",
    "sequence_id",
    "sort_order",
    "target_date",
    "start_date",
    "completed_at",
    "archived_at",
    "priority",
    "state__name",
    "state__group",
    "assignees__first_name",
    "labels__name",
    "issue_module__module__name",
];

/// `sanitize_order_by` (`order_queryset.py:135`).
pub fn sanitize_order_by(value: &str, allowed: &[&str], default: &str) -> String {
    if value.is_empty() {
        return default.to_owned();
    }
    let is_desc = value.starts_with('-');
    let bare = if is_desc { &value[1..] } else { value };
    if bare.starts_with('-') {
        return default.to_owned();
    }
    if !allowed.contains(&bare) {
        return default.to_owned();
    }
    if is_desc {
        format!("-{bare}")
    } else {
        bare.to_owned()
    }
}

/// Which expression an ordering sorts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderKey {
    /// `priority_order`: a `CASE` over `PRIORITY_ORDER`.
    PriorityOrder,
    /// `state_order`: a `CASE` over `STATE_ORDER` with `default=len`.
    StateOrder,
    /// `min_values`: `Min(<field>)`, which also adds `GROUP BY issues.id`.
    MinValues(&'static str),
    /// A plain column or joined column.
    Plain(&'static str),
}

/// A resolved ordering.
#[derive(Debug, Clone)]
pub struct Ordering {
    /// Exactly what `order_issue_queryset` returns (`"-priority_order"`).
    pub returned: String,
    pub key: OrderKey,
    /// `returned` starts with `-` (what the paginators use).
    pub desc: bool,
    /// The sanitized *input* starts with `-` (what `order_issue_queryset`
    /// uses itself). The two differ for `priority` and `state__group`, whose
    /// returned string is inverted.
    pub own_desc: bool,
    /// The expression the paginator sorts on (the annotation, or the column).
    pub expression: Frag,
    /// The ordering `order_issue_queryset` itself applies, before the
    /// paginator re-applies `returned`. Used by the endpoints that paginate
    /// without `OffsetPaginator` (`IssueListEndpoint`).
    pub own_order: Vec<Frag>,
    /// `annotate(Min(...))` puts the query in `GROUP BY issues.id`.
    pub group_by_issue: bool,
    /// Joins the ordering needs.
    pub joins: JoinSet,
}

impl Ordering {
    /// `order_issue_queryset(issue_queryset, order_by_param)`.
    pub fn resolve(param: &str) -> Self {
        let param = sanitize_order_by(param, &ISSUE_ORDER_BY_ALLOWLIST, "-created_at");
        let own_desc = param.starts_with('-');
        let mut joins = JoinSet::new();
        if param == "priority" || param == "-priority" {
            let desc = param.starts_with('-');
            // The returned string is the *opposite* of the annotation's own
            // direction: `priority` → `-priority_order`.
            let returned = if desc {
                "priority_order"
            } else {
                "-priority_order"
            };
            let expression = priority_case();
            let own_order = vec![
                directed(expression.clone(), if desc { " DESC" } else { " ASC" }),
                Frag::raw("issues.created_at DESC"),
            ];
            return Self {
                returned: returned.to_owned(),
                key: OrderKey::PriorityOrder,
                desc: returned.starts_with('-'),
                // `order_issue_queryset` always orders the annotation ASC; only
                // the returned parameter is inverted.
                own_desc: false,
                expression,
                own_order,
                group_by_issue: false,
                joins,
            };
        }
        if param == "state__group" || param == "-state__group" {
            let desc = param.starts_with('-');
            let order: Vec<&str> = if desc {
                STATE_ORDER.iter().rev().copied().collect()
            } else {
                STATE_ORDER.to_vec()
            };
            joins.require_left(Relation::States);
            let expression = state_case(&order);
            let own_order = vec![
                directed(expression.clone(), if desc { " DESC" } else { " ASC" }),
                Frag::raw("issues.created_at DESC"),
            ];
            let returned = if desc { "-state_order" } else { "state_order" };
            return Self {
                returned: returned.to_owned(),
                key: OrderKey::StateOrder,
                desc: returned.starts_with('-'),
                // The CASE itself carries the direction, so the ordering is
                // always ASC (`order_queryset.py:174-183`).
                own_desc: false,
                expression,
                own_order,
                group_by_issue: false,
                joins,
            };
        }
        if let Some(field) = min_values_field(&param) {
            let desc = param.starts_with('-');
            for relation in field_relations(field) {
                joins.require_left(relation);
            }
            let expression = Frag::raw(format!("MIN({})", field_column(field)));
            let own_order = vec![
                directed(expression.clone(), if desc { " DESC" } else { " ASC" }),
                Frag::raw("issues.created_at DESC"),
            ];
            return Self {
                returned: if desc {
                    "-min_values".to_owned()
                } else {
                    "min_values".to_owned()
                },
                key: OrderKey::MinValues(field),
                desc,
                own_desc,
                expression,
                own_order,
                group_by_issue: true,
                joins,
            };
        }

        // The plain branch. `state__name` needs the states join;
        // `state__group` never reaches here (it is the CASE branch above).
        let desc = param.starts_with('-');
        let bare_owned = if desc {
            param[1..].to_owned()
        } else {
            param.clone()
        };
        let bare = bare_owned.as_str();
        if bare == "state__name" {
            joins.require_left(Relation::States);
        }
        let column = plain_column(bare);
        let expression = Frag::raw(column);
        let expression_with_direction =
            directed(expression.clone(), if desc { " DESC" } else { " ASC" });
        let own_order = if bare.contains("created_at") {
            vec![expression_with_direction]
        } else {
            vec![
                expression_with_direction,
                Frag::raw("issues.created_at DESC"),
            ]
        };
        Self {
            returned: param,
            key: OrderKey::Plain(bare_static(bare)),
            desc,
            own_desc,
            expression,
            own_order,
            group_by_issue: false,
            joins,
        }
    }

    /// The `issue_on_results` field that carries the key, when the key is a
    /// plain column already in the projection.
    fn key_field(&self) -> Option<crate::issue::model::ValueField> {
        use crate::issue::model::ValueField;
        match self.key {
            OrderKey::Plain(bare) => match bare {
                "created_at" => Some(ValueField::CreatedAt),
                "updated_at" => Some(ValueField::UpdatedAt),
                "sequence_id" => Some(ValueField::SequenceId),
                "sort_order" => Some(ValueField::SortOrder),
                "target_date" => Some(ValueField::TargetDate),
                "start_date" => Some(ValueField::StartDate),
                "completed_at" => Some(ValueField::CompletedAt),
                "archived_at" => Some(ValueField::ArchivedAt),
                "priority" => Some(ValueField::Priority),
                _ => None,
            },
            _ => None,
        }
    }

    /// Make sure the key expression is in the select list and return its
    /// 1-based position.
    ///
    /// Postgres matches an `ORDER BY` expression against the select list
    /// structurally, and two bind parameters with different numbers are not
    /// equal — so a parameterised key (the `CASE` expressions) has to be
    /// ordered by **ordinal**, exactly as Django does (`ORDER BY 29 ASC`).
    fn key_position(
        &self,
        select: &mut Select,
        fields: &[crate::issue::model::ValueField],
    ) -> usize {
        if let Some(field) = self.key_field()
            && let Some(index) = fields.iter().position(|candidate| *candidate == field)
        {
            return index + 1;
        }
        if let Some(index) = select
            .projection
            .iter()
            .position(|frag| frag.sql == self.expression.sql)
        {
            return index + 1;
        }
        select.projection.push(self.expression.clone());
        select.projection.len()
    }

    /// Set the select's ordering to what the paginators apply:
    /// `key ASC|DESC NULLS LAST, created_at DESC` (`paginator.py:136-140`).
    pub fn apply_paginator_order(
        &self,
        select: &mut Select,
        fields: &[crate::issue::model::ValueField],
    ) {
        let position = self.key_position(select, fields);
        let direction = if self.desc { "DESC" } else { "ASC" };
        select.order_by = vec![
            Frag::raw(format!("{position} {direction} NULLS LAST")),
            Frag::raw("issues.created_at DESC"),
        ];
    }

    /// Set the select's ordering to what `order_issue_queryset` itself applies
    /// (used by endpoints that paginate without `OffsetPaginator`).
    pub fn apply_own_order(&self, select: &mut Select, fields: &[crate::issue::model::ValueField]) {
        let position = self.key_position(select, fields);
        let direction = if self.own_desc { "DESC" } else { "ASC" };
        select.order_by = vec![Frag::raw(format!("{position} {direction}"))];
        if self.expression.sql != "issues.created_at" {
            select.order_by.push(Frag::raw("issues.created_at DESC"));
        }
    }

    /// The paginator's ordering: `field ASC|DESC NULLS LAST, created_at DESC`.
    ///
    /// `OffsetPaginator.get_result` (`paginator.py:136-140`) and the grouped
    /// paginators (`paginator.py:255-269,461-472`) all re-apply the key this
    /// way, discarding whatever `order_issue_queryset` set.
    pub fn paginator_order(&self) -> Vec<Frag> {
        let direction = if self.desc {
            " DESC NULLS LAST"
        } else {
            " ASC NULLS LAST"
        };
        vec![
            directed(self.expression.clone(), direction),
            Frag::raw("issues.created_at DESC"),
        ]
    }

    /// The `ORDER BY` list for endpoints that do not re-apply the key.
    pub fn own_order(&self) -> Vec<Frag> {
        self.own_order.clone()
    }
}

/// `priority_order` as Django renders it: `CASE WHEN … THEN 0 … END` with no
/// `ELSE`, so an unknown priority is `NULL` (and sorts last under `ASC NULLS
/// LAST`).
fn priority_case() -> Frag {
    let mut frag = Frag::raw("CASE");
    for (index, priority) in PRIORITY_ORDER.iter().enumerate() {
        frag.push_raw(" WHEN issues.priority = ");
        frag.push(Frag::bind(Value::Text((*priority).to_owned())));
        frag.push_raw(&format!(" THEN {index}"));
    }
    frag.push_raw(" ELSE NULL END");
    frag
}

/// `state_order` as Django renders it, including the `default`.
fn state_case(order: &[&str]) -> Frag {
    let mut frag = Frag::raw("CASE");
    for (index, group) in order.iter().enumerate() {
        frag.push_raw(" WHEN states.group = ");
        frag.push(Frag::bind(Value::Text((*group).to_owned())));
        frag.push_raw(&format!(" THEN {index}"));
    }
    frag.push_raw(&format!(" ELSE {} END", order.len()));
    frag
}

/// The min-values orderings and the column they aggregate.
fn min_values_field(param: &str) -> Option<&'static str> {
    let bare = param.strip_prefix('-').unwrap_or(param);
    match bare {
        "labels__name" => Some("labels.name"),
        "assignees__first_name" => Some("users.first_name"),
        "issue_module__module__name" => Some("modules.name"),
        _ => None,
    }
}

fn field_column(field: &str) -> &'static str {
    match field {
        "labels.name" => "labels.name",
        "users.first_name" => "users.first_name",
        "modules.name" => "modules.name",
        _ => "issues.id",
    }
}

fn field_relations(field: &str) -> Vec<Relation> {
    match field {
        "labels.name" => vec![Relation::IssueLabels, Relation::Labels],
        "users.first_name" => vec![Relation::IssueAssignees, Relation::Users],
        "modules.name" => vec![Relation::ModuleIssues, Relation::Modules],
        _ => Vec::new(),
    }
}

/// The column a plain ordering field maps to.
fn plain_column(bare: &str) -> &'static str {
    match bare {
        "created_at" => "issues.created_at",
        "updated_at" => "issues.updated_at",
        "sequence_id" => "issues.sequence_id",
        "sort_order" => "issues.sort_order",
        "target_date" => "issues.target_date",
        "start_date" => "issues.start_date",
        "completed_at" => "issues.completed_at",
        "archived_at" => "issues.archived_at",
        "priority" => "issues.priority",
        "state__name" => "states.name",
        _ => "issues.created_at",
    }
}

/// Intern the allowlisted field name; the fallback is the sanitizer's default
/// and cannot be reached because `sanitize_order_by` rejected everything else.
fn bare_static(bare: &str) -> &'static str {
    ISSUE_ORDER_BY_ALLOWLIST
        .iter()
        .copied()
        .find(|field| *field == bare)
        .unwrap_or("created_at")
}

/// Append a direction to an expression, keeping its parameters in place.
fn directed(mut expression: Frag, suffix: &str) -> Frag {
    expression.push_raw(suffix);
    expression
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_unknown_and_double_dashes() {
        assert_eq!(
            sanitize_order_by("", &ISSUE_ORDER_BY_ALLOWLIST, "-created_at"),
            "-created_at"
        );
        assert_eq!(
            sanitize_order_by("nonsense", &ISSUE_ORDER_BY_ALLOWLIST, "-created_at"),
            "-created_at"
        );
        assert_eq!(
            sanitize_order_by("--created_at", &ISSUE_ORDER_BY_ALLOWLIST, "-created_at"),
            "-created_at"
        );
        assert_eq!(
            sanitize_order_by("-sequence_id", &ISSUE_ORDER_BY_ALLOWLIST, "-created_at"),
            "-sequence_id"
        );
        assert_eq!(
            sanitize_order_by("priority_order", &ISSUE_ORDER_BY_ALLOWLIST, "-created_at"),
            "-created_at"
        );
    }

    #[test]
    fn priority_returns_the_opposite_direction() {
        let ordering = Ordering::resolve("priority");
        assert_eq!(ordering.returned, "-priority_order");
        assert!(ordering.desc);
        let order = ordering.paginator_order();
        assert_eq!(
            order[0].sql,
            "CASE WHEN issues.priority = ? THEN 0 WHEN issues.priority = ? THEN 1 WHEN issues.priority = ? THEN 2 WHEN issues.priority = ? THEN 3 WHEN issues.priority = ? THEN 4 ELSE NULL END DESC NULLS LAST"
        );
        assert_eq!(order[0].params.len(), 5);

        let ordering = Ordering::resolve("-priority");
        assert_eq!(ordering.returned, "priority_order");
        assert!(!ordering.desc);
    }

    #[test]
    fn state_group_reverses_the_case_for_descending() {
        let ascending = Ordering::resolve("state__group");
        assert!(ascending.expression.sql.contains("THEN 0 WHEN"));
        assert!(ascending.expression.sql.ends_with("ELSE 5 END"));
        let descending = Ordering::resolve("-state__group");
        assert!(
            descending
                .expression
                .sql
                .contains("WHEN states.group = ? THEN 0")
        );
        assert_eq!(descending.returned, "-state_order");
        assert_eq!(
            descending.expression.params[0],
            Value::Text("cancelled".to_owned())
        );
    }

    #[test]
    fn min_values_groups_by_the_primary_key() {
        let ordering = Ordering::resolve("labels__name");
        assert_eq!(ordering.returned, "min_values");
        assert!(ordering.group_by_issue);
        assert!(ordering.joins.contains(Relation::IssueLabels));
        assert!(ordering.joins.contains(Relation::Labels));
        assert_eq!(ordering.expression.sql, "MIN(labels.name)");
    }

    #[test]
    fn created_at_is_not_tiebroken_by_itself() {
        let ordering = Ordering::resolve("created_at");
        assert_eq!(ordering.returned, "created_at");
        assert_eq!(ordering.own_order().len(), 1);
        let ordering = Ordering::resolve("-created_at");
        assert_eq!(ordering.own_order().len(), 1);
    }
}
