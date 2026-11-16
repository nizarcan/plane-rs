// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The stored `analytic_views.query` dictionary, translated into SQL.
//!
//! `SavedAnalyticEndpoint.get` does `Issue.issue_objects.filter(**analytic_view.query)`
//! (`plane/app/views/analytic/base.py:201-208`). The dictionary is whatever
//! `issue_filters(query_dict, "POST")` produced when the view was created
//! (`plane/app/serializers/analytic.py:16-22`), so the keys are exactly the
//! POST branch's: `state__in`, `state__group__in`, `priority__in`, … — never
//! the GET-only `isnull` variants for the m2m relations.
//!
//! Each key below is one lookup of `plane/utils/issue_filters.py`. A key this
//! module does not know is a Django `FieldError`, i.e. Plane's family 500
//! (`plane/app/views/base.py:235-239`), which is what
//! [`ViewError::Unknown`] renders.

use chrono::NaiveDate;
use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_query::issue::model::Relation;
use plane_query::sql::{Frag, JoinKind, Value, prep_for_like_query};
use uuid::Uuid;

use crate::analytics::sql::QueryBase;

/// `Queryset.filter(**filter)` for a stored analytic query.
pub(crate) fn apply(
    base: &mut QueryBase,
    filter: &serde_json::Value,
    timezone: Tz,
) -> Result<(), ViewError> {
    let Some(entries) = filter.as_object() else {
        // `**filter` with a non-dict raises `TypeError` → 500.
        return Err(ViewError::Unknown(
            "TypeError: argument of type is not a mapping".to_owned(),
        ));
    };
    for (key, value) in entries {
        match key.as_str() {
            "state__in" => {
                base.and(Frag::in_uuids("issues.state_id", &uuids(value)?));
            }
            "state__group__in" => {
                base.require_relation(Relation::States, JoinKind::Inner);
                base.and(Frag::in_texts("states.group", &texts(value)?));
            }
            "estimate_point__in" => {
                base.and(Frag::in_uuids("issues.estimate_point_id", &uuids(value)?));
            }
            "priority__in" => {
                base.and(Frag::in_texts("issues.priority", &texts(value)?));
            }
            "parent__in" => {
                base.and(Frag::in_uuids("issues.parent_id", &uuids(value)?));
            }
            "parent__isnull" => {
                if flag(value)? {
                    base.and(Frag::is_null("issues.parent_id"));
                } else {
                    base.and(Frag::is_not_null("issues.parent_id"));
                }
            }
            "labels__in" => {
                base.require_relation(Relation::IssueLabels, JoinKind::Inner);
                base.and(Frag::in_uuids("issue_labels.label_id", &uuids(value)?));
            }
            "label_issue__deleted_at__isnull" => {
                base.require_relation(Relation::IssueLabels, JoinKind::Left);
                if flag(value)? {
                    base.and(Frag::is_null("issue_labels.deleted_at"));
                } else {
                    base.and(Frag::is_not_null("issue_labels.deleted_at"));
                }
            }
            "assignees__in" => {
                base.require_relation(Relation::IssueAssignees, JoinKind::Inner);
                base.and(Frag::in_uuids(
                    "issue_assignees.assignee_id",
                    &uuids(value)?,
                ));
            }
            "issue_assignee__deleted_at__isnull" => {
                base.require_relation(Relation::IssueAssignees, JoinKind::Left);
                if flag(value)? {
                    base.and(Frag::is_null("issue_assignees.deleted_at"));
                } else {
                    base.and(Frag::is_not_null("issue_assignees.deleted_at"));
                }
            }
            "issue_mention__mention__id__in" => {
                base.require_relation(Relation::IssueMentions, JoinKind::Inner);
                base.and(Frag::in_uuids("issue_mentions.mention_id", &uuids(value)?));
            }
            "created_by__in" => {
                base.and(Frag::in_uuids("issues.created_by_id", &uuids(value)?));
            }
            "name__icontains" => {
                base.and(Frag::icontains("issues.name", &text(value)?));
            }
            "created_at__date__gte" => {
                date_bound(base, "issues.created_at", ">=", timezone, value)?;
            }
            "created_at__date__lte" => {
                date_bound(base, "issues.created_at", "<=", timezone, value)?;
            }
            "created_at__date__contains" => {
                date_contains(base, "issues.created_at", timezone, value)?;
            }
            "updated_at__date__gte" => {
                date_bound(base, "issues.updated_at", ">=", timezone, value)?;
            }
            "updated_at__date__lte" => {
                date_bound(base, "issues.updated_at", "<=", timezone, value)?;
            }
            "updated_at__date__contains" => {
                date_contains(base, "issues.updated_at", timezone, value)?;
            }
            "completed_at__date__gte" => {
                date_bound(base, "issues.completed_at", ">=", timezone, value)?;
            }
            "completed_at__date__lte" => {
                date_bound(base, "issues.completed_at", "<=", timezone, value)?;
            }
            "completed_at__date__contains" => {
                date_contains(base, "issues.completed_at", timezone, value)?;
            }
            "start_date" => {
                base.and(Frag::compare(
                    "issues.start_date",
                    "=",
                    Value::Date(date(value)?),
                ));
            }
            "target_date" => {
                base.and(Frag::compare(
                    "issues.target_date",
                    "=",
                    Value::Date(date(value)?),
                ));
            }
            "start_date__isnull" => {
                if flag(value)? {
                    base.and(Frag::is_null("issues.start_date"));
                } else {
                    base.and(Frag::is_not_null("issues.start_date"));
                }
            }
            "target_date__isnull" => {
                if flag(value)? {
                    base.and(Frag::is_null("issues.target_date"));
                } else {
                    base.and(Frag::is_not_null("issues.target_date"));
                }
            }
            "project__in" => {
                base.and(Frag::in_uuids("issues.project_id", &uuids(value)?));
            }
            "issue_cycle__cycle_id__in" => {
                base.require_relation(Relation::CycleIssues, JoinKind::Inner);
                base.and(Frag::in_uuids("cycle_issues.cycle_id", &uuids(value)?));
            }
            "issue_cycle__deleted_at__isnull" => {
                base.require_relation(Relation::CycleIssues, JoinKind::Left);
                if flag(value)? {
                    base.and(Frag::is_null("cycle_issues.deleted_at"));
                } else {
                    base.and(Frag::is_not_null("cycle_issues.deleted_at"));
                }
            }
            "issue_module__module_id__in" => {
                base.require_relation(Relation::ModuleIssues, JoinKind::Inner);
                base.and(Frag::in_uuids("module_issues.module_id", &uuids(value)?));
            }
            "issue_module__deleted_at__isnull" => {
                base.require_relation(Relation::ModuleIssues, JoinKind::Left);
                if flag(value)? {
                    base.and(Frag::is_null("module_issues.deleted_at"));
                } else {
                    base.and(Frag::is_not_null("module_issues.deleted_at"));
                }
            }
            "issue_intake__status__in" => {
                base.require_relation(Relation::IntakeIssues, JoinKind::Inner);
                base.and(Frag::in_list(
                    "intake_issues.status",
                    ints(value)?.into_iter().map(Value::Int).collect(),
                ));
            }
            "issue_subscribers__subscriber_id__in" => {
                base.require_relation(Relation::IssueSubscribers, JoinKind::Inner);
                base.and(Frag::in_uuids(
                    "issue_subscribers.subscriber_id",
                    &uuids(value)?,
                ));
            }
            "issue_subscribers__deleted_at__isnull" => {
                base.require_relation(Relation::IssueSubscribers, JoinKind::Left);
                if flag(value)? {
                    base.and(Frag::is_null("issue_subscribers.deleted_at"));
                } else {
                    base.and(Frag::is_not_null("issue_subscribers.deleted_at"));
                }
            }
            // Exact-match lookups. `issue_filters` only ever produces the
            // `__in` forms, but a view created by hand (or by the fixture)
            // may store `{"priority": "high"}`, which Django applies as
            // `priority = 'high'`.
            "priority" => {
                base.and(Frag::compare(
                    "issues.priority",
                    "=",
                    Value::Text(text(value)?),
                ));
            }
            "state__group" => {
                base.require_relation(Relation::States, JoinKind::Inner);
                base.and(Frag::compare(
                    "states.group",
                    "=",
                    Value::Text(text(value)?),
                ));
            }
            "state" | "state_id" => {
                base.and(Frag::compare(
                    "issues.state_id",
                    "=",
                    Value::Uuid(uuid(value)?),
                ));
            }
            "project" | "project_id" => {
                base.and(Frag::compare(
                    "issues.project_id",
                    "=",
                    Value::Uuid(uuid(value)?),
                ));
            }
            "parent" | "parent_id" => {
                base.and(Frag::compare(
                    "issues.parent_id",
                    "=",
                    Value::Uuid(uuid(value)?),
                ));
            }
            "created_by" | "created_by_id" => {
                base.and(Frag::compare(
                    "issues.created_by_id",
                    "=",
                    Value::Uuid(uuid(value)?),
                ));
            }
            "name" => {
                base.and(Frag::compare("issues.name", "=", Value::Text(text(value)?)));
            }
            // `logged_by` adds `logged_by__in`, but `Issue` has no such
            // column: `FieldError` → 500 (`issue_filters.py:414-425`).
            other => {
                return Err(ViewError::Unknown(format!(
                    "FieldError: Cannot resolve keyword '{other}' into field"
                )));
            }
        }
    }
    Ok(())
}

/// `(column AT TIME ZONE tz)::date <op> date`.
fn date_bound(
    base: &mut QueryBase,
    column: &str,
    op: &str,
    timezone: Tz,
    value: &serde_json::Value,
) -> Result<(), ViewError> {
    let mut frag = Frag::raw("(");
    frag.push_raw(column);
    frag.push_raw(" ");
    frag.push_raw(&crate::analytics::sql::timezone_literal(timezone));
    frag.push_raw(")::date ");
    frag.push_raw(op);
    frag.push_raw(" ");
    frag.push(Frag::bind(Value::Date(date(value)?)));
    base.and(frag);
    Ok(())
}

/// `(column AT TIME ZONE tz)::date::text LIKE '%value%'`.
fn date_contains(
    base: &mut QueryBase,
    column: &str,
    timezone: Tz,
    value: &serde_json::Value,
) -> Result<(), ViewError> {
    let mut frag = Frag::raw("(");
    frag.push_raw(column);
    frag.push_raw(" ");
    frag.push_raw(&crate::analytics::sql::timezone_literal(timezone));
    frag.push_raw(")::date::text LIKE ");
    frag.push(Frag::bind(Value::Text(format!(
        "%{}%",
        prep_for_like_query(&text(value)?)
    ))));
    base.and(frag);
    Ok(())
}

fn uuid(value: &serde_json::Value) -> Result<Uuid, ViewError> {
    Uuid::parse_str(&text(value)?).map_err(|_| ViewError::Validation)
}

fn uuids(value: &serde_json::Value) -> Result<Vec<Uuid>, ViewError> {
    let mut out = Vec::new();
    for item in list(value)? {
        let Some(text) = item.as_str() else {
            return Err(ViewError::Validation);
        };
        out.push(Uuid::parse_str(text).map_err(|_| ViewError::Validation)?);
    }
    Ok(out)
}

fn texts(value: &serde_json::Value) -> Result<Vec<String>, ViewError> {
    let mut out = Vec::new();
    for item in list(value)? {
        let Some(text) = item.as_str() else {
            return Err(ViewError::Validation);
        };
        out.push(text.to_owned());
    }
    Ok(out)
}

fn ints(value: &serde_json::Value) -> Result<Vec<i32>, ViewError> {
    let mut out = Vec::new();
    for item in list(value)? {
        // Django's `IntegerField.get_prep_value` accepts `int` and `str(int)`.
        let parsed = match item {
            serde_json::Value::Number(number) => number.as_i64().map(|value| value as i32),
            serde_json::Value::String(text) => text.parse::<i32>().ok(),
            _ => None,
        };
        out.push(parsed.ok_or(ViewError::Validation)?);
    }
    Ok(out)
}

fn list(value: &serde_json::Value) -> Result<&Vec<serde_json::Value>, ViewError> {
    value
        .as_array()
        .ok_or_else(|| ViewError::Unknown("TypeError: 'str' object is not iterable".to_owned()))
}

fn text(value: &serde_json::Value) -> Result<String, ViewError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or(ViewError::Validation)
}

fn date(value: &serde_json::Value) -> Result<NaiveDate, ViewError> {
    let raw = text(value)?;
    NaiveDate::parse_from_str(&raw, "%Y-%m-%d").map_err(|_| ViewError::Validation)
}

fn flag(value: &serde_json::Value) -> Result<bool, ViewError> {
    value.as_bool().ok_or(ViewError::Validation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_queries_become_django_lookups() {
        // `analytic_views.query` of the fixture's default view.
        let mut base = QueryBase::issue_objects();
        apply(
            &mut base,
            &serde_json::json!({"priority": "high"}),
            chrono_tz::UTC,
        )
        .expect("priority is a known lookup");
        assert!(
            base.predicate()
                .into_sql()
                .text
                .contains("issues.priority = $1"),
            "{}",
            base.predicate().into_sql().text
        );

        // …and of the custom view: the group goes through the `states` join.
        let mut base = QueryBase::issue_objects();
        apply(
            &mut base,
            &serde_json::json!({"state__group": "started"}),
            chrono_tz::UTC,
        )
        .expect("state__group is a known lookup");
        assert!(
            base.predicate()
                .into_sql()
                .text
                .contains("states.group = $1")
        );

        // The `issue_filters(..., "POST")` shapes: `__in` lists and the
        // `deleted_at` conditions its m2m branches always add.
        let mut base = QueryBase::issue_objects();
        apply(
            &mut base,
            &serde_json::json!({
                "priority__in": ["urgent", "high"],
                "labels__in": ["51298d1d-71da-4345-baa7-3c68b16b1f00"],
                "label_issue__deleted_at__isnull": true,
            }),
            chrono_tz::UTC,
        )
        .expect("post filters");
        let sql = base.predicate().into_sql().text;
        assert!(sql.contains("issues.priority IN ("), "{sql}");
        assert!(sql.contains("issue_labels.label_id IN ("), "{sql}");
        assert!(sql.contains("issue_labels.deleted_at IS NULL"), "{sql}");
    }

    #[test]
    fn an_unknown_lookup_is_the_django_field_error() {
        let mut base = QueryBase::issue_objects();
        let error = apply(
            &mut base,
            &serde_json::json!({"logged_by__in": ["x"]}),
            chrono_tz::UTC,
        )
        .expect_err("logged_by has no column");
        assert!(matches!(error, ViewError::Unknown(_)));
        // A malformed UUID is `UUIDField`'s `ValidationError` (400).
        let mut base = QueryBase::issue_objects();
        let error = apply(
            &mut base,
            &serde_json::json!({"state__in": ["not-a-uuid"]}),
            chrono_tz::UTC,
        )
        .expect_err("invalid uuid");
        assert_eq!(error, ViewError::Validation);
    }
}
