// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `issue_on_results` projection and its row decoding.
//!
//! `issue_on_results` (`plane/utils/grouper.py:93`) is a `values()` call: the
//! response rows contain exactly these fields, in this order, and the JSON keys
//! follow that order. Two details are easy to miss:
//!
//! * `estimate_point` is the FK column (`estimate_point_id`), so the JSON value
//!   is the point's UUID, not its number.
//! * `sub_issues_count`, `attachment_count` and `link_count` are correlated
//!   `COUNT(*)` subqueries **with a `GROUP BY`**: an issue with no children,
//!   links or attachments gets one row with a NULL count and therefore `null`,
//!   not `0`.

use chrono::{DateTime, NaiveDate, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::error::QueryError;
use crate::issue::model::{GroupField, ValueField, cycle_id_expression};
use crate::json::Json;
use crate::sql::{Frag, Value};

/// Comment and entity-type constants the subqueries need.
mod constants {
    pub const IS_ISSUE_ATTACHMENT: &str = "ISSUE_ATTACHMENT";
    pub const TRIAGE_GROUP: &str = "triage";
}

/// Which `assignee_ids` subquery to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Projection {
    /// `issue_queryset_grouper`'s annotation: every live assignee row.
    #[default]
    Grouped,
    /// `IssueListEndpoint`'s annotation: only assignees with an active project
    /// membership (`assignee__member_project__is_active=True`,
    /// `plane/app/views/issue/base.py:939`).
    IssueListEndpoint,
}

/// The SQL projection for `fields`, each expression aliased to its JSON key.
pub fn projection(fields: &[ValueField]) -> Vec<Frag> {
    projection_with(Projection::Grouped, fields)
}

/// [`projection`] for one of the two annotation variants.
pub fn projection_with(kind: Projection, fields: &[ValueField]) -> Vec<Frag> {
    fields
        .iter()
        .map(|field| {
            let mut frag = expression(*field, kind);
            frag.push_raw(" AS \"");
            frag.push_raw(field.key());
            frag.push_raw("\"");
            frag
        })
        .collect()
}

fn expression(field: ValueField, kind: Projection) -> Frag {
    match field {
        ValueField::Id => Frag::raw("issues.id"),
        ValueField::Name => Frag::raw("issues.name"),
        ValueField::StateId => Frag::raw("issues.state_id"),
        ValueField::SortOrder => Frag::raw("issues.sort_order"),
        ValueField::CompletedAt => Frag::raw("issues.completed_at"),
        ValueField::EstimatePoint => Frag::raw("issues.estimate_point_id"),
        ValueField::Priority => Frag::raw("issues.priority"),
        ValueField::StartDate => Frag::raw("issues.start_date"),
        ValueField::TargetDate => Frag::raw("issues.target_date"),
        ValueField::SequenceId => Frag::raw("issues.sequence_id"),
        ValueField::ProjectId => Frag::raw("issues.project_id"),
        ValueField::ParentId => Frag::raw("issues.parent_id"),
        ValueField::CycleId => cycle_id_expression(),
        ValueField::SubIssuesCount => sub_issues_count(),
        ValueField::CreatedAt => Frag::raw("issues.created_at"),
        ValueField::UpdatedAt => Frag::raw("issues.updated_at"),
        ValueField::CreatedBy => Frag::raw("issues.created_by_id"),
        ValueField::UpdatedBy => Frag::raw("issues.updated_by_id"),
        ValueField::AttachmentCount => attachment_count(),
        ValueField::LinkCount => link_count(),
        ValueField::IsDraft => Frag::raw("issues.is_draft"),
        ValueField::ArchivedAt => Frag::raw("issues.archived_at"),
        ValueField::StateGroup => Frag::raw("states.group"),
        ValueField::AssigneeIds => match kind {
            Projection::Grouped => array_aggregate(
                "issue_assignees",
                "assignee_id",
                "issue_assignees.deleted_at IS NULL",
            ),
            Projection::IssueListEndpoint => Frag::raw(
                "COALESCE((SELECT ARRAY_AGG(DISTINCT issue_assignees.assignee_id) \
                 FROM issue_assignees INNER JOIN users ON issue_assignees.assignee_id = users.id \
                 INNER JOIN project_members ON users.id = project_members.member_id \
                 WHERE issue_assignees.deleted_at IS NULL AND project_members.is_active \
                 AND project_members.deleted_at IS NULL AND issue_assignees.issue_id = issues.id \
                 GROUP BY issue_assignees.issue_id), ARRAY[]::uuid[])",
            ),
        },
        ValueField::LabelIds => array_aggregate(
            "issue_labels",
            "label_id",
            "issue_labels.deleted_at IS NULL",
        ),
        ValueField::ModuleIds => Frag::raw(
            "COALESCE((SELECT ARRAY_AGG(DISTINCT module_issues.module_id) \
             FROM module_issues INNER JOIN modules ON module_issues.module_id = modules.id \
             WHERE module_issues.deleted_at IS NULL AND module_issues.issue_id = issues.id \
             AND modules.archived_at IS NULL GROUP BY module_issues.issue_id), ARRAY[]::uuid[])",
        ),
        ValueField::Group(group) => group.expression(),
    }
}

fn array_aggregate(table: &str, column: &str, filter: &str) -> Frag {
    Frag::raw(format!(
        "COALESCE((SELECT ARRAY_AGG(DISTINCT {table}.{column}) FROM {table} \
         WHERE {filter} AND {table}.issue_id = issues.id GROUP BY {table}.issue_id), ARRAY[]::uuid[])"
    ))
}

/// `Issue.issue_objects.filter(parent=OuterRef("id"))` as a count subquery.
///
/// The manager's filters are reproduced in full: sub-issues in a triage state,
/// archived, draft, or in an archived project are not counted.
fn sub_issues_count() -> Frag {
    let mut frag = Frag::raw(
        "(SELECT COUNT(sub_issues.id) FROM issues sub_issues \
         LEFT OUTER JOIN states sub_states ON sub_issues.state_id = sub_states.id \
         INNER JOIN projects sub_projects ON sub_issues.project_id = sub_projects.id \
         WHERE sub_issues.deleted_at IS NULL AND (sub_states.group IS NULL OR sub_states.group <> ",
    );
    frag.push(Frag::bind(Value::Text(constants::TRIAGE_GROUP.to_owned())));
    frag.push_raw(
        ") AND sub_issues.archived_at IS NULL AND sub_projects.archived_at IS NULL \
         AND NOT sub_issues.is_draft AND sub_issues.parent_id = issues.id \
         GROUP BY sub_issues.parent_id)",
    );
    frag
}

fn attachment_count() -> Frag {
    let mut frag = Frag::raw(
        "(SELECT COUNT(file_assets.id) FROM file_assets \
         WHERE file_assets.deleted_at IS NULL AND file_assets.entity_type = ",
    );
    frag.push(Frag::bind(Value::Text(
        constants::IS_ISSUE_ATTACHMENT.to_owned(),
    )));
    frag.push_raw(" AND file_assets.issue_id = issues.id GROUP BY file_assets.issue_id)");
    frag
}

fn link_count() -> Frag {
    Frag::raw(
        "(SELECT COUNT(issue_links.id) FROM issue_links \
         WHERE issue_links.deleted_at IS NULL AND issue_links.issue_id = issues.id \
         GROUP BY issue_links.issue_id)",
    )
}

/// Decode one row into the response object for `fields`.
pub fn decode(row: &Row, fields: &[ValueField]) -> Result<Json, QueryError> {
    let mut entries: Vec<(&str, Json)> = Vec::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let value = decode_field(row, index, *field)?;
        entries.push((field.key(), value));
    }
    Ok(Json::object(entries))
}

fn column_error(index: usize, field: ValueField) -> QueryError {
    QueryError::unsupported(format!(
        "column {index} for {:?} has an unexpected type",
        field.key()
    ))
}

fn decode_field(row: &Row, index: usize, field: ValueField) -> Result<Json, QueryError> {
    let value = match field {
        ValueField::Id | ValueField::ProjectId => Json::uuid(
            row.try_get::<_, Uuid>(index)
                .map_err(|_| column_error(index, field))?,
        ),
        ValueField::Name | ValueField::Priority => Json::string(
            row.try_get::<_, String>(index)
                .map_err(|_| column_error(index, field))?,
        ),
        ValueField::StateId
        | ValueField::ParentId
        | ValueField::CycleId
        | ValueField::EstimatePoint
        | ValueField::CreatedBy
        | ValueField::UpdatedBy => Json::opt_uuid(
            row.try_get::<_, Option<Uuid>>(index)
                .map_err(|_| column_error(index, field))?,
        ),
        ValueField::SortOrder => Json::Float(
            row.try_get::<_, f64>(index)
                .map_err(|_| column_error(index, field))?,
        ),
        ValueField::CompletedAt | ValueField::CreatedAt | ValueField::UpdatedAt => {
            let value = match field {
                ValueField::CompletedAt => row.try_get::<_, Option<DateTime<Utc>>>(index),
                _ => row.try_get::<_, DateTime<Utc>>(index).map(Some),
            }
            .map_err(|_| column_error(index, field))?;
            Json::opt_datetime(value)
        }
        ValueField::StartDate | ValueField::TargetDate | ValueField::ArchivedAt => Json::opt_date(
            row.try_get::<_, Option<NaiveDate>>(index)
                .map_err(|_| column_error(index, field))?,
        ),
        ValueField::SequenceId => Json::Int(i64::from(
            row.try_get::<_, i32>(index)
                .map_err(|_| column_error(index, field))?,
        )),
        ValueField::SubIssuesCount | ValueField::AttachmentCount | ValueField::LinkCount => {
            // A NULL count stays `null`: the subquery returns no row when
            // there is nothing to count.
            match row
                .try_get::<_, Option<i64>>(index)
                .map_err(|_| column_error(index, field))?
            {
                Some(value) => Json::Int(value),
                None => Json::Null,
            }
        }
        ValueField::IsDraft => Json::Bool(
            row.try_get::<_, bool>(index)
                .map_err(|_| column_error(index, field))?,
        ),
        ValueField::StateGroup => Json::opt_string(
            row.try_get::<_, Option<String>>(index)
                .map_err(|_| column_error(index, field))?,
        ),
        ValueField::AssigneeIds | ValueField::LabelIds | ValueField::ModuleIds => {
            let mut ids = row
                .try_get::<_, Option<Vec<Uuid>>>(index)
                .map_err(|_| column_error(index, field))?
                .unwrap_or_default();
            // `ARRAY_AGG(DISTINCT …)` returns ascending order; sorting here
            // makes that explicit instead of depending on the aggregate's
            // internal sort.
            ids.sort();
            Json::Array(ids.into_iter().map(Json::uuid).collect())
        }
        ValueField::Group(group) => match group {
            GroupField::Priority | GroupField::StateGroup => Json::opt_string(
                row.try_get::<_, Option<String>>(index)
                    .map_err(|_| column_error(index, field))?,
            ),
            GroupField::TargetDate | GroupField::StartDate => Json::opt_date(
                row.try_get::<_, Option<NaiveDate>>(index)
                    .map_err(|_| column_error(index, field))?,
            ),
            _ => Json::opt_uuid(
                row.try_get::<_, Option<Uuid>>(index)
                    .map_err(|_| column_error(index, field))?,
            ),
        },
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_aliases_the_json_keys() {
        let fields = vec![
            ValueField::Id,
            ValueField::EstimatePoint,
            ValueField::StateGroup,
        ];
        let sql = projection(&fields)
            .into_iter()
            .map(|frag| frag.into_sql().text)
            .collect::<Vec<_>>();
        assert_eq!(sql[0], "issues.id AS \"id\"");
        assert_eq!(sql[1], "issues.estimate_point_id AS \"estimate_point\"");
        assert_eq!(sql[2], "states.group AS \"state__group\"");
    }

    #[test]
    fn counts_are_subqueries_with_a_group_by() {
        let sql = link_count().into_sql().text;
        assert!(sql.contains("COUNT(issue_links.id)"));
        assert!(sql.contains("GROUP BY issue_links.issue_id"));
    }

    #[test]
    fn sub_issue_count_uses_the_manager_filters() {
        let sql = sub_issues_count().into_sql();
        assert!(sql.text.contains("sub_issues.deleted_at IS NULL"));
        assert!(sql.text.contains("sub_projects.archived_at IS NULL"));
        assert_eq!(sql.params, vec![Value::Text("triage".to_owned())]);
    }
}
