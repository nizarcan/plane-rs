// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `issue_group_values` (`plane/utils/grouper.py:144`).
//!
//! The paginators take the *complete* list of group keys up front, separately
//! from the page, and use it to decide which groups exist and in what order
//! they appear. The list comes from the model managers, not from the issues:
//!
//! * states are ordered by `State.Meta.ordering` (`sequence`),
//! * labels, modules, cycles, projects and members by `-created_at`,
//! * `priority` and `state__group` are fixed lists (note: `priority` is
//!   `low, medium, high, urgent, none`, *not* `PRIORITY_ORDER`),
//! * `labels__id`, `issue_module__module_id` and `cycle_id` append the string
//!   `"None"` so the null group exists even before any issue is placed in it,
//! * `target_date`, `start_date` and `created_by` read the issues themselves
//!   with `values_list(...).distinct()`. The default `-created_at` ordering is
//!   still applied, and Django adds the ordering column to the SELECT list of a
//!   `DISTINCT` query, so the DISTINCT never collapses anything: the function
//!   returns **one entry per issue**, in `-created_at` order. The paginator
//!   then uses the list as a dict, so duplicates collapse and the group order
//!   becomes "newest issue first". Reproducing that is required: the group
//!   order in the response depends on it.

use chrono::NaiveDate;
use uuid::Uuid;

use crate::error::QueryError;
use crate::issue::model::GroupField;
use crate::sql::{Frag, Join, Select, Sql, Value};

/// What the caller knows about the scope.
#[derive(Debug, Clone)]
pub struct GroupValuesRequest<'a> {
    pub field: GroupField,
    pub workspace_slug: &'a str,
    /// The project the list endpoint is scoped to, when there is one.
    pub project_id: Option<Uuid>,
    /// The user, for workspace-wide endpoints (`project_id` is then derived
    /// from their active project memberships).
    pub user: Option<Uuid>,
    /// `filtered_issue_queryset`, needed by the three issue-derived fields.
    pub issues: Option<&'a Select>,
}

/// What to do to get the group keys.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupValues {
    /// A fixed list; no query, exactly like the Python.
    Static(Vec<String>),
    /// One column: the first column of every row, rendered with `str()`.
    /// `append_none` reproduces the trailing `"None"` the Python adds for
    /// `labels__id`, `issue_module__module_id` and `cycle_id`.
    Query { sql: Sql, append_none: bool },
}

impl GroupValuesRequest<'_> {
    /// The project-scope expression: one project id, or the subquery over the
    /// user's active project memberships.
    fn project_scope(&self) -> Result<Frag, QueryError> {
        match self.project_id {
            Some(project_id) => Ok(Frag::bind(Value::Uuid(project_id))),
            None => {
                let user = self.user.ok_or_else(|| {
                    QueryError::unsupported("issue_group_values needs a project_id or a user")
                })?;
                let mut frag = Frag::raw(
                    "SELECT project_members.project_id FROM project_members \
                     INNER JOIN workspaces ON project_members.workspace_id = workspaces.id \
                     WHERE project_members.deleted_at IS NULL AND project_members.is_active \
                     AND project_members.member_id = ",
                );
                frag.push(Frag::bind(Value::Uuid(user)));
                frag.push_raw(" AND workspaces.slug = ");
                frag.push(Frag::bind(Value::Text(self.workspace_slug.to_owned())));
                Ok(frag)
            }
        }
    }

    fn require_scope(&self) -> Result<(), QueryError> {
        if self.project_id.is_none() && self.user.is_none() {
            return Err(QueryError::unsupported(
                "issue_group_values needs a project_id or a user",
            ));
        }
        Ok(())
    }
}

/// The group-value query for one field.
pub fn values(request: &GroupValuesRequest<'_>) -> Result<GroupValues, QueryError> {
    request.require_scope()?;
    match request.field {
        GroupField::Priority => Ok(GroupValues::Static(
            ["low", "medium", "high", "urgent", "none"]
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        )),
        GroupField::StateGroup => Ok(GroupValues::Static(
            ["backlog", "unstarted", "started", "completed", "cancelled"]
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        )),
        GroupField::StateId => {
            let scope = request.project_scope()?;
            let mut in_scope = Frag::raw("states.project_id IN (");
            in_scope.push(scope);
            in_scope.push_raw(")");
            let select = Select::new("states")
                .project("states.id")
                .join(Join::inner(
                    "workspaces",
                    "workspaces",
                    Frag::raw("states.workspace_id = workspaces.id"),
                ))
                .where_(Frag::is_null("states.deleted_at"))
                .where_(Frag::raw("NOT (states.group = 'triage')"))
                .where_(Frag::raw("NOT states.is_triage"))
                .where_(in_scope)
                .where_(Frag::compare(
                    "workspaces.slug",
                    "=",
                    Value::Text(request.workspace_slug.to_owned()),
                ))
                .order_by("states.sequence ASC");
            Ok(GroupValues::Query {
                sql: select.into_sql(),
                append_none: false,
            })
        }
        GroupField::LabelsId => Ok(GroupValues::Query {
            sql: model_query(request, "labels")?,
            append_none: true,
        }),
        GroupField::ModuleId => Ok(GroupValues::Query {
            sql: model_query(request, "modules")?,
            append_none: true,
        }),
        GroupField::CycleId => Ok(GroupValues::Query {
            sql: model_query(request, "cycles")?,
            append_none: true,
        }),
        GroupField::ProjectId => Ok(GroupValues::Query {
            sql: model_query(request, "projects")?,
            append_none: false,
        }),
        GroupField::AssigneesId => {
            let (table, column) = match request.project_id {
                Some(_) => ("project_members", "project_members.member_id"),
                None => ("workspace_members", "workspace_members.member_id"),
            };
            let mut select = Select::new(table)
                .project(column)
                .join(Join::inner(
                    "workspaces",
                    "workspaces",
                    Frag::raw(format!("{table}.workspace_id = workspaces.id")),
                ))
                .where_(Frag::is_null(format!("{table}.deleted_at").as_str()))
                .where_(Frag::raw(format!("{table}.is_active")))
                .where_(Frag::compare(
                    "workspaces.slug",
                    "=",
                    Value::Text(request.workspace_slug.to_owned()),
                ))
                .order_by(format!("{table}.created_at DESC"));
            if let Some(project_id) = request.project_id {
                select = select.where_(Frag::compare(
                    "project_members.project_id",
                    "=",
                    Value::Uuid(project_id),
                ));
            }
            Ok(GroupValues::Query {
                sql: select.into_sql(),
                append_none: false,
            })
        }
        GroupField::TargetDate | GroupField::StartDate | GroupField::CreatedBy => {
            let base = request.issues.ok_or_else(|| {
                QueryError::unsupported("issue_group_values needs a queryset for this field")
            })?;
            let column = match request.field {
                GroupField::TargetDate => "issues.target_date",
                GroupField::StartDate => "issues.start_date",
                GroupField::CreatedBy => "issues.created_by_id",
                _ => unreachable!(),
            };
            let mut select = base.clone();
            // `values_list(field, flat=True).distinct()`: DISTINCT over
            // (field, created_at), ordered by created_at DESC. Because
            // `created_at` is unique per issue, the DISTINCT is a no-op and the
            // function returns one row per issue — the quirk this port keeps.
            select.distinct = true;
            select.projection = vec![Frag::raw(column), Frag::raw("issues.created_at")];
            select.group_by.clear();
            select.order_by = vec![Frag::raw("issues.created_at DESC")];
            select.limit = None;
            select.offset = None;
            Ok(GroupValues::Query {
                sql: select.into_sql(),
                append_none: false,
            })
        }
    }
}

/// `Model.objects.filter(workspace__slug=slug, project_id__in=project_ids)`
/// ordered by `-created_at`.
fn model_query(request: &GroupValuesRequest<'_>, table: &'static str) -> Result<Sql, QueryError> {
    let scope = request.project_scope()?;
    // `Project` is filtered by its own primary key (`id__in`); the other models
    // by their `project_id` column (`issue_group_values`, grouper.py:190).
    let scope_column = if table == "projects" {
        format!("{}.id", table)
    } else {
        format!("{}.project_id", table)
    };
    let mut in_scope = Frag::raw(format!("{scope_column} IN ("));
    in_scope.push(scope);
    in_scope.push_raw(")");
    let select = Select::new(table)
        .project(format!("{table}.id"))
        .join(Join::inner(
            "workspaces",
            "workspaces",
            Frag::raw(format!("{table}.workspace_id = workspaces.id")),
        ))
        .where_(Frag::is_null(format!("{table}.deleted_at").as_str()))
        .where_(in_scope)
        .where_(Frag::compare(
            "workspaces.slug",
            "=",
            Value::Text(request.workspace_slug.to_owned()),
        ))
        .order_by(format!("{table}.created_at DESC"));
    Ok(select.into_sql())
}

/// Render one UUID group value the way Python's `str()` does.
pub fn render_uuid(value: Option<Uuid>) -> String {
    value.map_or_else(|| "None".to_owned(), |value| value.to_string())
}

/// Render one date group value.
pub fn render_date(value: Option<NaiveDate>) -> String {
    value.map_or_else(|| "None".to_owned(), |value| value.to_string())
}

/// Render one string group value (or `None`).
pub fn render_string(value: Option<&str>) -> String {
    value.unwrap_or("None").to_owned()
}

/// Append the literal `"None"` group, as `issue_group_values` does for the
/// fields that can be null.
pub fn with_none_group(mut values: Vec<String>) -> Vec<String> {
    values.push("None".to_owned());
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(field: GroupField, project: Option<Uuid>) -> GroupValuesRequest<'static> {
        GroupValuesRequest {
            field,
            workspace_slug: "w",
            project_id: project,
            user: None,
            issues: None,
        }
    }

    #[test]
    fn priority_is_the_fixed_list_with_low_first() {
        let values = values(&request(GroupField::Priority, Some(Uuid::nil()))).expect("values");
        assert_eq!(
            values,
            GroupValues::Static(
                ["low", "medium", "high", "urgent", "none"]
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect()
            )
        );
    }

    #[test]
    fn state_ids_filter_the_manager_and_order_by_sequence() {
        let values = values(&request(GroupField::StateId, Some(Uuid::nil()))).expect("values");
        let GroupValues::Query { sql, .. } = values else {
            panic!("expected a query");
        };
        assert!(sql.text.contains("NOT states.is_triage"));
        assert!(sql.text.contains("ORDER BY states.sequence ASC"));
        assert!(sql.text.contains("states.project_id IN ($1)"));
        assert_eq!(sql.params[0], Value::Uuid(Uuid::nil()));
        assert_eq!(sql.params[1], Value::Text("w".to_owned()));
    }

    #[test]
    fn workspace_scope_uses_the_member_subquery() {
        let values = values(&GroupValuesRequest {
            field: GroupField::LabelsId,
            workspace_slug: "w",
            project_id: None,
            user: Some(Uuid::nil()),
            issues: None,
        })
        .expect("values");
        let GroupValues::Query { sql, .. } = values else {
            panic!("expected a query");
        };
        assert!(sql.text.contains("SELECT project_members.project_id"));
        assert_eq!(sql.params[0], Value::Uuid(Uuid::nil()));
    }

    #[test]
    fn issue_derived_fields_need_a_queryset() {
        let error = values(&request(GroupField::StartDate, Some(Uuid::nil())));
        assert!(error.is_err());
    }

    #[test]
    fn rendering_matches_python_str() {
        assert_eq!(render_uuid(None), "None");
        assert_eq!(render_date(None), "None");
        assert_eq!(
            render_date(NaiveDate::from_ymd_opt(2026, 6, 15)),
            "2026-06-15"
        );
    }
}
